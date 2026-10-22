//! Blocking HTTP client for the daemon API.

use anyhow::{Context, anyhow};
use od_contracts::ApiErrorResponse;
use reqwest::Url;
use reqwest::blocking::{Client as Http, RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

pub struct Client {
    base: Url,
    http: Http,
    /// No total timeout: event streams stay open for the whole run.
    streaming: Http,
}

/// One SSE event of a run stream.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamEvent {
    pub id: u64,
    pub event: String,
    pub data: serde_json::Value,
}

impl Client {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        let base = Url::parse(base).with_context(|| format!("invalid daemon url {base:?}"))?;
        let http = Http::builder()
            .timeout(Duration::from_secs(120))
            // Never route loopback calls through a system proxy.
            .no_proxy()
            .build()?;
        let streaming = Http::builder().no_proxy().timeout(None).build()?;
        Ok(Self {
            base,
            http,
            streaming,
        })
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    /// Joins percent-encoded path segments onto the base URL.
    fn url(&self, segments: &[&str]) -> anyhow::Result<Url> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|()| anyhow!("daemon url cannot be a base"))?
            .pop_if_empty()
            .extend(segments);
        Ok(url)
    }

    fn send(&self, req: RequestBuilder) -> anyhow::Result<Response> {
        let res = req.send().map_err(|e| {
            if e.is_connect() {
                anyhow!(
                    "cannot reach the daemon at {} (start it with `od`)",
                    self.base
                )
            } else {
                anyhow!(e)
            }
        })?;
        if res.status().is_success() {
            return Ok(res);
        }
        let status = res.status();
        let text = res.text().unwrap_or_default();
        match serde_json::from_str::<ApiErrorResponse>(&text) {
            Ok(body) => {
                // Print the wire form (`PROJECT_NOT_FOUND`), not the Rust name.
                let code = serde_json::to_value(body.error.code)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                Err(anyhow!(
                    "{} {code}: {}",
                    status.as_u16(),
                    body.error.message
                ))
            }
            Err(_) => Err(anyhow!("{}: {}", status, text.trim())),
        }
    }

    pub fn get_json<T: DeserializeOwned>(&self, segments: &[&str]) -> anyhow::Result<T> {
        Ok(self.send(self.http.get(self.url(segments)?))?.json()?)
    }

    pub fn get_json_query<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        Ok(self
            .send(self.http.get(self.url(segments)?).query(query))?
            .json()?)
    }

    /// Reads `GET /api/runs/:id/events` until the server closes it, calling
    /// `on_event` for each event (keepalive comments skipped).
    pub fn stream_events(
        &self,
        run_id: &str,
        after: Option<u64>,
        mut on_event: impl FnMut(StreamEvent) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        use std::io::BufRead;
        let mut req = self
            .streaming
            .get(self.url(&["api", "runs", run_id, "events"])?);
        if let Some(after) = after {
            req = req.header("Last-Event-ID", after.to_string());
        }
        let res = self.send(req)?;
        let reader = std::io::BufReader::new(res);
        let mut ev = StreamEvent {
            id: 0,
            event: String::new(),
            data: serde_json::Value::Null,
        };
        let mut data = String::new();
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                if !data.is_empty() {
                    ev.data = serde_json::from_str(&data)?;
                    on_event(std::mem::replace(
                        &mut ev,
                        StreamEvent {
                            id: 0,
                            event: String::new(),
                            data: serde_json::Value::Null,
                        },
                    ))?;
                    data.clear();
                }
                continue;
            }
            if let Some(v) = line.strip_prefix("id:") {
                ev.id = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("event:") {
                v.trim().clone_into(&mut ev.event);
            } else if let Some(v) = line.strip_prefix("data:") {
                data.push_str(v.strip_prefix(' ').unwrap_or(v));
            }
        }
        Ok(())
    }

    pub fn get_bytes(&self, segments: &[&str]) -> anyhow::Result<Vec<u8>> {
        Ok(self
            .send(self.http.get(self.url(segments)?))?
            .bytes()?
            .to_vec())
    }

    pub fn send_json<B: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        segments: &[&str],
        body: &B,
    ) -> anyhow::Result<T> {
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        let req = self.http.request(method, self.url(segments)?).json(body);
        Ok(self.send(req)?.json()?)
    }

    pub fn send_empty<T: DeserializeOwned>(
        &self,
        method: &str,
        segments: &[&str],
    ) -> anyhow::Result<T> {
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        Ok(self
            .send(self.http.request(method, self.url(segments)?))?
            .json()?)
    }
}
