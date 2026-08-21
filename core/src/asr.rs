//! The local speech recogniser: the official Fun-ASR llama.cpp runtimes.
//!
//! This is slice 3 of `docs/core-extraction.md`, and it is the reason the split
//! exists at all — it is the part neither shell should own a copy of. What it
//! does is narrow: given a 16 kHz mono WAV and a directory of GGUF files, run
//! the right runtime binary and return what it said.
//!
//! ## What is deliberately *not* here
//!
//! **Recording.** The two shells capture audio in completely different ways —
//! `cpal` on Linux, `AVAudioEngine` on macOS — and neither is better in the
//! abstract. Both can write a 16 kHz mono WAV, so that file is the boundary. A
//! byte buffer over the FFI would have been the obvious alternative and is
//! worse: the runtimes take a path anyway, so it would mean copying tens of
//! megabytes across a C ABI in order to write it back out to a temporary file.
//!
//! **Where the models live.** Resolved by the caller, because that is a
//! question about the platform's directory layout, not about recognition.
//!
//! ## Failures are data
//!
//! [`AsrOutcome`] carries `failure: Option<String>` rather than this returning a
//! `Result`. A runtime that cannot start, a model file that is missing, a
//! process that exits non-zero — each of those already has a sentence the user
//! can act on ("`fsmn-vad.gguf` is missing from …"), and every one of them can
//! happen on a path where the recording still exists and is still worth keeping.
//! Turning them into an FFI error would make each shell translate them back, and
//! UniFFI's generated `errorDescription` would print a debug dump of the variant
//! instead of the sentence inside it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Backend id for Fun-ASR-Nano on the official llama.cpp runtime.
pub const BACKEND_NANO: &str = "funasr-nano-gguf-cpu";
/// Backend id for SenseVoiceSmall on the official llama.cpp runtime.
pub const BACKEND_SENSEVOICE: &str = "funasr-sensevoice-gguf-cpu";
/// Backend id for a hosted OpenAI-compatible transcription endpoint. It has no
/// local assets, so it is never downloaded — only configured.
pub const BACKEND_CLOUD: &str = "cloud-openai-transcriptions";

/// One file that has to be present before a GGUF model can run.
pub struct GgufAsset {
    pub repo_id: &'static str,
    pub filename: &'static str,
}

/// Fun-ASR-Nano needs the audio encoder, the Qwen3 decoder, and the shared VAD.
///
/// q4km is the default: measured on this project it is both faster and no less
/// accurate than q8_0 (8.8x vs 7.8x realtime on a 30 s clip).
pub const NANO_ASSETS: &[GgufAsset] = &[
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
pub const SENSEVOICE_ASSETS: &[GgufAsset] = &[
    GgufAsset {
        repo_id: "FunAudioLLM/SenseVoiceSmall-GGUF",
        filename: "sensevoice-small-q8.gguf",
    },
    GgufAsset {
        repo_id: "FunAudioLLM/fsmn-vad-GGUF",
        filename: "fsmn-vad.gguf",
    },
];

pub fn gguf_assets_for(backend: &str) -> Option<&'static [GgufAsset]> {
    match backend {
        BACKEND_NANO => Some(NANO_ASSETS),
        BACKEND_SENSEVOICE => Some(SENSEVOICE_ASSETS),
        _ => None,
    }
}

/// Where the shell put the runtime binaries.
///
/// Passed in rather than discovered, because "next to the executable" means
/// `Contents/Resources` in an `.app`, `usr/lib` in an AppImage, and a target
/// directory in a dev build. The core has no way to be right about that and no
/// business guessing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AsrRuntimePaths {
    /// `llama-funasr-cli` — Fun-ASR-Nano.
    pub nano_cli: String,
    /// `llama-funasr-sensevoice` — SenseVoiceSmall.
    pub sensevoice_cli: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AsrRequest {
    /// One of the `BACKEND_*` ids.
    pub backend: String,
    /// Directory holding this model's GGUF files.
    pub model_dir: String,
    /// A 16 kHz mono WAV. Both runtimes accept other rates and resample
    /// internally, but doing it once at capture is cheaper and deterministic.
    pub wav_path: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AsrOutcome {
    /// What was said. Empty when `failure` is set, and also legitimately empty
    /// when the recording contained no speech — those are different situations
    /// and the caller can tell them apart by looking at `failure`.
    pub text: String,
    /// The backend that actually ran, for the transcript row.
    pub runtime: String,
    /// A sentence the user can act on. Show it; do not re-translate it.
    pub failure: Option<String>,
    /// Wall-clock time the runtime took, for the "is this fast enough?"
    /// question the settings screen exists to answer.
    pub elapsed_ms: u64,
    /// True when [`AsrJob::cancel`] ended it. Not a failure: the user let go of
    /// the key, which is an ordinary thing to do.
    pub cancelled: bool,
}

/// Files this model needs that are not on disk.
///
/// Exported separately from transcription so the models screen can say what is
/// wrong before somebody holds the key and waits for silence.
#[uniffi::export]
pub fn missing_assets(backend: String, model_dir: String) -> Vec<String> {
    let Some(assets) = gguf_assets_for(&backend) else {
        return Vec::new();
    };
    let dir = PathBuf::from(model_dir);
    assets
        .iter()
        .filter(|asset| !dir.join(asset.filename).exists())
        .map(|asset| asset.filename.to_string())
        .collect()
}

/// One transcription, with a way to stop it.
///
/// An object rather than a bare function because cancellation has to be
/// expressible: dropping a future does not cross a C ABI, and these runs take
/// seconds — long enough that a user who has changed their mind should not have
/// to wait for a result nobody will read. `cancel` kills the child process,
/// which is the only thing that actually stops the work.
#[derive(uniffi::Object)]
pub struct AsrJob {
    child: Mutex<Option<Child>>,
    cancelled: AtomicBool,
}

#[uniffi::export]
impl AsrJob {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            child: Mutex::new(None),
            cancelled: AtomicBool::new(false),
        })
    }

    /// Blocks until the runtime finishes. Call it off whatever thread the UI
    /// lives on; `cancel` is safe to call from another thread meanwhile.
    pub fn run(&self, runtime: AsrRuntimePaths, request: AsrRequest) -> AsrOutcome {
        let started = Instant::now();
        let outcome = |text: String, failure: Option<String>| AsrOutcome {
            text,
            runtime: request.backend.clone(),
            failure,
            elapsed_ms: started.elapsed().as_millis() as u64,
            cancelled: self.cancelled.load(Ordering::SeqCst),
        };

        let (binary, args) = match self.plan(&runtime, &request) {
            Ok(plan) => plan,
            Err(message) => return outcome(String::new(), Some(message)),
        };

        let mut command = low_priority_command(&binary);
        command.args(&args).stdout(Stdio::piped()).stderr(Stdio::piped());

        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                return outcome(
                    String::new(),
                    Some(format!("Could not start {}: {err}", binary.display())),
                )
            }
        };

        // Published before waiting, so a `cancel` that lands mid-run has
        // something to kill. A cancel that lands *before* this is caught by the
        // flag check below.
        {
            let mut slot = self.child.lock().unwrap();
            if self.cancelled.load(Ordering::SeqCst) {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return outcome(String::new(), None);
            }
            *slot = Some(child);
        }

        let output = {
            let mut slot = self.child.lock().unwrap();
            match slot.take() {
                Some(child) => child.wait_with_output(),
                None => return outcome(String::new(), None), // cancelled
            }
        };

        if self.cancelled.load(Ordering::SeqCst) {
            return outcome(String::new(), None);
        }

        match output {
            Ok(output) if output.status.success() => {
                outcome(clean_runtime_stdout(&output.stdout), None)
            }
            Ok(output) => outcome(
                String::new(),
                Some(compact_process_error(&output.stdout, &output.stderr)),
            ),
            Err(err) => outcome(String::new(), Some(err.to_string())),
        }
    }

    /// Kill the runtime. Idempotent, and safe before `run` has started.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

impl AsrJob {
    /// Which binary, and with which flags.
    ///
    /// Kept apart from `run` so the argument lists — which are the part that
    /// silently breaks when a runtime release changes — can be read in one
    /// place and tested without spawning anything.
    fn plan(
        &self,
        runtime: &AsrRuntimePaths,
        request: &AsrRequest,
    ) -> Result<(PathBuf, Vec<PathBuf>), String> {
        let dir = PathBuf::from(&request.model_dir);
        let missing = missing_assets(request.backend.clone(), request.model_dir.clone());
        if !missing.is_empty() {
            return Err(format!(
                "{} is missing from {}. Download the model again from the settings screen.",
                missing.join(", "),
                dir.display()
            ));
        }

        let wav = PathBuf::from(&request.wav_path);
        if !wav.exists() {
            return Err(format!("The recording is gone: {} .", wav.display()));
        }

        match request.backend.as_str() {
            // The official CLI has no `--language` flag — Nano detects language
            // itself, and the built-in ggml FSMN-VAD does the segmentation.
            BACKEND_NANO => {
                let binary = PathBuf::from(&runtime.nano_cli);
                require(&binary, "Fun-ASR")?;
                Ok((
                    binary,
                    vec![
                        PathBuf::from("--enc"),
                        dir.join("funasr-encoder-f16.gguf"),
                        PathBuf::from("-m"),
                        dir.join("qwen3-0.6b-q4km.gguf"),
                        PathBuf::from("-a"),
                        wav,
                        PathBuf::from("--vad"),
                        dir.join("fsmn-vad.gguf"),
                    ],
                ))
            }
            BACKEND_SENSEVOICE => {
                let binary = PathBuf::from(&runtime.sensevoice_cli);
                require(&binary, "SenseVoice")?;
                Ok((
                    binary,
                    vec![
                        PathBuf::from("-m"),
                        dir.join("sensevoice-small-q8.gguf"),
                        PathBuf::from("-a"),
                        wav,
                        PathBuf::from("--vad"),
                        dir.join("fsmn-vad.gguf"),
                    ],
                ))
            }
            other => Err(format!("No local runtime for backend {other}.")),
        }
    }
}

fn require(binary: &Path, label: &str) -> Result<(), String> {
    if binary.exists() {
        Ok(())
    } else {
        Err(format!(
            "The bundled {label} runtime is missing from the app resources ({}).",
            binary.display()
        ))
    }
}

/// Both official runtimes keep stdout clean: every log, timing and VAD line goes
/// to stderr, and stdout carries only transcript text (verified against
/// runtime-llamacpp-v0.1.9). One line per VAD segment, so join them.
///
/// Deliberately no content-based filtering — a transcript may legitimately begin
/// with any character, and dropping lines by prefix would silently eat it.
pub fn clean_runtime_stdout(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prefer stderr, which is where these runtimes put the reason.
pub fn compact_process_error(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    if message.len() > 1800 {
        let mut cut = 1800;
        // Never split a UTF-8 sequence: these messages are frequently Chinese,
        // and a byte-sliced string would be replaced wholesale by U+FFFD.
        while cut > 0 && !message.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...", &message[..cut])
    } else if message.is_empty() {
        "The runtime failed without printing anything.".to_string()
    } else {
        message
    }
}

/// Run at a lower priority, so a transcription does not make the machine feel
/// slow while the user carries on working. `nice` is used when present rather
/// than `setpriority`, because it needs no unsafe and no libc dependency.
fn low_priority_command(program: &Path) -> Command {
    if which("nice").is_some() {
        let mut command = Command::new("nice");
        command.arg("-n").arg("10").arg(program);
        command
    } else {
        Command::new(program)
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> AsrRuntimePaths {
        AsrRuntimePaths {
            nano_cli: "/nonexistent/llama-funasr-cli".into(),
            sensevoice_cli: "/nonexistent/llama-funasr-sensevoice".into(),
        }
    }

    #[test]
    fn missing_model_files_are_named() {
        let missing = missing_assets(BACKEND_NANO.to_string(), "/nonexistent".to_string());
        assert_eq!(missing.len(), NANO_ASSETS.len());
        assert!(missing.contains(&"fsmn-vad.gguf".to_string()));
    }

    #[test]
    fn an_unknown_backend_has_no_assets_rather_than_panicking() {
        assert!(missing_assets("something-else".to_string(), "/tmp".to_string()).is_empty());
    }

    #[test]
    fn a_missing_model_fails_with_a_sentence_not_a_code() {
        let job = AsrJob::new();
        let outcome = job.run(
            paths(),
            AsrRequest {
                backend: BACKEND_NANO.to_string(),
                model_dir: "/nonexistent".to_string(),
                wav_path: "/nonexistent/a.wav".to_string(),
            },
        );
        let failure = outcome.failure.expect("should have failed");
        assert!(failure.contains("missing"), "unhelpful: {failure}");
        assert!(failure.contains("fsmn-vad.gguf"), "does not say which: {failure}");
        assert!(outcome.text.is_empty());
    }

    #[test]
    fn cancelling_before_running_never_starts_the_process() {
        let job = AsrJob::new();
        job.cancel();
        let outcome = job.run(
            paths(),
            AsrRequest {
                backend: BACKEND_NANO.to_string(),
                model_dir: "/nonexistent".to_string(),
                wav_path: "/nonexistent/a.wav".to_string(),
            },
        );
        assert!(outcome.cancelled);
    }

    #[test]
    fn stdout_is_joined_and_blank_lines_dropped() {
        assert_eq!(clean_runtime_stdout(b"  hello \n\n world \n"), "hello world");
    }

    #[test]
    fn a_long_chinese_error_is_cut_on_a_character_boundary() {
        let long = "转写失败".repeat(400);
        let message = compact_process_error(b"", long.as_bytes());
        assert!(message.ends_with("..."));
        // The real assertion: it is still valid UTF-8 that reads as Chinese
        // rather than a string of replacement characters.
        assert!(!message.contains('\u{FFFD}'));
    }
}
