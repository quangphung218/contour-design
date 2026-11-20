//! Help ▸ Report Issue (desktop menu and the quick settings menu):
//!
//! - `GET  /api/diagnostics/summary` → `{text, logDir}`: a plain-text
//!   report to paste into an issue (version, platform, agents, recent
//!   runs, the daemon log: the lines of `?id=` or of the last request
//!   that failed first, then the last lines), with nothing secret in it.
//! - `POST /api/doctor`: the checks of `od doctor` ([`crate::doctor`]).
//!   POST, not GET: it starts Chrome and the agent CLIs, so a page
//!   elsewhere must not set it off with an `<img>` (the origin guard
//!   checks the `Origin` a browser always sends with a POST).
//! - `POST /api/diagnostics/open-logs`: `<data>/logs` (the daemon log) in
//!   the OS file
//!   manager (`OD_OPEN_COMMAND` overrides, as for handoff).
//!
//! Nothing leaves the machine: the user decides where to paste the text.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::manage::diagnostics::LogReport;
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

/// Of [`SUMMARY_LOG_BYTES`], the most the lines of one id take.
const SUMMARY_ID_BYTES: usize = 16 * 1024;

/// One line per fact, for an issue body; `log` (scrubbed log lines) goes
/// last, folded, cut to its newest [`SUMMARY_LOG_BYTES`].
#[must_use]
pub fn summary_text(
    agents: &[od_contracts::AgentInfo],
    configured: Option<&str>,
    runs: &[od_contracts::RunInfo],
    log: &LogReport,
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
    // The lines of the failed request first, so they survive the cut.
    let mut budget = SUMMARY_LOG_BYTES;
    if let Some(id) = &log.id {
        let lines = newest(&log.matched, SUMMARY_ID_BYTES);
        budget -= lines.len();
        fold(&mut out, &format!("Daemon log: lines with {id}"), lines);
    }
    if !log.tail.is_empty() {
        fold(
            &mut out,
            "Daemon log (last lines)",
            newest(&log.tail, budget),
        );
    }
    out
}

/// The newest whole lines of `text` that fit in `max` bytes.
fn newest(text: &str, max: usize) -> &str {
    let start = text.len().saturating_sub(max);
    // Start on a line (a byte search: `start` may fall inside a char).
    let start = if start == 0 {
        0
    } else {
        text.as_bytes()[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(text.len(), |i| start + i + 1)
    };
    &text[start..]
}

/// `text` in a folded block titled `title`.
fn fold(out: &mut String, title: &str, text: &str) {
    // Four backticks: a log line with ``` cannot close the block.
    let _ = write!(
        out,
        "\n<details><summary>{title}</summary>\n\n````\n{text}````\n\n</details>\n"
    );
}

pub async fn summary(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<crate::routes::manage::diagnostics::IdQuery>,
) -> ApiResult<axum::Json<Value>> {
    let id = crate::routes::manage::diagnostics::lookup_id(query.id.as_deref());
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
    let log = crate::spawn::spawn_blocking_in_span(move || {
        crate::routes::manage::diagnostics::log_report(&root, id.as_deref())
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    let log_dir = crate::logs::dir(state.store.data_dir().root());
    Ok(axum::Json(json!({
        "text": summary_text(&agents, configured.as_deref(), &runs, &log),
        "logDir": log_dir.display().to_string(),
    })))
}

/// One check of the setup at a time: a second press waits for the first
/// instead of starting a second Chrome.
static DOCTOR: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `POST /api/doctor`: Settings ▸ About ▸ Check setup, the checks of `od
/// doctor` run by this daemon (its own data folder, its agents' `PATH`,
/// the renderer exports use). The daemon line says it answered.
pub async fn doctor(State(state): State<AppState>) -> axum::Json<crate::doctor::Report> {
    let _one = DOCTOR.lock().await;
    let version = env!("CARGO_PKG_VERSION");
    let url = state
        .config
        .daemon_url
        .clone()
        .unwrap_or_else(|| "this address".to_owned());
    let mut checks = vec![crate::doctor::daemon_check(
        Some(&url),
        Some(version),
        version,
    )];
    checks.extend(
        crate::doctor::run_local(crate::doctor::Inputs {
            data: state.store.data_dir().clone(),
            agent_path: state.runs.engine.config().path.clone(),
            renderer: std::sync::Arc::new(od_render_cdp::CdpRenderer::new()),
        })
        .await,
    );
    axum::Json(crate::doctor::Report::new(checks))
}

pub async fn open_logs(State(state): State<AppState>) -> ApiResult<ApiJson<OkResponse>> {
    let dir = crate::logs::dir(state.store.data_dir().root());
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
    crate::spawn::spawn_in_span(async move {
        let _ = child.wait().await;
    });
    Ok(ApiJson(OkResponse { ok: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_goes_last_and_fits_an_issue() {
        let tail = |t: &str| LogReport {
            tail: t.to_owned(),
            ..LogReport::default()
        };
        assert!(!summary_text(&[], None, &[], &tail("")).contains("Daemon log"));
        let log: String = (0..5_000).map(|i| format!("ligne é {i}\n")).collect();
        let text = summary_text(&[], None, &[], &tail(&log));
        let at = text.find("<details>").unwrap();
        assert!(text.find("### What happened").unwrap() < at);
        let shown = text[at..].split("````\n").nth(1).unwrap();
        assert!(shown.len() <= SUMMARY_LOG_BYTES, "{}", shown.len());
        assert!(shown.starts_with("ligne é "), "{}", &shown[..20]);
        assert!(shown.ends_with("ligne é 4999\n"));
        assert!(text.ends_with("</details>\n"));
        // A fence in the log stays inside the block.
        let text = summary_text(&[], None, &[], &tail("a\n```\nb\n"));
        assert!(text.contains("\n````\na\n```\nb\n````\n"), "{text}");
    }

    #[test]
    fn the_lines_of_the_failed_request_come_first_and_survive_a_long_tail() {
        let tail: String = (0..5_000).map(|i| format!("noise {i}\n")).collect();
        let report = LogReport {
            id: Some("ab12cd34".to_owned()),
            matched: "req{request_id=ab12cd34}: error answer\n".to_owned(),
            tail,
        };
        let text = summary_text(&[], None, &[], &report);
        let mine = text.find("lines with ab12cd34").unwrap();
        let last = text.find("Daemon log (last lines)").unwrap();
        assert!(mine < last);
        assert!(text.contains("req{request_id=ab12cd34}: error answer"));
        let logs: usize = text.split("````\n").skip(1).step_by(2).map(str::len).sum();
        assert!(logs <= SUMMARY_LOG_BYTES, "{logs}");
    }
}
