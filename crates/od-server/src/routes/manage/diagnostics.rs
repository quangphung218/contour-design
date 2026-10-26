//! Settings ▸ About ▸ "Export diagnostics": `GET /api/diagnostics/export`
//! returns a zip built locally, nothing is uploaded.
//!
//! Contents: `summary/manifest.json` (version, platform, time),
//! `summary/machine-info.json`, `app-config.json` without secrets (the BYOK
//! key, agent CLI env, anything named like a key or token), `agents.json`,
//! `runs.json` (the last [`MAX_RUNS`] runs), the tail of each of the
//! last [`MAX_RUN_LOGS`] runs' `events.jsonl`, and `logs/daemon.log`: the
//! last [`LOG_LINES`] lines of the daemon log ([`crate::logs`]), scrubbed.

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use serde_json::{Map, Value, json};
use std::io::Write;

pub const MAX_RUNS: usize = 100;
pub const MAX_RUN_LOGS: usize = 10;
/// Daemon log lines in the archive and in the Report Issue text.
pub const LOG_LINES: usize = 200;
/// Bytes kept from the end of each `events.jsonl`.
const LOG_TAIL_BYTES: usize = 64 * 1024;

/// Whether a field named `key` holds a secret (`apiKey`, `token`…; not
/// the `has…` flags).
fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    ["key", "token", "secret", "password", "authorization"]
        .iter()
        .any(|s| lower.contains(s))
        && !lower.starts_with("has")
}

/// Drops values whose key looks secret, at any depth.
#[must_use]
pub fn scrub(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    if is_secret_key(&k) {
                        (k, Value::String("[redacted]".to_owned()))
                    } else {
                        (k, scrub(v))
                    }
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(scrub).collect()),
        other => other,
    }
}

/// [`scrub`] for a log line: hides the value of each `key=value` field
/// whose key looks secret (`api_key="sk-…"` → `api_key=[redacted]`).
#[must_use]
pub fn scrub_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(eq) = rest.find('=') {
        let (head, value) = (&rest[..eq], &rest[eq + 1..]);
        let key_start = head
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let key = &head[key_start..];
        let len = value_len(value);
        out.push_str(head);
        out.push('=');
        if !key.is_empty() && is_secret_key(key) {
            out.push_str("[redacted]");
        } else {
            out.push_str(&value[..len]);
        }
        rest = &value[len..];
    }
    out.push_str(rest);
    out
}

/// Bytes of a field value at the start of `s`: a quoted string up to its
/// closing quote, else up to the next whitespace.
fn value_len(s: &str) -> usize {
    if let Some(quoted) = s.strip_prefix('"') {
        let mut escaped = false;
        for (i, c) in quoted.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => return i + 2,
                _ => {}
            }
        }
        return s.len();
    }
    s.find(char::is_whitespace).unwrap_or(s.len())
}

/// The last [`LOG_LINES`] lines of the daemon log, scrubbed, one per line.
#[must_use]
pub fn log_tail(data_root: &std::path::Path) -> String {
    crate::logs::tail_lines(data_root, LOG_LINES)
        .iter()
        .map(|l| scrub_line(l) + "\n")
        .collect()
}

fn tail(bytes: &[u8]) -> &[u8] {
    let start = bytes.len().saturating_sub(LOG_TAIL_BYTES);
    // Start on a line boundary.
    let start = if start == 0 {
        0
    } else {
        bytes[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| start + i + 1)
    };
    &bytes[start..]
}

pub async fn export(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let byok = state
        .store
        .data_dir()
        .app_setting(od_runtime::byok::APP_CONFIG_KEY)
        .and_then(|v| od_runtime::byok::ByokConfig::from_value(&v))
        .is_some();
    let engine = state.runs.engine.config();
    let agents =
        od_runtime::detect::detect_all_in(engine.path.clone(), engine.base_env.clone(), byok).await;
    let runs: Vec<_> = state
        .runs
        .registry
        .list()
        .into_iter()
        .take(MAX_RUNS)
        .collect();
    let root = state.store.data_dir().root().to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut add = |name: &str, data: &[u8]| -> std::io::Result<()> {
            zip.start_file(name, opts).map_err(std::io::Error::other)?;
            zip.write_all(data)
        };
        let pretty = |v: &Value| serde_json::to_vec_pretty(v).unwrap_or_default();
        add(
            "summary/manifest.json",
            &pretty(&json!({
                "name": "contour",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "exportedAt": od_runs::now_ms(),
            })),
        )?;
        add(
            "summary/machine-info.json",
            &pretty(&json!({
                "os": std::env::consts::OS,
                "family": std::env::consts::FAMILY,
                "arch": std::env::consts::ARCH,
                "cpus": std::thread::available_parallelism().map_or(0, std::num::NonZero::get),
            })),
        )?;
        let config: Value = std::fs::read(root.join("app-config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| json!({}));
        add("app-config.json", &pretty(&scrub(config)))?;
        add(
            "agents.json",
            &pretty(&serde_json::to_value(&agents).unwrap_or_default()),
        )?;
        let infos: Vec<Value> = runs
            .iter()
            .map(|r| serde_json::to_value(r.info()).unwrap_or_default())
            .collect();
        add("runs.json", &pretty(&Value::Array(infos)))?;
        for run in runs.iter().take(MAX_RUN_LOGS) {
            if let Ok(log) = std::fs::read(run.dir().join("events.jsonl")) {
                add(&format!("runs/{}/events.jsonl", run.id()), tail(&log))?;
            }
        }
        let log = log_tail(&root);
        if !log.is_empty() {
            add("logs/daemon.log", log.as_bytes())?;
        }
        let cursor = zip.finish().map_err(std::io::Error::other)?;
        Ok(cursor.into_inner())
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(|e| ApiErr::internal(format!("building the archive: {e}")))?;
    let name = format!(
        "attachment; filename=\"contour-diagnostics-{}.zip\"",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (header::CONTENT_DISPOSITION, name),
        ],
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_hides_secrets_but_keeps_flags() {
        let v = scrub(json!({
            "byok": {"apiKey": "sk-1", "hasApiKey": true, "model": "m"},
            "list": [{"OPENAI_API_KEY": "x"}],
            "theme": "dark",
        }));
        assert_eq!(v["byok"]["apiKey"], "[redacted]");
        assert_eq!(v["byok"]["hasApiKey"], true);
        assert_eq!(v["list"][0]["OPENAI_API_KEY"], "[redacted]");
        assert_eq!(v["theme"], "dark");
        assert!(!v.to_string().contains("sk-1"));
    }

    #[test]
    fn scrub_line_hides_secret_fields() {
        let line = r#"2026-10-26T10:00:00Z  INFO od_server: saved api_key="sk-1 2" token=abc model="m" hasApiKey=true url=http://x/?a=b"#;
        let s = scrub_line(line);
        assert!(!s.contains("sk-1") && !s.contains("abc"), "{s}");
        assert!(
            s.ends_with(
                r#"saved api_key=[redacted] token=[redacted] model="m" hasApiKey=true url=http://x/?a=b"#
            ),
            "{s}"
        );
        assert_eq!(scrub_line("no fields here"), "no fields here");
        assert_eq!(scrub_line(r#"key="a\"b" x=1"#), "key=[redacted] x=1");
        assert_eq!(scrub_line("é=1 secret="), "é=1 secret=[redacted]");
    }

    #[test]
    fn tail_starts_on_a_line() {
        let mut log = Vec::new();
        for i in 0..20_000 {
            log.extend_from_slice(format!("{{\"id\":{i}}}\n").as_bytes());
        }
        let t = tail(&log);
        assert!(t.len() <= LOG_TAIL_BYTES);
        assert!(t.starts_with(b"{\"id\":"));
        assert_eq!(tail(b"short\n"), b"short\n");
    }
}
