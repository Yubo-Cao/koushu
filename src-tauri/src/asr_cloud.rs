//! Cloud speech-to-text over the OpenAI `/v1/audio/transcriptions` protocol.
//!
//! One protocol, several providers: OpenAI, Groq (whisper-large-v3-turbo),
//! and self-hosted whisper.cpp / faster-whisper servers all speak it. Pointing
//! `base_url` at localhost keeps everything on the machine, exactly like the
//! LLM layer.
//!
//! ## Why this exists alongside a perfectly good local model
//!
//! Not for privacy reasons — that argument does not survive contact with the
//! facts. Once the LLM formatting layer is enabled the transcript is already
//! leaving the machine, and the transcript *is* the content; the audio is just
//! its carrier. The real privacy boundary is "cloud at all", and it has
//! already been crossed by then.
//!
//! It exists because a hosted model is simply more accurate than an 800M local
//! one on hard input: heavy accents, code-switching, and domain jargon. The
//! local runtime stays in the loop as the low-latency preview — it is 8-20x
//! realtime and needs no network — while the cloud pass produces the committed
//! text. Neither replaces the other.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Endpoint and credentials for the transcription service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAsrConfig {
    /// API root without the `/audio/transcriptions` suffix.
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    /// ISO-639-1 hint. Omitted when empty, which lets the service autodetect —
    /// the right default for code-switched speech.
    #[serde(default)]
    pub language: String,
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
}

/// Upload a WAV and return its transcript.
pub fn transcribe(config: &CloudAsrConfig, wav_path: &Path) -> Result<String, String> {
    if config.base_url.trim().is_empty() {
        return Err("No cloud ASR base URL configured.".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("No cloud ASR model configured.".to_string());
    }

    let bytes = std::fs::read(wav_path).map_err(|err| err.to_string())?;
    let part = reqwest::blocking::multipart::Part::bytes(bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|err| err.to_string())?;

    let mut form = reqwest::blocking::multipart::Form::new()
        .text("model", config.model.clone())
        .part("file", part);
    if !config.language.trim().is_empty() {
        form = form.text("language", config.language.trim().to_string());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|err| err.to_string())?;

    let url = format!(
        "{}/audio/transcriptions",
        config.base_url.trim_end_matches('/')
    );
    let mut request = client.post(&url).multipart(form);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }

    let response = request.send().map_err(|err| err.to_string())?;
    let status = response.status();
    let raw = response.text().map_err(|err| err.to_string())?;

    if !status.is_success() {
        // Surface the provider's own message; a bare status code does not say
        // whether the key, the model name, or the URL is at fault.
        let detail = serde_json::from_str::<ApiErrorBody>(&raw)
            .ok()
            .and_then(|body| body.error.and_then(|e| e.message))
            .unwrap_or_else(|| raw.chars().take(300).collect());
        return Err(format!("Cloud ASR failed (HTTP {status}): {detail}"));
    }

    // `response_format=json` is the default and yields {"text": "..."}, but
    // some servers return bare text; accept both rather than failing on a
    // response that is perfectly usable.
    let text = serde_json::from_str::<TranscriptionResponse>(&raw)
        .ok()
        .and_then(|body| body.text)
        .unwrap_or(raw);

    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Cloud ASR returned an empty transcript.".to_string());
    }
    Ok(text)
}
