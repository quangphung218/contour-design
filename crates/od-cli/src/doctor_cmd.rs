//! `od doctor`: checks each condition of the main flow and prints one
//! line per check (`✓`, `!` or `✗`) with how to fix it; `--json` prints
//! the report for Report Issue. Exits 1 when a check failed.
//!
//! The checks are `od_server::doctor`'s, shared with Settings ▸ About;
//! only the daemon check is the CLI's own, since it asks the daemon from
//! outside. A daemon that answers runs the other checks itself
//! (`POST /api/doctor`): its data folder and its agents, not the ones this
//! shell's `OD_DATA_DIR` names (QA-119).

use od_server::doctor::{self, Check, Report, Status};
use std::time::Duration;

/// How long the daemon has to answer `GET /api/version`.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the daemon has to run the checks: Chrome's limit, the agents'
/// and room for a second caller holding its one-at-a-time lock.
const DAEMON_DOCTOR_TIMEOUT: Duration = Duration::from_secs(150);

/// Runs every check; returns the exit code.
#[must_use]
pub fn run(daemon_url: &str, json: bool) -> u8 {
    let (daemon, answered_at) = daemon_line(daemon_url);
    // Same version only: an older daemon may have no route or other checks.
    let remote = answered_at
        .filter(|_| daemon.status == Status::Ok)
        .and_then(|url| checks_at(&url));
    let mut checks = vec![daemon];
    if let Some(remote) = remote {
        checks.extend(remote);
        return finish(checks, json);
    }
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
    checks.extend(
        runtime.block_on(async { doctor::run_local(doctor::Inputs::from_env().await).await }),
    );
    finish(checks, json)
}

fn finish(checks: Vec<Check>, json: bool) -> u8 {
    let report = Report::new(checks);
    print(&report, json);
    u8::from(!report.ok)
}

/// The checks the daemon at `url` ran on itself, its daemon line left out
/// (the CLI's own comes first). `None` when it did not run them.
fn checks_at(url: &str) -> Option<Vec<Check>> {
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(DAEMON_DOCTOR_TIMEOUT)
        .build()
        .ok()?;
    let res = http
        .post(format!("{}/api/doctor", url.trim_end_matches('/')))
        .send()
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let report: Report = res.json().ok()?;
    Some(
        report
            .checks
            .into_iter()
            .filter(|c| c.id != "daemon")
            .collect(),
    )
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
/// the running app recorded in `daemon.lock`; with the address that
/// answered.
fn daemon_line(url: &str) -> (Check, Option<String>) {
    let data = od_store::DataDir::from_env();
    let recorded = std::fs::read_to_string(data.root().join(od_server::lock::LOCK_FILE))
        .ok()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty());
    let candidates: Vec<&str> = std::iter::once(url).chain(recorded.as_deref()).collect();
    for candidate in &candidates {
        if let Some(version) = version_at(candidate) {
            let check =
                doctor::daemon_check(Some(candidate), Some(&version), env!("CARGO_PKG_VERSION"));
            return (check, Some((*candidate).to_owned()));
        }
    }
    let check = doctor::daemon_check(recorded.as_deref(), None, env!("CARGO_PKG_VERSION"));
    (check, None)
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
