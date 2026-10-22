//! Bring-your-own-key runtime (spec 1.5): no CLI, the daemon streams the
//! provider API itself with the user's key, the Anthropic Messages API or
//! any OpenAI-compatible `chat/completions` endpoint.
//!
//! The profile has no tools, so the model is told to answer with one
//! `<artifact>` block; the engine turns it into a project file like any
//! plain-text runtime. The key lives in `<data>/app-config.json` under
//! `byok` (written `0600` by the daemon) and is only ever sent to the
//! configured endpoint.

use od_contracts::{AgentEvent, FailureCategory};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

/// `app-config.json` key holding [`ByokConfig`].
pub const APP_CONFIG_KEY: &str = "byok";

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-5";
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
const DEFAULT_MAX_TOKENS: u64 = 64_000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Prepended as the system prompt: one self-contained `<artifact>`.
pub const SYSTEM_PROMPT: &str = "You are running inside Open Design without tools: you cannot \
read or write files or run commands. Answer with exactly one \
<artifact identifier=\"index\" type=\"text/html\" title=\"...\"> ... </artifact> block that \
contains a complete, self-contained HTML document (all CSS and JavaScript inline, no external \
dependencies). One or two plain sentences before the block are fine. Do not wrap the artifact \
in Markdown code fences.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByokProtocol {
    Anthropic,
    /// OpenAI or any OpenAI-compatible endpoint.
    OpenAi,
}

/// The user's provider settings.
#[derive(Clone, PartialEq, Eq)]
pub struct ByokConfig {
    pub protocol: ByokProtocol,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u64>,
}

impl std::fmt::Debug for ByokConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ByokConfig")
            .field("protocol", &self.protocol)
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

fn non_empty(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

impl ByokConfig {
    /// Reads `config.byok`; `None` without a protocol or a key.
    #[must_use]
    pub fn from_app_config(config: &Value) -> Option<Self> {
        Self::from_value(config.get(APP_CONFIG_KEY)?)
    }

    /// Reads the `byok` object itself.
    #[must_use]
    pub fn from_value(byok: &Value) -> Option<Self> {
        let protocol = match non_empty(byok, "protocol")?.to_ascii_lowercase().as_str() {
            "anthropic" => ByokProtocol::Anthropic,
            "openai" | "openai-compatible" => ByokProtocol::OpenAi,
            _ => return None,
        };
        Some(Self {
            protocol,
            api_key: non_empty(byok, "apiKey")?,
            base_url: non_empty(byok, "baseUrl"),
            model: non_empty(byok, "model"),
            max_tokens: byok.get("maxTokens").and_then(Value::as_u64),
        })
    }

    fn url(&self) -> String {
        match self.protocol {
            ByokProtocol::Anthropic => {
                let base = self
                    .base_url
                    .as_deref()
                    .unwrap_or(ANTHROPIC_BASE_URL)
                    .trim_end_matches('/');
                if base.ends_with("/v1") {
                    format!("{base}/messages")
                } else {
                    format!("{base}/v1/messages")
                }
            }
            ByokProtocol::OpenAi => {
                let base = self
                    .base_url
                    .as_deref()
                    .unwrap_or(OPENAI_BASE_URL)
                    .trim_end_matches('/');
                format!("{base}/chat/completions")
            }
        }
    }

    /// `name:host` for the run's `start.bin`, never the key.
    #[must_use]
    pub fn label(&self) -> String {
        let name = match self.protocol {
            ByokProtocol::Anthropic => "anthropic",
            ByokProtocol::OpenAi => "openai",
        };
        format!("byok:{name} {}", self.url())
    }
}

/// One BYOK call.
#[derive(Debug, Clone)]
pub struct ByokRequest {
    pub config: ByokConfig,
    /// The run's model; falls back to the configured one, then the default.
    pub model: Option<String>,
    pub prompt: String,
}

/// Why a call failed, classified for the retry policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByokFailure {
    pub category: FailureCategory,
    pub message: String,
}

impl ByokFailure {
    fn new(category: FailureCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

fn http_failure(status: u16, body: &str) -> ByokFailure {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    let category = match status {
        429 => FailureCategory::RateLimit,
        500..=599 => FailureCategory::Transient,
        _ => FailureCategory::AgentError,
    };
    let what = match status {
        401 | 403 => "the provider rejected the API key",
        _ => "the provider returned an error",
    };
    ByokFailure::new(category, format!("{what} (HTTP {status}): {detail}"))
}

impl ByokRequest {
    fn model(&self) -> String {
        self.model
            .clone()
            .filter(|m| !m.is_empty() && m != "default")
            .or_else(|| self.config.model.clone())
            .unwrap_or_else(|| {
                match self.config.protocol {
                    ByokProtocol::Anthropic => ANTHROPIC_DEFAULT_MODEL,
                    ByokProtocol::OpenAi => OPENAI_DEFAULT_MODEL,
                }
                .to_owned()
            })
    }

    fn body(&self) -> Value {
        let model = self.model();
        match self.config.protocol {
            ByokProtocol::Anthropic => json!({
                "model": model,
                "max_tokens": self.config.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
                "stream": true,
                "system": SYSTEM_PROMPT,
                "messages": [{"role": "user", "content": self.prompt}],
            }),
            ByokProtocol::OpenAi => {
                let mut body = json!({
                    "model": model,
                    "stream": true,
                    "stream_options": {"include_usage": true},
                    "messages": [
                        {"role": "system", "content": SYSTEM_PROMPT},
                        {"role": "user", "content": self.prompt},
                    ],
                });
                if let Some(n) = self.config.max_tokens {
                    body["max_tokens"] = json!(n);
                }
                body
            }
        }
    }
}

/// Streams one call, sending normalized events to `tx` and a final
/// `usage`. Dropping the future aborts the request.
///
/// # Errors
/// HTTP, transport or in-stream provider errors, classified.
pub async fn stream(req: ByokRequest, tx: mpsc::Sender<AgentEvent>) -> Result<(), ByokFailure> {
    // No redirects: reqwest keeps custom headers (`x-api-key`) when one
    // points at another host, and a provider API never needs one.
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ByokFailure::new(FailureCategory::SpawnFailed, e.to_string()))?;
    let mut builder = client
        .post(req.config.url())
        .header("content-type", "application/json")
        .header("accept", "text/event-stream");
    builder = match req.config.protocol {
        ByokProtocol::Anthropic => builder
            .header("x-api-key", &req.config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION),
        ByokProtocol::OpenAi => builder.bearer_auth(&req.config.api_key),
    };
    let mut res = builder.json(&req.body()).send().await.map_err(|e| {
        ByokFailure::new(
            FailureCategory::Transient,
            format!("could not reach the provider: {e}"),
        )
    })?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(http_failure(status.as_u16(), &body));
    }
    let mut decoder = SseDecoder::new(req.config.protocol);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let chunk = res.chunk().await.map_err(|e| {
            ByokFailure::new(
                FailureCategory::Transient,
                format!("stream interrupted: {e}"),
            )
        })?;
        let Some(chunk) = chunk else { break };
        buf.extend_from_slice(&chunk);
        while let Some(block) = take_block(&mut buf) {
            let mut events = Vec::new();
            decoder.block(&block, &mut events)?;
            for ev in events {
                if tx.send(ev).await.is_err() {
                    return Ok(());
                }
            }
            if decoder.finished {
                break;
            }
        }
        if decoder.finished {
            buf.clear();
            break;
        }
    }
    if !buf.is_empty() {
        let block = String::from_utf8_lossy(&std::mem::take(&mut buf)).into_owned();
        let mut events = Vec::new();
        decoder.block(&block, &mut events)?;
        for ev in events {
            let _ = tx.send(ev).await;
        }
    }
    let _ = tx.send(decoder.usage_event()).await;
    Ok(())
}

/// Cuts one SSE event (up to a blank line) off the front of `buf`.
fn take_block(buf: &mut Vec<u8>) -> Option<String> {
    let (end, sep) = buf
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|i| (i, 2))
        .or_else(|| {
            buf.windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| (i, 4))
        })?;
    let block = String::from_utf8_lossy(&buf[..end]).into_owned();
    buf.drain(..end + sep);
    Some(block)
}

/// Turns SSE event blocks into normalized events.
#[derive(Debug)]
pub struct SseDecoder {
    protocol: ByokProtocol,
    thinking_started: bool,
    usage: serde_json::Map<String, Value>,
    stop_reason: Option<String>,
    finished: bool,
}

impl SseDecoder {
    #[must_use]
    pub fn new(protocol: ByokProtocol) -> Self {
        Self {
            protocol,
            thinking_started: false,
            usage: serde_json::Map::new(),
            stop_reason: None,
            finished: false,
        }
    }

    fn thinking(&mut self, delta: &str, out: &mut Vec<AgentEvent>) {
        if delta.is_empty() {
            return;
        }
        if !self.thinking_started {
            self.thinking_started = true;
            out.push(AgentEvent::ThinkingStart);
        }
        out.push(AgentEvent::ThinkingDelta {
            delta: delta.to_owned(),
        });
    }

    fn add_usage(&mut self, src: &Value, keys: &[(&str, &str)]) {
        for (from, to) in keys {
            if let Some(n) = src.get(*from).filter(|v| v.is_number()) {
                self.usage.insert((*to).to_owned(), n.clone());
            }
        }
    }

    /// Handles one event block (`event:`/`data:` lines).
    ///
    /// # Errors
    /// An in-stream `error` event.
    pub fn block(&mut self, block: &str, out: &mut Vec<AgentEvent>) -> Result<(), ByokFailure> {
        let data: String = block
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| d.strip_prefix(' ').unwrap_or(d))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            return Ok(());
        }
        if data.trim() == "[DONE]" {
            self.finished = true;
            return Ok(());
        }
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            return Ok(());
        };
        match self.protocol {
            ByokProtocol::Anthropic => self.anthropic(&v, out),
            ByokProtocol::OpenAi => self.openai(&v, out),
        }
    }

    fn anthropic(&mut self, v: &Value, out: &mut Vec<AgentEvent>) -> Result<(), ByokFailure> {
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "message_start" => {
                if let Some(u) = v.pointer("/message/usage") {
                    self.add_usage(
                        u,
                        &[
                            ("input_tokens", "input_tokens"),
                            ("cache_read_input_tokens", "cached_read_tokens"),
                            ("cache_creation_input_tokens", "cached_write_tokens"),
                        ],
                    );
                }
            }
            "content_block_delta" => {
                let delta = v.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(t) = delta.get("text").and_then(Value::as_str) {
                            if !t.is_empty() {
                                out.push(AgentEvent::TextDelta {
                                    delta: t.to_owned(),
                                });
                            }
                        }
                    }
                    Some("thinking_delta") => {
                        let t = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        self.thinking(t, out);
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(u) = v.get("usage") {
                    self.add_usage(u, &[("output_tokens", "output_tokens")]);
                }
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(r.to_owned());
                }
            }
            "message_stop" => self.finished = true,
            "error" => {
                let kind = v
                    .pointer("/error/type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let message = v
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("provider error");
                let category = match kind {
                    "rate_limit_error" => FailureCategory::RateLimit,
                    "overloaded_error" | "api_error" => FailureCategory::Transient,
                    _ => FailureCategory::AgentError,
                };
                return Err(ByokFailure::new(category, format!("{kind}: {message}")));
            }
            _ => {}
        }
        Ok(())
    }

    fn openai(&mut self, v: &Value, out: &mut Vec<AgentEvent>) -> Result<(), ByokFailure> {
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| err.to_string(), str::to_owned);
            return Err(ByokFailure::new(FailureCategory::AgentError, message));
        }
        if let Some(choice) = v.pointer("/choices/0") {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            if let Some(r) = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(Value::as_str)
            {
                self.thinking(r, out);
            }
            if let Some(t) = delta.get("content").and_then(Value::as_str) {
                if !t.is_empty() {
                    out.push(AgentEvent::TextDelta {
                        delta: t.to_owned(),
                    });
                }
            }
            if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
                self.stop_reason = Some(r.to_owned());
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.add_usage(
                u,
                &[
                    ("prompt_tokens", "input_tokens"),
                    ("completion_tokens", "output_tokens"),
                    ("total_tokens", "total_tokens"),
                ],
            );
        }
        Ok(())
    }

    /// The closing `usage` event.
    #[must_use]
    pub fn usage_event(&self) -> AgentEvent {
        AgentEvent::Usage {
            usage: (!self.usage.is_empty()).then(|| Value::Object(self.usage.clone())),
            cost_usd: None,
            duration_ms: None,
            stop_reason: Some(
                self.stop_reason
                    .clone()
                    .unwrap_or_else(|| "end_turn".to_owned()),
            ),
            is_error: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_app_config_and_debug_hides_key() {
        let cfg = ByokConfig::from_app_config(&json!({
            "byok": {"protocol": "openai", "apiKey": " sk-secret ", "baseUrl": "http://h/v1/"}
        }))
        .unwrap_or_else(|| panic!("config"));
        assert_eq!(cfg.api_key, "sk-secret");
        assert_eq!(cfg.url(), "http://h/v1/chat/completions");
        assert!(!format!("{cfg:?}").contains("sk-secret"));
        assert!(!cfg.label().contains("sk-secret"));
        assert!(ByokConfig::from_app_config(&json!({"byok": {"protocol": "anthropic"}})).is_none());
        let a =
            ByokConfig::from_app_config(&json!({"byok": {"protocol": "anthropic", "apiKey": "k"}}))
                .unwrap_or_else(|| panic!("config"));
        assert_eq!(a.url(), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn http_errors_are_classified() {
        assert_eq!(http_failure(429, "").category, FailureCategory::RateLimit);
        assert_eq!(http_failure(503, "").category, FailureCategory::Transient);
        let f = http_failure(401, r#"{"error":{"message":"invalid x-api-key"}}"#);
        assert_eq!(f.category, FailureCategory::AgentError);
        assert!(f.message.contains("invalid x-api-key"));
    }
}
