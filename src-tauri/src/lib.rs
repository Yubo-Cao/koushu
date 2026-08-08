use arboard::Clipboard;
use base64::{engine::general_purpose, Engine as _};
use chrono::Local;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use flate2::read::GzDecoder;
use reqwest::header::{CONTENT_LENGTH, RANGE};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tar::Archive;
use tauri::{ipc::Channel, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CRISPASR_BIN_NAME: &str = "crispasr-x86_64-unknown-linux-gnu";

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
const CRISPASR_BIN_NAME: &str = "crispasr-unsupported-platform";

struct AppState {
    db: Mutex<Connection>,
    app_dir: PathBuf,
    python_script: PathBuf,
    crispasr_bin: PathBuf,
    downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    audio_capture: Mutex<Option<AudioCaptureHandle>>,
}

struct AudioCaptureHandle {
    stop_tx: mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<AudioLevelInfo>>,
    sample_rate: u32,
}

impl Drop for AudioCaptureHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[derive(Debug, Serialize)]
struct PlatformInfo {
    os: String,
    arch: String,
    session_type: Option<String>,
    wayland_display: bool,
    x11_display: bool,
    paste_tools: Vec<String>,
    python: String,
    bundled_asr: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ModelInfo {
    id: String,
    name: String,
    backend: String,
    source: String,
    repo_id: String,
    local_path: String,
    status: String,
    size_bytes: Option<i64>,
    installed_at: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum ModelDownloadEvent {
    Started {
        model_id: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Progress {
        model_id: String,
        chunk_bytes: u64,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Paused {
        model_id: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Finished {
        model_id: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        model: ModelInfo,
    },
    Error {
        model_id: String,
        error: String,
    },
}

enum DownloadResult {
    Installed(u64),
    Paused { downloaded_bytes: u64 },
}

#[derive(Debug, Serialize)]
struct SessionInfo {
    id: String,
    title: String,
    started_at: String,
    ended_at: Option<String>,
    date_key: String,
    model: String,
    language: String,
    runtime: String,
}

#[derive(Debug, Serialize)]
struct TranscriptInfo {
    id: String,
    session_id: String,
    text: String,
    status: String,
    source: String,
    created_at: String,
    duration_ms: Option<i64>,
    model: String,
    language: String,
}

#[derive(Debug, Serialize)]
struct Bootstrap {
    setup_complete: bool,
    settings: serde_json::Value,
    platform: PlatformInfo,
    models: Vec<ModelInfo>,
    sessions: Vec<SessionInfo>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    title: Option<String>,
    model: String,
    language: String,
    runtime: String,
}

#[derive(Debug, Deserialize)]
struct TranscribeAudioRequest {
    session_id: Option<String>,
    audio_base64: String,
    model_id: String,
    language: String,
    hotwords: Option<Vec<String>>,
}

struct AsrJob {
    session_id: String,
    model_id: String,
    model: ModelInfo,
    audio_path: PathBuf,
    language: String,
    hotwords: Option<Vec<String>>,
    save_final: bool,
    retain_audio: bool,
    python_script: PathBuf,
    gpu_python: PathBuf,
    crispasr_bin: PathBuf,
}

struct AsrJobOutput {
    session_id: String,
    model_id: String,
    model_backend: String,
    language: String,
    save_final: bool,
    transcription: Result<(String, String), String>,
}

#[derive(Debug, Serialize)]
struct AsrResult {
    session_id: String,
    transcript: Option<TranscriptInfo>,
    text: String,
    runtime: String,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct PasteResult {
    copied: bool,
    pasted: bool,
    method: Option<String>,
    message: String,
    session_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct PythonProbe {
    ok: bool,
    python: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuRuntimeInfo {
    ok: bool,
    installed: bool,
    driver_ok: bool,
    driver: String,
    runtime_dir: String,
    uv: Option<String>,
    python: Option<String>,
    python_version: Option<String>,
    torch: Option<String>,
    torch_cuda: Option<String>,
    cuda_available: bool,
    device: Option<String>,
    vllm: Option<String>,
    funasr: Option<String>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct PythonGpuInfo {
    ok: bool,
    missing: Option<Vec<String>>,
    torch: Option<String>,
    torch_cuda: Option<String>,
    cuda_available: bool,
    device_count: Option<i64>,
    device: Option<String>,
    vllm: Option<String>,
    funasr: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum GpuRuntimeInstallEvent {
    Started { message: String },
    Progress { message: String },
    Finished { runtime: GpuRuntimeInfo },
    Error { error: String },
}

#[derive(Debug, Deserialize)]
struct PythonAsrResponse {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioLevelInfo {
    rms: f32,
    peak: f32,
    db: f32,
    percent: f32,
}

impl Default for AudioLevelInfo {
    fn default() -> Self {
        Self {
            rms: 0.0,
            peak: 0.0,
            db: -90.0,
            percent: 0.0,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioInputInfo {
    id: String,
    name: String,
    is_default: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeAudioCaptureResult {
    audio_base64: String,
    duration_ms: f64,
    rms: f32,
    peak: f32,
    db: f32,
    speech_like: bool,
    sample_rate: u32,
}

#[tauri::command]
fn list_audio_inputs() -> Result<Vec<AudioInputInfo>, String> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let devices = host
        .input_devices()
        .map_err(|err| format!("Failed to enumerate audio inputs: {err}"))?;
    let mut inputs = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| format!("Input device {}", index + 1));
        let is_default = default_name.as_deref() == Some(name.as_str());
        inputs.push(AudioInputInfo {
            id: index.to_string(),
            name,
            is_default,
        });
    }
    Ok(inputs)
}

#[tauri::command]
fn start_audio_capture(
    state: tauri::State<'_, AppState>,
    device_id: Option<String>,
) -> Result<(), String> {
    let old_capture = {
        let mut capture = state
            .audio_capture
            .lock()
            .map_err(|_| "Audio capture lock poisoned".to_string())?;
        capture.take()
    };
    drop(old_capture);

    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let level = Arc::new(Mutex::new(AudioLevelInfo::default()));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let thread_samples = Arc::clone(&samples);
    let thread_level = Arc::clone(&level);
    let selected_device_id = device_id.filter(|id| !id.is_empty());

    let join = thread::spawn(move || {
        let result =
            build_audio_input_stream(selected_device_id.as_deref(), thread_samples, thread_level);
        match result {
            Ok((stream, sample_rate)) => {
                let _ = ready_tx.send(Ok(sample_rate));
                let _ = stop_rx.recv();
                drop(stream);
            }
            Err(err) => {
                let _ = ready_tx.send(Err(err));
            }
        }
    });

    let sample_rate = match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(sample_rate)) => sample_rate,
        Ok(Err(err)) => {
            let _ = join.join();
            return Err(err);
        }
        Err(err) => {
            let _ = stop_tx.send(());
            let _ = join.join();
            return Err(format!("Timed out starting microphone: {err}"));
        }
    };

    let mut capture = state
        .audio_capture
        .lock()
        .map_err(|_| "Audio capture lock poisoned".to_string())?;
    *capture = Some(AudioCaptureHandle {
        stop_tx,
        join: Some(join),
        samples,
        level,
        sample_rate,
    });
    Ok(())
}

#[tauri::command]
fn get_audio_level(state: tauri::State<'_, AppState>) -> Result<AudioLevelInfo, String> {
    let capture = state
        .audio_capture
        .lock()
        .map_err(|_| "Audio capture lock poisoned".to_string())?;
    let Some(capture) = capture.as_ref() else {
        return Ok(AudioLevelInfo::default());
    };
    capture
        .level
        .lock()
        .map(|level| *level)
        .map_err(|_| "Audio level lock poisoned".to_string())
}

#[tauri::command]
fn snapshot_audio_capture(
    state: tauri::State<'_, AppState>,
    max_ms: Option<u32>,
) -> Result<NativeAudioCaptureResult, String> {
    let capture = state
        .audio_capture
        .lock()
        .map_err(|_| "Audio capture lock poisoned".to_string())?;
    let Some(capture) = capture.as_ref() else {
        return Err("No active microphone recording.".to_string());
    };
    let samples = capture
        .samples
        .lock()
        .map_err(|_| "Audio sample lock poisoned".to_string())?
        .clone();
    let samples = trim_audio_samples(&samples, capture.sample_rate, max_ms);
    encode_capture_result(&samples, capture.sample_rate)
}

#[tauri::command]
fn stop_audio_capture(
    state: tauri::State<'_, AppState>,
) -> Result<NativeAudioCaptureResult, String> {
    let capture = {
        let mut capture = state
            .audio_capture
            .lock()
            .map_err(|_| "Audio capture lock poisoned".to_string())?;
        capture.take()
    }
    .ok_or_else(|| "No active microphone recording.".to_string())?;

    let samples = capture
        .samples
        .lock()
        .map_err(|_| "Audio sample lock poisoned".to_string())?
        .clone();
    encode_capture_result(&samples, capture.sample_rate)
}

#[tauri::command]
fn get_bootstrap(state: tauri::State<'_, AppState>) -> Result<Bootstrap, String> {
    Ok(Bootstrap {
        setup_complete: setting_bool(&state, "setup.complete")?,
        settings: load_settings_json(&state)?,
        platform: platform_info(&state),
        models: list_models_inner(&state)?,
        sessions: list_sessions_inner(&state, 60)?,
    })
}

#[tauri::command]
fn complete_onboarding(state: tauri::State<'_, AppState>) -> Result<(), String> {
    set_setting_inner(&state, "setup.complete", "true")
}

#[tauri::command]
fn reset_onboarding(state: tauri::State<'_, AppState>) -> Result<(), String> {
    set_setting_inner(&state, "setup.complete", "false")
}

#[tauri::command]
fn list_models(state: tauri::State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    list_models_inner(&state)
}

#[tauri::command]
fn list_sessions(
    state: tauri::State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<SessionInfo>, String> {
    list_sessions_inner(&state, limit.unwrap_or(60))
}

#[tauri::command]
fn list_transcripts(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<TranscriptInfo>, String> {
    list_transcripts_inner(&state, &session_id)
}

#[tauri::command]
fn create_session(
    state: tauri::State<'_, AppState>,
    request: CreateSessionRequest,
) -> Result<SessionInfo, String> {
    create_session_inner(
        &state,
        request.title,
        &request.model,
        &request.language,
        &request.runtime,
    )
}

#[tauri::command]
fn set_setting(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    set_setting_inner(&state, &key, &value)
}

#[tauri::command]
fn probe_python(state: tauri::State<'_, AppState>) -> PythonProbe {
    let python = python_bin();
    match Command::new(&python)
        .arg(&state.python_script)
        .arg("probe")
        .output()
    {
        Ok(output) if output.status.success() => PythonProbe {
            ok: true,
            python,
            message: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        },
        Ok(output) => PythonProbe {
            ok: false,
            python,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        },
        Err(err) => PythonProbe {
            ok: false,
            python,
            message: err.to_string(),
        },
    }
}

#[tauri::command]
async fn probe_gpu_runtime(state: tauri::State<'_, AppState>) -> Result<GpuRuntimeInfo, String> {
    let app_dir = state.app_dir.clone();
    let python_script = state.python_script.clone();
    tauri::async_runtime::spawn_blocking(move || inspect_gpu_runtime(&app_dir, &python_script))
        .await
        .map_err(|err| format!("GPU runtime probe failed to join: {err}"))?
}

#[tauri::command]
async fn install_gpu_runtime_with_progress(
    state: tauri::State<'_, AppState>,
    on_event: Channel<GpuRuntimeInstallEvent>,
) -> Result<GpuRuntimeInfo, String> {
    let app_dir = state.app_dir.clone();
    let python_script = state.python_script.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_gpu_runtime_inner(&app_dir, &python_script, &on_event);
        if let Err(err) = &result {
            send_gpu_install_event(
                &on_event,
                GpuRuntimeInstallEvent::Error { error: err.clone() },
            );
        }
        result
    })
    .await
    .map_err(|err| format!("GPU runtime install failed to join: {err}"))?
}

fn send_download_event(channel: &Channel<ModelDownloadEvent>, event: ModelDownloadEvent) {
    let _ = channel.send(event);
}

fn download_gguf_model(
    model: &ModelInfo,
    cancel: &AtomicBool,
    on_event: &Channel<ModelDownloadEvent>,
) -> Result<DownloadResult, String> {
    let destination = Path::new(&model.local_path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model.repo_id, "funasr-nano-2512-q4_k.gguf"
    );
    let tmp_path = destination.with_extension("gguf.download");
    let mut existing_bytes = fs::metadata(&tmp_path).map(|meta| meta.len()).unwrap_or(0);
    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|err| err.to_string())?;
    let mut request = client.get(&url);
    if existing_bytes > 0 {
        request = request.header(RANGE, format!("bytes={existing_bytes}-"));
    }

    let mut response = request.send().map_err(|err| err.to_string())?;
    if existing_bytes > 0 && response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let _ = fs::remove_file(&tmp_path);
        existing_bytes = 0;
        response = client.get(&url).send().map_err(|err| err.to_string())?;
    }
    if !response.status().is_success() {
        return Err(format!(
            "Hugging Face returned HTTP {} for {url}",
            response.status()
        ));
    }

    if existing_bytes > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        existing_bytes = 0;
    }

    let response_len = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| response.content_length());
    let total_bytes = response_len.map(|len| len + existing_bytes);
    send_download_event(
        on_event,
        ModelDownloadEvent::Started {
            model_id: model.id.clone(),
            downloaded_bytes: existing_bytes,
            total_bytes,
        },
    );

    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(existing_bytes > 0)
        .truncate(existing_bytes == 0)
        .open(&tmp_path)
        .map_err(|err| err.to_string())?;

    let mut downloaded_bytes = existing_bytes;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            file.flush().map_err(|err| err.to_string())?;
            send_download_event(
                on_event,
                ModelDownloadEvent::Paused {
                    model_id: model.id.clone(),
                    downloaded_bytes,
                    total_bytes,
                },
            );
            return Ok(DownloadResult::Paused { downloaded_bytes });
        }

        let read = response.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }

        file.write_all(&buffer[..read])
            .map_err(|err| err.to_string())?;
        downloaded_bytes += read as u64;
        send_download_event(
            on_event,
            ModelDownloadEvent::Progress {
                model_id: model.id.clone(),
                chunk_bytes: read as u64,
                downloaded_bytes,
                total_bytes,
            },
        );
    }

    file.flush().map_err(|err| err.to_string())?;
    fs::rename(&tmp_path, destination).map_err(|err| err.to_string())?;
    Ok(DownloadResult::Installed(downloaded_bytes))
}

fn download_python_model(
    state: &AppState,
    model: &ModelInfo,
    on_event: &Channel<ModelDownloadEvent>,
) -> Result<DownloadResult, String> {
    let python = if model.backend == "funasr-vllm-gpu" {
        let python = gpu_python_bin(&state.app_dir);
        if !python.exists() {
            return Err(
                "Install the GPU runtime in Settings before downloading the vLLM model."
                    .to_string(),
            );
        }
        python
    } else {
        PathBuf::from(python_bin())
    };
    send_download_event(
        on_event,
        ModelDownloadEvent::Started {
            model_id: model.id.clone(),
            downloaded_bytes: 0,
            total_bytes: None,
        },
    );
    let output = Command::new(python)
        .arg(&state.python_script)
        .arg("ensure-model")
        .arg("--repo")
        .arg(&model.repo_id)
        .arg("--local-dir")
        .arg(&model.local_path)
        .env("PYTHONUNBUFFERED", "1")
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        dir_size(Path::new(&model.local_path))
            .map(DownloadResult::Installed)
            .map_err(|err| err.to_string())
    } else {
        Err(compact_process_error(&output.stdout, &output.stderr))
    }
}

#[tauri::command]
fn pause_model_download(state: tauri::State<'_, AppState>, model_id: String) -> Result<(), String> {
    let downloads = state
        .downloads
        .lock()
        .map_err(|_| "Download lock poisoned".to_string())?;
    if let Some(cancel) = downloads.get(&model_id) {
        cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn download_model_with_progress(
    state: tauri::State<'_, AppState>,
    model_id: String,
    on_event: Channel<ModelDownloadEvent>,
) -> Result<ModelInfo, String> {
    let model = get_model(&state, &model_id)?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut downloads = state
            .downloads
            .lock()
            .map_err(|_| "Download lock poisoned".to_string())?;
        if downloads.contains_key(&model_id) {
            return Err("That model is already downloading.".to_string());
        }
        downloads.insert(model_id.clone(), cancel.clone());
    }

    set_model_status(&state, &model_id, "downloading", None, None)?;

    let result = if model.backend == "crispasr-gguf-cpu" {
        download_gguf_model(&model, &cancel, &on_event)
    } else {
        download_python_model(&state, &model, &on_event)
    };

    let response = match result {
        Ok(DownloadResult::Installed(size)) => {
            set_model_status(
                &state,
                &model_id,
                "installed",
                Some(size as i64),
                Some(Local::now().to_rfc3339()),
            )?;
            let updated = get_model(&state, &model_id)?;
            send_download_event(
                &on_event,
                ModelDownloadEvent::Finished {
                    model_id: model_id.clone(),
                    downloaded_bytes: size,
                    total_bytes: Some(size),
                    model: updated.clone(),
                },
            );
            Ok(updated)
        }
        Ok(DownloadResult::Paused { downloaded_bytes }) => {
            set_model_status(
                &state,
                &model_id,
                "paused",
                Some(downloaded_bytes as i64),
                None,
            )?;
            get_model(&state, &model_id)
        }
        Err(err) => {
            set_model_error(&state, &model_id, &err)?;
            send_download_event(
                &on_event,
                ModelDownloadEvent::Error {
                    model_id: model_id.clone(),
                    error: err.clone(),
                },
            );
            Err(err)
        }
    };

    let mut downloads = state
        .downloads
        .lock()
        .map_err(|_| "Download lock poisoned".to_string())?;
    downloads.remove(&model_id);
    response
}

fn transcribe_with_crispasr(
    crispasr_bin: &Path,
    model: &ModelInfo,
    audio_path: &Path,
    language: &str,
) -> Result<(String, String), String> {
    let model_path = Path::new(&model.local_path);
    if !model_path.exists() {
        return Err("Model is not downloaded yet. Download Fun-ASR-Nano from the welcome or settings screen.".to_string());
    }
    if !crispasr_bin.exists() {
        return Err("Bundled CrispASR runtime is missing from the app resources.".to_string());
    }

    let mut command = low_priority_command(crispasr_bin);
    let output = command
        .arg("--backend")
        .arg("funasr")
        .arg("--no-gpu")
        .arg("--no-prints")
        .arg("--no-timestamps")
        .arg("--language")
        .arg(crispasr_language(language))
        .arg("--model")
        .arg(model_path)
        .arg("--file")
        .arg(audio_path)
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        Ok((
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
            "crispasr-gguf-cpu".to_string(),
        ))
    } else {
        Err(compact_process_error(&output.stdout, &output.stderr))
    }
}

fn transcribe_with_python(
    python_script: &Path,
    model: &ModelInfo,
    audio_path: &Path,
    language: &str,
    hotwords: Option<&[String]>,
) -> Result<(String, String), String> {
    let python = PathBuf::from(python_bin());
    let mut command = low_priority_command(&python);
    let output = command
        .arg(python_script)
        .arg("transcribe")
        .arg("--audio")
        .arg(audio_path)
        .arg("--repo")
        .arg(&model.repo_id)
        .arg("--local-dir")
        .arg(&model.local_path)
        .arg("--language")
        .arg(language)
        .args(hotword_args(hotwords))
        .env("PYTHONUNBUFFERED", "1")
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        return Err(compact_process_error(&output.stdout, &output.stderr));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_python_asr_response(&raw, "Python ASR")?;
    if parsed.ok {
        Ok((parsed.text.unwrap_or_default(), "python-hf-cpu".to_string()))
    } else {
        Err(parsed
            .error
            .unwrap_or_else(|| "Python ASR failed".to_string()))
    }
}

fn transcribe_with_vllm(
    python: &Path,
    python_script: &Path,
    model: &ModelInfo,
    audio_path: &Path,
    language: &str,
    hotwords: Option<&[String]>,
) -> Result<(String, String), String> {
    if !python.exists() {
        return Err("GPU runtime is not installed. Open Settings and install the GPU runtime before using the vLLM backend.".to_string());
    }
    let mut command = low_priority_command(python);
    command
        .arg(python_script)
        .arg("transcribe-vllm")
        .arg("--audio")
        .arg(audio_path)
        .arg("--repo")
        .arg(&model.repo_id)
        .arg("--local-dir")
        .arg(&model.local_path)
        .arg("--language")
        .arg(language)
        .args(hotword_args(hotwords))
        .arg("--gpu-memory-utilization")
        .arg(vllm_gpu_memory_utilization())
        .arg("--max-model-len")
        .arg(vllm_max_model_len())
        .arg("--max-num-seqs")
        .arg(vllm_max_num_seqs());
    if vllm_enforce_eager() {
        command.arg("--enforce-eager");
    }
    let output = command
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        return Err(compact_process_error(&output.stdout, &output.stderr));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_python_asr_response(&raw, "vLLM ASR")?;
    if parsed.ok {
        Ok((
            parsed.text.unwrap_or_default(),
            "funasr-vllm-gpu".to_string(),
        ))
    } else {
        Err(parsed.error.unwrap_or_else(|| {
            "Fun-ASR vLLM failed. Open Settings and run Install GPU Runtime, then retry."
                .to_string()
        }))
    }
}

fn crispasr_language(language: &str) -> &'static str {
    match language {
        "中文" => "zh",
        "English" => "en",
        "日本語" => "ja",
        "粤语" => "yue",
        "한국어" => "ko",
        _ => "auto",
    }
}

#[tauri::command]
async fn transcribe_audio(
    state: tauri::State<'_, AppState>,
    request: TranscribeAudioRequest,
) -> Result<AsrResult, String> {
    transcribe_audio_inner(&state, request, true).await
}

async fn transcribe_audio_inner(
    state: &AppState,
    request: TranscribeAudioRequest,
    save_final: bool,
) -> Result<AsrResult, String> {
    let job = prepare_asr_job(state, request, save_final)?;
    let output = tauri::async_runtime::spawn_blocking(move || run_asr_job(job))
        .await
        .map_err(|err| format!("ASR worker failed to join: {err}"))?;
    finish_asr_job(state, output)
}

fn prepare_asr_job(
    state: &AppState,
    request: TranscribeAudioRequest,
    save_final: bool,
) -> Result<AsrJob, String> {
    let session = match request.session_id {
        Some(id) => id,
        None => {
            if save_final {
                create_session_inner(
                    state,
                    Some("Voice note".to_string()),
                    &request.model_id,
                    &request.language,
                    "crispasr-gguf-cpu",
                )?
                .id
            } else {
                "preview".to_string()
            }
        }
    };

    let audio = decode_audio_payload(&request.audio_base64)?;
    let audio_dir = state.app_dir.join("audio").join("incoming");
    fs::create_dir_all(&audio_dir).map_err(|err| err.to_string())?;
    let audio_path = audio_dir.join(format!("{}.wav", Uuid::new_v4()));
    fs::write(&audio_path, audio).map_err(|err| err.to_string())?;

    let model = get_model(state, &request.model_id)?;

    Ok(AsrJob {
        session_id: session,
        model_id: request.model_id,
        model,
        audio_path,
        language: request.language,
        hotwords: request.hotwords,
        save_final,
        retain_audio: setting_bool(state, "audio.retain").unwrap_or(false),
        python_script: state.python_script.clone(),
        gpu_python: gpu_python_bin(&state.app_dir),
        crispasr_bin: state.crispasr_bin.clone(),
    })
}

fn run_asr_job(job: AsrJob) -> AsrJobOutput {
    let transcription = match job.model.backend.as_str() {
        "crispasr-gguf-cpu" => transcribe_with_crispasr(
            &job.crispasr_bin,
            &job.model,
            &job.audio_path,
            &job.language,
        ),
        "funasr-vllm-gpu" => transcribe_with_vllm(
            &job.gpu_python,
            &job.python_script,
            &job.model,
            &job.audio_path,
            &job.language,
            job.hotwords.as_deref(),
        ),
        _ => transcribe_with_python(
            &job.python_script,
            &job.model,
            &job.audio_path,
            &job.language,
            job.hotwords.as_deref(),
        ),
    };

    if !job.retain_audio {
        let _ = fs::remove_file(&job.audio_path);
    }

    AsrJobOutput {
        session_id: job.session_id,
        model_id: job.model_id,
        model_backend: job.model.backend,
        language: job.language,
        save_final: job.save_final,
        transcription,
    }
}

fn finish_asr_job(state: &AppState, output: AsrJobOutput) -> Result<AsrResult, String> {
    match output.transcription {
        Ok((text, runtime)) => {
            let transcript = if output.save_final {
                Some(insert_transcript_inner(
                    state,
                    &output.session_id,
                    &text,
                    "final",
                    "microphone",
                    &output.model_id,
                    &output.language,
                    None,
                )?)
            } else {
                None
            };
            Ok(AsrResult {
                session_id: output.session_id,
                transcript,
                text,
                runtime,
                error: None,
            })
        }
        Err(err) => Ok(AsrResult {
            session_id: output.session_id,
            transcript: None,
            text: String::new(),
            runtime: output.model_backend,
            error: Some(err),
        }),
    }
}

#[tauri::command]
async fn preview_audio(
    state: tauri::State<'_, AppState>,
    request: TranscribeAudioRequest,
) -> Result<AsrResult, String> {
    transcribe_audio_inner(&state, request, false).await
}

#[tauri::command]
fn save_text_transcript(
    state: tauri::State<'_, AppState>,
    session_id: String,
    text: String,
    model: String,
    language: String,
) -> Result<TranscriptInfo, String> {
    insert_transcript_inner(
        &state,
        &session_id,
        &text,
        "final",
        "typed",
        &model,
        &language,
        None,
    )
}

#[tauri::command]
fn copy_text(text: String) -> PasteResult {
    copy_text_native(&text)
}

#[tauri::command]
fn auto_paste_text(text: String) -> PasteResult {
    let previous_clipboard = read_clipboard_text().ok();
    let copy_result = copy_text_native(&text);
    if !copy_result.copied {
        return copy_result;
    }

    thread::sleep(Duration::from_millis(300));
    let result = match paste_from_clipboard() {
        Ok(method) => {
            restore_clipboard_if_unchanged(&text, previous_clipboard);
            PasteResult {
                copied: true,
                pasted: true,
                method: Some(method),
                message: "Copied to clipboard and sent paste keystroke.".to_string(),
                session_type: env::var("XDG_SESSION_TYPE").ok(),
            }
        }
        Err(err) => {
            restore_clipboard_if_unchanged(&text, previous_clipboard);
            PasteResult {
                copied: true,
                pasted: false,
                method: None,
                message: format!("Copied to clipboard, but auto-paste is unavailable: {err}"),
                session_type: env::var("XDG_SESSION_TYPE").ok(),
            }
        }
    };
    result
}

#[tauri::command]
fn show_voice_bar(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    window.show().map_err(|err| err.to_string())?;
    window.set_focus().map_err(|err| err.to_string())
}

#[tauri::command]
fn hide_voice_bar(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    window.hide().map_err(|err| err.to_string())
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|err| err.to_string())?;
        return window.set_focus().map_err(|err| err.to_string());
    }

    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("/settings".into()))
        .title("Fun ASR Settings")
        .inner_size(1080.0, 760.0)
        .min_inner_size(960.0, 640.0)
        .center()
        .build()
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
            fs::create_dir_all(&app_dir).map_err(|err| err.to_string())?;
            fs::create_dir_all(app_dir.join("models")).map_err(|err| err.to_string())?;
            fs::create_dir_all(app_dir.join("audio")).map_err(|err| err.to_string())?;

            let db = init_db(&app_dir).map_err(|err| err.to_string())?;
            let python_script = app
                .path()
                .resource_dir()
                .ok()
                .map(|dir| dir.join("python").join("funasr_worker.py"))
                .filter(|path| path.exists())
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("python/funasr_worker.py")
                });
            let crispasr_bin = app
                .path()
                .resource_dir()
                .ok()
                .map(|dir| dir.join("binaries").join(CRISPASR_BIN_NAME))
                .filter(|path| path.exists())
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("binaries")
                        .join(CRISPASR_BIN_NAME)
                });

            app.manage(AppState {
                db: Mutex::new(db),
                app_dir,
                python_script,
                crispasr_bin,
                downloads: Mutex::new(HashMap::new()),
                audio_capture: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_inputs,
            start_audio_capture,
            get_audio_level,
            snapshot_audio_capture,
            stop_audio_capture,
            get_bootstrap,
            complete_onboarding,
            reset_onboarding,
            list_models,
            list_sessions,
            list_transcripts,
            create_session,
            set_setting,
            probe_python,
            probe_gpu_runtime,
            install_gpu_runtime_with_progress,
            download_model_with_progress,
            pause_model_download,
            preview_audio,
            transcribe_audio,
            save_text_transcript,
            copy_text,
            auto_paste_text,
            show_voice_bar,
            hide_voice_bar,
            show_settings_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fun ASR Desktop");
}

fn init_db(app_dir: &Path) -> rusqlite::Result<Connection> {
    let db_path = app_dir.join("fun_asr_desktop.sqlite3");
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS models (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            backend TEXT NOT NULL,
            source TEXT NOT NULL,
            repo_id TEXT NOT NULL,
            local_path TEXT NOT NULL,
            status TEXT NOT NULL,
            size_bytes INTEGER,
            installed_at TEXT,
            last_error TEXT
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            date_key TEXT NOT NULL,
            model TEXT NOT NULL,
            language TEXT NOT NULL,
            runtime TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS transcripts (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            text TEXT NOT NULL,
            status TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            duration_ms INTEGER,
            model TEXT NOT NULL,
            language TEXT NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS segments (
            id TEXT PRIMARY KEY,
            transcript_id TEXT NOT NULL,
            text TEXT NOT NULL,
            start_ms INTEGER,
            end_ms INTEGER,
            confidence REAL,
            FOREIGN KEY(transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts USING fts5(
            text,
            content='transcripts',
            content_rowid='rowid'
        );

        CREATE TRIGGER IF NOT EXISTS transcripts_ai AFTER INSERT ON transcripts BEGIN
            INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS transcripts_ad AFTER DELETE ON transcripts BEGIN
            INSERT INTO transcripts_fts(transcripts_fts, rowid, text)
            VALUES ('delete', old.rowid, old.text);
        END;

        CREATE TRIGGER IF NOT EXISTS transcripts_au AFTER UPDATE ON transcripts BEGIN
            INSERT INTO transcripts_fts(transcripts_fts, rowid, text)
            VALUES ('delete', old.rowid, old.text);
            INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
        END;
        "#,
    )?;

    let default_model_path = app_dir
        .join("models")
        .join("fun-asr-nano-2512")
        .join("funasr-nano-2512-q4_k.gguf")
        .to_string_lossy()
        .to_string();
    let vllm_model_path = app_dir
        .join("models")
        .join("fun-asr-nano-2512-vllm")
        .to_string_lossy()
        .to_string();
    conn.execute(
        r#"
        INSERT INTO models
        (id, name, backend, source, repo_id, local_path, status, size_bytes, installed_at, last_error)
        VALUES
        (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            backend = excluded.backend,
            source = excluded.source,
            repo_id = excluded.repo_id,
            local_path = excluded.local_path,
            status = CASE
                WHEN models.status = 'installed' AND NOT EXISTS(SELECT 1 WHERE excluded.local_path = models.local_path)
                THEN 'available'
                ELSE models.status
            END,
            last_error = NULL
        "#,
        params![
            "fun-asr-nano-2512",
            "Fun-ASR-Nano GGUF Q4_K",
            "crispasr-gguf-cpu",
            "huggingface",
            "cstr/funasr-nano-GGUF",
            default_model_path,
            "available"
        ],
    )?;
    conn.execute(
        r#"
        INSERT INTO models
        (id, name, backend, source, repo_id, local_path, status, size_bytes, installed_at, last_error)
        VALUES
        (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            backend = excluded.backend,
            source = excluded.source,
            repo_id = excluded.repo_id,
            local_path = excluded.local_path,
            status = CASE
                WHEN models.status = 'installed' AND NOT EXISTS(SELECT 1 WHERE excluded.local_path = models.local_path)
                THEN 'available'
                ELSE models.status
            END,
            last_error = NULL
        "#,
        params![
            "fun-asr-nano-2512-vllm",
            "Fun-ASR-Nano vLLM GPU",
            "funasr-vllm-gpu",
            "huggingface",
            "FunAudioLLM/Fun-ASR-Nano-2512",
            vllm_model_path,
            "available"
        ],
    )?;

    conn.execute(
        "DELETE FROM models WHERE id = 'fun-asr-nano-2512-python'",
        [],
    )?;

    let defaults = [
        ("setup.complete", "false"),
        ("defaults.model", "fun-asr-nano-2512"),
        ("defaults.language", "中文"),
        ("defaults.runtime", "crispasr-gguf-cpu"),
        ("audio.retain", "false"),
        ("floating.autoPaste", "true"),
    ];
    for (key, value) in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    conn.execute(
        "UPDATE settings SET value = 'crispasr-gguf-cpu' WHERE key = 'defaults.runtime' AND value = 'python-hf-cpu'",
        [],
    )?;
    conn.execute(
        "UPDATE settings SET value = 'fun-asr-nano-2512' WHERE key = 'defaults.model' AND value = 'fun-asr-nano-2512-python'",
        [],
    )?;

    Ok(conn)
}

fn load_settings_json(state: &AppState) -> Result<serde_json::Value, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings ORDER BY key")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| err.to_string())?;
    let mut map = serde_json::Map::new();
    for row in rows {
        let (key, value) = row.map_err(|err| err.to_string())?;
        map.insert(key, serde_json::Value::String(value));
    }
    Ok(serde_json::Value::Object(map))
}

fn setting_bool(state: &AppState, key: &str) -> Result<bool, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| err.to_string())?;
    Ok(matches!(
        value.as_deref(),
        Some("true") | Some("1") | Some("yes")
    ))
}

fn set_setting_inner(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    conn.execute(
        r#"
        INSERT INTO settings (key, value) VALUES (?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![key, value],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn list_models_inner(state: &AppState) -> Result<Vec<ModelInfo>, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, name, backend, source, repo_id, local_path, status,
                   size_bytes, installed_at, last_error
            FROM models
            ORDER BY id
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], map_model)
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

fn get_model(state: &AppState, model_id: &str) -> Result<ModelInfo, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    conn.query_row(
        r#"
        SELECT id, name, backend, source, repo_id, local_path, status,
               size_bytes, installed_at, last_error
        FROM models
        WHERE id = ?1
        "#,
        params![model_id],
        map_model,
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| format!("Unknown model: {model_id}"))
}

fn map_model(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelInfo> {
    Ok(ModelInfo {
        id: row.get(0)?,
        name: row.get(1)?,
        backend: row.get(2)?,
        source: row.get(3)?,
        repo_id: row.get(4)?,
        local_path: row.get(5)?,
        status: row.get(6)?,
        size_bytes: row.get(7)?,
        installed_at: row.get(8)?,
        last_error: row.get(9)?,
    })
}

fn set_model_status(
    state: &AppState,
    model_id: &str,
    status: &str,
    size: Option<i64>,
    installed_at: Option<String>,
) -> Result<(), String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    conn.execute(
        r#"
        UPDATE models
        SET status = ?2,
            size_bytes = COALESCE(?3, size_bytes),
            installed_at = COALESCE(?4, installed_at),
            last_error = NULL
        WHERE id = ?1
        "#,
        params![model_id, status, size, installed_at],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn set_model_error(state: &AppState, model_id: &str, error: &str) -> Result<(), String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    conn.execute(
        "UPDATE models SET status = 'error', last_error = ?2 WHERE id = ?1",
        params![model_id, error],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn create_session_inner(
    state: &AppState,
    title: Option<String>,
    model: &str,
    language: &str,
    runtime: &str,
) -> Result<SessionInfo, String> {
    let now = Local::now();
    let id = Uuid::new_v4().to_string();
    let session = SessionInfo {
        id,
        title: title.unwrap_or_else(|| format!("Session {}", now.format("%H:%M"))),
        started_at: now.to_rfc3339(),
        ended_at: None,
        date_key: now.format("%Y-%m-%d").to_string(),
        model: model.to_string(),
        language: language.to_string(),
        runtime: runtime.to_string(),
    };

    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    conn.execute(
        r#"
        INSERT INTO sessions
        (id, title, started_at, ended_at, date_key, model, language, runtime)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            session.id,
            session.title,
            session.started_at,
            session.ended_at,
            session.date_key,
            session.model,
            session.language,
            session.runtime
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(session)
}

fn list_sessions_inner(state: &AppState, limit: i64) -> Result<Vec<SessionInfo>, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, title, started_at, ended_at, date_key, model, language, runtime
            FROM sessions
            ORDER BY started_at DESC
            LIMIT ?1
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![limit], map_session)
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionInfo> {
    Ok(SessionInfo {
        id: row.get(0)?,
        title: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        date_key: row.get(4)?,
        model: row.get(5)?,
        language: row.get(6)?,
        runtime: row.get(7)?,
    })
}

fn insert_transcript_inner(
    state: &AppState,
    session_id: &str,
    text: &str,
    status: &str,
    source: &str,
    model: &str,
    language: &str,
    duration_ms: Option<i64>,
) -> Result<TranscriptInfo, String> {
    let transcript = TranscriptInfo {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        text: text.to_string(),
        status: status.to_string(),
        source: source.to_string(),
        created_at: Local::now().to_rfc3339(),
        duration_ms,
        model: model.to_string(),
        language: language.to_string(),
    };

    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    conn.execute(
        r#"
        INSERT INTO transcripts
        (id, session_id, text, status, source, created_at, duration_ms, model, language)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            transcript.id,
            transcript.session_id,
            transcript.text,
            transcript.status,
            transcript.source,
            transcript.created_at,
            transcript.duration_ms,
            transcript.model,
            transcript.language
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(transcript)
}

fn list_transcripts_inner(
    state: &AppState,
    session_id: &str,
) -> Result<Vec<TranscriptInfo>, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, session_id, text, status, source, created_at, duration_ms, model, language
            FROM transcripts
            WHERE session_id = ?1
            ORDER BY created_at ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![session_id], map_transcript)
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

fn map_transcript(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptInfo> {
    Ok(TranscriptInfo {
        id: row.get(0)?,
        session_id: row.get(1)?,
        text: row.get(2)?,
        status: row.get(3)?,
        source: row.get(4)?,
        created_at: row.get(5)?,
        duration_ms: row.get(6)?,
        model: row.get(7)?,
        language: row.get(8)?,
    })
}

fn select_audio_input(device_id: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(id) = device_id.filter(|id| !id.is_empty()) {
        let target = id
            .parse::<usize>()
            .map_err(|_| format!("Invalid microphone id: {id}"))?;
        let mut devices = host
            .input_devices()
            .map_err(|err| format!("Failed to enumerate audio inputs: {err}"))?;
        return devices
            .nth(target)
            .ok_or_else(|| format!("Microphone is no longer available: {id}"));
    }

    host.default_input_device()
        .ok_or_else(|| "No default microphone found.".to_string())
}

fn build_audio_input_stream(
    device_id: Option<&str>,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<AudioLevelInfo>>,
) -> Result<(cpal::Stream, u32), String> {
    let device = select_audio_input(device_id)?;
    let supported = device
        .default_input_config()
        .map_err(|err| format!("Failed to read microphone config: {err}"))?;
    let sample_rate = supported.sample_rate().0;
    let config: cpal::StreamConfig = supported.clone().into();
    let channels = usize::from(config.channels).max(1);
    let err_fn = |err| eprintln!("Audio input stream error: {err}");

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let samples = Arc::clone(&samples);
            let level = Arc::clone(&level);
            device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        handle_audio_input(data, channels, &samples, &level, |sample| sample);
                    },
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to start microphone stream: {err}"))?
        }
        cpal::SampleFormat::I16 => {
            let samples = Arc::clone(&samples);
            let level = Arc::clone(&level);
            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        handle_audio_input(data, channels, &samples, &level, |sample| {
                            sample as f32 / i16::MAX as f32
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to start microphone stream: {err}"))?
        }
        cpal::SampleFormat::U16 => {
            let samples = Arc::clone(&samples);
            let level = Arc::clone(&level);
            device
                .build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        handle_audio_input(data, channels, &samples, &level, |sample| {
                            (sample as f32 / u16::MAX as f32) * 2.0 - 1.0
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to start microphone stream: {err}"))?
        }
        sample_format => {
            return Err(format!(
                "Unsupported microphone sample format: {sample_format:?}"
            ));
        }
    };

    stream
        .play()
        .map_err(|err| format!("Failed to activate microphone stream: {err}"))?;
    Ok((stream, sample_rate))
}

fn handle_audio_input<T, F>(
    data: &[T],
    channels: usize,
    samples: &Arc<Mutex<Vec<f32>>>,
    level: &Arc<Mutex<AudioLevelInfo>>,
    convert: F,
) where
    T: Copy,
    F: Fn(T) -> f32,
{
    if data.is_empty() {
        return;
    }

    let mut mono = Vec::with_capacity((data.len() / channels).max(1));
    for frame in data.chunks(channels) {
        let mut sum = 0.0_f32;
        for sample in frame {
            sum += convert(*sample);
        }
        mono.push((sum / frame.len() as f32).clamp(-1.0, 1.0));
    }

    if let Ok(mut collected) = samples.lock() {
        collected.extend_from_slice(&mono);
    }
    if let Ok(mut current_level) = level.lock() {
        *current_level = audio_level_from_samples(&mono);
    }
}

fn trim_audio_samples(samples: &[f32], sample_rate: u32, max_ms: Option<u32>) -> Vec<f32> {
    let Some(max_ms) = max_ms else {
        return samples.to_vec();
    };
    if max_ms == 0 || sample_rate == 0 {
        return samples.to_vec();
    }
    let max_samples = ((u64::from(sample_rate) * u64::from(max_ms)) / 1000) as usize;
    if max_samples == 0 || samples.len() <= max_samples {
        samples.to_vec()
    } else {
        samples[samples.len() - max_samples..].to_vec()
    }
}

fn encode_capture_result(
    samples: &[f32],
    input_sample_rate: u32,
) -> Result<NativeAudioCaptureResult, String> {
    let resampled = resample_linear(samples, input_sample_rate, 16_000);
    let level = audio_level_from_samples(&resampled);
    let duration_ms = if input_sample_rate > 0 {
        samples.len() as f64 / input_sample_rate as f64 * 1000.0
    } else {
        0.0
    };
    let speech_like = duration_ms >= 650.0 && (level.rms >= 0.006 || level.peak >= 0.025);
    let wav = encode_wav_i16(&resampled, 16_000)?;
    Ok(NativeAudioCaptureResult {
        audio_base64: general_purpose::STANDARD.encode(wav),
        duration_ms,
        rms: level.rms,
        peak: level.peak,
        db: level.db,
        speech_like,
        sample_rate: 16_000,
    })
}

fn audio_level_from_samples(samples: &[f32]) -> AudioLevelInfo {
    if samples.is_empty() {
        return AudioLevelInfo::default();
    }

    let mut sum_squares = 0.0_f64;
    let mut peak = 0.0_f32;
    for sample in samples {
        let value = sample.abs().min(1.0);
        peak = peak.max(value);
        sum_squares += f64::from(value * value);
    }
    let rms = (sum_squares / samples.len() as f64).sqrt() as f32;
    let db = if rms > 0.0 {
        (20.0 * rms.max(0.000_031_6).log10()).max(-90.0)
    } else {
        -90.0
    };
    let percent = ((db + 60.0) / 60.0 * 100.0).clamp(0.0, 100.0);

    AudioLevelInfo {
        rms,
        peak,
        db,
        percent,
    }
}

fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || from_rate == to_rate {
        return samples.to_vec();
    }

    let output_len =
        ((samples.len() as f64) * (to_rate as f64 / from_rate as f64)).round() as usize;
    let output_len = output_len.max(1);
    let ratio = from_rate as f64 / to_rate as f64;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let source = index as f64 * ratio;
        let left = source.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let frac = (source - left as f64) as f32;
        let sample = samples[left] * (1.0 - frac) + samples[right] * frac;
        output.push(sample.clamp(-1.0, 1.0));
    }

    output
}

fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    let data_len = samples
        .len()
        .checked_mul(2)
        .ok_or_else(|| "Audio recording is too large.".to_string())?;
    let riff_len = 36_usize
        .checked_add(data_len)
        .ok_or_else(|| "Audio recording is too large.".to_string())?;
    if riff_len > u32::MAX as usize {
        return Err("Audio recording is too large.".to_string());
    }

    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(riff_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());

    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        wav.extend_from_slice(&value.to_le_bytes());
    }

    Ok(wav)
}

fn decode_audio_payload(payload: &str) -> Result<Vec<u8>, String> {
    let data = payload
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or(payload);
    general_purpose::STANDARD
        .decode(data)
        .map_err(|err| format!("Invalid base64 audio payload: {err}"))
}

fn hotword_args(hotwords: Option<&[String]>) -> Vec<String> {
    match hotwords {
        Some(words) if !words.is_empty() => vec![
            "--hotwords".to_string(),
            words
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
        ],
        _ => Vec::new(),
    }
}

fn parse_python_asr_response(raw: &str, label: &str) -> Result<PythonAsrResponse, String> {
    for line in raw.lines().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            return serde_json::from_str(trimmed)
                .map_err(|err| format!("{label} returned invalid JSON: {err}: {trimmed}"));
        }
    }
    serde_json::from_str(raw.trim())
        .map_err(|err| format!("{label} returned invalid JSON: {err}: {raw}"))
}

fn vllm_gpu_memory_utilization() -> String {
    env::var("FUN_ASR_DESKTOP_VLLM_GPU_MEMORY").unwrap_or_else(|_| "0.50".to_string())
}

fn vllm_max_model_len() -> String {
    env::var("FUN_ASR_DESKTOP_VLLM_MAX_MODEL_LEN").unwrap_or_else(|_| "2048".to_string())
}

fn vllm_max_num_seqs() -> String {
    env::var("FUN_ASR_DESKTOP_VLLM_MAX_NUM_SEQS").unwrap_or_else(|_| "1".to_string())
}

fn vllm_enforce_eager() -> bool {
    env::var("FUN_ASR_DESKTOP_VLLM_ENFORCE_EAGER")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

fn python_bin() -> String {
    env::var("FUN_ASR_DESKTOP_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

fn gpu_runtime_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("runtimes").join("vllm-gpu")
}

fn gpu_venv_dir(app_dir: &Path) -> PathBuf {
    gpu_runtime_dir(app_dir).join("venv")
}

fn gpu_python_bin(app_dir: &Path) -> PathBuf {
    gpu_venv_dir(app_dir).join("bin").join("python")
}

fn gpu_uv_bin(app_dir: &Path) -> PathBuf {
    app_dir.join("runtimes").join("uv").join("bin").join("uv")
}

fn gpu_uv_cache_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("runtimes").join("uv-cache")
}

fn torch_backend() -> String {
    env::var("FUN_ASR_DESKTOP_TORCH_BACKEND").unwrap_or_else(|_| "cu130".to_string())
}

fn requirements_vllm_path(python_script: &Path) -> PathBuf {
    python_script
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("requirements-vllm.txt")
}

fn send_gpu_install_event(
    channel: &Channel<GpuRuntimeInstallEvent>,
    event: GpuRuntimeInstallEvent,
) {
    let _ = channel.send(event);
}

fn inspect_gpu_runtime(app_dir: &Path, python_script: &Path) -> Result<GpuRuntimeInfo, String> {
    let runtime_dir = gpu_runtime_dir(app_dir);
    let uv_path = gpu_uv_bin(app_dir);
    let python_path = gpu_python_bin(app_dir);
    let driver = gpu_driver_status();
    let python = python_path
        .exists()
        .then(|| python_path.to_string_lossy().to_string());
    let uv = if uv_path.exists() {
        Some(uv_path.to_string_lossy().to_string())
    } else {
        command_path("uv").map(|path| path.to_string_lossy().to_string())
    };

    if !python_path.exists() {
        return Ok(GpuRuntimeInfo {
            ok: false,
            installed: false,
            driver_ok: driver.0,
            driver: driver.1,
            runtime_dir: runtime_dir.to_string_lossy().to_string(),
            uv,
            python,
            python_version: None,
            torch: None,
            torch_cuda: None,
            cuda_available: false,
            device: None,
            vllm: None,
            funasr: None,
            message: if driver.0 {
                "GPU runtime is not installed. Install it to download Python, PyTorch CUDA wheels, and vLLM into app data.".to_string()
            } else {
                "NVIDIA driver/libcuda was not detected. Install the NVIDIA driver before using the GPU backend.".to_string()
            },
        });
    }

    let python_version = Command::new(&python_path)
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            } else {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        });

    let output = Command::new(&python_path)
        .arg(python_script)
        .arg("gpu-info")
        .env("PYTHONUNBUFFERED", "1")
        .output()
        .map_err(|err| format!("Failed to run GPU runtime probe: {err}"))?;
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parsed: PythonGpuInfo = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "GPU runtime probe returned invalid JSON: {err}: {}",
            compact_process_error(&output.stdout, &output.stderr)
        )
    })?;
    let missing = parsed
        .missing
        .as_ref()
        .map(|items| items.join(", "))
        .filter(|items| !items.is_empty());
    let message = if parsed.ok && driver.0 {
        format!(
            "GPU runtime is ready{}.",
            parsed
                .device
                .as_ref()
                .map(|device| format!(" on {device}"))
                .unwrap_or_default()
        )
    } else if !driver.0 {
        "NVIDIA driver/libcuda was not detected. PyTorch CUDA wheels are installed, but cannot run without the host driver.".to_string()
    } else if let Some(error) = parsed.error.clone() {
        error
    } else if let Some(missing) = missing {
        format!("GPU runtime is missing packages: {missing}")
    } else {
        "GPU runtime is installed, but CUDA is not available to PyTorch.".to_string()
    };

    Ok(GpuRuntimeInfo {
        ok: parsed.ok && driver.0,
        installed: true,
        driver_ok: driver.0,
        driver: driver.1,
        runtime_dir: runtime_dir.to_string_lossy().to_string(),
        uv,
        python,
        python_version,
        torch: parsed.torch,
        torch_cuda: parsed.torch_cuda,
        cuda_available: parsed.cuda_available,
        device: parsed.device.or_else(|| {
            parsed
                .device_count
                .filter(|count| *count > 0)
                .map(|count| format!("{count} CUDA device(s)"))
        }),
        vllm: parsed.vllm,
        funasr: parsed.funasr,
        message,
    })
}

fn install_gpu_runtime_inner(
    app_dir: &Path,
    python_script: &Path,
    channel: &Channel<GpuRuntimeInstallEvent>,
) -> Result<GpuRuntimeInfo, String> {
    send_gpu_install_event(
        channel,
        GpuRuntimeInstallEvent::Started {
            message: "Preparing app-managed GPU runtime.".to_string(),
        },
    );
    fs::create_dir_all(gpu_runtime_dir(app_dir)).map_err(|err| err.to_string())?;
    fs::create_dir_all(gpu_uv_cache_dir(app_dir)).map_err(|err| err.to_string())?;

    let uv = ensure_uv(app_dir, channel)?;
    let venv = gpu_venv_dir(app_dir);
    let python = gpu_python_bin(app_dir);
    let requirements = requirements_vllm_path(python_script);
    if !requirements.exists() {
        return Err(format!(
            "Bundled GPU requirements file is missing: {}",
            requirements.display()
        ));
    }

    send_gpu_install_event(
        channel,
        GpuRuntimeInstallEvent::Progress {
            message: "Creating managed Python 3.12 environment.".to_string(),
        },
    );
    let mut venv_cmd = Command::new(&uv);
    venv_cmd
        .arg("venv")
        .arg(&venv)
        .arg("--python")
        .arg("3.12")
        .arg("--managed-python")
        .arg("--allow-existing")
        .arg("--seed");
    run_runtime_step(
        app_dir,
        venv_cmd,
        "Failed to create the GPU Python environment",
    )?;

    send_gpu_install_event(
        channel,
        GpuRuntimeInstallEvent::Progress {
            message: format!(
                "Installing FunASR, PyTorch CUDA wheels ({}) and vLLM. This can download several GB.",
                torch_backend()
            ),
        },
    );
    let mut install_cmd = Command::new(&uv);
    install_cmd
        .arg("pip")
        .arg("install")
        .arg("--python")
        .arg(&python)
        .arg("--torch-backend")
        .arg(torch_backend())
        .arg("--upgrade")
        .arg("--reinstall-package")
        .arg("torch")
        .arg("--reinstall-package")
        .arg("torchaudio")
        .arg("--reinstall-package")
        .arg("torchvision")
        .arg("-r")
        .arg(&requirements);
    run_runtime_step(
        app_dir,
        install_cmd,
        "Failed to install the GPU Python packages",
    )?;

    send_gpu_install_event(
        channel,
        GpuRuntimeInstallEvent::Progress {
            message: "Checking CUDA visibility from the managed runtime.".to_string(),
        },
    );
    let info = inspect_gpu_runtime(app_dir, python_script)?;
    send_gpu_install_event(
        channel,
        GpuRuntimeInstallEvent::Finished {
            runtime: info.clone(),
        },
    );
    Ok(info)
}

fn ensure_uv(app_dir: &Path, channel: &Channel<GpuRuntimeInstallEvent>) -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("FUN_ASR_DESKTOP_UV").map(PathBuf::from) {
        if path.exists() {
            return Ok(path);
        }
    }
    if let Some(path) = command_path("uv") {
        return Ok(path);
    }

    let uv_path = gpu_uv_bin(app_dir);
    if uv_path.exists() {
        return Ok(uv_path);
    }

    let asset = uv_release_asset()?;
    let url = format!("https://github.com/astral-sh/uv/releases/latest/download/{asset}.tar.gz");
    send_gpu_install_event(
        channel,
        GpuRuntimeInstallEvent::Progress {
            message: "Downloading uv bootstrap binary into app data.".to_string(),
        },
    );
    let bytes = reqwest::blocking::get(&url)
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Failed to download uv from {url}: {err}"))?
        .bytes()
        .map_err(|err| format!("Failed to read uv download: {err}"))?;

    let bin_dir = uv_path
        .parent()
        .ok_or_else(|| "Invalid uv runtime path".to_string())?;
    fs::create_dir_all(bin_dir).map_err(|err| err.to_string())?;
    let tmp_path = uv_path.with_extension("download");
    let decoder = GzDecoder::new(bytes.as_ref());
    let mut archive = Archive::new(decoder);
    let mut found = false;
    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path().map_err(|err| err.to_string())?;
        if path.file_name().and_then(|name| name.to_str()) == Some("uv") {
            let mut file = fs::File::create(&tmp_path).map_err(|err| err.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|err| err.to_string())?;
            found = true;
            break;
        }
    }
    if !found {
        let _ = fs::remove_file(&tmp_path);
        return Err("Downloaded uv archive did not contain a uv binary.".to_string());
    }
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&tmp_path)
            .map_err(|err| err.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp_path, permissions).map_err(|err| err.to_string())?;
    }
    fs::rename(&tmp_path, &uv_path).map_err(|err| err.to_string())?;
    Ok(uv_path)
}

fn uv_release_asset() -> Result<&'static str, String> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok("uv-x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("uv-aarch64-unknown-linux-gnu"),
        _ => Err("GPU runtime bootstrap currently supports Linux x86_64 and aarch64.".to_string()),
    }
}

fn run_runtime_step(app_dir: &Path, mut command: Command, context: &str) -> Result<(), String> {
    command
        .env("UV_CACHE_DIR", gpu_uv_cache_dir(app_dir))
        .env("UV_LINK_MODE", "copy")
        .env("UV_NO_PROGRESS", "0");
    let output = command
        .output()
        .map_err(|err| format!("{context}: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{context}: {}",
            compact_process_error(&output.stdout, &output.stderr)
        ))
    }
}

fn gpu_driver_status() -> (bool, String) {
    if let Ok(output) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,driver_version,memory.total",
            "--format=csv,noheader",
        ])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !text.is_empty() {
                return (
                    true,
                    text.lines()
                        .next()
                        .unwrap_or("NVIDIA GPU detected")
                        .to_string(),
                );
            }
        }
    }

    if let Ok(output) = Command::new("ldconfig").arg("-p").output() {
        let text = String::from_utf8_lossy(&output.stdout);
        if text.contains("libcuda.so") {
            return (true, "libcuda.so detected".to_string());
        }
    }

    for candidate in [
        "/usr/lib/libcuda.so.1",
        "/usr/lib64/libcuda.so.1",
        "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
    ] {
        if Path::new(candidate).exists() {
            return (true, format!("{candidate} detected"));
        }
    }

    (false, "No NVIDIA driver/libcuda detected".to_string())
}

fn command_path(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.components().count() > 1 && direct.is_file() {
        return Some(direct.to_path_buf());
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|path| path.join(name))
            .find(|path| path.is_file())
    })
}

fn platform_info(state: &AppState) -> PlatformInfo {
    let tools = [
        "wl-copy", "wtype", "ydotool", "xclip", "xsel", "xdotool", "xte",
    ]
    .into_iter()
    .filter(|tool| command_exists(tool))
    .map(str::to_string)
    .collect();
    PlatformInfo {
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
        session_type: env::var("XDG_SESSION_TYPE").ok(),
        wayland_display: env::var("WAYLAND_DISPLAY").is_ok(),
        x11_display: env::var("DISPLAY").is_ok(),
        paste_tools: tools,
        python: python_bin(),
        bundled_asr: state.crispasr_bin.exists(),
    }
}

fn command_exists(name: &str) -> bool {
    command_path(name).is_some()
}

fn low_priority_command(program: &Path) -> Command {
    if command_exists("nice") {
        let mut command = Command::new("nice");
        command.arg("-n").arg("10").arg(program);
        command
    } else {
        Command::new(program)
    }
}

fn copy_text_native(text: &str) -> PasteResult {
    if let Ok(mut clipboard) = Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_ok() {
            return PasteResult {
                copied: true,
                pasted: false,
                method: Some("arboard".to_string()),
                message: "Copied to clipboard.".to_string(),
                session_type: env::var("XDG_SESSION_TYPE").ok(),
            };
        }
    }

    let candidates: Vec<(&str, Vec<&str>)> = if is_wayland_session() {
        vec![("wl-copy", vec![])]
    } else {
        vec![
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ]
    };

    for (program, args) in candidates {
        if !command_exists(program) {
            continue;
        }
        if write_to_command(program, &args, text).is_ok() {
            return PasteResult {
                copied: true,
                pasted: false,
                method: Some(program.to_string()),
                message: "Copied to clipboard.".to_string(),
                session_type: env::var("XDG_SESSION_TYPE").ok(),
            };
        }
    }

    PasteResult {
        copied: false,
        pasted: false,
        method: None,
        message: "No usable clipboard backend found. Install wl-clipboard on Wayland or xclip/xsel on X11.".to_string(),
        session_type: env::var("XDG_SESSION_TYPE").ok(),
    }
}

fn paste_from_clipboard() -> Result<String, String> {
    let candidates: Vec<(&str, Vec<&str>)> = if is_wayland_session() {
        vec![
            (
                "wtype",
                vec!["-M", "shift", "-P", "insert", "-p", "insert", "-m", "shift"],
            ),
            ("ydotool", vec!["key", "42:1", "110:1", "110:0", "42:0"]),
        ]
    } else {
        vec![
            ("xdotool", vec!["key", "--clearmodifiers", "ctrl+v"]),
            ("xte", vec!["keydown Control_L", "key v", "keyup Control_L"]),
        ]
    };

    let mut missing = Vec::new();
    let mut failures = Vec::new();
    for (program, args) in candidates {
        if !command_exists(program) {
            missing.push(program);
            continue;
        }
        match Command::new(program).args(args).output() {
            Ok(output) if output.status.success() => return Ok(program.to_string()),
            Ok(output) => failures.push(format!(
                "{program}: {}",
                compact_process_error(&output.stdout, &output.stderr)
            )),
            Err(err) => failures.push(format!("{program}: {err}")),
        }
    }

    if failures.is_empty() {
        Err(format!("missing paste tool; tried {}", missing.join(", ")))
    } else {
        Err(failures.join("; "))
    }
}

fn is_wayland_session() -> bool {
    matches!(env::var("XDG_SESSION_TYPE").as_deref(), Ok("wayland"))
        || env::var("WAYLAND_DISPLAY").is_ok()
}

fn read_clipboard_text() -> Result<String, String> {
    if let Ok(mut clipboard) = Clipboard::new() {
        if let Ok(text) = clipboard.get_text() {
            return Ok(text);
        }
    }

    let candidates: Vec<(&str, Vec<&str>)> = if is_wayland_session() {
        vec![("wl-paste", vec!["--no-newline"])]
    } else {
        vec![
            ("xclip", vec!["-o", "-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--output"]),
        ]
    };

    for (program, args) in candidates {
        if !command_exists(program) {
            continue;
        }
        match Command::new(program).args(args).output() {
            Ok(output) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            _ => {}
        }
    }
    Err("No readable clipboard backend found.".to_string())
}

fn restore_clipboard_if_unchanged(payload: &str, previous: Option<String>) {
    let Some(previous) = previous else {
        return;
    };
    thread::sleep(Duration::from_millis(300));
    if matches!(read_clipboard_text(), Ok(current) if current == payload) {
        let _ = copy_text_native(&previous);
    }
}

fn write_to_command(program: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "failed to open command stdin".to_string())?
        .write_all(text.as_bytes())
        .map_err(|err| err.to_string())?;
    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

fn compact_process_error(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    if message.len() > 1800 {
        format!("{}...", &message[..1800])
    } else if message.is_empty() {
        "process failed without output".to_string()
    } else {
        message
    }
}

fn dir_size(path: &Path) -> Result<u64, std::io::Error> {
    let mut size = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                size += dir_size(&entry.path())?;
            } else {
                size += meta.len();
            }
        }
    }
    Ok(size)
}
