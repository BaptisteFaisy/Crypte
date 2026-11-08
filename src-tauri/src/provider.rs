//! Minimal OpenAI-compatible chat client (non-streaming).
//!
//! Same wire contract as the TUI branch: plain `content` string for
//! text-only turns, content parts (`text` + `image_url` data URLs)
//! for imaged turns, tolerant response parsing (`string` | parts |
//! `null`). Config from `KH_BASE_URL` / `KH_API_KEY` / `KH_MODEL`
//! (`KH_TIMEOUT_S` optional). The key never appears in errors.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default base URL (OpenAI-compatible gateway).
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Default model when `KH_MODEL` is missing.
const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// Default HTTP timeout, seconds (override: `KH_TIMEOUT_S`).
const DEFAULT_TIMEOUT_S: u64 = 120;

/// Provider config: base URL + key + model (key never serialized).
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_s: u64,
}

impl ProviderConfig {
    /// From the environment (`KH_*`, same names as the TUI branch).
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("KH_API_KEY").context("KH_API_KEY is missing")?;
        if api_key.trim().is_empty() {
            anyhow::bail!("KH_API_KEY is empty");
        }
        let timeout_s = std::env::var("KH_TIMEOUT_S")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_S);
        Ok(Self {
            base_url: std::env::var("KH_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into()),
            api_key,
            model: std::env::var("KH_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into()),
            timeout_s,
        })
    }

    /// Public status (base + model, never the key).
    pub fn status(&self) -> ProviderStatus {
        ProviderStatus {
            base_url: self.base_url.clone(),
            model: self.model.clone(),
        }
    }
}

/// Public provider status for the frontend (no secret).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub base_url: String,
    pub model: String,
}

/// Wire message: plain text, or OpenAI content parts when imaged.
#[derive(Debug, Clone, Serialize)]
pub struct WireMsg {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<WireContent>,
}

/// Wire content: simple string (unchanged JSON) or parts array.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum WireContent {
    Text(String),
    Parts(Vec<WirePart>),
}

/// One OpenAI part: `{type: text, text}` or `{type: image_url,
/// image_url: {url}}` (absent fields omitted).
#[derive(Debug, Clone, Serialize)]
pub struct WirePart {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<WireImage>,
}

/// OpenAI image URL (`data:` base64 inline).
#[derive(Debug, Clone, Serialize)]
pub struct WireImage {
    url: String,
}

impl WireMsg {
    /// Plain text message (any role).
    pub fn text(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: Some(WireContent::Text(content.to_string())),
        }
    }

    /// Imaged `user` turn: `text` part (when non-empty) + one
    /// `image_url` part per data URL.
    pub fn user_rich(text: &str, data_urls: &[String]) -> Self {
        let mut parts = Vec::with_capacity(data_urls.len() + 1);
        if !text.is_empty() {
            parts.push(WirePart {
                kind: "text",
                text: Some(text.to_string()),
                image_url: None,
            });
        }
        for url in data_urls {
            parts.push(WirePart {
                kind: "image_url",
                text: None,
                image_url: Some(WireImage { url: url.clone() }),
            });
        }
        Self {
            role: String::from("user"),
            content: Some(WireContent::Parts(parts)),
        }
    }
}

/// Chat completion: reply text + billed usage (0 when absent).
#[derive(Debug, Clone, Default)]
pub struct Completion {
    pub text: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Sends `messages` (non-streaming), parses text + usage. Network
/// errors name the base URL, never the key.
pub async fn complete(config: &ProviderConfig, messages: &[WireMsg]) -> Result<Completion> {
    let body = ChatRequest {
        model: config.model.clone(),
        messages: messages.to_vec(),
        stream: false,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_s))
        .build()
        .context("cannot build HTTP client")?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("provider unreachable ({url})"))?;
    let status = resp.status();
    let raw = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("provider error {status}: {}", truncate(&raw));
    }
    let parsed: ChatResponse =
        serde_json::from_str(&raw).context("cannot parse provider response")?;
    Ok(from_response(parsed))
}

/// Request body (keys `model`/`messages`, `stream: false`).
#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<WireMsg>,
    stream: bool,
}

/// Response subset: first choice + usage (tolerant).
#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    message: ResponseMessage,
}

#[derive(Debug, Default, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// First choice → text (string, parts concatenated, `null` → `""`).
fn from_response(resp: ChatResponse) -> Completion {
    let text = resp
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .map(|raw| recv_text(&raw))
        .unwrap_or_default();
    Completion {
        text,
        prompt_tokens: resp.usage.prompt_tokens,
        completion_tokens: resp.usage.completion_tokens,
    }
}

/// Response text from a raw `content` value.
fn recv_text(raw: &serde_json::Value) -> String {
    match raw {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text")?.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// First 200 chars of an error body (single line, no secret inside —
/// error bodies never contain the key, only the base URL is named).
fn truncate(raw: &str) -> String {
    raw.chars().take(200).collect::<String>().replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_rich_serialises_openai_parts() {
        let msg = WireMsg::user_rich("look", &[String::from("data:image/png;base64,AAA")]);
        let raw = serde_json::to_value(&msg).unwrap();
        assert_eq!(raw["role"], "user");
        assert_eq!(raw["content"][0], json!({"type": "text", "text": "look"}));
        assert_eq!(raw["content"][1]["type"], "image_url");
        assert_eq!(
            raw["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAA"
        );
        let bare = WireMsg::user_rich("", &[String::from("data:x")]);
        let raw = serde_json::to_value(&bare).unwrap();
        assert_eq!(raw["content"].as_array().unwrap().len(), 1);
        let plain = WireMsg::text("user", "hi");
        assert_eq!(
            serde_json::to_value(&plain).unwrap()["content"],
            json!("hi"),
            "text-only stays a string"
        );
    }

    #[test]
    fn from_response_reads_string_parts_and_usage() {
        let raw = json!({
            "choices": [{"message": {"content": "hello"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 5},
        });
        let done: ChatResponse = serde_json::from_value(raw).unwrap();
        let completion = from_response(done);
        assert_eq!(completion.text, "hello");
        assert_eq!(completion.prompt_tokens, 3);
        assert_eq!(completion.completion_tokens, 5);
        let parts = json!({
            "choices": [{"message": {"content": [
                {"type": "text", "text": "a"},
                {"type": "text", "text": "b"},
            ]}}],
        });
        let done: ChatResponse = serde_json::from_value(parts).unwrap();
        assert_eq!(from_response(done).text, "ab");
        let none = json!({"choices": [{"message": {}}]});
        let done: ChatResponse = serde_json::from_value(none).unwrap();
        assert_eq!(from_response(done).text, "");
    }

    #[test]
    fn config_defaults_and_key_required() {
        unsafe {
            std::env::set_var("KH_TAURI_TEST_GUARD", "1");
            std::env::remove_var("KH_API_KEY");
        }
        assert!(ProviderConfig::from_env().is_err(), "no key, no client");
        unsafe {
            std::env::set_var("KH_API_KEY", "secret");
            std::env::remove_var("KH_BASE_URL");
            std::env::remove_var("KH_MODEL");
        }
        let config = ProviderConfig::from_env().unwrap();
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.model, DEFAULT_MODEL);
        let status = config.status();
        let shown = serde_json::to_string(&status).unwrap();
        assert!(!shown.contains("secret"), "key never serialised");
        unsafe {
            std::env::remove_var("KH_API_KEY");
            std::env::remove_var("KH_TAURI_TEST_GUARD");
        }
    }
}
