//! `od doctor`: checks each condition of the main flow and prints one
//! line per check (`✓`, `!` or `✗`) with how to fix it; `--json` prints
//! the report for Report Issue. Exits 1 when a check failed.
//!
//! The checks are `od_server::doctor`'s, shared with Settings ▸ About;
//! only the daemon check is the CLI's own, since it asks the daemon from
//! outside.

use od_server::doctor::{self, Check, Report};
use std::time::Duration;

/// How long the daemon has to answer `GET /api/version`.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(3);

/// Runs every check; returns the exit code.
#[must_use]
pub fn run(daemon_url: &str, json: bool) -> u8 {
    let daemon = daemon_line(daemon_url);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: starting the async runtime: {e}");
            return 1;
        }
    };
    let mut checks = vec![daemon];
    checks.extend(
        runtime.block_on(async { doctor::run_local(doctor::Inputs::from_env().await).await }),
    );
    let report = Report::new(checks);
    print(&report, json);
    u8::from(!report.ok)
}

fn print(report: &Report, json: bool) {
    if json {
        match serde_json::to_string_pretty(report) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("error: {e}"),
        }
    } else {
        print!("{}", report.text());
    }
}

/// The daemon check: `url` (the flag, the env, or the default) or the one
/// the running app recorded in `daemon.lock`.
fn daemon_line(url: &str) -> Check {
    let data = od_store::DataDir::from_env();
    let recorded = std::fs::read_to_string(data.root().join(od_server::lock::LOCK_FILE))
        .ok()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty());
    let candidates: Vec<&str> = std::iter::once(url).chain(recorded.as_deref()).collect();
    for candidate in &candidates {
        if let Some(version) = version_at(candidate) {
            return doctor::daemon_check(
                Some(candidate),
                Some(&version),
                env!("CARGO_PKG_VERSION"),
            );
        }
    }
    doctor::daemon_check(recorded.as_deref(), None, env!("CARGO_PKG_VERSION"))
}

/// The version the daemon at `url` reports, if it answers in time.
fn version_at(url: &str) -> Option<String> {
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(DAEMON_TIMEOUT)
        .build()
        .ok()?;
    let res = http
        .get(format!("{}/api/version", url.trim_end_matches('/')))
        .send()
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let body: od_contracts::AppVersionResponse = res.json().ok()?;
    Some(body.version)
}
