pub mod hotkey;
pub mod llm;
mod panel;

/// Test hook for `examples/ptt_probe.rs`: start a listener and hand back the
/// resolved backend so the fallback chain can be exercised without the GUI.
pub fn hotkey_start_for_probe<F>(
    trigger: &str,
    on_edge: F,
) -> (hotkey::HotkeyBackend, String, String)
where
    F: Fn(hotkey::PttEdge) + Send + Sync + 'static,
{
    let listener = hotkey::start(trigger, on_edge);
    let status = listener.status.clone();
    // Leak deliberately: the probe process exits when it is done listening.
    std::mem::forget(listener);
    (status.backend, status.trigger, status.detail)
}

use arboard::Clipboard;
use base64::{engine::general_purpose, Engine as _};
use chrono::Local;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

// Official QwenAudio/Fun-ASR llama.cpp runtime binaries (release runtime-llamacpp-v0.1.9).
// `llama-funasr-cli` drives Fun-ASR-Nano (SAN-M encoder + Qwen3-0.6B decoder);
// `llama-funasr-sensevoice` drives the encoder-only SenseVoiceSmall checkpoint.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const FUNASR_CLI_BIN_NAME: &str = "llama-funasr-cli-x86_64-unknown-linux-gnu";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FUNASR_CLI_BIN_NAME: &str = "llama-funasr-cli-aarch64-apple-darwin";

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
const FUNASR_CLI_BIN_NAME: &str = "llama-funasr-cli-unsupported-platform";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const FUNASR_SENSEVOICE_BIN_NAME: &str = "llama-funasr-sensevoice-x86_64-unknown-linux-gnu";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FUNASR_SENSEVOICE_BIN_NAME: &str = "llama-funasr-sensevoice-aarch64-apple-darwin";

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
const FUNASR_SENSEVOICE_BIN_NAME: &str = "llama-funasr-sensevoice-unsupported-platform";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const FUNASR_VAD_BIN_NAME: &str = "llama-funasr-vad-x86_64-unknown-linux-gnu";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FUNASR_VAD_BIN_NAME: &str = "llama-funasr-vad-aarch64-apple-darwin";

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
const FUNASR_VAD_BIN_NAME: &str = "llama-funasr-vad-unsupported-platform";

/// Backend id for Fun-ASR-Nano on the official llama.cpp runtime.
const BACKEND_NANO: &str = "funasr-nano-gguf-cpu";
/// Backend id for SenseVoiceSmall on the official llama.cpp runtime.
const BACKEND_SENSEVOICE: &str = "funasr-sensevoice-gguf-cpu";

/// One file that has to be present before a GGUF model can run.
struct GgufAsset {
    repo_id: &'static str,
    filename: &'static str,
}

/// Fun-ASR-Nano needs the audio encoder, the Qwen3 decoder, and the shared VAD.
/// q4km is the default: measured on this project it is both faster and no less
/// accurate than q8_0 (8.8x vs 7.8x realtime on a 30 s clip).
const NANO_ASSETS: &[GgufAsset] = &[
    GgufAsset {
        repo_id: "FunAudioLLM/Fun-ASR-Nano-GGUF",
        filename: "funasr-encoder-f16.gguf",
    },
    GgufAsset {
        repo_id: "FunAudioLLM/Fun-ASR-Nano-GGUF",
        filename: "qwen3-0.6b-q4km.gguf",
    },
    GgufAsset {
        repo_id: "FunAudioLLM/fsmn-vad-GGUF",
        filename: "fsmn-vad.gguf",
    },
];

/// SenseVoiceSmall is a single encoder+CTC file, plus the same shared VAD.
const SENSEVOICE_ASSETS: &[GgufAsset] = &[
    GgufAsset {
        repo_id: "FunAudioLLM/SenseVoiceSmall-GGUF",
        filename: "sensevoice-small-q8.gguf",
    },
    GgufAsset {
        repo_id: "FunAudioLLM/fsmn-vad-GGUF",
        filename: "fsmn-vad.gguf",
    },
];

fn gguf_assets_for(backend: &str) -> Option<&'static [GgufAsset]> {
    match backend {
        BACKEND_NANO => Some(NANO_ASSETS),
        BACKEND_SENSEVOICE => Some(SENSEVOICE_ASSETS),
        _ => None,
    }
}

struct AppState {
    db: Mutex<Connection>,
    app_dir: PathBuf,
    funasr_cli_bin: PathBuf,
    funasr_sensevoice_bin: PathBuf,
    funasr_vad_bin: PathBuf,
    downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    audio_capture: Mutex<Option<AudioCaptureHandle>>,
    streaming: Mutex<Option<StreamingHandle>>,
    push_to_talk: Mutex<Option<hotkey::HotkeyListener>>,
}

/// A running streaming-transcription worker. Dropping it stops the worker.
struct StreamingHandle {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for StreamingHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
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
    formatted_text: Option<String>,
    formatted_preset: Option<String>,
    formatted_at: Option<String>,
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
}

struct AsrJob {
    session_id: String,
    model_id: String,
    model: ModelInfo,
    audio_path: PathBuf,
    language: String,
    save_final: bool,
    retain_audio: bool,
    funasr_cli_bin: PathBuf,
    funasr_sensevoice_bin: PathBuf,
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

// ---------------------------------------------------------------------------
// Streaming transcription
//
// The official CLIs are one-shot: they load the model, transcribe a file, and
// exit. Load costs ~0.17 s, and inference runs at 8.8x realtime (Nano) or 20.8x
// (SenseVoice) on CPU, so re-spawning per update is cheap enough that no
// resident server process is needed.
//
// Two tiers, because they have different strengths:
//   - while you are still speaking, SenseVoice re-transcribes the in-progress
//     segment for a live preview (fast, occasionally wrong);
//   - once the segment ends, the model you actually selected re-runs it to
//     produce the committed text (accurate).
//
// Text accumulates per segment, so nothing scrolls out of the preview the way
// it did with the old fixed rolling window.
//
// Everything this module emits is a preview. Segmentation costs accuracy, so
// the caller re-transcribes the full recording on stop for the real text.
// ---------------------------------------------------------------------------

/// How often the worker wakes to re-run VAD over the uncommitted audio.
const STREAM_POLL_MS: u64 = 250;
/// A segment is committed once VAD reports this much audio after its end.
/// Without the margin, a brief mid-sentence pause would commit early.
const SEGMENT_TAIL_SILENCE_MS: u64 = 500;
/// Segments shorter than this are treated as noise and dropped.
const VAD_MIN_SPEECH_MS: u64 = 320;
/// How often the in-progress segment is re-transcribed for the live preview.
const PARTIAL_REFRESH_MS: u64 = 900;
/// Commit an in-progress span once it reaches this length, even though VAD has
/// not seen a pause yet.
///
/// Preview cost grows with span length (~0.17 s startup + length/20.8 for
/// SenseVoice), so an unbroken monologue would eventually outrun
/// `PARTIAL_REFRESH_MS` and stop feeling live — and nothing would be committed
/// for as long as the speaker kept going. Capping at 12 s keeps a preview pass
/// near 0.75 s and guarantees text lands on screen regularly.
const FORCE_COMMIT_MS: u64 = 12_000;

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum StreamingEvent {
    /// Live, still-changing text for the segment currently being spoken.
    Partial { segment_index: usize, text: String },
    /// Text for a segment that has stopped changing.
    ///
    /// Still a preview, not a final answer. Segment boundaries — especially the
    /// forced 12 s break — can cut mid-sentence, and the models lose accuracy on
    /// short isolated spans. Measured on this project's test clip: transcribing
    /// the whole 30 s at once yields "adding noise in two different parts, one
    /// in the uh boundary line", while the 2.8 s tail segment alone decodes as
    /// "All right, do I need a laundry bin now?".
    ///
    /// Callers must re-transcribe the complete recording on stop and treat that
    /// as the authoritative text.
    Segment {
        segment_index: usize,
        text: String,
        start_ms: u64,
        end_ms: u64,
    },
    Error { error: String },
}

/// Everything the worker thread needs, resolved once at start so it never
/// touches the database or Tauri state while running.
struct StreamingJob {
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    final_model: ModelInfo,
    final_bin: PathBuf,
    /// SenseVoice, when installed, for the low-latency preview pass.
    preview: Option<(ModelInfo, PathBuf)>,
    vad_bin: PathBuf,
    vad_gguf: PathBuf,
    scratch_dir: PathBuf,
    stop: Arc<AtomicBool>,
}

/// Write samples to a temporary 16 kHz mono WAV and transcribe it.
fn transcribe_samples(
    model: &ModelInfo,
    bin: &Path,
    samples: &[f32],
    sample_rate: u32,
    scratch: &Path,
) -> Result<String, String> {
    let resampled = resample_linear(samples, sample_rate, 16_000);
    let wav = encode_wav_i16(&resampled, 16_000)?;
    let path = scratch.join(format!("stream-{}.wav", Uuid::new_v4()));
    fs::write(&path, wav).map_err(|err| err.to_string())?;

    let result = match model.backend.as_str() {
        BACKEND_NANO => transcribe_with_funasr_nano(bin, model, &path),
        BACKEND_SENSEVOICE => transcribe_with_sensevoice(bin, model, &path),
        other => Err(format!("Unknown ASR backend '{other}'")),
    };
    let _ = fs::remove_file(&path);
    result.map(|(text, _)| text)
}

fn samples_for_ms(sample_rate: u32, ms: u64) -> usize {
    ((u64::from(sample_rate) * ms) / 1000) as usize
}

fn ms_for_samples(sample_rate: u32, count: usize) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    (count as u64 * 1000) / u64::from(sample_rate)
}

/// Run the official ggml FSMN-VAD over a WAV and return `(start_ms, end_ms)`
/// speech spans.
///
/// A trained VAD is used rather than an energy threshold on purpose: real
/// recordings sit on a noise floor that swamps any fixed cutoff. On a sample of
/// this project's own test audio the quietest 20 ms frame measured RMS 0.030 —
/// above every plausible threshold — so an energy gate marked 100% of the clip
/// as speech, while FSMN-VAD correctly found the trailing 3 s of silence.
/// It is also cheap: ~14 ms for 3 s of audio, ~95 ms for 30 s.
fn run_vad(vad_bin: &Path, vad_gguf: &Path, wav_path: &Path) -> Result<Vec<(u64, u64)>, String> {
    if !vad_bin.exists() {
        return Err("Bundled FSMN-VAD runtime is missing from the app resources.".to_string());
    }
    let output = low_priority_command(vad_bin)
        .arg("-m")
        .arg(vad_gguf)
        .arg("-a")
        .arg(wav_path)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(compact_process_error(&output.stdout, &output.stderr));
    }

    let mut spans = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        if let (Some(start), Some(end)) = (parts.next(), parts.next()) {
            if let (Ok(start), Ok(end)) = (start.parse::<u64>(), end.parse::<u64>()) {
                if end > start {
                    spans.push((start, end));
                }
            }
        }
    }
    Ok(spans)
}

fn run_streaming_worker(job: StreamingJob, on_event: Channel<StreamingEvent>) {
    // Absolute sample index. Everything before it has been committed.
    let mut cursor = 0_usize;
    let mut segment_index = 0_usize;
    let mut last_partial = Instant::now() - Duration::from_millis(PARTIAL_REFRESH_MS);
    let mut last_partial_text = String::new();
    let min_pending = samples_for_ms(job.sample_rate, VAD_MIN_SPEECH_MS);

    while !job.stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(STREAM_POLL_MS));

        let samples = match job.samples.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => break,
        };
        if samples.len().saturating_sub(cursor) < min_pending {
            continue;
        }
        let pending = &samples[cursor..];
        let pending_ms = ms_for_samples(job.sample_rate, pending.len());

        // VAD wants a file, so stage the uncommitted audio once and reuse it
        // for both the VAD pass and any transcription below.
        let resampled = resample_linear(pending, job.sample_rate, 16_000);
        let staged = job.scratch_dir.join(format!("pending-{}.wav", Uuid::new_v4()));
        let write_ok = encode_wav_i16(&resampled, 16_000)
            .and_then(|wav| fs::write(&staged, wav).map_err(|err| err.to_string()));
        if let Err(err) = write_ok {
            let _ = on_event.send(StreamingEvent::Error { error: err });
            let _ = fs::remove_file(&staged);
            continue;
        }

        let spans = match run_vad(&job.vad_bin, &job.vad_gguf, &staged) {
            Ok(spans) => spans,
            Err(err) => {
                let _ = on_event.send(StreamingEvent::Error { error: err });
                let _ = fs::remove_file(&staged);
                continue;
            }
        };
        let _ = fs::remove_file(&staged);

        let Some(&(first_start_ms, first_end_ms)) = spans.first() else {
            // Nothing but silence. Drop all but a small tail so the buffer we
            // re-scan every tick does not grow without bound.
            let keep = samples_for_ms(job.sample_rate, SEGMENT_TAIL_SILENCE_MS);
            cursor = samples.len().saturating_sub(keep);
            last_partial_text.clear();
            continue;
        };

        // The first span is committable once enough audio has arrived after it
        // for the silence to be real rather than the recording simply ending.
        let settled = pending_ms.saturating_sub(first_end_ms) >= SEGMENT_TAIL_SILENCE_MS;
        // Or once it has run long enough that waiting for a pause would stall
        // both the preview and any committed output.
        let overlong = first_end_ms.saturating_sub(first_start_ms) >= FORCE_COMMIT_MS;

        if settled || overlong {
            let start = samples_for_ms(job.sample_rate, first_start_ms);
            let end_ms = if settled {
                first_end_ms
            } else {
                first_start_ms + FORCE_COMMIT_MS
            };
            let end = samples_for_ms(job.sample_rate, end_ms).min(pending.len());
            if end > start && ms_for_samples(job.sample_rate, end - start) >= VAD_MIN_SPEECH_MS {
                let abs_start_ms = ms_for_samples(job.sample_rate, cursor + start);
                let abs_end_ms = ms_for_samples(job.sample_rate, cursor + end);
                match transcribe_samples(
                    &job.final_model,
                    &job.final_bin,
                    &pending[start..end],
                    job.sample_rate,
                    &job.scratch_dir,
                ) {
                    Ok(text) if !text.trim().is_empty() => {
                        let _ = on_event.send(StreamingEvent::Segment {
                            segment_index,
                            text,
                            start_ms: abs_start_ms,
                            end_ms: abs_end_ms,
                        });
                        segment_index += 1;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        let _ = on_event.send(StreamingEvent::Error { error: err });
                    }
                }
            }
            cursor += end;
            last_partial_text.clear();
            continue;
        }

        // Still speaking: refresh the live preview for the in-progress span.
        if last_partial.elapsed() < Duration::from_millis(PARTIAL_REFRESH_MS) {
            continue;
        }
        let start = samples_for_ms(job.sample_rate, first_start_ms);
        if pending.len().saturating_sub(start) < min_pending {
            continue;
        }
        let (model, bin) = job
            .preview
            .as_ref()
            .map(|(model, bin)| (model, bin.as_path()))
            .unwrap_or((&job.final_model, job.final_bin.as_path()));
        last_partial = Instant::now();
        match transcribe_samples(
            model,
            bin,
            &pending[start..],
            job.sample_rate,
            &job.scratch_dir,
        ) {
            // Only emit on change; consecutive preview passes often agree.
            Ok(text) if !text.trim().is_empty() && text != last_partial_text => {
                last_partial_text = text.clone();
                let _ = on_event.send(StreamingEvent::Partial {
                    segment_index,
                    text,
                });
            }
            Ok(_) => {}
            Err(err) => {
                let _ = on_event.send(StreamingEvent::Error { error: err });
            }
        }
    }
}

#[tauri::command]
fn start_streaming_transcription(
    state: tauri::State<'_, AppState>,
    model_id: String,
    on_event: Channel<StreamingEvent>,
) -> Result<(), String> {
    let final_model = get_model(&state, &model_id)?;
    // Every GGUF model ships the shared FSMN-VAD alongside it.
    let vad_gguf = gguf_model_dir(&final_model)?.join("fsmn-vad.gguf");
    let final_bin = match final_model.backend.as_str() {
        BACKEND_NANO => state.funasr_cli_bin.clone(),
        BACKEND_SENSEVOICE => state.funasr_sensevoice_bin.clone(),
        other => return Err(format!("Unknown ASR backend '{other}'")),
    };

    // SenseVoice drives the live preview when it is installed and is not
    // already the selected model. If it is missing, previews fall back to the
    // selected model, which is slower but still correct.
    let preview = if final_model.backend == BACKEND_SENSEVOICE {
        None
    } else {
        get_model(&state, "sensevoice-small")
            .ok()
            .filter(|model| gguf_model_dir(model).is_ok())
            .map(|model| (model, state.funasr_sensevoice_bin.clone()))
    };

    let (samples, sample_rate) = {
        let capture = state
            .audio_capture
            .lock()
            .map_err(|_| "Audio capture lock poisoned".to_string())?;
        let capture = capture
            .as_ref()
            .ok_or_else(|| "Start the microphone before streaming.".to_string())?;
        (Arc::clone(&capture.samples), capture.sample_rate)
    };

    let scratch_dir = state.app_dir.join("audio").join("streaming");
    fs::create_dir_all(&scratch_dir).map_err(|err| err.to_string())?;

    let stop = Arc::new(AtomicBool::new(false));
    let job = StreamingJob {
        samples,
        sample_rate,
        final_model,
        final_bin,
        preview,
        vad_bin: state.funasr_vad_bin.clone(),
        vad_gguf,
        scratch_dir,
        stop: Arc::clone(&stop),
    };
    let join = thread::spawn(move || run_streaming_worker(job, on_event));

    let mut streaming = state
        .streaming
        .lock()
        .map_err(|_| "Streaming lock poisoned".to_string())?;
    // Dropping any previous handle stops that worker first.
    *streaming = Some(StreamingHandle {
        stop,
        join: Some(join),
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// LLM formatting
//
// The second layer of the two-layer flow: the raw transcript lands first and
// is never modified, then this produces a Markdown-typeset version alongside
// it. Both are stored, so the user can always fall back to their own words.
// ---------------------------------------------------------------------------

/// Service name under which the API key is stored in the OS credential store.
const LLM_KEYRING_SERVICE: &str = "dev.yubo.fun-asr-desktop";
const LLM_KEYRING_USER: &str = "llm-api-key";

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum FormatEvent {
    Delta { text: String },
    Done { text: String },
    Error { error: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PresetInfo {
    id: String,
    label: String,
    description: String,
    prompt: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LlmSettings {
    base_url: String,
    model: String,
    /// Never the key itself — only whether one is stored.
    has_api_key: bool,
    preset: String,
    auto_format: bool,
    presets: Vec<PresetInfo>,
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(LLM_KEYRING_SERVICE, LLM_KEYRING_USER).map_err(|err| err.to_string())
}

fn stored_api_key() -> Option<String> {
    keyring_entry()
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .filter(|key| !key.trim().is_empty())
}

#[tauri::command]
fn get_llm_settings(state: tauri::State<'_, AppState>) -> Result<LlmSettings, String> {
    let preset = setting_value(&state, "llm.preset")?
        .unwrap_or_else(|| llm::presets::DEFAULT_ID.to_string());
    Ok(LlmSettings {
        base_url: setting_value(&state, "llm.baseUrl")?.unwrap_or_default(),
        model: setting_value(&state, "llm.model")?.unwrap_or_default(),
        has_api_key: stored_api_key().is_some(),
        preset,
        auto_format: setting_bool(&state, "llm.autoFormat").unwrap_or(false),
        presets: llm::presets::ALL
            .iter()
            .map(|preset| PresetInfo {
                id: preset.id.to_string(),
                label: preset.label.to_string(),
                description: preset.description.to_string(),
                // The stored override, when the user has edited this preset.
                prompt: setting_value(&state, &format!("llm.prompt.{}", preset.id))
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| preset.prompt.to_string()),
            })
            .collect(),
    })
}

/// Store or clear the API key. Passing `None` removes it.
///
/// The key goes to the OS credential store rather than the settings table:
/// the SQLite file sits in app data in the clear, and a leaked transcript
/// database should not also leak the user's API credentials.
#[tauri::command]
fn set_llm_api_key(key: Option<String>) -> Result<(), String> {
    let entry = keyring_entry()?;
    match key.filter(|value| !value.trim().is_empty()) {
        Some(value) => entry
            .set_password(value.trim())
            .map_err(|err| format!("Could not save the API key: {err}")),
        None => match entry.delete_credential() {
            Ok(()) => Ok(()),
            // Clearing an already-absent key is success, not an error.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("Could not clear the API key: {err}")),
        },
    }
}

fn resolve_prompt(state: &AppState, preset_id: &str) -> Result<String, String> {
    let preset = llm::presets::by_id(preset_id)
        .ok_or_else(|| format!("Unknown formatting preset '{preset_id}'."))?;
    Ok(
        setting_value(state, &format!("llm.prompt.{}", preset.id))?
            .filter(|prompt| !prompt.trim().is_empty())
            .unwrap_or_else(|| preset.prompt.to_string()),
    )
}

#[tauri::command]
async fn format_transcript(
    state: tauri::State<'_, AppState>,
    transcript_id: Option<String>,
    text: String,
    preset: Option<String>,
    on_event: Channel<FormatEvent>,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("Nothing to format.".to_string());
    }
    let preset_id = preset.unwrap_or(
        setting_value(&state, "llm.preset")?
            .unwrap_or_else(|| llm::presets::DEFAULT_ID.to_string()),
    );
    let config = llm::LlmConfig {
        base_url: setting_value(&state, "llm.baseUrl")?.unwrap_or_default(),
        model: setting_value(&state, "llm.model")?.unwrap_or_default(),
        api_key: stored_api_key().unwrap_or_default(),
        temperature: None,
    };
    let prompt = resolve_prompt(&state, &preset_id)?;

    let events = on_event.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        llm::format_streaming(
            &config,
            &prompt,
            &text,
            |delta| {
                let _ = events.send(FormatEvent::Delta {
                    text: delta.to_string(),
                });
            },
            || false,
        )
    })
    .await
    .map_err(|err| format!("Formatting worker failed to join: {err}"))?;

    match result {
        Ok(formatted) => {
            if let Some(id) = transcript_id {
                let conn = state
                    .db
                    .lock()
                    .map_err(|_| "Database lock poisoned".to_string())?;
                conn.execute(
                    "UPDATE transcripts
                       SET formatted_text = ?1, formatted_preset = ?2, formatted_at = ?3
                     WHERE id = ?4",
                    params![formatted, preset_id, Local::now().to_rfc3339(), id],
                )
                .map_err(|err| err.to_string())?;
            }
            let _ = on_event.send(FormatEvent::Done {
                text: formatted.clone(),
            });
            Ok(formatted)
        }
        Err(err) => {
            let _ = on_event.send(FormatEvent::Error { error: err.clone() });
            Err(err)
        }
    }
}

// ---------------------------------------------------------------------------
// Push-to-talk
// ---------------------------------------------------------------------------

/// Default chord. Ctrl+Alt+Space avoids the desktop's own bindings and is easy
/// to hold with one hand.
const DEFAULT_PTT_TRIGGER: &str = "CTRL+ALT+space";

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum PushToTalkEvent {
    Pressed,
    Released,
}

#[tauri::command]
fn start_push_to_talk(
    state: tauri::State<'_, AppState>,
    trigger: Option<String>,
    on_event: Channel<PushToTalkEvent>,
) -> Result<hotkey::HotkeyStatus, String> {
    let trigger = trigger
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PTT_TRIGGER.to_string());

    let listener = hotkey::start(&trigger, move |edge| {
        let _ = on_event.send(match edge {
            hotkey::PttEdge::Pressed => PushToTalkEvent::Pressed,
            hotkey::PttEdge::Released => PushToTalkEvent::Released,
        });
    });
    let status = listener.status.clone();

    let mut slot = state
        .push_to_talk
        .lock()
        .map_err(|_| "Push-to-talk lock poisoned".to_string())?;
    // Replacing the previous listener drops it, releasing the old binding.
    *slot = Some(listener);
    Ok(status)
}

#[tauri::command]
fn stop_push_to_talk(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut slot = state
        .push_to_talk
        .lock()
        .map_err(|_| "Push-to-talk lock poisoned".to_string())?;
    slot.take();
    Ok(())
}

#[tauri::command]
fn stop_streaming_transcription(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut streaming = state
        .streaming
        .lock()
        .map_err(|_| "Streaming lock poisoned".to_string())?;
    streaming.take();
    Ok(())
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

fn send_download_event(channel: &Channel<ModelDownloadEvent>, event: ModelDownloadEvent) {
    let _ = channel.send(event);
}

fn asset_url(asset: &GgufAsset) -> String {
    format!(
        "https://huggingface.co/{}/resolve/main/{}",
        asset.repo_id, asset.filename
    )
}

/// Ask Hugging Face how big an asset is so the UI can show a real total across
/// all files before the first byte lands. A failure here is not fatal: the
/// download still works, the progress bar just runs without a known total.
fn probe_asset_size(client: &reqwest::blocking::Client, url: &str) -> Option<u64> {
    let response = client.head(url).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| response.content_length())
}

/// Fetch one asset into `dir`, resuming a partial `.download` file when possible.
/// `already_done` is the number of bytes counted for previously finished assets;
/// progress events are reported against the whole model, not this single file.
fn download_one_asset(
    client: &reqwest::blocking::Client,
    model_id: &str,
    asset: &GgufAsset,
    dir: &Path,
    already_done: u64,
    total_bytes: Option<u64>,
    cancel: &AtomicBool,
    on_event: &Channel<ModelDownloadEvent>,
) -> Result<DownloadResult, String> {
    let destination = dir.join(asset.filename);
    let url = asset_url(asset);
    let tmp_path = destination.with_extension("gguf.download");
    let mut existing_bytes = fs::metadata(&tmp_path).map(|meta| meta.len()).unwrap_or(0);

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

    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(existing_bytes > 0)
        .truncate(existing_bytes == 0)
        .open(&tmp_path)
        .map_err(|err| err.to_string())?;

    let mut file_bytes = existing_bytes;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            file.flush().map_err(|err| err.to_string())?;
            send_download_event(
                on_event,
                ModelDownloadEvent::Paused {
                    model_id: model_id.to_string(),
                    downloaded_bytes: already_done + file_bytes,
                    total_bytes,
                },
            );
            return Ok(DownloadResult::Paused {
                downloaded_bytes: already_done + file_bytes,
            });
        }

        let read = response.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }

        file.write_all(&buffer[..read])
            .map_err(|err| err.to_string())?;
        file_bytes += read as u64;
        send_download_event(
            on_event,
            ModelDownloadEvent::Progress {
                model_id: model_id.to_string(),
                chunk_bytes: read as u64,
                downloaded_bytes: already_done + file_bytes,
                total_bytes,
            },
        );
    }

    file.flush().map_err(|err| err.to_string())?;
    fs::rename(&tmp_path, &destination).map_err(|err| err.to_string())?;
    Ok(DownloadResult::Installed(file_bytes))
}

/// Download every GGUF file a model needs into its directory.
///
/// Unlike the previous single-file runtime, the official Fun-ASR-Nano CLI needs
/// three separate files (audio encoder, Qwen3 decoder, shared FSMN-VAD) pulled
/// from two different Hugging Face repos, so `local_path` is a directory.
fn download_gguf_model(
    model: &ModelInfo,
    cancel: &AtomicBool,
    on_event: &Channel<ModelDownloadEvent>,
) -> Result<DownloadResult, String> {
    let assets = gguf_assets_for(&model.backend)
        .ok_or_else(|| format!("No GGUF asset list for backend {}", model.backend))?;
    let dir = Path::new(&model.local_path);
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;

    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|err| err.to_string())?;

    // Size every asset up front so the progress bar has a real denominator.
    // Files already on disk at full size count as done and are skipped below.
    let mut sizes: Vec<Option<u64>> = Vec::with_capacity(assets.len());
    for asset in assets {
        sizes.push(probe_asset_size(&client, &asset_url(asset)));
    }
    let total_bytes = if sizes.iter().all(|size| size.is_some()) {
        Some(sizes.iter().map(|size| size.unwrap_or(0)).sum())
    } else {
        None
    };

    let mut done_bytes = 0_u64;
    for (index, asset) in assets.iter().enumerate() {
        let destination = dir.join(asset.filename);
        if let (Ok(meta), Some(expected)) = (fs::metadata(&destination), sizes[index]) {
            if meta.len() == expected {
                done_bytes += expected;
                continue;
            }
        }

        send_download_event(
            on_event,
            ModelDownloadEvent::Started {
                model_id: model.id.clone(),
                downloaded_bytes: done_bytes,
                total_bytes,
            },
        );

        match download_one_asset(
            &client,
            &model.id,
            asset,
            dir,
            done_bytes,
            total_bytes,
            cancel,
            on_event,
        )? {
            DownloadResult::Installed(bytes) => done_bytes += bytes,
            paused @ DownloadResult::Paused { .. } => return Ok(paused),
        }
    }

    Ok(DownloadResult::Installed(done_bytes))
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

    let result = if gguf_assets_for(&model.backend).is_some() {
        download_gguf_model(&model, &cancel, &on_event)
    } else {
        Err(format!("Unknown model backend: {}", model.backend))
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

/// Check that every GGUF file a backend needs is on disk, and return the model dir.
fn gguf_model_dir(model: &ModelInfo) -> Result<PathBuf, String> {
    let assets = gguf_assets_for(&model.backend)
        .ok_or_else(|| format!("No GGUF asset list for backend {}", model.backend))?;
    let dir = PathBuf::from(&model.local_path);
    for asset in assets {
        if !dir.join(asset.filename).exists() {
            return Err(format!(
                "{} is missing from {}. Download the model again from the welcome or settings screen.",
                asset.filename,
                dir.display()
            ));
        }
    }
    Ok(dir)
}

/// Both official runtimes keep stdout clean: every log, timing and VAD line goes
/// to stderr, and stdout carries only transcript text (verified against
/// runtime-llamacpp-v0.1.9). One line per VAD segment, so join them.
///
/// Deliberately no content-based filtering here — a transcript may legitimately
/// begin with any character, and dropping lines by prefix would silently eat it.
fn clean_runtime_stdout(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fun-ASR-Nano through the official `llama-funasr-cli`: SAN-M encoder GGUF +
/// Qwen3-0.6B decoder GGUF, with the built-in ggml FSMN-VAD doing segmentation.
///
/// The official CLI has no `--language` flag — Nano detects language itself.
fn transcribe_with_funasr_nano(
    cli_bin: &Path,
    model: &ModelInfo,
    audio_path: &Path,
) -> Result<(String, String), String> {
    let dir = gguf_model_dir(model)?;
    if !cli_bin.exists() {
        return Err("Bundled Fun-ASR runtime is missing from the app resources.".to_string());
    }

    let mut command = low_priority_command(cli_bin);
    let output = command
        .arg("--enc")
        .arg(dir.join("funasr-encoder-f16.gguf"))
        .arg("-m")
        .arg(dir.join("qwen3-0.6b-q4km.gguf"))
        .arg("-a")
        .arg(audio_path)
        .arg("--vad")
        .arg(dir.join("fsmn-vad.gguf"))
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        Ok((clean_runtime_stdout(&output.stdout), BACKEND_NANO.to_string()))
    } else {
        Err(compact_process_error(&output.stdout, &output.stderr))
    }
}

/// SenseVoiceSmall through the official `llama-funasr-sensevoice`: a single
/// encoder+CTC pass, ~20x realtime on CPU. Faster than Nano but weaker on
/// English, so it is the explicit "go fast" choice rather than the default.
fn transcribe_with_sensevoice(
    sensevoice_bin: &Path,
    model: &ModelInfo,
    audio_path: &Path,
) -> Result<(String, String), String> {
    let dir = gguf_model_dir(model)?;
    if !sensevoice_bin.exists() {
        return Err("Bundled SenseVoice runtime is missing from the app resources.".to_string());
    }

    let mut command = low_priority_command(sensevoice_bin);
    let output = command
        .arg("-m")
        .arg(dir.join("sensevoice-small-q8.gguf"))
        .arg("-a")
        .arg(audio_path)
        .arg("--vad")
        .arg(dir.join("fsmn-vad.gguf"))
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        Ok((
            clean_runtime_stdout(&output.stdout),
            BACKEND_SENSEVOICE.to_string(),
        ))
    } else {
        Err(compact_process_error(&output.stdout, &output.stderr))
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
                    BACKEND_NANO,
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
        save_final,
        retain_audio: setting_bool(state, "audio.retain").unwrap_or(false),
        funasr_cli_bin: state.funasr_cli_bin.clone(),
        funasr_sensevoice_bin: state.funasr_sensevoice_bin.clone(),
    })
}

fn run_asr_job(job: AsrJob) -> AsrJobOutput {
    let transcription = match job.model.backend.as_str() {
        BACKEND_NANO => {
            transcribe_with_funasr_nano(&job.funasr_cli_bin, &job.model, &job.audio_path)
        }
        BACKEND_SENSEVOICE => {
            transcribe_with_sensevoice(&job.funasr_sensevoice_bin, &job.model, &job.audio_path)
        }
        other => Err(format!(
            "Unknown ASR backend '{other}'. Pick Fun-ASR-Nano or SenseVoiceSmall in settings."
        )),
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

    // Deliberately NOT restoring the previous clipboard afterwards.
    //
    // Restoring is the espanso pattern, and it is right for a text expander
    // that should not disturb what the user had copied. It is wrong here: the
    // transcript *is* the thing the user just produced, and they expect to be
    // able to paste it again later. Restoring made every transcription look
    // like the clipboard silently failed.
    let _ = previous_clipboard;

    thread::sleep(Duration::from_millis(300));
    match paste_from_clipboard() {
        Ok(method) => PasteResult {
            copied: true,
            pasted: true,
            method: Some(method),
            message: "Copied and pasted.".to_string(),
            session_type: env::var("XDG_SESSION_TYPE").ok(),
        },
        Err(err) => PasteResult {
            copied: true,
            pasted: false,
            method: None,
            message: format!("Copied to clipboard. Auto-paste unavailable: {err}"),
            session_type: env::var("XDG_SESSION_TYPE").ok(),
        },
    }
}

#[tauri::command]
fn show_voice_bar(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    window.show().map_err(|err| err.to_string())?;
    window.set_focus().map_err(|err| err.to_string())
}

/// Show the voice bar without taking focus.
///
/// Push-to-talk fires while another application is focused, and the whole
/// point is to paste back into that application afterwards. Calling
/// `set_focus()` here — as `show_voice_bar` does for the manual case — would
/// steal focus mid-utterance and break the paste target.
#[tauri::command]
fn show_voice_bar_passive(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    window.show().map_err(|err| err.to_string())
}

/// Snap the voice bar to whichever screen edge it currently sits nearest.
///
/// Deliberately computed in Rust from the window's own position and its
/// current monitor. Doing it in the webview used `window.screen`, which
/// reports only the primary display — on a multi-monitor desk every drag
/// resolved to the same corner regardless of where the pill actually was.
#[tauri::command]
fn snap_voice_bar(app: AppHandle, margin: Option<i32>) -> Result<String, String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|err| err.to_string())?
        .or(window.primary_monitor().map_err(|err| err.to_string())?)
        .ok_or_else(|| "No monitor found for the voice bar.".to_string())?;

    let screen = monitor.size();
    let origin = monitor.position();
    let position = window.outer_position().map_err(|err| err.to_string())?;
    let size = window.outer_size().map_err(|err| err.to_string())?;

    // Centre of the pill, relative to the monitor it is on.
    let cx = (position.x - origin.x) as f64 + size.width as f64 / 2.0;
    let cy = (position.y - origin.y) as f64 + size.height as f64 / 2.0;
    let w = screen.width as f64;
    let h = screen.height as f64;

    let vertical = if cy < h / 2.0 { "top" } else { "bottom" };
    let horizontal = if cx < w / 3.0 {
        "left"
    } else if cx > w * 2.0 / 3.0 {
        "right"
    } else {
        "center"
    };
    let name = format!("{vertical}-{horizontal}");
    let target = panel::PanelAnchor::parse(&name)
        .ok_or_else(|| format!("Unknown panel anchor '{name}'."))?;
    panel::anchor(&window, target, margin.unwrap_or(18), true)?;
    Ok(name)
}

/// Resize the voice bar to fit its content.
///
/// The bar is a pill that grows only as much as its current state needs, so
/// the window has to follow the DOM rather than sit at a fixed size. A
/// layer-shell surface that is anchored to one edge (not stretched across it)
/// takes its size from the client, so this works there too.
#[tauri::command]
fn resize_voice_bar(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    // Guard against a mid-layout measurement collapsing the window to nothing.
    let width = width.max(48.0).min(1600.0);
    let height = height.max(28.0).min(400.0);
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|err| err.to_string())
}

/// Anchor the voice bar to a screen edge. Returns what the platform actually
/// achieved, so the UI can tell a real panel from a positioned window.
#[tauri::command]
fn anchor_voice_bar(
    app: AppHandle,
    anchor: String,
    margin: Option<i32>,
) -> Result<panel::PanelStatus, String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    let anchor = panel::PanelAnchor::parse(&anchor)
        .ok_or_else(|| format!("Unknown panel anchor '{anchor}'."))?;
    panel::anchor(&window, anchor, margin.unwrap_or(16), true)
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
    // WebKitGTK's DMA-BUF renderer is broken on a number of Linux GPU and
    // compositor combinations. On a KDE Wayland session with a hybrid
    // Intel/NVIDIA laptop it does not degrade gracefully — it aborts window
    // creation before anything is drawn:
    //
    //     Gdk-Message: Error 71 (protocol error) dispatching to Wayland display.
    //
    // Disabling it gives up a rendering fast path, not a feature. Forcing
    // GDK_BACKEND=x11 is not an alternative: that trades this for repeated
    // "Failed to create GBM buffer of size WxH" failures.
    //
    // An explicit value from the environment always wins, so anyone whose
    // stack handles DMA-BUF correctly can set it back to 0.
    #[cfg(target_os = "linux")]
    if env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
            fs::create_dir_all(&app_dir).map_err(|err| err.to_string())?;
            fs::create_dir_all(app_dir.join("models")).map_err(|err| err.to_string())?;
            fs::create_dir_all(app_dir.join("audio")).map_err(|err| err.to_string())?;

            let db = init_db(&app_dir).map_err(|err| err.to_string())?;
            let resolve_bundled_bin = |name: &str| {
                app.path()
                    .resource_dir()
                    .ok()
                    .map(|dir| dir.join("binaries").join(name))
                    .filter(|path| path.exists())
                    .unwrap_or_else(|| {
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("binaries")
                            .join(name)
                    })
            };
            let funasr_cli_bin = resolve_bundled_bin(FUNASR_CLI_BIN_NAME);
            let funasr_sensevoice_bin = resolve_bundled_bin(FUNASR_SENSEVOICE_BIN_NAME);
            let funasr_vad_bin = resolve_bundled_bin(FUNASR_VAD_BIN_NAME);

            app.manage(AppState {
                db: Mutex::new(db),
                app_dir,
                funasr_cli_bin,
                funasr_sensevoice_bin,
                funasr_vad_bin,
                downloads: Mutex::new(HashMap::new()),
                audio_capture: Mutex::new(None),
                streaming: Mutex::new(None),
                push_to_talk: Mutex::new(None),
            });

            // Anchor the voice bar while it is still unmapped. gtk-layer-shell
            // must claim the surface before the GTK window is realized, which
            // is why the window is declared `visible: false` in the config.
            if let Some(bar) = app.get_webview_window("voice-bar") {
                match panel::anchor(&bar, panel::PanelAnchor::BottomCenter, 24, true) {
                    Ok(status) => eprintln!(
                        "[voice-bar] anchored (layer_shell={}): {}",
                        status.layer_shell, status.detail
                    ),
                    Err(err) => eprintln!("[voice-bar] anchor failed: {err}"),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_inputs,
            start_audio_capture,
            get_audio_level,
            snapshot_audio_capture,
            stop_audio_capture,
            start_streaming_transcription,
            stop_streaming_transcription,
            start_push_to_talk,
            stop_push_to_talk,
            get_llm_settings,
            set_llm_api_key,
            format_transcript,
            get_bootstrap,
            complete_onboarding,
            reset_onboarding,
            list_models,
            list_sessions,
            list_transcripts,
            create_session,
            set_setting,
            download_model_with_progress,
            pause_model_download,
            preview_audio,
            transcribe_audio,
            save_text_transcript,
            copy_text,
            auto_paste_text,
            show_voice_bar,
            show_voice_bar_passive,
            anchor_voice_bar,
            resize_voice_bar,
            snap_voice_bar,
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

        -- formatted_text holds the LLM-typeset Markdown for this transcript.
        -- The raw `text` is never overwritten: the user dictated it, and the
        -- formatted version is a derived view they can discard or regenerate.
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

    // Two-layer transcripts: raw ASR text plus an optional LLM-formatted
    // version. Added after the initial schema, so applied as a migration.
    for (column, decl) in [
        ("formatted_text", "TEXT"),
        ("formatted_preset", "TEXT"),
        ("formatted_at", "TEXT"),
    ] {
        let exists: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('transcripts') WHERE name = ?1")?
            .exists(params![column])?;
        if !exists {
            conn.execute(
                &format!("ALTER TABLE transcripts ADD COLUMN {column} {decl}"),
                [],
            )?;
        }
    }

    // Both models run on the official llama.cpp CPU runtime. `local_path` is a
    // directory now, not a single file, because each model is several GGUFs.
    let catalog: [(&str, &str, &str, &str); 2] = [
        (
            "fun-asr-nano-2512",
            "Fun-ASR-Nano (accurate)",
            BACKEND_NANO,
            "FunAudioLLM/Fun-ASR-Nano-GGUF",
        ),
        (
            "sensevoice-small",
            "SenseVoiceSmall (fast)",
            BACKEND_SENSEVOICE,
            "FunAudioLLM/SenseVoiceSmall-GGUF",
        ),
    ];
    for (id, name, backend, repo_id) in catalog {
        let local_path = app_dir
            .join("models")
            .join(id)
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
                -- A changed backend or path means the old artifacts no longer
                -- satisfy this entry, so force a re-download.
                status = CASE
                    WHEN models.backend = excluded.backend
                     AND models.local_path = excluded.local_path
                    THEN models.status
                    ELSE 'available'
                END,
                last_error = NULL
            "#,
            params![id, name, backend, "huggingface", repo_id, local_path, "available"],
        )?;
    }

    // Retired entries: the Python-only build, and the vLLM GPU path (dropped in
    // favour of CPU-only; it cannot fit alongside a desktop on an 8 GB GPU).
    conn.execute(
        "DELETE FROM models WHERE id IN ('fun-asr-nano-2512-python', 'fun-asr-nano-2512-vllm')",
        [],
    )?;

    let defaults = [
        ("setup.complete", "false"),
        ("defaults.model", "fun-asr-nano-2512"),
        ("defaults.language", "中文"),
        ("defaults.runtime", BACKEND_NANO),
        ("audio.retain", "false"),
        ("floating.autoPaste", "true"),
    ];
    for (key, value) in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    // Migrate anyone pinned to a runtime or model that no longer exists.
    conn.execute(
        "UPDATE settings SET value = ?1 WHERE key = 'defaults.runtime'
           AND value IN ('python-hf-cpu', 'crispasr-gguf-cpu', 'funasr-vllm-gpu')",
        params![BACKEND_NANO],
    )?;
    conn.execute(
        "UPDATE settings SET value = 'fun-asr-nano-2512' WHERE key = 'defaults.model'
           AND value IN ('fun-asr-nano-2512-python', 'fun-asr-nano-2512-vllm')",
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

/// Read a settings value, treating blank as absent.
fn setting_value(state: &AppState, key: &str) -> Result<Option<String>, String> {
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
    Ok(value.filter(|value| !value.trim().is_empty()))
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
        // Formatting happens later, asynchronously, if at all.
        formatted_text: None,
        formatted_preset: None,
        formatted_at: None,
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
            SELECT id, session_id, text, status, source, created_at, duration_ms, model, language,
                   formatted_text, formatted_preset, formatted_at
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
        formatted_text: row.get(9)?,
        formatted_preset: row.get(10)?,
        formatted_at: row.get(11)?,
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
        bundled_asr: state.funasr_cli_bin.exists() && state.funasr_sensevoice_bin.exists(),
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
    let wayland = is_wayland_session();

    // On Wayland the clipboard is *owned by a live process*: whoever set the
    // selection must stay connected to serve it, and the moment they exit the
    // selection is gone. An in-process `Clipboard` dropped at the end of this
    // function therefore reports success and leaves the user with nothing to
    // paste — which is exactly the bug this ordering fixes.
    //
    // `wl-copy` forks a background process that holds the selection until
    // something replaces it, so it is the correct primary path here, not a
    // fallback. On X11 arboard spawns its own owner thread and is fine.
    let helpers: Vec<(&str, Vec<&str>)> = if wayland {
        vec![("wl-copy", vec![])]
    } else {
        vec![
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ]
    };

    if wayland {
        for (program, args) in &helpers {
            if command_exists(program) && write_to_command(program, args, text).is_ok() {
                return PasteResult {
                    copied: true,
                    pasted: false,
                    method: Some((*program).to_string()),
                    message: "Copied to clipboard.".to_string(),
                    session_type: env::var("XDG_SESSION_TYPE").ok(),
                };
            }
        }
    }

    if let Ok(mut clipboard) = Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_ok() {
            // Under Wayland this is a last resort and may not survive; say so
            // rather than claiming a clean copy.
            let message = if wayland {
                "Copied, but wl-clipboard is missing so it may not persist. Install wl-clipboard."
            } else {
                "Copied to clipboard."
            };
            return PasteResult {
                copied: true,
                pasted: false,
                method: Some("arboard".to_string()),
                message: message.to_string(),
                session_type: env::var("XDG_SESSION_TYPE").ok(),
            };
        }
    }

    if !wayland {
        for (program, args) in &helpers {
            if command_exists(program) && write_to_command(program, args, text).is_ok() {
                return PasteResult {
                    copied: true,
                    pasted: false,
                    method: Some((*program).to_string()),
                    message: "Copied to clipboard.".to_string(),
                    session_type: env::var("XDG_SESSION_TYPE").ok(),
                };
            }
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

