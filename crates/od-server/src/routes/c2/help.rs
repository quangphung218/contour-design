//! Help ▸ Report Issue (desktop menu and the quick settings menu):
//!
//! - `GET  /api/diagnostics/summary` → `{text, logDir}`: a plain-text
//!   report to paste into an issue (version, platform, agents, recent
//!   runs, the last lines of the daemon log), with nothing secret in it.
//! - `POST /api/diagnostics/open-logs`: the run logs folder in the OS file
//!   manager (`OD_OPEN_COMMAND` overrides, as for handoff).
//!
//! Nothing leaves the machine: the user decides where to paste the text.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use od_contracts::OkResponse;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// Recent runs listed in the report.
pub const SUMMARY_RUNS: usize = 5;

/// Bytes of daemon log kept in the text: GitHub refuses issue bodies over
/// 65,536 characters.
const SUMMARY_LOG_BYTES: usize = 40 * 1024;

/// One line per fact, for an issue body; `log` (scrubbed log lines) goes
/// last, folded, cut to its newest [`SUMMARY_LOG_BYTES`].
#[must_use]
pub fn summary_text(
    agents: &[od_contracts::AgentInfo],
    configured: Option<&str>,
    runs: &[od_contracts::RunInfo],
    log: &str,
) -> String {
    let mut out = String::from("### Contour diagnostics\n\n");
    let _ = writeln!(out, "- Version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "- Platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "- Agent in use: {}", configured.unwrap_or("auto"));
    let ready: Vec<String> = agents
        .iter()
        .filter(|a| a.available)
        .map(|a| match &a.version {
            Some(v) => format!("{} {v}", a.id),
            None => a.id.clone(),
        })
        .collect();
    let _ = writeln!(
        out,
        "- Agents found: {}",
        if ready.is_empty() {
            "none".to_owned()
        } else {
            ready.join(", ")
        }
    );
    out.push_str("\n### Recent runs\n\n");
    if runs.is_empty() {
        out.push_str("- none\n");
    }
    for r in runs.iter().take(SUMMARY_RUNS) {
        let status = serde_json::to_value(r.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let _ = writeln!(out, "- {} · {} · {status}", r.id, r.agent_id);
    }
    out.push_str(
        "\n### What happened\n\n<!-- Steps to reproduce, what you expected, what you saw. -->\n",
    );
    if !log.is_empty() {
        let start = log.len().saturating_sub(SUMMARY_LOG_BYTES);
        // Start on a line (a byte search: `start` may fall inside a char).
        let start = if start == 0 {
            0
        } else {
            log.as_bytes()[start..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(log.len(), |i| start + i + 1)
        };
        out.push_str("\n<details><summary>Daemon log (last lines)</summary>\n\n```\n");
        out.push_str(&log[start..]);
        out.push_str("```\n\n</details>\n");
    }
    out
}

pub async fn summary(State(state): State<AppState>) -> ApiResult<axum::Json<Value>> {
    let byok = state
        .store
        .data_dir()
        .app_setting(od_runtime::byok::APP_CONFIG_KEY)
        .and_then(|v| od_runtime::byok::ByokConfig::from_value(&v))
        .is_some();
    let agents =
        od_runtime::detect::detect_all(state.runs.engine.config().path.clone(), byok).await;
    let configured = state
        .store
        .data_dir()
        .app_setting("agentId")
        .and_then(|v| v.as_str().map(str::to_owned));
    let runs: Vec<od_contracts::RunInfo> = state
        .runs
        .registry
        .list()
        .into_iter()
        .take(SUMMARY_RUNS)
        .map(|r| r.info())
        .collect();
    let root = state.store.data_dir().root().to_path_buf();
    let log =
        tokio::task::spawn_blocking(move || crate::routes::manage::diagnostics::log_tail(&root))
            .await
            .map_err(|e| ApiErr::internal(e.to_string()))?;
    let log_dir = state.store.data_dir().runs_dir();
    Ok(axum::Json(json!({
        "text": summary_text(&agents, configured.as_deref(), &runs, &log),
        "logDir": log_dir.display().to_string(),
    })))
}

pub async fn open_logs(State(state): State<AppState>) -> ApiResult<ApiJson<OkResponse>> {
    let dir = state.store.data_dir().runs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| ApiErr::internal(e.to_string()))?;
    let program = crate::routes::handoff::opener();
    // Not awaited: file managers may stay attached to the child.
    let mut child = tokio::process::Command::new(&program)
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| ApiErr::internal(format!("cannot run {program}: {e}")))?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(ApiJson(OkResponse { ok: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_goes_last_and_fits_an_issue() {
        assert!(!summary_text(&[], None, &[], "").contains("Daemon log"));
        let log: String = (0..5_000).map(|i| format!("ligne é {i}\n")).collect();
        let text = summary_text(&[], None, &[], &log);
        let at = text.find("<details>").unwrap();
        assert!(text.find("### What happened").unwrap() < at);
        let shown = text[at..].split("```\n").nth(1).unwrap();
        assert!(shown.len() <= SUMMARY_LOG_BYTES, "{}", shown.len());
        assert!(shown.starts_with("ligne é "), "{}", &shown[..20]);
        assert!(shown.ends_with("ligne é 4999\n"));
        assert!(text.ends_with("</details>\n"));
    }
}
