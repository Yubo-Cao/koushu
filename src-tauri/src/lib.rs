pub mod asr_cloud;
pub mod hotkey;
pub mod inject;
pub mod license;
pub mod llm;
mod panel;
mod tray;

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
use rusqlite::{params, types::Value, Connection, OptionalExtension};
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
use tauri::{ipc::Channel, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
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
/// Backend id for a hosted OpenAI-compatible transcription endpoint. Has no
/// local assets, so it is never downloaded — only configured.
const BACKEND_CLOUD: &str = "cloud-openai-transcriptions";

const CLOUD_ASR_KEYRING_USER: &str = "cloud-asr-api-key";

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
    /// Where the voice bar is docked. A resize has to re-apply this, otherwise
    /// the pill grows away from its edge instead of staying against it.
    voice_bar_anchor: Mutex<panel::PanelAnchor>,
    /// Live position during a drag, in logical pixels relative to the output
    /// the bar is on. Held in Rust so the accumulated position never depends
    /// on reading the window back — reading it is what caused the jitter.
    voice_bar_drag: Mutex<Option<DragState>>,
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
    /// When the user put this session away. `None` means it is still in the
    /// main list. Archiving never deletes anything.
    archived_at: Option<String>,
}

/// Which side of the archive line to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ArchiveScope {
    /// Everything the user has not put away. The default view.
    #[default]
    Active,
    Archived,
    All,
}

impl ArchiveScope {
    /// SQL predicate over `sessions`, aliased as `s`.
    fn predicate(self) -> Option<&'static str> {
        match self {
            ArchiveScope::Active => Some("s.archived_at IS NULL"),
            ArchiveScope::Archived => Some("s.archived_at IS NOT NULL"),
            ArchiveScope::All => None,
        }
    }
}

/// Narrowing shared by the session list and by search.
///
/// Every field is optional and an absent field means "do not narrow on this",
/// so the default value is the unfiltered view.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SessionFilter {
    language: Option<String>,
    model: Option<String>,
    /// Inclusive `date_key` bounds, `YYYY-MM-DD`. That format sorts
    /// lexicographically, so plain string comparison is a date comparison.
    from: Option<String>,
    to: Option<String>,
    archived: ArchiveScope,
}

impl SessionFilter {
    /// Non-empty, trimmed value or `None` — an empty select box is not a filter.
    fn cleaned(value: &Option<String>) -> Option<&str> {
        value.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }

    /// Appends `AND …` clauses for whichever fields are set, pushing their bound
    /// values onto `binds` in the same order.
    ///
    /// `language_col` and `model_col` are qualified column names so search can
    /// filter on the transcript's own language while the session list filters
    /// on the session's.
    fn push_sql(
        &self,
        language_col: &str,
        model_col: &str,
        sql: &mut String,
        binds: &mut Vec<Value>,
    ) {
        if let Some(language) = Self::cleaned(&self.language) {
            sql.push_str(&format!(" AND {language_col} = ?"));
            binds.push(Value::Text(language.to_string()));
        }
        if let Some(model) = Self::cleaned(&self.model) {
            sql.push_str(&format!(" AND {model_col} = ?"));
            binds.push(Value::Text(model.to_string()));
        }
        if let Some(from) = Self::cleaned(&self.from) {
            sql.push_str(" AND s.date_key >= ?");
            binds.push(Value::Text(from.to_string()));
        }
        if let Some(to) = Self::cleaned(&self.to) {
            sql.push_str(" AND s.date_key <= ?");
            binds.push(Value::Text(to.to_string()));
        }
        if let Some(predicate) = self.archived.predicate() {
            sql.push_str(" AND ");
            sql.push_str(predicate);
        }
    }
}

/// One transcript that matched a search, with enough session context to show
/// and open it without a second round trip.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchHit {
    transcript_id: String,
    session_id: String,
    session_title: String,
    date_key: String,
    created_at: String,
    language: String,
    model: String,
    archived: bool,
    /// A window of the transcript around the first match, elided with `…`.
    snippet: String,
}

/// How the query was answered. Shown nowhere, but it makes the difference
/// between "no matches" and "your query was too short to index" testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SearchMode {
    /// Nothing to search for.
    Empty,
    /// Trigram index.
    Fts,
    /// A term shorter than a trigram, matched by scanning.
    Substring,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    /// The whitespace-separated terms actually searched for, so the UI can
    /// highlight them in each snippet without re-deriving the split.
    terms: Vec<String>,
    hits: Vec<SearchHit>,
    /// More matched than `limit`; what came back is the most recent slice.
    truncated: bool,
    mode: SearchMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest {
    query: String,
    #[serde(default)]
    filter: SessionFilter,
    limit: Option<i64>,
}

/// The values that actually occur, so the filter controls can offer real
/// choices instead of a hardcoded list the database may not contain.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilterOptions {
    languages: Vec<String>,
    models: Vec<String>,
    earliest_date: Option<String>,
    latest_date: Option<String>,
    archived_count: i64,
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
    funasr_vad_bin: PathBuf,
    /// Where to find fsmn-vad.gguf when the selected model has no local files.
    vad_gguf_dir: Option<PathBuf>,
    cloud: Option<asr_cloud::CloudAsrConfig>,
}

struct AsrJobOutput {
    session_id: String,
    model_id: String,
    model_backend: String,
    language: String,
    save_final: bool,
    transcription: Result<(String, String), String>,
    /// Speech seconds found by VAD in this recording, for trial metering.
    vad_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct AsrResult {
    trial: Option<TrialStatus>,
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
    /// Only the committed pass may use this; previews stay local so they keep
    /// their sub-second latency and never bill per keystroke.
    cloud: Option<asr_cloud::CloudAsrConfig>,
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
    cloud: Option<&asr_cloud::CloudAsrConfig>,
) -> Result<String, String> {
    let resampled = resample_linear(samples, sample_rate, 16_000);
    let wav = encode_wav_i16(&resampled, 16_000)?;
    let path = scratch.join(format!("stream-{}.wav", Uuid::new_v4()));
    fs::write(&path, wav).map_err(|err| err.to_string())?;

    let result = match model.backend.as_str() {
        BACKEND_NANO => transcribe_with_funasr_nano(bin, model, &path),
        BACKEND_SENSEVOICE => transcribe_with_sensevoice(bin, model, &path),
        BACKEND_CLOUD => match cloud {
            Some(config) => asr_cloud::transcribe(config, &path).map(|t| (t, String::new())),
            None => Err("Cloud ASR is selected but not configured.".to_string()),
        },
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
                    job.cloud.as_ref(),
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
            // Previews are deliberately local-only: a network round trip per
            // 900 ms refresh would be neither fast enough nor cheap enough.
            None,
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

    // The cloud backend has no local files, but segmentation still runs
    // locally, so VAD comes from whichever GGUF model is installed.
    let vad_gguf = if final_model.backend == BACKEND_CLOUD {
        ["fun-asr-nano-2512", "sensevoice-small"]
            .iter()
            .find_map(|id| get_model(&state, id).ok().and_then(|m| gguf_model_dir(&m).ok()))
            .ok_or_else(|| {
                "Cloud ASR still needs a local model installed for voice activity detection."
                    .to_string()
            })?
            .join("fsmn-vad.gguf")
    } else {
        gguf_model_dir(&final_model)?.join("fsmn-vad.gguf")
    };

    let final_bin = match final_model.backend.as_str() {
        BACKEND_NANO => state.funasr_cli_bin.clone(),
        BACKEND_SENSEVOICE => state.funasr_sensevoice_bin.clone(),
        // Never invoked for the cloud backend; transcribe_samples branches on
        // the backend before it would be used.
        BACKEND_CLOUD => PathBuf::new(),
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
        cloud: cloud_asr_config(&state).ok(),
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
// Trial metering
//
// The free build is fully functional for a fixed budget of *transcribed
// speech*. Two deliberate choices, both recorded in docs/monetisation.md:
//
//   - Seconds, not words. Chinese has no word boundary, so a word cap means
//     something entirely different depending on the language spoken.
//   - VAD speech, not capture length. Holding the key while thinking should
//     not spend the trial.
//
// This counter lives in the client and can be removed by rebuilding, which is
// stated plainly rather than obfuscated. What is sold is not access — the
// source is public — but not having to do that work.
// ---------------------------------------------------------------------------

const TRIAL_LIMIT_SECONDS: f64 = 120.0 * 60.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrialStatus {
    used_seconds: f64,
    limit_seconds: f64,
    licensed: bool,
    /// True the first time a transcript is produced, so the UI can mark the
    /// moment rather than having to guess at it.
    first_transcript: bool,
}

/// Add speech time to the trial counter and report the new state.
fn record_trial_usage(state: &AppState, speech_seconds: f64) -> Result<TrialStatus, String> {
    let used = setting_value(state, "trial.speechSeconds")?
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let first = used == 0.0 && speech_seconds > 0.0;
    let total = used + speech_seconds.max(0.0);
    set_setting_inner(state, "trial.speechSeconds", &format!("{total:.3}"))?;
    Ok(TrialStatus {
        used_seconds: total,
        limit_seconds: TRIAL_LIMIT_SECONDS,
        licensed: setting_value(state, "trial.licenseKey")?
            .map(|key| license::verify(&key).valid)
            .unwrap_or(false),
        first_transcript: first,
    })
}

/// Store a licence after verifying it. Rejected keys are never persisted, so
/// the app cannot end up in a state where it believes it is licensed.
#[tauri::command]
fn activate_license(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<license::LicenseInfo, String> {
    let info = license::verify(&key);
    if info.valid {
        set_setting_inner(&state, "trial.licenseKey", key.trim())?;
    }
    Ok(info)
}

#[tauri::command]
fn get_license(state: tauri::State<'_, AppState>) -> Result<license::LicenseInfo, String> {
    match setting_value(&state, "trial.licenseKey")? {
        // Re-verified on every read rather than trusting a stored "activated"
        // flag: a flag can be flipped in the database, a signature cannot be
        // forged without the private key.
        Some(key) => Ok(license::verify(&key)),
        None => Ok(license::LicenseInfo {
            valid: false,
            email: None,
            issued: None,
            detail: "No licence installed.".to_string(),
        }),
    }
}

#[tauri::command]
fn get_trial_status(state: tauri::State<'_, AppState>) -> Result<TrialStatus, String> {
    Ok(TrialStatus {
        used_seconds: setting_value(&state, "trial.speechSeconds")?
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0),
        limit_seconds: TRIAL_LIMIT_SECONDS,
        licensed: setting_value(&state, "trial.licenseKey")?
            .map(|key| license::verify(&key).valid)
            .unwrap_or(false),
        first_transcript: false,
    })
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

fn cloud_asr_config(state: &AppState) -> Result<asr_cloud::CloudAsrConfig, String> {
    Ok(asr_cloud::CloudAsrConfig {
        base_url: setting_value(state, "asr.cloud.baseUrl")?.unwrap_or_default(),
        model: setting_value(state, "asr.cloud.model")?.unwrap_or_default(),
        api_key: keyring::Entry::new(LLM_KEYRING_SERVICE, CLOUD_ASR_KEYRING_USER)
            .ok()
            .and_then(|entry| entry.get_password().ok())
            .unwrap_or_default(),
        language: setting_value(state, "asr.cloud.language")?.unwrap_or_default(),
    })
}

/// Store or clear the cloud ASR key. Kept separate from the LLM key because
/// the two endpoints are often different providers.
#[tauri::command]
fn set_cloud_asr_api_key(key: Option<String>) -> Result<(), String> {
    let entry = keyring::Entry::new(LLM_KEYRING_SERVICE, CLOUD_ASR_KEYRING_USER)
        .map_err(|err| err.to_string())?;
    match key.filter(|value| !value.trim().is_empty()) {
        Some(value) => entry
            .set_password(value.trim())
            .map_err(|err| format!("Could not save the key: {err}")),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("Could not clear the key: {err}")),
        },
    }
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

/// Whether the hotkey permission is held, and a way to ask again. The UI needs
/// both: the system prompt appears only once ever, so after a decline the only
/// route is System Settings and the app has to say so.
#[tauri::command]
fn hotkey_permission() -> bool {
    hotkey::has_permission()
}

#[tauri::command]
fn request_hotkey_permission() -> bool {
    hotkey::request_permission()
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
        sessions: list_sessions_inner(&state, 60, &SessionFilter::default())?,
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
    filter: Option<SessionFilter>,
) -> Result<Vec<SessionInfo>, String> {
    list_sessions_inner(&state, limit.unwrap_or(60), &filter.unwrap_or_default())
}

/// Full-text search across every transcript, newest match first.
#[tauri::command]
fn search_transcripts(
    state: tauri::State<'_, AppState>,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    search_transcripts_inner(
        &state,
        &request.query,
        &request.filter,
        request.limit.unwrap_or(80),
    )
}

/// Puts a session away, or brings it back. Never deletes.
#[tauri::command]
fn set_session_archived(
    state: tauri::State<'_, AppState>,
    session_id: String,
    archived: bool,
) -> Result<Option<SessionInfo>, String> {
    set_session_archived_inner(&state, &session_id, archived)
}

#[tauri::command]
fn session_filter_options(state: tauri::State<'_, AppState>) -> Result<FilterOptions, String> {
    filter_options_inner(&state)
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
    // Held for the whole job so the tray can show that transcription is still
    // running after the microphone has already closed — the part of the wait
    // the user otherwise has no way to see.
    let _tray_busy = tray::AsrJobGuard::new();
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
        funasr_vad_bin: state.funasr_vad_bin.clone(),
        vad_gguf_dir: ["fun-asr-nano-2512", "sensevoice-small"]
            .iter()
            .find_map(|id| get_model(state, id).ok().and_then(|m| gguf_model_dir(&m).ok())),
        cloud: cloud_asr_config(state).ok(),
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
        BACKEND_CLOUD => match job.cloud.as_ref() {
            Some(config) => asr_cloud::transcribe(config, &job.audio_path)
                .map(|text| (text, BACKEND_CLOUD.to_string())),
            None => Err("Cloud ASR is selected but not configured.".to_string()),
        },
        other => Err(format!(
            "Unknown ASR backend '{other}'. Pick Fun-ASR-Nano or SenseVoiceSmall in settings."
        )),
    };

    // Measure speech before the audio is discarded. Only on success: a failed
    // transcription should not spend the trial.
    let vad_seconds = if transcription.is_ok() {
        gguf_model_dir(&job.model)
            .ok()
            .or_else(|| {
                // The cloud backend has no directory of its own; VAD still runs
                // locally, from whichever local model is installed.
                job.vad_gguf_dir.clone()
            })
            .map(|dir| dir.join("fsmn-vad.gguf"))
            .filter(|path| path.exists())
            .and_then(|vad| run_vad(&job.funasr_vad_bin, &vad, &job.audio_path).ok())
            .map(|spans| {
                spans
                    .iter()
                    .map(|(start, end)| (end.saturating_sub(*start)) as f64 / 1000.0)
                    .sum()
            })
    } else {
        None
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
        vad_seconds,
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
            // Meter the speech actually detected, not how long the key was
            // held. A VAD pass here costs ~95 ms for 30 s of audio.
            let speech = output
                .vad_seconds
                .unwrap_or(0.0);
            let trial = record_trial_usage(state, speech).ok();
            Ok(AsrResult {
                trial,
                session_id: output.session_id,
                transcript,
                text,
                runtime,
                error: None,
            })
        }
        Err(err) => Ok(AsrResult {
            trial: None,
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

/// Where the next utterance will be delivered.
///
/// Called when push-to-talk starts, not when text is ready. By the time the
/// words exist the user may have switched windows, and the one thing worse than
/// not inserting the transcript is inserting it into the wrong application.
#[tauri::command]
fn capture_inject_target() -> inject::Target {
    inject::capture_target()
}

/// Insert text into a previously captured target.
///
/// `keepClipboard` is set for live, mid-utterance delivery: overwriting the
/// clipboard once per spoken phrase would wipe whatever the user had copied,
/// many times a minute. The final delivery leaves it unset so the finished
/// transcript lands on the clipboard as well, where it can be pasted again.
#[tauri::command]
fn inject_text(
    text: String,
    target: Option<inject::Target>,
    keep_clipboard: Option<bool>,
) -> inject::InjectReport {
    let target = target.unwrap_or_else(inject::capture_target);
    inject::inject(&text, &target, keep_clipboard.unwrap_or(false))
}

#[tauri::command]
fn auto_paste_text(text: String) -> PasteResult {
    // Route through the injector so the chord matches the focused application.
    // The old path always sent one hard-coded chord, which meant dictating into
    // a terminal pasted nothing at all: terminals moved paste to Ctrl+Shift+V
    // because Ctrl+V was already a terminal control code.
    let report = inject::inject(&text, &inject::capture_target(), false);
    if report.delivered || report.clipboard_used {
        return PasteResult {
            copied: report.clipboard_used || report.delivered,
            pasted: report.delivered,
            method: report.chord.clone(),
            message: report.message.clone(),
            session_type: env::var("XDG_SESSION_TYPE").ok(),
        };
    }

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
/// Set once the compositor blur has been requested, so it is attempted after
/// the window exists but never re-requested on every show.
static BLUR_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the compositor accepted the blur request. The UI needs this: its
/// material is tuned for an unblurred backdrop by default, and stacking that
/// tint on top of real blur is what turns the bar into a dark slab.
static BLUR_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Does the desktop behind the bar get blurred by the compositor?
#[tauri::command]
fn desktop_blur_active() -> bool {
    BLUR_ACTIVE.load(Ordering::SeqCst)
}

#[tauri::command]
fn show_voice_bar_passive(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    window.show().map_err(|err| err.to_string())?;

    // Compositor-side blur needs a realized surface, which does not exist
    // during setup(). CSS backdrop-filter cannot substitute: it samples the
    // page's own compositing result, and behind a transparent pill-sized
    // window there is nothing there — that is why the bar could only ever look
    // like painted gradients before.
    if !BLUR_REQUESTED.swap(true, Ordering::SeqCst) {
        match panel::enable_background_blur(&window) {
            Ok(()) => {
                BLUR_ACTIVE.store(true, Ordering::SeqCst);
                // Deliberately says "accepted", not "enabled": this only means
                // the compositor took the request. Whether anything is
                // actually blurred depends on the effect being switched on,
                // which a bind cannot tell us.
                eprintln!("[voice-bar] background blur request accepted");
            }
            Err(err) => {
                eprintln!("[voice-bar] background blur unavailable: {err}");
                // Allow a later attempt; the surface may simply not be mapped
                // yet on this first show.
                BLUR_REQUESTED.store(false, Ordering::SeqCst);
            }
        }
    }
    Ok(())
}

/// Inset between the voice bar and the screen edge it is docked to, in logical
/// pixels.
///
/// One constant because a drag has to start from the position the bar is
/// actually in. `begin_voice_bar_drag` reconstructs that from the dock and this
/// margin, so a different value used when docking would make the bar jump by
/// the difference the instant it was grabbed.
const VOICE_BAR_MARGIN: i32 = 18;

/// Where the bar and the cursor were when a drag began. Positions are derived
/// from these, never accumulated.
struct DragState {
    origin_x: f64,
    origin_y: f64,
    cursor_x: f64,
    cursor_y: f64,
    have_cursor: bool,
}

/// Global cursor position in logical pixels, via KWin.
///
/// Wayland deliberately hides the pointer from clients, so this asks the
/// compositor instead. It is the only input that makes dragging work here:
/// pointer deltas from the webview are polluted by the window's own movement
/// (a feedback loop that reads as jitter), while an absolute cursor position
/// is independent of where the window is. Measured at 3-4 ms per call, which
/// fits comfortably in a frame.
fn cursor_position() -> Option<(f64, f64)> {
    let output = Command::new("kdotool")
        .arg("getmouselocation")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut x = None;
    let mut y = None;
    for field in text.split_whitespace() {
        if let Some(value) = field.strip_prefix("x:") {
            x = value.parse::<f64>().ok();
        } else if let Some(value) = field.strip_prefix("y:") {
            y = value.parse::<f64>().ok();
        }
    }
    Some((x?, y?))
}

/// Begin a drag: resolve where the bar currently is, in global logical pixels,
/// and remember it as the running position.
#[tauri::command]
fn begin_voice_bar_drag(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    let geom = panel::output_geometry(&window)?;
    let anchor = app
        .state::<AppState>()
        .voice_bar_anchor
        .lock()
        .map(|value| *value)
        .unwrap_or(panel::PanelAnchor::BottomCenter);

    // Where the current anchor puts it, expressed as a free position on the
    // desktop. The output's origin has to be added back in: on the second
    // screen here that is (512, 1440), and leaving it out made the bar jump by
    // exactly that much the moment a drag started.
    let margin = VOICE_BAR_MARGIN as f64;
    let x = geom.origin_x
        + match anchor.horizontal() {
            Some(true) => margin,
            Some(false) => geom.width - geom.win_width - margin,
            None => (geom.width - geom.win_width) / 2.0,
        };
    let y = geom.origin_y
        + if anchor.is_top() {
            margin
        } else {
            geom.height - geom.win_height - margin
        };

    let cursor = cursor_position();
    if let Ok(mut slot) = app.state::<AppState>().voice_bar_drag.lock() {
        *slot = Some(DragState {
            origin_x: x,
            origin_y: y,
            cursor_x: cursor.map(|c| c.0).unwrap_or(0.0),
            cursor_y: cursor.map(|c| c.1).unwrap_or(0.0),
            have_cursor: cursor.is_some(),
        });
    }
    Ok(())
}

/// Track the cursor during a drag. Called on a timer by the frontend.
///
/// Position is derived from where the cursor is *now* relative to where it was
/// when the drag started, added to the bar's starting position. Nothing is
/// accumulated and the window is never read back, so there is no path for the
/// window's own movement to influence the next update.
#[tauri::command]
fn track_voice_bar_drag(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    let Some((cx, cy)) = cursor_position() else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let (x, y) = {
        let mut slot = state
            .voice_bar_drag
            .lock()
            .map_err(|_| "Drag lock poisoned".to_string())?;
        let Some(drag) = slot.as_mut() else {
            return Ok(());
        };
        // If the cursor was unavailable at drag start, adopt the first reading
        // rather than jumping by the full distance from (0,0).
        if !drag.have_cursor {
            drag.cursor_x = cx;
            drag.cursor_y = cy;
            drag.have_cursor = true;
        }
        // Deliberately unclamped: the cursor is in desktop coordinates and so
        // is this, so the bar has to be allowed past the edge of one output to
        // reach the next. `move_to` clamps against whichever output it lands
        // on, which is the only place that knows which one that is.
        (
            drag.origin_x + (cx - drag.cursor_x),
            drag.origin_y + (cy - drag.cursor_y),
        )
    };
    panel::move_to(&window, x.round() as i32, y.round() as i32)
}

/// Finish a drag by snapping to the nearest edge of the output it ended on.
#[tauri::command]
fn end_voice_bar_drag(app: AppHandle) -> Result<String, String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    let state = app.state::<AppState>();
    let position = state
        .voice_bar_drag
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .and_then(|drag| {
            cursor_position().map(|(cx, cy)| {
                (
                    drag.origin_x + (cx - drag.cursor_x),
                    drag.origin_y + (cy - drag.cursor_y),
                )
            })
        });
    let Some((x, y)) = position else {
        return Ok("bottom-center".to_string());
    };

    // Read the output *after* the drag: the bar may have been handed to a
    // different one on the way, and snapping it against the screen it started
    // on would fling it back across the desk.
    let geom = panel::output_geometry(&window)?;
    let cx = x - geom.origin_x + geom.win_width / 2.0;
    let cy = y - geom.origin_y + geom.win_height / 2.0;
    let name = nearest_dock(cx, cy, geom.width, geom.height);
    let target = panel::PanelAnchor::parse(&name)
        .ok_or_else(|| format!("Unknown panel anchor '{name}'."))?;
    panel::anchor(&window, target, VOICE_BAR_MARGIN, false)?;
    if let Ok(mut slot) = state.voice_bar_anchor.lock() {
        *slot = target;
    }
    Ok(name)
}

/// Which of the six docks a point in an output belongs to.
///
/// Thirds horizontally, halves vertically — the centre docks get the widest
/// catchment because that is where the bar lives by default.
fn nearest_dock(x: f64, y: f64, width: f64, height: f64) -> String {
    let vertical = if y < height / 2.0 { "top" } else { "bottom" };
    let horizontal = if x < width / 3.0 {
        "left"
    } else if x > width * 2.0 / 3.0 {
        "right"
    } else {
        "center"
    };
    format!("{vertical}-{horizontal}")
}

/// Snap the voice bar to whichever screen edge it currently sits nearest.
///
/// Deliberately computed in Rust from the bar's own position and the output it
/// is on. Doing it in the webview used `window.screen`, which reports only the
/// primary display — on a multi-monitor desk every drag resolved to the same
/// corner regardless of where the pill actually was.
///
/// The position comes from `panel::current_position`, not `outer_position()`:
/// Wayland never tells a client where its surface is, so `outer_position()` is
/// a permanent (0, 0) there and this always snapped to the same corner. When
/// the platform genuinely cannot say, the current dock is kept rather than
/// invented.
#[tauri::command]
fn snap_voice_bar(app: AppHandle, margin: Option<i32>) -> Result<String, String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    let state = app.state::<AppState>();
    let current = state
        .voice_bar_anchor
        .lock()
        .map(|value| *value)
        .unwrap_or(panel::PanelAnchor::BottomCenter);

    let geom = panel::output_geometry(&window)?;
    let name = match panel::current_position(&window) {
        Some((x, y)) => nearest_dock(
            x - geom.origin_x + geom.win_width / 2.0,
            y - geom.origin_y + geom.win_height / 2.0,
            geom.width,
            geom.height,
        ),
        None => return Ok(current.name().to_string()),
    };
    let target = panel::PanelAnchor::parse(&name)
        .ok_or_else(|| format!("Unknown panel anchor '{name}'."))?;
    panel::anchor(&window, target, margin.unwrap_or(VOICE_BAR_MARGIN), false)?;
    if let Ok(mut slot) = state.voice_bar_anchor.lock() {
        *slot = target;
    }
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
        .map_err(|err| err.to_string())?;

    // Re-dock immediately. Growing the window without re-applying the anchor
    // leaves it expanding away from its edge — on a HiDPI screen by the scale
    // factor too, which is what made the pill drift toward the middle.
    //
    // The size is handed over as logical pixels, the same units it arrived in.
    // Converting to physical here and back inside the panel meant multiplying
    // by the window's scale and dividing by the monitor's, which are not always
    // the same number.
    let size = tauri::LogicalSize::new(width, height);
    let anchor = app
        .state::<AppState>()
        .voice_bar_anchor
        .lock()
        .map(|value| *value)
        .unwrap_or(panel::PanelAnchor::BottomCenter);
    panel::reposition(&window, anchor, VOICE_BAR_MARGIN, Some(size))?;
    // The native material is a view in this window, so it has to be told the
    // capsule changed shape. Its radius is half the height, and the height is
    // exactly what just moved.
    panel::sync_material(&window);
    Ok(())
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
    let status = panel::anchor(&window, anchor, margin.unwrap_or(16), false)?;
    if let Ok(mut slot) = app.state::<AppState>().voice_bar_anchor.lock() {
        *slot = anchor;
    }
    Ok(status)
}

#[tauri::command]
fn hide_voice_bar(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("voice-bar")
        .ok_or_else(|| "Voice bar window is not configured.".to_string())?;
    window.hide().map_err(|err| err.to_string())
}

/// Whether the frontend draws the window frame itself.
///
/// Now false everywhere, and the reason is worth keeping.
///
/// The app used to take the decorations off on Linux, make the window
/// transparent, and paint its own shadow into an 18px transparent ring — a
/// "gutter" — around the visible shell. That could not be made to work, because
/// **WebKitGTK never clears a transparent window's surface**: every frame is
/// composited `src OVER dst` onto whatever was there before, so any pixel ever
/// painted opaque stays opaque for the life of that backing store. The window's
/// first frame is painted before any script has run, with the gutter still
/// closed, and it stamps the page background into the ring permanently.
///
/// Measured on the shipped build: the ring transmitted 0.5–1.7% of the desktop
/// behind it — 98–99% opaque — in the ambient gradient's own colours, blue along
/// the top and warm along the bottom. That was the "shadow" the user saw: not a
/// shadow, a frozen copy of the window's own background, which is exactly why it
/// ended on a hard straight line instead of fading out.
///
/// The frame is now GTK's job (see `adopt_gtk_csd`), which puts the shadow in a
/// layer the webview does not own and therefore cannot spoil. The frontend needs
/// no gutter, no radius, no shadow and no resize grips, and `window_chrome`
/// reporting false is what turns all four off in one answer.
const CSD_TRANSPARENT: bool = false;

/// Width of that gutter. Zero: there is no gutter any more.
const CSD_GUTTER: f64 = 0.0;

/// Total width and height the gutter adds — one on each side.
const CSD_PADDING: f64 = CSD_GUTTER * 2.0;

/// Hand the window frame to GTK.
///
/// # What this buys
///
/// GTK3 has a complete client-side-decoration implementation of its own, and it
/// switches into it the moment a window is given a titlebar widget. From then on
/// GTK — not the page — draws the drop shadow and the rounded corners, in the
/// area *outside* the child allocation, and hands the compositor the real frame
/// bounds via the window geometry. Three things fall out of that:
///
///   * **The shadow is real.** It is painted by GTK into the toplevel's own
///     surface region, which the WebKitGTK never-clears bug cannot reach, and it
///     is a true gradient rather than a box-shadow clipped by a window rectangle.
///   * **Maximising is correct by construction.** GTK drops the shadow and the
///     corner radius itself when the toplevel is maximised or tiled; there is no
///     `isMaximized()` to poll and no race to lose.
///   * **Resizing is the toolkit's.** GTK owns the resize edges around the
///     shadow, so a drag never crosses the JS bridge — which is what made the
///     hand-drawn grips lag the pointer.
///
/// # The two constraints
///
/// `gtk_window_set_titlebar` must be called **before the window is realized**,
/// so every window that wants this is built hidden and shown from here.
///
/// And the window must *not* be app-paintable, which is what Tauri's
/// `transparent(true)` sets: `gtk_window_draw` skips rendering the decoration
/// node entirely for an app-paintable window, so asking for transparency would
/// silently switch the shadow back off. GTK gives itself the RGBA visual it
/// needs when CSD is enabled, so nothing is lost by leaving transparency alone.
///
/// The titlebar widget is an empty box with a zero height request: GTK needs
/// *a* widget to enter CSD, the app already draws its own header inside the
/// page, and a real `GtkHeaderBar` would mean two title bars stacked.
#[cfg(target_os = "linux")]
fn adopt_gtk_csd<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use gtk::prelude::*;

    match window.gtk_window() {
        Ok(gtk_window) => {
            let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            titlebar.set_size_request(-1, 0);
            // Hidden, not merely empty. A *visible* titlebar still gets a row of
            // its own — measured at 1 logical px even with the theme's
            // `min-height` removed — and since it is transparent that row read
            // as a hairline of desktop along the top of the window, including
            // when maximised. GTK skips allocating an invisible title box
            // entirely while still counting the window as client-decorated.
            // `no_show_all` is what stops `gtk_widget_show_all` putting it back.
            titlebar.set_no_show_all(true);
            titlebar.hide();
            gtk_window.set_titlebar(Some(&titlebar));
            gtk_window.set_decorated(true);
            collapse_gtk_titlebar(&gtk_window);
        }
        Err(err) => {
            eprintln!(
                "[{}] no GTK window to decorate ({err}); the frame stays bare",
                window.label()
            );
        }
    }
    // Unconditional: the window was built hidden so the titlebar could be set
    // before realize, and a failure above must not cost the user the window.
    if let Err(err) = window.show() {
        eprintln!("[{}] could not be shown: {err}", window.label());
    }
}

/// Take the height out of the titlebar GTK insists on having.
///
/// `set_size_request(-1, 0)` is not enough: the theme gives the `titlebar` node
/// a `min-height` (40px under Breeze), and a size request cannot go below a
/// widget's CSS minimum. Measured before this existed — the window came up 40px
/// taller than it asked for, with an empty white band above the app's own
/// header. So the minimum is removed in the same language it was set in.
///
/// Scoped to `.csd`, and to this process, which only ever has this app's own
/// GTK windows in it.
#[cfg(target_os = "linux")]
fn collapse_gtk_titlebar(gtk_window: &gtk::ApplicationWindow) {
    use gtk::prelude::*;

    const CSS: &[u8] = b"
window.csd > .titlebar:not(headerbar) {
  min-height: 0;
  padding: 0;
  margin: 0;
  border: 0;
  background: none;
  box-shadow: none;
}
/* A maximised window has no outside. The theme still leaves the decoration a
   hairline of margin there, which shows up as a 1px transparent line along the
   top of the screen -- measured, alpha 0 across the full width. */
window.csd.maximized decoration,
window.csd.tiled decoration,
window.csd.fullscreen decoration {
  margin: 0;
  border: 0;
  border-radius: 0;
  box-shadow: none;
}
";
    let provider = gtk::CssProvider::new();
    if let Err(err) = provider.load_from_data(CSS) {
        eprintln!("[gtk-csd] titlebar css rejected: {err}");
        return;
    }
    if let Some(screen) = GtkWindowExt::screen(gtk_window) {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn adopt_gtk_csd<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

/// Apply the decoration policy both app windows share.
///
/// macOS keeps its real title bar and lets content run under the traffic
/// lights — dropping decorations there would mean reimplementing
/// close/minimise/zoom, and a hand-drawn imitation never matches the
/// platform's behaviour or its accessibility affordances.
///
/// Linux keeps its decorations too, in GTK's sense of the word: the toolkit is
/// told to decorate the window and then handed an empty titlebar, which is what
/// puts it in charge of the shadow and the resize edges without putting a second
/// header above the app's own. The window is built hidden because
/// `adopt_gtk_csd` has to run before it is realized.
fn apply_window_chrome<'a, R: tauri::Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(target_os = "macos")]
    {
        builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
    }
    #[cfg(target_os = "linux")]
    {
        builder.visible(false)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        builder
    }
}

/// What the frontend needs to know about how this window is framed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowChrome {
    /// Draw the gutter, the rounded corners and the shadow?
    ///
    /// True only when the window is genuinely transparent. `isDecorated()`
    /// cannot answer this — decorations being off says nothing about whether
    /// the surface has an alpha channel, and in the state where the first is
    /// true and the second is not, a gutter renders in the page background and
    /// looks like a fake window frame drawn around the real window.
    pub csd_gutter: bool,
    /// Width of that gutter in logical pixels, so the CSS and the window size
    /// cannot drift apart.
    pub gutter: f64,
}

/// Tell the frontend whether it may draw its own window frame.
///
/// Deliberately a statement of fact rather than an inference the caller has to
/// make: if the transparency is ever rolled back, this starts returning false
/// on its own and the frontend degrades to square corners with no shadow, with
/// no second edit anywhere.
#[tauri::command]
fn window_chrome() -> WindowChrome {
    WindowChrome {
        csd_gutter: CSD_TRANSPARENT,
        gutter: CSD_GUTTER,
    }
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|err| err.to_string())?;
        return window.set_focus().map_err(|err| err.to_string());
    }

    let width = 1080.0 + CSD_PADDING;
    let height = 760.0 + CSD_PADDING;
    let window = apply_window_chrome(
        WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("/settings".into()))
            .title("Fun ASR Settings")
            .inner_size(width, height)
            .min_inner_size(960.0 + CSD_PADDING, 640.0 + CSD_PADDING)
            .center(),
    )
    .build()
    .map_err(|err| err.to_string())?;

    adopt_gtk_csd(&window);
    enforce_window_size(window, width, height);
    Ok(())
}

/// Make a window actually be the size it was asked to be: wait for it to
/// settle, then ask again and check.
///
/// # What goes wrong without this
///
/// A window built while the event loop is already running does not come up at
/// the size the builder was given. Measured on KDE Wayland, asking for
/// 1116x796 logical:
///
/// | Source | Reads |
/// |---|---|
/// | `inner_size()` the instant `build()` returns | 1202x882 |
/// | what the builder was asked for | 1116x796 |
/// | what the compositor actually shows | 1030x710 |
///
/// Evenly spaced, step 86 — one frame margin applied twice with opposite signs
/// is the shape of it, though the exact mechanism has not been pinned down.
/// The main window is unaffected: asked for 1276x856 during `setup()`, it gets
/// exactly that. Being created after the loop starts is the difference.
///
/// # Why waiting is the whole trick
///
/// A `set_size` issued immediately after `build()` is swallowed the same way —
/// that was tried first and produced the same 1030x710. Once the window has
/// been through one configure cycle, though, `set_size` is honoured exactly:
/// the correction below was observed moving it 1030 → 1202 → 1116 with each
/// request landing precisely. So the fix is not a cleverer number, it is
/// asking a moment later.
///
/// # Why it matters
///
/// The settings page switches to a two-column layout at 1150px. 1030 minus the
/// gutter is 994, so the breakpoint would never fire and the page would stay
/// single-column forever while every line of its CSS looked correct. That
/// exact failure — a layout whose breakpoint the window could never reach —
/// has already shipped once in this project, which is why this verifies rather
/// than assumes, and says what it found either way.
fn enforce_window_size(window: tauri::WebviewWindow, width: f64, height: f64) {
    thread::spawn(move || {
        // Setting a size is a request, not an assignment: `set_size` returns
        // long before the compositor has done anything, so measuring straight
        // after it would only ever record our own optimism.
        let measure = || -> Option<(f64, f64)> {
            thread::sleep(Duration::from_millis(700));
            // On a GTK client-side-decorated window, `inner_size()` reports the
            // whole GdkWindow, which *includes* the shadow margins the toolkit
            // reserves outside the frame — measured at 43 logical px a side, so
            // 86 too wide and 86 too tall. Comparing that against the size that
            // was asked for would read as "the compositor made it 86px too big"
            // on every check and shrink the window by that much, forever. GTK's
            // own `size()` is the frame, which is what was requested.
            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::*;
                let gtk_window = window.gtk_window().ok()?;
                let (w, h) = gtk_window.size();
                return Some((w as f64, h as f64));
            }
            #[cfg(not(target_os = "linux"))]
            {
                let scale = window.scale_factor().ok()?;
                let size = window.inner_size().ok()?;
                Some((size.width as f64 / scale, size.height as f64 / scale))
            }
        };
        let good = |w: f64, h: f64| (w - width).abs() <= 1.0 && (h - height).abs() <= 1.0;

        // Two attempts. The first re-states the target, which is what the
        // settled window honours. The second falls back to correcting by the
        // measured error, in case some other platform subtracts rather than
        // ignores. Bounded, so a compositor that simply refuses the size
        // cannot turn this into an argument.
        let mut request = (width, height);
        for attempt in 0..2 {
            let Some((actual_w, actual_h)) = measure() else {
                return;
            };
            // A user who maximised the window inside the settling window meant
            // it. Re-stating the built size here would silently un-maximise a
            // window the compositor had already sized correctly.
            if window.is_maximized().unwrap_or(false) {
                eprintln!("[{}] maximised while settling; size left alone", window.label());
                return;
            }
            if good(actual_w, actual_h) {
                let how = if attempt == 0 { "confirmed" } else { "corrected" };
                eprintln!(
                    "[{}] size {how} at {actual_w}x{actual_h} logical",
                    window.label()
                );
                return;
            }
            if attempt == 1 {
                request = (
                    request.0 + (width - actual_w),
                    request.1 + (height - actual_h),
                );
            }
            eprintln!(
                "[{}] size came back {actual_w}x{actual_h}, wanted {width}x{height}; \
                 re-asking for {}x{}",
                window.label(),
                request.0,
                request.1
            );
            if window
                .set_size(tauri::LogicalSize::new(request.0, request.1))
                .is_err()
            {
                return;
            }
        }
        if let Some((actual_w, actual_h)) = measure() {
            if good(actual_w, actual_h) {
                eprintln!(
                    "[{}] size corrected to {actual_w}x{actual_h} logical",
                    window.label()
                );
            } else {
                eprintln!(
                    "[{}] size still {actual_w}x{actual_h} after correction; wanted \
                     {width}x{height}",
                    window.label()
                );
            }
        }
    });
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
                voice_bar_anchor: Mutex::new(panel::PanelAnchor::BottomCenter),
                voice_bar_drag: Mutex::new(None),
            });

            // Ask for the hotkey permission at startup. Without it push-to-talk
            // is silently inert, and finding that out by holding a key and
            // getting nothing is the worst possible first run. No-op where the
            // platform needs no up-front grant.
            if !hotkey::request_permission() {
                eprintln!(
                    "[hotkey] permission not granted yet; push-to-talk stays off until it is"
                );
            }

            // The main window is built here rather than declared in
            // tauri.conf.json because transparency cannot be expressed there
            // per platform: `transparent` in the config applies everywhere, and
            // on macOS it trips tauri-build's `macos-private-api` check for a
            // window whose shadow the system already draws. A platform config
            // file would not help either — merging replaces the whole `windows`
            // array, so both window definitions would have to be duplicated and
            // would drift. In Rust the `cfg` is exact and lives next to the
            // reason for it.
            let main_window = apply_window_chrome(
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("/".into()))
                    .title("Fun ASR Desktop")
                    .inner_size(1240.0 + CSD_PADDING, 820.0 + CSD_PADDING)
                    .min_inner_size(1040.0 + CSD_PADDING, 700.0 + CSD_PADDING)
                    .center(),
            )
            .build()?;
            adopt_gtk_csd(&main_window);

            // The tray icon is how the user knows the app is alive at all: it
            // is driven by a global hotkey, so the main window is usually shut
            // and nothing else of it is on screen. Not fatal if it fails —
            // there may be no StatusNotifier host running.
            if let Err(err) = tray::init(app.handle()) {
                eprintln!("[tray] unavailable: {err}");
            }

            // Anchor the voice bar while it is still unmapped. gtk-layer-shell
            // must claim the surface before the GTK window is realized, which
            // is why the window is declared `visible: false` in the config.
            if let Some(bar) = app.get_webview_window("voice-bar") {
                match panel::anchor(&bar, panel::PanelAnchor::BottomCenter, VOICE_BAR_MARGIN, false)
                {
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
            hotkey_permission,
            request_hotkey_permission,
            get_trial_status,
            activate_license,
            get_license,
            desktop_blur_active,
            get_llm_settings,
            set_llm_api_key,
            set_cloud_asr_api_key,
            format_transcript,
            get_bootstrap,
            complete_onboarding,
            reset_onboarding,
            list_models,
            list_sessions,
            list_transcripts,
            search_transcripts,
            set_session_archived,
            session_filter_options,
            create_session,
            set_setting,
            download_model_with_progress,
            pause_model_download,
            preview_audio,
            transcribe_audio,
            save_text_transcript,
            copy_text,
            auto_paste_text,
            capture_inject_target,
            inject_text,
            show_voice_bar,
            show_voice_bar_passive,
            anchor_voice_bar,
            resize_voice_bar,
            snap_voice_bar,
            begin_voice_bar_drag,
            track_voice_bar_drag,
            end_voice_bar_drag,
            hide_voice_bar,
            show_settings_window,
            window_chrome
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fun ASR Desktop");
}

fn init_db(app_dir: &Path) -> rusqlite::Result<Connection> {
    let db_path = app_dir.join("fun_asr_desktop.sqlite3");
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    apply_schema(&conn)?;
    seed_db(&conn, app_dir)?;
    Ok(conn)
}

/// Every table, index, trigger and migration, in the order they must run.
///
/// Split out from `init_db` so tests can build the real schema in memory
/// instead of approximating it — a search test against a hand-written FTS table
/// would prove nothing about the one that ships.
fn apply_schema(conn: &Connection) -> rusqlite::Result<()> {
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

        -- tokenize='trigram', not the default unicode61.
        --
        -- unicode61 splits on spaces and punctuation, which is meaningless for
        -- Chinese: 「今天所做的这一个渲染」 is one enormous token, so searching
        -- 「渲染」 matches nothing at all. The trigram tokenizer indexes every
        -- overlapping run of three characters instead, which makes MATCH a true
        -- substring search and works for CJK, Latin and mixed text alike. The
        -- cost is a larger index and a three-character floor on queries —
        -- shorter ones fall back to LIKE in `search_transcripts_inner`.
        CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts USING fts5(
            text,
            content='transcripts',
            content_rowid='rowid',
            tokenize='trigram'
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

    // Archiving. A timestamp rather than a boolean, because "when did I put
    // this away" is worth keeping and costs nothing over a flag.
    let archived_exists: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = ?1")?
        .exists(params!["archived_at"])?;
    if !archived_exists {
        conn.execute("ALTER TABLE sessions ADD COLUMN archived_at TEXT", [])?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_sessions_archived_started
             ON sessions(archived_at, started_at DESC);
         CREATE INDEX IF NOT EXISTS idx_transcripts_session
             ON transcripts(session_id, created_at);",
    )?;

    // Re-tokenize an index built before trigram.
    //
    // `CREATE VIRTUAL TABLE IF NOT EXISTS` above is a no-op on databases that
    // already have the unicode61 index, and those cannot match Chinese at all.
    // The FTS content lives entirely in `transcripts`, so dropping and
    // rebuilding loses nothing — it is a derived index, not data.
    let fts_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'transcripts_fts'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if fts_sql.is_some_and(|sql| !sql.contains("trigram")) {
        conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS transcripts_fts;
            CREATE VIRTUAL TABLE transcripts_fts USING fts5(
                text,
                content='transcripts',
                content_rowid='rowid',
                tokenize='trigram'
            );
            INSERT INTO transcripts_fts(transcripts_fts) VALUES('rebuild');
            "#,
        )?;
    }

    Ok(())
}

/// Startup housekeeping and the built-in model catalog.
fn seed_db(conn: &Connection, app_dir: &Path) -> rusqlite::Result<()> {
    // Drop sessions that never captured anything.
    //
    // A session is created the moment recording starts, so any push-to-talk
    // that picked up no speech — a mis-hit, a false trigger, a moment of
    // silence — leaves an empty row behind. They accumulate quickly and make
    // the sidebar useless. Only sessions older than an hour are removed, so a
    // session being recorded into right now is never touched.
    conn.execute(
        "DELETE FROM sessions
          WHERE NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.session_id = sessions.id)
            AND started_at < datetime('now', '-1 hour')",
        [],
    )?;

    // Both models run on the official llama.cpp CPU runtime. `local_path` is a
    // directory now, not a single file, because each model is several GGUFs.
    let catalog: [(&str, &str, &str, &str); 3] = [
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
        (
            "cloud-asr",
            "Cloud transcription (most accurate)",
            BACKEND_CLOUD,
            "OpenAI-compatible /v1/audio/transcriptions",
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
            params![
                id,
                name,
                backend,
                if backend == BACKEND_CLOUD { "remote" } else { "huggingface" },
                repo_id,
                local_path,
                // The cloud backend has nothing to fetch; it is ready as soon
                // as it is configured, so it must not sit in the UI forever
                // showing a Download button that cannot do anything.
                if backend == BACKEND_CLOUD { "installed" } else { "available" }
            ],
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

    Ok(())
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
        archived_at: None,
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

const SESSION_COLUMNS: &str =
    "s.id, s.title, s.started_at, s.ended_at, s.date_key, s.model, s.language, s.runtime, s.archived_at";

fn list_sessions_inner(
    state: &AppState,
    limit: i64,
    filter: &SessionFilter,
) -> Result<Vec<SessionInfo>, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    list_sessions_on(&conn, limit, filter)
}

fn list_sessions_on(
    conn: &Connection,
    limit: i64,
    filter: &SessionFilter,
) -> Result<Vec<SessionInfo>, String> {
    // `WHERE 1 = 1` so every clause below can be appended uniformly as `AND …`.
    let mut sql = format!("SELECT {SESSION_COLUMNS} FROM sessions s WHERE 1 = 1");
    let mut binds: Vec<Value> = Vec::new();
    filter.push_sql("s.language", "s.model", &mut sql, &mut binds);
    sql.push_str(" ORDER BY s.started_at DESC LIMIT ?");
    binds.push(Value::Integer(limit));

    let mut stmt = conn.prepare(&sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(binds), map_session)
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
        archived_at: row.get(8)?,
    })
}

/// Moves a session in or out of the archive.
///
/// Nothing is deleted and nothing is copied: only `archived_at` changes, so the
/// transcripts, the FTS index and every id stay exactly as they were. Returns
/// the session in its new state, or `None` if the id is unknown.
fn set_session_archived_inner(
    state: &AppState,
    session_id: &str,
    archived: bool,
) -> Result<Option<SessionInfo>, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    set_session_archived_on(&conn, session_id, archived)
}

fn set_session_archived_on(
    conn: &Connection,
    session_id: &str,
    archived: bool,
) -> Result<Option<SessionInfo>, String> {
    let stamp = archived.then(|| Local::now().to_rfc3339());
    conn.execute(
        "UPDATE sessions SET archived_at = ?1 WHERE id = ?2",
        params![stamp, session_id],
    )
    .map_err(|err| err.to_string())?;

    conn.query_row(
        &format!("SELECT {SESSION_COLUMNS} FROM sessions s WHERE s.id = ?1"),
        params![session_id],
        map_session,
    )
    .optional()
    .map_err(|err| err.to_string())
}

fn filter_options_inner(state: &AppState) -> Result<FilterOptions, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    filter_options_on(&conn)
}

fn filter_options_on(conn: &Connection) -> Result<FilterOptions, String> {
    let collect = |sql: &str| -> Result<Vec<String>, String> {
        let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| err.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?;
        Ok(rows)
    };

    let languages =
        collect("SELECT DISTINCT language FROM sessions WHERE language <> '' ORDER BY language")?;
    let models = collect("SELECT DISTINCT model FROM sessions WHERE model <> '' ORDER BY model")?;
    let (earliest_date, latest_date) = conn
        .query_row(
            "SELECT MIN(date_key), MAX(date_key) FROM sessions",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .map_err(|err| err.to_string())?;
    let archived_count = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE archived_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;

    Ok(FilterOptions {
        languages,
        models,
        earliest_date,
        latest_date,
        archived_count,
    })
}

/// Splits a raw query box into search terms.
///
/// Whitespace-separated, because that is what every search box in the world
/// does. Quoted phrases are deliberately not supported: with a trigram index
/// every term is already a literal substring, so `"液态 玻璃"` and `液态 玻璃`
/// would only differ in whether the space itself has to match.
fn search_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

/// Wraps each term as an FTS5 string literal and ANDs them together.
///
/// Every term becomes a `"…"` phrase so that punctuation and CJK inside it are
/// matched literally rather than parsed as FTS5 operator syntax; an embedded
/// double quote is escaped by doubling, as SQL requires.
fn fts_match_expression(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escapes a term for use inside `LIKE '%' || ? || '%' ESCAPE '\'`.
fn like_pattern(term: &str) -> String {
    let mut escaped = String::with_capacity(term.len() + 2);
    escaped.push('%');
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped.push('%');
    escaped
}

/// The trigram tokenizer cannot answer a query shorter than one trigram.
const MIN_TRIGRAM_CHARS: usize = 3;

/// A window of `text` around the first term that occurs in it.
///
/// Counted in `char`s, never bytes: a byte window would give a Chinese snippet
/// a third the content of an English one and could split a character in half.
fn build_snippet(text: &str, terms: &[String], window: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= window {
        return text.trim().to_string();
    }

    // Case-insensitive search on a lowered copy, indexed by char position so
    // the offset maps straight back onto `chars`.
    let lowered: Vec<char> = chars.iter().flat_map(|ch| ch.to_lowercase()).collect();
    let hit = terms.iter().find_map(|term| {
        let needle: Vec<char> = term.chars().flat_map(|ch| ch.to_lowercase()).collect();
        if needle.is_empty() || needle.len() > lowered.len() {
            return None;
        }
        (0..=lowered.len() - needle.len())
            .find(|&start| lowered[start..start + needle.len()] == needle[..])
    });

    // `to_lowercase` can change a string's length (ß, İ), which would shift the
    // offset. Clamping keeps the window inside the text either way; the worst
    // case is a snippet centred a character or two off.
    let hit = hit.unwrap_or(0).min(chars.len().saturating_sub(1));
    let lead = window / 3;
    let start = hit.saturating_sub(lead);
    let end = (start + window).min(chars.len());
    let start = end.saturating_sub(window);

    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(&chars[start..end]);
    if end < chars.len() {
        snippet.push('…');
    }
    snippet
}

const SNIPPET_CHARS: usize = 110;

fn search_transcripts_inner(
    state: &AppState,
    query: &str,
    filter: &SessionFilter,
    limit: i64,
) -> Result<SearchResponse, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    search_transcripts_on(&conn, query, filter, limit)
}

fn search_transcripts_on(
    conn: &Connection,
    query: &str,
    filter: &SessionFilter,
    limit: i64,
) -> Result<SearchResponse, String> {
    let terms = search_terms(query);
    if terms.is_empty() {
        return Ok(SearchResponse {
            terms,
            hits: Vec::new(),
            truncated: false,
            mode: SearchMode::Empty,
        });
    }

    // The trigram index answers anything three characters or longer. A shorter
    // term — 「的」, `UI` — has no trigram to look up, so those queries scan
    // instead. Mixing the two would need an intersection across two indexes for
    // no real gain, so a single short term puts the whole query on the scan.
    let use_fts = terms
        .iter()
        .all(|term| term.chars().count() >= MIN_TRIGRAM_CHARS);

    let mut binds: Vec<Value> = Vec::new();
    let mut sql = String::from(
        "SELECT t.id, t.session_id, t.text, t.created_at, t.language, t.model,
                s.title, s.date_key, s.archived_at
         FROM transcripts t
         JOIN sessions s ON s.id = t.session_id
         WHERE ",
    );

    if use_fts {
        sql.push_str(
            "t.rowid IN (SELECT rowid FROM transcripts_fts WHERE transcripts_fts MATCH ?)",
        );
        binds.push(Value::Text(fts_match_expression(&terms)));
    } else {
        // All terms must appear, matching the AND semantics of the FTS path.
        for (index, term) in terms.iter().enumerate() {
            if index > 0 {
                sql.push_str(" AND ");
            }
            sql.push_str("t.text LIKE ? ESCAPE '\\'");
            binds.push(Value::Text(like_pattern(term)));
        }
    }

    // Filter on the transcript's own language and model: the row on screen is a
    // transcript, so it should be judged by what actually produced it, not by
    // what the session was set to when it was opened.
    filter.push_sql("t.language", "t.model", &mut sql, &mut binds);

    // One row over the limit, purely to tell "exactly full" from "there is more".
    sql.push_str(" ORDER BY t.created_at DESC LIMIT ?");
    binds.push(Value::Integer(limit.max(1) + 1));

    let mut stmt = conn.prepare(&sql).map_err(|err| err.to_string())?;
    let mut hits = stmt
        .query_map(rusqlite::params_from_iter(binds), |row| {
            let text: String = row.get(2)?;
            let archived: Option<String> = row.get(8)?;
            Ok(SearchHit {
                transcript_id: row.get(0)?,
                session_id: row.get(1)?,
                snippet: build_snippet(&text, &terms, SNIPPET_CHARS),
                created_at: row.get(3)?,
                language: row.get(4)?,
                model: row.get(5)?,
                session_title: row.get(6)?,
                date_key: row.get(7)?,
                archived: archived.is_some(),
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;

    let truncated = hits.len() as i64 > limit.max(1);
    hits.truncate(limit.max(1) as usize);

    Ok(SearchResponse {
        terms,
        hits,
        truncated,
        mode: if use_fts {
            SearchMode::Fts
        } else {
            SearchMode::Substring
        },
    })
}


#[cfg(test)]
mod search_tests {
    use super::*;

    /// Verbatim transcripts from a real session, kept as-is on purpose.
    ///
    /// The whole point of the trigram tokenizer is that unsegmented Mandarin
    /// works, and paraphrased or space-separated fixtures would quietly pass
    /// under the default tokenizer too — proving nothing.
    const ZH_GLASS: &str = "麻烦你去给这个正在改液态玻璃页面的A正的提点意见吧。他现在做这个玩意儿呢，感觉和液态玻璃关系不大。";
    const ZH_UI: &str = "目前的话呢，它拖动的话好像还是不是很跟手。希望你能够把这个东西的UI重新设置做成类似于iOS 27这种液态玻璃的质感。";
    const ZH_MATERIAL: &str =
        "今天所做的这一个 MATERIAL 的这个 BLEND RENDER，我可以希望你把它做得更好看一些。";
    const EN_ALPHA: &str =
        "Approve the architectural work on the alpha plane and alpha compositing.";

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_schema(&conn).expect("apply the shipping schema");
        conn
    }

    fn add_session(conn: &Connection, id: &str, date_key: &str, language: &str, model: &str) {
        conn.execute(
            "INSERT INTO sessions
             (id, title, started_at, ended_at, date_key, model, language, runtime, archived_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, 'test', NULL)",
            params![
                id,
                format!("Session {id}"),
                format!("{date_key}T09:00:00Z"),
                date_key,
                model,
                language
            ],
        )
        .unwrap();
    }

    fn add_transcript(conn: &Connection, id: &str, session_id: &str, text: &str) {
        let (language, model): (String, String) = conn
            .query_row(
                "SELECT language, model FROM sessions WHERE id = ?1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO transcripts
             (id, session_id, text, status, source, created_at, duration_ms, model, language)
             VALUES (?1, ?2, ?3, 'final', 'mic', ?4, NULL, ?5, ?6)",
            params![
                id,
                session_id,
                text,
                format!("2026-08-08T12:{id:0>2}:00Z"),
                model,
                language
            ],
        )
        .unwrap();
    }

    /// A corpus that mixes Mandarin, English and dates, like the real database.
    fn corpus() -> Connection {
        let conn = memory_db();
        add_session(&conn, "1", "2026-08-01", "中文", "fun-asr-nano-2512");
        add_session(&conn, "2", "2026-08-05", "中文", "sensevoice-small");
        add_session(&conn, "3", "2026-08-08", "English", "fun-asr-nano-2512");
        add_transcript(&conn, "11", "1", ZH_GLASS);
        add_transcript(&conn, "22", "2", ZH_UI);
        add_transcript(&conn, "33", "2", ZH_MATERIAL);
        add_transcript(&conn, "44", "3", EN_ALPHA);
        conn
    }

    fn search(conn: &Connection, query: &str) -> SearchResponse {
        search_transcripts_on(conn, query, &SessionFilter::default(), 50).unwrap()
    }

    fn ids(response: &SearchResponse) -> Vec<String> {
        response
            .hits
            .iter()
            .map(|hit| hit.transcript_id.clone())
            .collect()
    }

    #[test]
    fn the_default_tokenizer_cannot_find_chinese() {
        // The failure this whole design exists to avoid. If this test ever
        // starts passing, FTS5 gained a CJK-aware default and the trigram
        // index — and its three-character floor — could be reconsidered.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE plain USING fts5(text);
             INSERT INTO plain(text) VALUES ('麻烦你去给这个正在改液态玻璃页面的意见吧');",
        )
        .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM plain WHERE plain MATCH '\"液态玻璃\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            hits, 0,
            "unicode61 indexes an unsegmented sentence as one token"
        );
    }

    #[test]
    fn finds_chinese_mid_sentence() {
        let conn = corpus();
        // 液态玻璃 sits in the middle of both sentences, with no delimiter of
        // any kind on either side.
        let response = search(&conn, "液态玻璃");
        assert_eq!(response.mode, SearchMode::Fts);
        assert_eq!(ids(&response), vec!["22", "11"], "newest match first");
        assert!(response.hits[0].snippet.contains("液态玻璃"));
    }

    #[test]
    fn finds_a_three_character_chinese_fragment() {
        let conn = corpus();
        assert_eq!(ids(&search(&conn, "玩意儿")), vec!["11"]);
        assert_eq!(ids(&search(&conn, "重新设置")), vec!["22"]);
    }

    #[test]
    fn all_terms_must_match() {
        let conn = corpus();
        // Both fragments are in transcript 22 only; 11 has 液态玻璃 but no 拖动.
        assert_eq!(ids(&search(&conn, "液态玻璃 拖动")), vec!["22"]);
        assert!(search(&conn, "液态玻璃 火星探测").hits.is_empty());
    }

    #[test]
    fn matches_latin_without_regard_to_case() {
        let conn = corpus();
        assert_eq!(ids(&search(&conn, "material")), vec!["33"]);
        assert_eq!(ids(&search(&conn, "MATERIAL")), vec!["33"]);
        assert_eq!(ids(&search(&conn, "architectural")), vec!["44"]);
    }

    #[test]
    fn short_queries_fall_back_to_scanning() {
        let conn = corpus();
        // "UI" is two characters, so it has no trigram to look up. The FTS
        // index would silently return nothing; the scan finds it.
        let response = search(&conn, "UI");
        assert_eq!(response.mode, SearchMode::Substring);
        assert_eq!(ids(&response), vec!["22"]);

        let single = search(&conn, "拖");
        assert_eq!(single.mode, SearchMode::Substring);
        assert_eq!(ids(&single), vec!["22"]);
    }

    #[test]
    fn a_mixed_length_query_uses_the_scan_for_every_term() {
        let conn = corpus();
        // 液态玻璃 is indexable, UI is not; taking the FTS path would drop the
        // UI half of the query and over-match.
        let response = search(&conn, "液态玻璃 UI");
        assert_eq!(response.mode, SearchMode::Substring);
        assert_eq!(ids(&response), vec!["22"]);
    }

    #[test]
    fn an_empty_query_matches_nothing_rather_than_everything() {
        let conn = corpus();
        for query in ["", "   ", "\t\n"] {
            let response = search(&conn, query);
            assert_eq!(response.mode, SearchMode::Empty);
            assert!(response.hits.is_empty(), "{query:?} should return no hits");
        }
    }

    #[test]
    fn fts_operators_in_the_query_are_matched_literally() {
        let conn = corpus();
        add_session(&conn, "9", "2026-08-08", "English", "fun-asr-nano-2512");
        add_transcript(
            &conn,
            "99",
            "9",
            r#"He said "hello there" and 100% meant it."#,
        );

        // Quotes, `*`, `NEAR` and `^` are all FTS5 syntax. Typed into a search
        // box they are just characters, and must not be parsed — a stray quote
        // used to be enough to turn a search into a SQL error.
        assert_eq!(ids(&search(&conn, r#""hello there""#)), vec!["99"]);
        assert_eq!(ids(&search(&conn, "100%")), vec!["99"]);
        for hostile in [r#"""#, "*", "NEAR(", "^foo", "AND", ")))"] {
            search(&conn, hostile);
        }
    }

    #[test]
    fn like_wildcards_in_short_queries_are_matched_literally() {
        let conn = corpus();
        add_session(&conn, "9", "2026-08-08", "English", "fun-asr-nano-2512");
        add_transcript(&conn, "99", "9", "it_really cost 50% more");
        add_transcript(&conn, "98", "9", "architecture rendered 507 frames");

        // Two characters, so these take the scan. An unescaped `_` is LIKE's
        // any-single-character wildcard and would also match "re" in
        // "rendered"; an unescaped `%` would match everything with a 0 in it.
        assert_eq!(ids(&search(&conn, "_r")), vec!["99"]);
        assert_eq!(ids(&search(&conn, "0%")), vec!["99"]);
    }

    #[test]
    fn hits_carry_their_session() {
        let conn = corpus();
        let hit = &search(&conn, "液态玻璃").hits[0];
        assert_eq!(hit.session_id, "2");
        assert_eq!(hit.session_title, "Session 2");
        assert_eq!(hit.date_key, "2026-08-05");
        assert_eq!(hit.language, "中文");
        assert!(!hit.archived);
    }

    #[test]
    fn filters_narrow_the_results() {
        let conn = corpus();
        let by_model = SessionFilter {
            model: Some("sensevoice-small".into()),
            ..Default::default()
        };
        assert_eq!(
            search_transcripts_on(&conn, "液态玻璃", &by_model, 50)
                .unwrap()
                .hits
                .len(),
            1
        );

        let by_language = SessionFilter {
            language: Some("English".into()),
            ..Default::default()
        };
        assert!(search_transcripts_on(&conn, "液态玻璃", &by_language, 50)
            .unwrap()
            .hits
            .is_empty());

        let by_date = SessionFilter {
            from: Some("2026-08-04".into()),
            to: Some("2026-08-06".into()),
            ..Default::default()
        };
        assert_eq!(
            ids(&search_transcripts_on(&conn, "液态玻璃", &by_date, 50).unwrap()),
            vec!["22"]
        );
    }

    #[test]
    fn a_blank_filter_field_is_not_a_filter() {
        let conn = corpus();
        // Select boxes hand back "" when set to the "Any" option; that has to
        // mean unfiltered, not "match sessions whose language is empty".
        let blank = SessionFilter {
            language: Some(String::new()),
            model: Some("  ".into()),
            from: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(
            search_transcripts_on(&conn, "液态玻璃", &blank, 50)
                .unwrap()
                .hits
                .len(),
            2
        );
    }

    #[test]
    fn truncation_is_reported() {
        let conn = corpus();
        let response =
            search_transcripts_on(&conn, "液态玻璃", &SessionFilter::default(), 1).unwrap();
        assert_eq!(response.hits.len(), 1);
        assert!(response.truncated);

        let full = search_transcripts_on(&conn, "液态玻璃", &SessionFilter::default(), 2).unwrap();
        assert_eq!(full.hits.len(), 2);
        assert!(!full.truncated, "exactly full is not truncated");
    }

    #[test]
    fn archiving_hides_a_session_without_deleting_it() {
        let conn = corpus();
        let archived = set_session_archived_on(&conn, "2", true).unwrap().unwrap();
        assert!(archived.archived_at.is_some());

        let active = list_sessions_on(&conn, 50, &SessionFilter::default()).unwrap();
        assert_eq!(active.len(), 2);
        assert!(!active.iter().any(|session| session.id == "2"));

        // The transcripts are untouched and still indexed.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transcripts WHERE session_id = '2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        let only_archived = SessionFilter {
            archived: ArchiveScope::Archived,
            ..Default::default()
        };
        let put_away = list_sessions_on(&conn, 50, &only_archived).unwrap();
        assert_eq!(put_away.len(), 1);
        assert_eq!(put_away[0].id, "2");

        let everything = SessionFilter {
            archived: ArchiveScope::All,
            ..Default::default()
        };
        assert_eq!(list_sessions_on(&conn, 50, &everything).unwrap().len(), 3);

        let restored = set_session_archived_on(&conn, "2", false).unwrap().unwrap();
        assert!(restored.archived_at.is_none());
        assert_eq!(
            list_sessions_on(&conn, 50, &SessionFilter::default())
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn archiving_an_unknown_session_reports_it() {
        let conn = corpus();
        assert!(set_session_archived_on(&conn, "no-such-session", true)
            .unwrap()
            .is_none());
    }

    #[test]
    fn search_defaults_to_the_unarchived_view() {
        let conn = corpus();
        set_session_archived_on(&conn, "2", true).unwrap();
        assert_eq!(ids(&search(&conn, "液态玻璃")), vec!["11"]);

        let everything = SessionFilter {
            archived: ArchiveScope::All,
            ..Default::default()
        };
        let all = search_transcripts_on(&conn, "液态玻璃", &everything, 50).unwrap();
        assert_eq!(ids(&all), vec!["22", "11"]);
        assert!(all.hits[0].archived, "an archived hit says so");
    }

    #[test]
    fn filter_options_come_from_the_data() {
        let conn = corpus();
        set_session_archived_on(&conn, "1", true).unwrap();
        let options = filter_options_on(&conn).unwrap();
        assert_eq!(options.languages, vec!["English", "中文"]);
        assert_eq!(
            options.models,
            vec!["fun-asr-nano-2512", "sensevoice-small"]
        );
        assert_eq!(options.earliest_date.as_deref(), Some("2026-08-01"));
        assert_eq!(options.latest_date.as_deref(), Some("2026-08-08"));
        assert_eq!(options.archived_count, 1);
    }

    #[test]
    fn an_index_built_before_trigram_is_rebuilt() {
        // A database from the shipped version: unicode61 index, Chinese in it.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE transcripts (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL, text TEXT NOT NULL,
                status TEXT NOT NULL, source TEXT NOT NULL, created_at TEXT NOT NULL,
                duration_ms INTEGER, model TEXT NOT NULL, language TEXT NOT NULL
            );
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, started_at TEXT NOT NULL,
                ended_at TEXT, date_key TEXT NOT NULL, model TEXT NOT NULL,
                language TEXT NOT NULL, runtime TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE transcripts_fts USING fts5(
                text, content='transcripts', content_rowid='rowid'
            );
            CREATE TRIGGER transcripts_ai AFTER INSERT ON transcripts BEGIN
                INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
            END;
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions
             (id, title, started_at, ended_at, date_key, model, language, runtime)
             VALUES ('1', 'old', '2026-08-01T09:00:00Z', NULL, '2026-08-01', 'm', '中文', 'r')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transcripts
             (id, session_id, text, status, source, created_at, duration_ms, model, language)
             VALUES ('11', '1', ?1, 'final', 'mic', '2026-08-01T09:00:00Z', NULL, 'm', '中文')",
            params![ZH_GLASS],
        )
        .unwrap();

        // The pre-existing index cannot answer the query. Asked directly,
        // because the schema this database has is too old for the search query
        // to even compile against.
        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transcripts_fts WHERE transcripts_fts MATCH '\"液态玻璃\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, 0);

        // After the migration it can, without re-inserting a single row.
        apply_schema(&conn).unwrap();
        let response =
            search_transcripts_on(&conn, "液态玻璃", &SessionFilter::default(), 50).unwrap();
        assert_eq!(ids(&response), vec!["11"]);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM transcripts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1,
            "the rebuild must not touch the content table"
        );
    }

    #[test]
    fn the_index_tracks_edits_and_deletes() {
        let conn = corpus();
        conn.execute(
            "UPDATE transcripts SET text = '完全换掉的内容，谈的是天气' WHERE id = '11'",
            [],
        )
        .unwrap();
        assert_eq!(ids(&search(&conn, "液态玻璃")), vec!["22"]);
        assert_eq!(
            ids(&search(&conn, "天气预报或者天气")),
            Vec::<String>::new()
        );
        assert_eq!(ids(&search(&conn, "换掉的内容")), vec!["11"]);

        conn.execute("DELETE FROM transcripts WHERE id = '22'", [])
            .unwrap();
        assert!(search(&conn, "液态玻璃").hits.is_empty());
    }

    #[test]
    fn snippets_centre_on_the_match_and_count_characters() {
        let terms = vec!["玻璃".to_string()];
        let text = format!("{}玻璃{}", "前".repeat(300), "后".repeat(300));
        let snippet = build_snippet(&text, &terms, 60);
        // 60 characters of window, plus an ellipsis at each end. A byte-based
        // window would have produced 20 Chinese characters here.
        assert_eq!(snippet.chars().count(), 62, "{snippet}");
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert!(snippet.contains("玻璃"));
        // Some context before the match, not the match flush at the edge.
        assert!(snippet.chars().nth(1) == Some('前'));
    }

    #[test]
    fn short_text_is_returned_whole() {
        let terms = vec!["玻璃".to_string()];
        assert_eq!(
            build_snippet("  液态玻璃的质感  ", &terms, 60),
            "液态玻璃的质感"
        );
    }

    #[test]
    fn a_snippet_falls_back_to_the_head_when_nothing_matches() {
        // The scan path can match on one term while the snippet is built from
        // another; the window must still be valid text rather than a panic.
        let snippet = build_snippet(&"中".repeat(200), &["никогда".to_string()], 40);
        assert_eq!(snippet.chars().count(), 41);
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn fts_expressions_quote_every_term() {
        assert_eq!(fts_match_expression(&["液态玻璃".into()]), "\"液态玻璃\"");
        assert_eq!(
            fts_match_expression(&["alpha".into(), "beta".into()]),
            "\"alpha\" \"beta\""
        );
        assert_eq!(
            fts_match_expression(&["say \"hi\"".into()]),
            "\"say \"\"hi\"\"\""
        );
    }

    #[test]
    fn like_patterns_escape_wildcards() {
        assert_eq!(like_pattern("100%"), "%100\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("c:\\d"), "%c:\\\\d%");
        assert_eq!(like_pattern("液态"), "%液态%");
    }

    #[test]
    fn terms_split_on_any_whitespace() {
        assert_eq!(
            search_terms("  液态玻璃   UI \t 拖动 "),
            vec!["液态玻璃", "UI", "拖动"]
        );
        assert!(search_terms("   ").is_empty());
    }

    /// Search a real database, migration and all.
    ///
    ///     FUN_ASR_REAL_DB=~/.local/share/dev.yubo.fun-asr-desktop/fun_asr_desktop.sqlite3 \
    ///       cargo test real_database -- --nocapture
    ///
    /// Skipped unless that variable is set, because it asserts against whatever
    /// text a particular machine happens to hold. The fixtures above are a
    /// developer's idea of what dictated Mandarin looks like; this is the real
    /// thing, with its own punctuation, its own ASR errors and its own mixing
    /// of scripts mid-sentence. It works on a copy, so the live database is
    /// never touched.
    #[test]
    fn real_database_migrates_and_matches_its_own_text() {
        let Ok(source) = std::env::var("FUN_ASR_REAL_DB") else {
            eprintln!("skipped: set FUN_ASR_REAL_DB to a fun_asr_desktop.sqlite3 to run this");
            return;
        };

        let copy = std::env::temp_dir().join(format!("fun-asr-search-{}.sqlite3", Uuid::new_v4()));
        std::fs::copy(&source, &copy).expect("copy the database");
        let conn = Connection::open(&copy).expect("open the copy");
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_schema(&conn).expect("migrate");

        // Every transcript with enough CJK to be worth searching, and a
        // substring taken from the middle of it — the position the default
        // tokenizer can never reach.
        let rows: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, text FROM transcripts WHERE length(text) > 30")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<(String, String)>, _>>()
                .unwrap();
            rows
        };

        let mut checked = 0;
        for (id, text) in &rows {
            let chars: Vec<char> = text.chars().collect();
            // A run of Han characters from the middle, so the needle is CJK
            // rather than an easy Latin word.
            let Some(start) = (chars.len() / 3..chars.len().saturating_sub(4)).find(|&i| {
                chars[i..i + 4]
                    .iter()
                    .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
            }) else {
                continue;
            };
            let long: String = chars[start..start + 4].iter().collect();
            // Two characters is the shape of an enormous share of Chinese
            // words — 数据, 拖拽, 玻璃 — and is below the trigram floor, so it
            // exercises the scan. Getting this wrong is invisible in English.
            let short: String = chars[start..start + 2].iter().collect();

            for (needle, expected) in [(&long, SearchMode::Fts), (&short, SearchMode::Substring)] {
                let response =
                    search_transcripts_on(&conn, needle, &SessionFilter::default(), 200).unwrap();
                assert_eq!(response.mode, expected, "wrong path for {needle:?}");
                let hit = response
                    .hits
                    .iter()
                    .find(|hit| &hit.transcript_id == id)
                    .unwrap_or_else(|| {
                        panic!(
                            "searching {needle:?} did not find the transcript it came from ({id})"
                        )
                    });
                assert!(
                    hit.snippet.contains(needle),
                    "the snippet for {needle:?} does not show the match: {}",
                    hit.snippet
                );
                eprintln!(
                    "{needle}  ->  {} hit(s) via {:?}  |  {} · {}",
                    response.hits.len(),
                    response.mode,
                    hit.session_title,
                    hit.snippet
                );
            }
            checked += 1;
        }

        let _ = std::fs::remove_file(&copy);
        assert!(checked > 0, "no Chinese transcripts found in {source}");
        eprintln!("{checked} real Chinese transcripts, each found by a substring of itself");
    }
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

    // Name the session after what was actually said.
    //
    // A sidebar of "Voice 15:41 / Voice 14:13 / Voice 13:02" carries no
    // information at all — every row looks identical and finding anything
    // means opening them one by one. The first thing spoken is almost always
    // the best short label, and it costs nothing to derive.
    if status == "final" && !text.trim().is_empty() {
        let title = summarize_for_title(text);
        if !title.is_empty() {
            // Only rename while the session still has its generated name, so a
            // title the user set by hand is never overwritten.
            conn.execute(
                "UPDATE sessions SET title = ?1
                   WHERE id = ?2
                     AND (title LIKE 'Voice %' OR title LIKE 'Session %'
                          OR title = 'Voice note' OR title = '')",
                params![title, session_id],
            )
            .map_err(|err| err.to_string())?;
        }
    }

    Ok(transcript)
}

/// First clause of a transcript, trimmed to something that fits a sidebar.
///
/// Counts characters rather than bytes: Chinese is three bytes per character
/// in UTF-8, so a byte limit would cut CJK titles to a third the length of
/// English ones, and could slice a character in half.
fn summarize_for_title(text: &str) -> String {
    const MAX_CHARS: usize = 28;
    let cleaned = text.trim();

    // Prefer a natural break, in either script's punctuation.
    let first = cleaned
        .split(['。', '！', '？', '\n', '.', '!', '?'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(cleaned);

    let mut chars = first.chars();
    let head: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

#[cfg(test)]
mod title_tests {
    use super::summarize_for_title;

    #[test]
    fn takes_the_first_sentence() {
        assert_eq!(summarize_for_title("你好。第二句"), "你好");
        assert_eq!(summarize_for_title("Hello there. Second"), "Hello there");
    }

    #[test]
    fn truncates_by_characters_not_bytes() {
        // 40 Chinese characters is 120 bytes; a byte limit would cut this to a
        // third of the intended length, or split a character.
        let long = "中".repeat(40);
        let title = summarize_for_title(&long);
        assert_eq!(title.chars().count(), 29, "28 chars plus the ellipsis");
        assert!(title.ends_with('…'));
    }

    #[test]
    fn empty_stays_empty() {
        assert_eq!(summarize_for_title("   "), "");
    }
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
