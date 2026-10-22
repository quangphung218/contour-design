//! Async HTTP client for the daemon. Every MCP tool is one or a few calls
//! through this.

use reqwest::{Method, StatusCode, Url};
use serde_json::Value;
use std::time::Duration;

/// Default daemon URL (spec 2.5: after `--daemon-url` and `OD_DAEMON_URL`).
pub const DEFAULT_DAEMON_URL: &str = "http://127.0.0.1:7456";

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("cannot reach the OpenDesign daemon at {0} (start it with `od`)")]
    Unreachable(String),
    #[error("daemon {status} on {path}: {message}")]
    Status {
        status: u16,
        path: String,
        code: Option<String>,
        message: String,
    },
    #[error("daemon request failed: {0}")]
    Other(String),
}

impl DaemonError {
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

pub type DaemonResult<T> = Result<T, DaemonError>;

/// A raw file read: bytes and the response `Content-Type`.
#[derive(Debug, Clone)]
pub struct Raw {
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Daemon {
    base: Url,
    http: reqwest::Client,
}

impl Daemon {
    /// # Errors
    /// An unparsable URL.
    pub fn new(base: &str) -> anyhow::Result<Self> {
        Self::with_bearer(base, None)
    }

    /// A client that sends `Authorization: Bearer <token>` on every
    /// request (an agent's `OD_TOOL_TOKEN`).
    ///
    /// # Errors
    /// An unparsable URL or a token that is not a valid header value.
    pub fn with_bearer(base: &str, token: Option<&str>) -> anyhow::Result<Self> {
        let base = Url::parse(base.trim_end_matches('/'))?;
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) {
            let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            // Loopback only: never route through a system proxy.
            .no_proxy()
            .default_headers(headers)
            .build()?;
        Ok(Self { base, http })
    }

    #[must_use]
    pub fn base(&self) -> &str {
        self.base.as_str().trim_end_matches('/')
    }

    /// `segments` are percent-encoded one by one.
    #[must_use]
    pub fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    /// `api/projects/<id>/raw/<a>/<b>`: browser-openable file URL.
    #[must_use]
    pub fn raw_url(&self, project: &str, path: &str) -> String {
        let mut segs = vec!["api", "projects", project, "raw"];
        segs.extend(path.split('/').filter(|s| !s.is_empty()));
        self.url(&segs).to_string()
    }

    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
    ) -> DaemonResult<reqwest::Response> {
        let path = url.path().to_owned();
        let mut req = self.http.request(method, url);
        if let Some(body) = body {
            req = req.json(body);
        }
        let res = req.send().await.map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                DaemonError::Unreachable(self.base().to_owned())
            } else {
                DaemonError::Other(e.to_string())
            }
        })?;
        if res.status().is_success() {
            return Ok(res);
        }
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        let parsed: Option<Value> = serde_json::from_str(&text).ok();
        let error = parsed.as_ref().and_then(|v| v.get("error"));
        let code = error
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let message = error
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map_or_else(
                || {
                    let t = text.trim();
                    if t.is_empty() {
                        status.canonical_reason().unwrap_or("").to_owned()
                    } else {
                        t.chars().take(500).collect()
                    }
                },
                str::to_owned,
            );
        Err(DaemonError::Status {
            status: status.as_u16(),
            path,
            code,
            message,
        })
    }

    async fn json(&self, res: reqwest::Response) -> DaemonResult<Value> {
        let text = res
            .text()
            .await
            .map_err(|e| DaemonError::Other(e.to_string()))?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| DaemonError::Other(format!("invalid JSON from daemon: {e}")))
    }

    /// # Errors
    /// Transport or non-2xx responses.
    pub async fn get(&self, segments: &[&str]) -> DaemonResult<Value> {
        let res = self.send(Method::GET, self.url(segments), None).await?;
        self.json(res).await
    }

    /// # Errors
    /// Transport or non-2xx responses.
    pub async fn get_query(
        &self,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> DaemonResult<Value> {
        let mut url = self.url(segments);
        url.query_pairs_mut().extend_pairs(query);
        let res = self.send(Method::GET, url, None).await?;
        self.json(res).await
    }

    /// # Errors
    /// Transport or non-2xx responses.
    pub async fn post(&self, segments: &[&str], body: &Value) -> DaemonResult<Value> {
        let res = self
            .send(Method::POST, self.url(segments), Some(body))
            .await?;
        self.json(res).await
    }

    /// # Errors
    /// Transport or non-2xx responses.
    pub async fn delete(&self, segments: &[&str]) -> DaemonResult<Value> {
        let res = self.send(Method::DELETE, self.url(segments), None).await?;
        self.json(res).await
    }

    /// Reads `GET api/projects/<id>/raw/<path>`.
    ///
    /// # Errors
    /// Transport or non-2xx responses.
    pub async fn raw(&self, project: &str, path: &str) -> DaemonResult<Raw> {
        let url = Url::parse(&self.raw_url(project, path))
            .map_err(|e| DaemonError::Other(e.to_string()))?;
        let res = self.send(Method::GET, url, None).await?;
        let mime = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| DaemonError::Other(e.to_string()))?;
        Ok(Raw {
            mime,
            bytes: bytes.to_vec(),
        })
    }

    /// Reads a finished run's SSE log (`GET api/runs/<id>/events`) and joins
    /// its `text_delta` payloads. Best effort: `None` on any failure.
    pub async fn run_agent_message(&self, run_id: &str) -> Option<String> {
        let url = self.url(&["api", "runs", run_id, "events"]);
        let res = self
            .http
            .get(url)
            .header("accept", "text/event-stream")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        if res.status() != StatusCode::OK {
            return None;
        }
        let text = res.text().await.ok()?;
        let message = agent_text(&text);
        (!message.trim().is_empty()).then_some(message)
    }
}

/// Concatenates `text_delta` text from SSE `agent` events.
#[must_use]
pub fn agent_text(sse: &str) -> String {
    let mut out = String::new();
    for block in sse.split("\n\n") {
        let mut event = "message";
        let mut data = String::new();
        for line in block.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                event = v.trim();
            } else if let Some(v) = line.strip_prefix("data:") {
                data.push_str(v.strip_prefix(' ').unwrap_or(v));
            }
        }
        if event != "agent" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) == Some("text_delta") {
            if let Some(t) = v
                .get("delta")
                .or_else(|| v.get("text"))
                .and_then(Value::as_str)
            {
                out.push_str(t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_text_deltas() {
        let sse = "id: 1\nevent: start\ndata: {}\n\nid: 2\nevent: agent\ndata: {\"type\":\"text_delta\",\"delta\":\"Hel\"}\n\nevent: agent\ndata: {\"type\":\"tool_use\"}\n\nevent: agent\ndata: {\"type\":\"text_delta\",\"text\":\"lo\"}\n\n";
        assert_eq!(agent_text(sse), "Hello");
    }

    #[test]
    fn urls_encode_segments() {
        let d = Daemon::new("http://127.0.0.1:1/").unwrap();
        assert_eq!(
            d.raw_url("p 1", "a/b c.html"),
            "http://127.0.0.1:1/api/projects/p%201/raw/a/b%20c.html"
        );
    }
}
