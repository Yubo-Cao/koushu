//! Turn a raw dictation transcript into typeset Markdown with an external LLM.
//!
//! Deliberately one protocol, not one vendor: the OpenAI `/v1/chat/completions`
//! shape is spoken by OpenAI, DeepSeek, Qwen, Moonshot, vLLM, llama.cpp's
//! server, LM Studio and Ollama. Pointing `base_url` at a local server is the
//! offline path — it is the same code, not a second implementation.
//!
//! Output streams, because this runs *after* the transcript is already on
//! screen. The user should watch the tidied version fill in rather than stare
//! at a spinner.

use std::io::{BufRead, BufReader};

use serde::{Deserialize, Serialize};

pub mod presets;

/// Where to send requests, and as whom.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    /// API root *without* the `/chat/completions` suffix, e.g.
    /// `https://api.openai.com/v1` or `http://localhost:11434/v1`.
    pub base_url: String,
    pub model: String,
    /// Empty is allowed and normal for local servers, which ignore auth.
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub temperature: Option<f32>,
}

impl LlmConfig {
    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

/// Error body shape shared by OpenAI-compatible servers.
#[derive(Deserialize)]
struct ApiErrorBody {
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
}

/// Stream a formatting request, invoking `on_delta` for each text fragment.
///
/// Returns the complete text. `cancel` is polled between chunks so a superseded
/// request stops promptly instead of burning tokens on a result nobody wants.
pub fn format_streaming<F, C>(
    config: &LlmConfig,
    system_prompt: &str,
    transcript: &str,
    mut on_delta: F,
    cancel: C,
) -> Result<String, String>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if config.base_url.trim().is_empty() {
        return Err("No LLM base URL configured.".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("No LLM model configured.".to_string());
    }

    let body = ChatRequest {
        model: &config.model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: system_prompt,
            },
            ChatMessage {
                role: "user",
                content: transcript,
            },
        ],
        stream: true,
        temperature: config.temperature,
    };

    let client = reqwest::blocking::Client::builder()
        // Generous: a long meeting transcript on a slow local model is not an
        // error, it is just slow.
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|err| err.to_string())?;

    let mut request = client.post(config.endpoint()).json(&body);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }

    let response = request.send().map_err(|err| err.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let raw = response.text().unwrap_or_default();
        // Surface the server's own message; "HTTP 400" alone tells the user
        // nothing about which of key, model or URL is wrong.
        let detail = serde_json::from_str::<ApiErrorBody>(&raw)
            .ok()
            .and_then(|body| body.error.and_then(|e| e.message))
            .unwrap_or_else(|| raw.chars().take(300).collect());
        return Err(format!("LLM request failed (HTTP {status}): {detail}"));
    }

    let mut reader = BufReader::new(response);
    let mut text = String::new();
    let mut line = String::new();

    loop {
        if cancel() {
            return Err("cancelled".to_string());
        }
        line.clear();
        let read = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        let line = line.trim();
        // Server-sent events: payload lines start with "data: ", everything
        // else (blank separators, ": ping" comments) is framing.
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) else {
            // Tolerate keepalives and unknown chunk shapes rather than aborting
            // a response that is otherwise fine.
            continue;
        };
        for choice in chunk.choices {
            if let Some(delta) = choice.delta.content {
                if !delta.is_empty() {
                    text.push_str(&delta);
                    on_delta(&delta);
                }
            }
        }
    }

    if text.trim().is_empty() {
        return Err("LLM returned an empty response.".to_string());
    }
    Ok(text)
}
