//! macOS: Chrome started through LaunchServices (`open -n -g -a`), not as
//! a child of this process.
//!
//! macOS charges what a child does to the app that started it (its
//! responsible process). Chrome wakes `GoogleUpdater`, which touches app
//! bundles; as Contour's child that asked for App Management in Contour's
//! name, and an ad-hoc signed Contour asked again after every update.
//! Started by LaunchServices, Chrome is its own responsible process.
//!
//! Without a child to read, the DevTools address comes from
//! `<profile>/DevToolsActivePort`, Chrome's stderr from `open --stderr`,
//! and the processes to end are found through the profile
//! ([`super::profile_users`]).

use super::{LaunchError, RENDER_SWITCHES, profile_users_checked};
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::{Browser, Handler};
use od_export::RenderError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const OPEN: &str = "/usr/bin/open";
/// Chrome's stderr, next to the profile's own files.
const STDERR_FILE: &str = "od-stderr.log";
/// How often the profile is checked for the DevTools port.
const POLL: Duration = Duration::from_millis(50);
/// How often (and first after how long) a browser that never wrote the port
/// is checked for having exited.
const EXIT_CHECK: Duration = Duration::from_millis(500);

/// The switches chromiumoxide 0.9 puts before ours on every launch
/// (`DEFAULT_ARGS`), so both ways of starting Chrome run it the same
/// (`tests/launch_services.rs` compares them).
const CHROMIUMOXIDE_DEFAULTS: &[&str] = &[
    "--disable-background-networking",
    "--enable-features=NetworkService,NetworkServiceInProcess",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-component-extensions-with-background-pages",
    "--disable-default-apps",
    "--disable-dev-shm-usage",
    "--disable-features=TranslateUI",
    "--disable-hang-monitor",
    "--disable-ipc-flooding-protection",
    "--disable-popup-blocking",
    "--disable-prompt-on-repost",
    "--disable-renderer-backgrounding",
    "--disable-sync",
    "--force-color-profile=srgb",
    "--metrics-recording-only",
    "--no-first-run",
    "--enable-automation",
    "--password-store=basic",
    "--use-mock-keychain",
    "--enable-blink-features=IdleDetection",
    "--lang=en_US",
];

/// The `.app` bundle whose main executable `exe` is
/// (`X.app/Contents/MacOS/<exe>`), symlinks resolved. `None` for anything
/// else (a bare binary, a wrapper script): LaunchServices opens bundles only.
pub(crate) fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let exe = std::fs::canonicalize(exe).ok()?;
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_app = app
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| app.to_path_buf())
}

/// Chrome's arguments, as `Browser::launch` builds them for our config.
fn switches(profile: &Path, no_sandbox: bool) -> Vec<String> {
    let mut out: Vec<String> = CHROMIUMOXIDE_DEFAULTS
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    out.extend(RENDER_SWITCHES.iter().map(|(k, v)| match v {
        Some(v) => format!("--{k}={v}"),
        None => format!("--{k}"),
    }));
    out.push("--remote-debugging-port=0".to_owned());
    out.push("--disable-extensions".to_owned());
    out.push(format!("--user-data-dir={}", profile.display()));
    if no_sandbox {
        out.push("--no-sandbox".to_owned());
        out.push("--disable-setuid-sandbox".to_owned());
    }
    out.push("--headless=new".to_owned());
    out.push("--hide-scrollbars".to_owned());
    out.push("--mute-audio".to_owned());
    merge_repeated(out)
}

/// One switch per key, as chromiumoxide's `ArgsBuilder` keeps them: a key
/// given again adds its values to the first (`--k=a` + `--k=b` →
/// `--k=a,b`; `--k` twice → `--k`).
fn merge_repeated(switches: Vec<String>) -> Vec<String> {
    let mut merged: Vec<(String, Vec<String>)> = Vec::new();
    for switch in switches {
        let (key, value) = match switch.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (switch, None),
        };
        match merged.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => values.extend(value),
            None => merged.push((key, value.into_iter().collect())),
        }
    }
    merged
        .into_iter()
        .map(|(key, values)| {
            if values.is_empty() {
                key
            } else {
                format!("{key}={}", values.join(","))
            }
        })
        .collect()
}

/// The browser's DevTools socket from `DevToolsActivePort` (the port, then
/// the browser target's path). `None` until Chrome has written both lines.
fn ws_url(port_file: &str) -> Option<String> {
    let mut lines = port_file.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    (port != 0 && path.starts_with('/')).then(|| format!("ws://127.0.0.1:{port}{path}"))
}

/// Starts `app` headless on `profile` and connects to it. Waits at most
/// `timeout` for DevTools ([`LaunchError::Timeout`]); a browser that exits
/// first fails at once with what it printed. The caller's profile ends
/// whatever was started when it drops.
///
/// `Ok(None)`: LaunchServices started nothing although `open` succeeded,
/// as from inside another sandbox (`sandbox-exec`, how the maintainer's
/// pre-merge check runs); the caller starts the browser itself.
pub(crate) async fn launch(
    app: &Path,
    exe: &Path,
    profile: &Path,
    tmp: Option<&Path>,
    no_sandbox: bool,
    timeout: Duration,
    request_timeout: Duration,
) -> Result<Option<(Browser, Handler)>, LaunchError> {
    let cannot_start =
        |why: &str| RenderError::Unavailable(format!("cannot start {}: {why}", exe.display()));
    let stderr = profile.join(STDERR_FILE);
    // Chrome gets no environment from us: the temp dir is the one thing
    // it used to inherit that matters (the profile's own on Linux).
    let tmp = tmp.map_or_else(std::env::temp_dir, Path::to_path_buf);
    let opened = tokio::process::Command::new(OPEN)
        .args(["-n", "-g", "-a"])
        .arg(app)
        .arg("--stderr")
        .arg(&stderr)
        .arg("--env")
        .arg(format!("TMPDIR={}", tmp.display()))
        .arg("--args")
        .args(switches(profile, no_sandbox))
        .output()
        .await
        .map_err(|e| LaunchError::Other(cannot_start(&format!("{OPEN}: {e}"))))?;
    if !opened.status.success() {
        let why = String::from_utf8_lossy(&opened.stderr);
        return Err(LaunchError::Other(cannot_start(why.trim())));
    }
    if !stderr_created(&stderr).await {
        return Ok(None);
    }

    let port_file = profile.join("DevToolsActivePort");
    let started = Instant::now();
    let mut next_exit_check = started + EXIT_CHECK;
    let ws = loop {
        if let Some(ws) = tokio::fs::read_to_string(&port_file)
            .await
            .ok()
            .as_deref()
            .and_then(ws_url)
        {
            break ws;
        }
        let now = Instant::now();
        if now >= next_exit_check {
            next_exit_check = now + EXIT_CHECK;
            let dir = profile.to_path_buf();
            let users = tokio::task::spawn_blocking(move || profile_users_checked(&dir))
                .await
                .ok()
                .flatten();
            // `None`: `ps` cannot run here (sandbox-exec); only the timeout
            // can tell then.
            if users.is_some_and(|u| u.is_empty()) {
                return Err(LaunchError::Other(cannot_start(&format!(
                    "it exited before opening DevTools{}",
                    last_line(&stderr).await
                ))));
            }
        }
        if started.elapsed() >= timeout {
            return Err(LaunchError::Timeout(cannot_start(&format!(
                "DevTools did not open within {} s{}",
                timeout.as_secs(),
                last_line(&stderr).await
            ))));
        }
        tokio::time::sleep(POLL).await;
    };
    let config = HandlerConfig {
        viewport: None,
        request_timeout,
        ..HandlerConfig::default()
    };
    Browser::connect_with_config(ws, config)
        .await
        .map(Some)
        .map_err(|e| LaunchError::Other(cannot_start(&e.to_string())))
}

/// The last line the browser printed, as ` (line)`; empty when none.
async fn last_line(stderr: &Path) -> String {
    let printed = tokio::fs::read_to_string(stderr).await.unwrap_or_default();
    printed
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| format!(" ({})", l.trim()))
        .unwrap_or_default()
}

/// `open` creates the `--stderr` file as it hands it to the new process,
/// before it returns; allow a second in case it lags.
async fn stderr_created(path: &Path) -> bool {
    for _ in 0..20 {
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            return true;
        }
        tokio::time::sleep(POLL).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bundles_main_executable_goes_through_launch_services() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Some Browser.app");
        let exe = app.join("Contents/MacOS/Some Browser");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        let bare = dir.path().join("chrome");
        std::fs::write(&bare, "").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&exe, &link).unwrap();

        let app = std::fs::canonicalize(&app).unwrap();
        assert_eq!(app_bundle(&exe), Some(app.clone()));
        assert_eq!(app_bundle(&link), Some(app), "a symlink to the binary");
        assert_eq!(app_bundle(&bare), None);
        assert_eq!(app_bundle(&dir.path().join("missing")), None);
    }

    #[test]
    fn the_devtools_address_needs_both_lines() {
        assert_eq!(
            ws_url("57001\n/devtools/browser/339a\n").as_deref(),
            Some("ws://127.0.0.1:57001/devtools/browser/339a")
        );
        assert_eq!(ws_url(""), None);
        assert_eq!(ws_url("57001\n"), None, "written halfway");
        assert_eq!(ws_url("0\n/devtools/browser/x"), None);
        assert_eq!(ws_url("x\n/devtools/browser/x"), None);
    }

    #[test]
    fn switches_carry_ours_and_the_sandbox_opt_out() {
        let p = Path::new("/tmp/od-chrome-x");
        let on = switches(p, false);
        assert!(on.contains(&"--user-data-dir=/tmp/od-chrome-x".to_owned()));
        assert!(
            on.contains(&"--host-resolver-rules=MAP * ~NOTFOUND , EXCLUDE localhost".to_owned())
        );
        assert!(!on.iter().any(|a| a.contains("no-sandbox")));
        assert!(switches(p, true).contains(&"--no-sandbox".to_owned()));
        let once = |a: &str| on.iter().filter(|s| *s == a).count() == 1;
        assert!(once("--no-first-run") && once("--disable-sync"), "{on:?}");
    }

    #[test]
    fn a_repeated_switch_is_merged_like_chromiumoxide_does() {
        let raw = ["--a", "--k=x", "--a", "--k=y", "--b=1"].map(str::to_owned);
        assert_eq!(merge_repeated(raw.to_vec()), ["--a", "--k=x,y", "--b=1"]);
    }
}
