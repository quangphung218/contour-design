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
//!
//! LaunchServices runs Gatekeeper's first-launch check, which a direct
//! start skips: an app still quarantined and never approved (Chrome
//! installed by `brew install --cask` and not yet opened from Finder, as on
//! the GitHub macOS runners) waits for someone to click "Open", and `open`
//! with it. Such a bundle is not opened this way ([`awaits_approval`]), and
//! the whole start has one deadline; the caller starts the browser as its
//! child when this way fails.

use super::{LaunchError, RENDER_SWITCHES, profile_users_checked};
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::{Browser, Handler};
use od_export::RenderError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) const OPEN: &str = "/usr/bin/open";
/// Gatekeeper's mark on what was downloaded: `<flags>;<time>;<agent>;<id>`.
const QUARANTINE: &str = "com.apple.quarantine";
/// Quarantine flag set once the user approved the app's first launch.
const QTN_USER_APPROVED: u32 = 0x40;
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

/// Whether LaunchServices would hold `app` for Gatekeeper's first-launch
/// approval: it is quarantined and the user has not approved it yet.
pub(crate) fn awaits_approval(app: &Path) -> bool {
    xattr::get(app, QUARANTINE)
        .ok()
        .flatten()
        .is_some_and(|v| unapproved(&String::from_utf8_lossy(&v)))
}

/// A quarantine value whose flags (hex, first field) lack the approval.
/// One that cannot be read is left to the launch deadline.
fn unapproved(quarantine: &str) -> bool {
    quarantine
        .split(';')
        .next()
        .and_then(|flags| u32::from_str_radix(flags, 16).ok())
        .is_some_and(|flags| flags & QTN_USER_APPROVED == 0)
}

/// The browser's DevTools socket from `DevToolsActivePort` (the port, then
/// the browser target's path). `None` until Chrome has written both lines.
fn ws_url(port_file: &str) -> Option<String> {
    let mut lines = port_file.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    (port != 0 && path.starts_with('/')).then(|| format!("ws://127.0.0.1:{port}{path}"))
}

/// Starts `app` headless on `profile` through the `open` at `open`
/// ([`OPEN`]; a stand-in in tests) and connects to it, all within `timeout`
/// ([`LaunchError::Timeout`]: `open` itself can hang, see the module docs);
/// a browser that exits first fails at once with what it printed. The
/// caller marks the profile before this ([`super::Profile::mark_opening`])
/// and abandons it on failure ([`super::Profile::abandon`]): LaunchServices
/// may still start the browser after `open` was killed, or after this
/// future was dropped.
///
/// `Ok(None)`, before starting anything: `ps` cannot run here, as inside
/// `sandbox-exec` (how the maintainer's pre-merge check runs). A browser
/// that is not our child can only be found, and ended, through `ps`; the
/// caller starts it as its child instead. (`open` still works there; only
/// its `--stderr` file is not created, so that is no sign of a launch.)
#[allow(clippy::too_many_arguments)] // what Chrome needs, plus `open`
pub(crate) async fn launch(
    open: &Path,
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
    let timed_out = |stderr: String| {
        LaunchError::Timeout(cannot_start(&format!(
            "DevTools did not open within {} s{stderr}",
            timeout.as_secs()
        )))
    };
    let probe = profile.to_path_buf();
    let ps_runs = tokio::task::spawn_blocking(move || profile_users_checked(&probe).is_some())
        .await
        .unwrap_or(false);
    if !ps_runs {
        return Ok(None);
    }
    let stderr = profile.join(STDERR_FILE);
    // Chrome gets no environment from us: the temp dir is the one thing
    // it used to inherit that matters (the profile's own on Linux).
    let tmp = tmp.map_or_else(std::env::temp_dir, Path::to_path_buf);
    let started = Instant::now();
    let deadline = started + timeout;
    let opening = tokio::process::Command::new(open)
        .args(["-n", "-g", "-a"])
        .arg(app)
        .arg("--stderr")
        .arg(&stderr)
        .arg("--env")
        .arg(format!("TMPDIR={}", tmp.display()))
        .arg("--args")
        .args(switches(profile, no_sandbox))
        .kill_on_drop(true)
        .output();
    let opened = tokio::time::timeout_at(deadline.into(), opening)
        .await
        .map_err(|_| timed_out(String::new()))?
        .map_err(|e| LaunchError::Other(cannot_start(&format!("{}: {e}", open.display()))))?;
    if !opened.status.success() {
        let why = String::from_utf8_lossy(&opened.stderr);
        return Err(LaunchError::Other(cannot_start(why.trim())));
    }

    let port_file = profile.join("DevToolsActivePort");
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
            // `None`: `ps` stopped answering; only the timeout can tell.
            if users.is_some_and(|u| u.is_empty()) {
                return Err(LaunchError::Other(cannot_start(&format!(
                    "it exited before opening DevTools{}",
                    last_line(&stderr).await
                ))));
            }
        }
        if Instant::now() >= deadline {
            return Err(timed_out(last_line(&stderr).await));
        }
        tokio::time::sleep(POLL).await;
    };
    let config = HandlerConfig {
        viewport: None,
        request_timeout,
        ..HandlerConfig::default()
    };
    tokio::time::timeout_at(deadline.into(), Browser::connect_with_config(ws, config))
        .await
        .map_err(|_| timed_out(String::new()))?
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

/// Removes Gatekeeper's quarantine from `path` and everything inside it,
/// and checks none is left: a test's bundle must never make LaunchServices
/// show "… Not Opened" on the screen of whoever runs the tests (a file this
/// process writes can be quarantined when what runs it quarantines files).
#[cfg(test)]
pub(crate) fn unquarantine(path: &Path) {
    let _ = xattr::remove(path, QUARANTINE);
    let left = xattr::get(path, QUARANTINE).ok().flatten();
    assert!(left.is_none(), "{} is still quarantined", path.display());
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        for entry in std::fs::read_dir(path).unwrap() {
            unquarantine(&entry.unwrap().path());
        }
    }
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
        unquarantine(&app);

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
    fn only_an_unapproved_quarantine_waits_for_gatekeeper() {
        // As Homebrew Cask leaves Chrome on the GitHub macOS runners.
        assert!(unapproved("0181;6af5c21a;Homebrew Cask;2274814B-610A"));
        assert!(unapproved("0083;6af5c21a;Safari;"));
        assert!(!unapproved("01c1;6af5c21a;Homebrew Cask;"), "approved");
        assert!(
            !unapproved("zz;6af5c21a;x;"),
            "unreadable: left to the deadline"
        );
        assert!(!unapproved(""));

        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Some Browser.app");
        std::fs::create_dir(&app).unwrap();
        assert!(!awaits_approval(&app), "not quarantined");
        xattr::set(&app, QUARANTINE, b"0181;6af5c21a;Homebrew Cask;").unwrap();
        assert!(awaits_approval(&app));
        xattr::set(&app, QUARANTINE, b"01c1;6af5c21a;Homebrew Cask;").unwrap();
        assert!(!awaits_approval(&app), "approved from Finder");
    }

    /// `open` itself can hang (Gatekeeper waiting for a click): the launch
    /// deadline covers it, and the `open` that hung is killed.
    #[tokio::test]
    async fn an_open_that_hangs_times_out_and_is_killed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let running = dir.path().join("running");
        let open = dir.path().join("open");
        // One process (`exec`), whose arguments keep ours: the profile in
        // them is what finds it below. A comment would not reach `ps`.
        std::fs::write(
            &open,
            format!(
                "#!/bin/sh\ntouch '{}'\nexec /usr/bin/perl -e 'sleep 60' -- \"$@\"\n",
                running.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        let profile = dir.path().join("profile");
        std::fs::create_dir(&profile).unwrap();
        let exe = Path::new("/nowhere/X.app/Contents/MacOS/X");

        let started = Instant::now();
        let res = launch(
            &open,
            Path::new("/nowhere/X.app"),
            exe,
            &profile,
            None,
            false,
            Duration::from_secs(1),
            Duration::from_secs(5),
        )
        .await;
        let took = started.elapsed();
        match res {
            Ok(None) => return, // no `ps` here (sandbox-exec): nothing was opened
            Ok(Some(_)) => panic!("a hung `open` connected"),
            Err(LaunchError::Other(e)) => panic!("not a timeout: {e}"),
            Err(LaunchError::Timeout(e)) => {
                assert!(e.to_string().contains("within 1 s"), "{e}");
            }
        }
        assert!(took < Duration::from_secs(10), "took {took:?}");
        assert!(running.exists(), "the stand-in `open` never ran");
        let needle = profile.display().to_string();
        let gone = || {
            std::process::Command::new("pgrep")
                .args(["-f", &needle])
                .output()
                .unwrap()
                .stdout
                .is_empty()
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gone() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let left = !gone();
        if left {
            let _ = std::process::Command::new("pkill")
                .args(["-9", "-f", &needle])
                .status();
        }
        assert!(!left, "the hung `open` was left running");
    }

    /// What the test bundles rely on to show no dialog.
    #[test]
    fn unquarantine_clears_a_bundle_all_the_way_down() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Some Browser.app");
        let exe = app.join("Contents/MacOS/Some Browser");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        let all = [&app, &app.join("Contents"), &exe];
        for path in all {
            xattr::set(path, QUARANTINE, b"0181;6af5c21a;Homebrew Cask;").unwrap();
        }
        unquarantine(&app);
        for path in all {
            assert_eq!(xattr::get(path, QUARANTINE).unwrap(), None, "{path:?}");
        }
        unquarantine(&app); // none left is fine too
    }

    #[test]
    fn a_repeated_switch_is_merged_like_chromiumoxide_does() {
        let raw = ["--a", "--k=x", "--a", "--k=y", "--b=1"].map(str::to_owned);
        assert_eq!(merge_repeated(raw.to_vec()), ["--a", "--k=x,y", "--b=1"]);
    }
}
