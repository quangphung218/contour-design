//! One-off BYOK provider calls for Settings (spec 1.5): a connection test
//! ("Reply with only: ok", non-streaming, small token cap) and the
//! provider's model list. Both talk only to the configured endpoint and
//! never follow redirects (custom auth headers would travel along).

use crate::byok::{ByokConfig, ByokProtocol, azure_chat_url, azure_versioned};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// The original's provider test budget.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(12);
pub const SMOKE_PROMPT: &str = "Reply with only: ok";
const SMOKE_MAX_TOKENS: u64 = 100;
/// Longest reply sample shown.
const SAMPLE_CHARS: usize = 120;

/// Outcome classes, as the original's `ConnectionTestResponse.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestKind {
    Success,
    AuthFailed,
    Forbidden,
    NotFoundModel,
    InvalidBaseUrl,
    RateLimited,
    UpstreamUnavailable,
    Timeout,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub kind: TestKind,
    pub latency_ms: u64,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `http(s)://host…` without credentials; trailing `/` removed.
///
/// # Errors
/// Anything else, with a readable reason.
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let url = reqwest::Url::parse(&with_scheme).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("use an http:// or https:// URL".to_owned());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("the URL has no host".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("put the key in the API key field, not in the URL".to_owned());
    }
    Ok(with_scheme)
}

fn base(config: &ByokConfig) -> String {
    config
        .base_url
        .as_deref()
        .unwrap_or(config.protocol.default_base_url())
        .trim_end_matches('/')
        .to_owned()
}

/// Azure has no shared host: the resource endpoint is required.
const AZURE_NEEDS_BASE: &str =
    "Azure OpenAI needs the resource endpoint, like https://<resource>.openai.azure.com";

/// Anthropic paths live under `/v1`, whether or not the base carries it.
fn anthropic_url(base: &str, path: &str) -> String {
    if base.ends_with("/v1") {
        format!("{base}/{path}")
    } else {
        format!("{base}/v1/{path}")
    }
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

fn error_detail(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error").filter(|e| e.is_string()))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// Maps an HTTP error status to a [`TestKind`].
#[must_use]
pub fn kind_of_status(status: u16, detail: &str) -> TestKind {
    match status {
        401 => TestKind::AuthFailed,
        403 => TestKind::Forbidden,
        404 => TestKind::NotFoundModel,
        400 if detail.to_ascii_lowercase().contains("model") => TestKind::NotFoundModel,
        429 => TestKind::RateLimited,
        500..=599 => TestKind::UpstreamUnavailable,
        _ => TestKind::Unknown,
    }
}

/// The reply text of a non-streaming completion.
fn reply_text(protocol: ByokProtocol, v: &Value) -> String {
    match protocol {
        ByokProtocol::Anthropic => v
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<String>()
            })
            .unwrap_or_default(),
        ByokProtocol::OpenAi | ByokProtocol::Azure => v
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

/// Sends one small completion and reports how it went. Never errors: every
/// failure is a [`TestResult`] with `ok: false`.
pub async fn test_connection(config: &ByokConfig, timeout: Duration) -> TestResult {
    let model = config
        .model
        .clone()
        .unwrap_or_else(|| config.protocol.default_model().to_owned());
    let started = Instant::now();
    let elapsed = |s: Instant| u64::try_from(s.elapsed().as_millis()).unwrap_or(u64::MAX);
    let fail = |kind: TestKind, status: Option<u16>, detail: String, ms: u64| TestResult {
        ok: false,
        kind,
        latency_ms: ms,
        model: model.clone(),
        status,
        sample: None,
        detail: Some(detail),
    };
    let base = match config.base_url.as_deref() {
        Some(raw) => match normalize_base_url(raw) {
            Ok(b) => b,
            Err(e) => return fail(TestKind::InvalidBaseUrl, None, e, 0),
        },
        None => base(config),
    };
    if base.is_empty() {
        return fail(
            TestKind::InvalidBaseUrl,
            None,
            AZURE_NEEDS_BASE.to_owned(),
            0,
        );
    }
    let (url, body) = match config.protocol {
        ByokProtocol::Anthropic => (
            anthropic_url(&base, "messages"),
            json!({
                "model": model,
                "max_tokens": SMOKE_MAX_TOKENS,
                "messages": [{"role": "user", "content": SMOKE_PROMPT}],
            }),
        ),
        ByokProtocol::OpenAi => (
            format!("{base}/chat/completions"),
            json!({
                "model": model,
                "max_tokens": SMOKE_MAX_TOKENS,
                "messages": [{"role": "user", "content": SMOKE_PROMPT}],
            }),
        ),
        ByokProtocol::Azure => {
            let mut body = json!({
                "max_tokens": SMOKE_MAX_TOKENS,
                "messages": [{"role": "user", "content": SMOKE_PROMPT}],
            });
            if azure_versioned(&base) {
                body["model"] = json!(model);
            }
            (
                azure_chat_url(&base, &model, config.api_version.as_deref()),
                body,
            )
        }
    };
    let http = match client(timeout) {
        Ok(c) => c,
        Err(e) => return fail(TestKind::Unknown, None, e, 0),
    };
    let res = config.authed(http.post(&url)).json(&body).send().await;
    let res = match res {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return fail(
                TestKind::Timeout,
                None,
                format!("no answer within {} ms", timeout.as_millis()),
                elapsed(started),
            );
        }
        Err(e) => {
            return fail(
                TestKind::UpstreamUnavailable,
                None,
                format!("could not reach {url}: {e}"),
                elapsed(started),
            );
        }
    };
    let status = res.status().as_u16();
    let text = res.text().await.unwrap_or_default();
    let ms = elapsed(started);
    if !(200..300).contains(&status) {
        let detail = error_detail(&text);
        return fail(kind_of_status(status, &detail), Some(status), detail, ms);
    }
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let sample: String = reply_text(config.protocol, &value)
        .trim()
        .chars()
        .take(SAMPLE_CHARS)
        .collect();
    TestResult {
        ok: true,
        kind: TestKind::Success,
        latency_ms: ms,
        model,
        status: Some(status),
        sample: Some(sample),
        detail: None,
    }
}

/// Model ids the key can use, sorted.
///
/// # Errors
/// A readable message for HTTP, transport or format errors.
pub async fn list_models(config: &ByokConfig, timeout: Duration) -> Result<Vec<String>, String> {
    let base = match config.base_url.as_deref() {
        Some(raw) => normalize_base_url(raw)?,
        None => base(config),
    };
    let url = match config.protocol {
        ByokProtocol::Anthropic => anthropic_url(&base, "models?limit=1000"),
        ByokProtocol::OpenAi => format!("{base}/models"),
        // The model field is a deployment name, which only the Azure
        // portal lists; the resource's model list would mislead.
        ByokProtocol::Azure => {
            return Err(
                "Azure OpenAI uses deployment names: type the deployment from the Azure portal"
                    .to_owned(),
            );
        }
    };
    let http = client(timeout)?;
    let res = config
        .authed(http.get(&url))
        .send()
        .await
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    let status = res.status().as_u16();
    let text = res.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", error_detail(&text)));
    }
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("unexpected answer: {e}"))?;
    let mut ids: Vec<String> = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or("the answer has no model list")?
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls_are_normalized_and_checked() {
        assert_eq!(
            normalize_base_url("api.example.com/v1/").as_deref(),
            Ok("https://api.example.com/v1")
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("https://user:pw@x.com").is_err());
        assert_eq!(
            anthropic_url("https://a/v1", "messages"),
            "https://a/v1/messages"
        );
        assert_eq!(anthropic_url("https://a", "models"), "https://a/v1/models");
    }

    #[test]
    fn statuses_map_to_kinds() {
        assert_eq!(kind_of_status(401, ""), TestKind::AuthFailed);
        assert_eq!(
            kind_of_status(400, "unknown model x"),
            TestKind::NotFoundModel
        );
        assert_eq!(kind_of_status(503, ""), TestKind::UpstreamUnavailable);
        assert_eq!(
            error_detail(r#"{"error":{"message":"bad key"}}"#),
            "bad key"
        );
    }
}
