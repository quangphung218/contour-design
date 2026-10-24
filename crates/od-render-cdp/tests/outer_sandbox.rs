//! Rendering from inside another sandbox (macOS `sandbox-exec`, which is how
//! the Atlat gate runs `make check`). Chrome cannot start its own sandbox
//! there: its renderers never come up and, before this test, every page
//! command waited out its timeout until `od export` gave up after 600 s.
//!
//! The outer test runs this binary again under `sandbox-exec` (or directly,
//! when it is already inside one and cannot nest) and checks that
//! `OD_CHROME_NO_SANDBOX=1` renders, and that without it the export fails
//! fast with a message naming the variable.
//! Needs Chrome like `chrome.rs`; `OD_SKIP_CHROME_TESTS=1` skips it.

#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::{CdpRenderer, find_chrome};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const INNER: &str = "OD_OUTER_SANDBOX_INNER";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const PROFILE: &str = "(version 1)(allow default)";

/// Runs the inner test with `env`, inside `sandbox-exec` when that can nest
/// here; kills it after `limit`. Returns (finished in time, success, output).
fn inner(env: &[(&str, &str)], limit: Duration) -> (bool, bool, String) {
    let nests = Command::new(SANDBOX_EXEC)
        .args(["-p", PROFILE, "/usr/bin/true"])
        .status()
        .is_ok_and(|s| s.success());
    let exe = std::env::current_exe().unwrap();
    let mut cmd = if nests {
        let mut c = Command::new(SANDBOX_EXEC);
        c.args(["-p", PROFILE]).arg(exe);
        c
    } else {
        Command::new(exe)
    };
    cmd.args(["inner_render", "--exact", "--nocapture"])
        .env(INNER, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let out = child.wait_with_output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr);
            return (true, status.success(), text);
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return (false, false, String::new());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn rendering_inside_an_outer_sandbox() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1")
        || std::env::var_os(INNER).is_some()
    {
        return;
    }
    let (in_time, ok, out) = inner(&[("OD_CHROME_NO_SANDBOX", "1")], Duration::from_secs(150));
    assert!(
        in_time,
        "OD_CHROME_NO_SANDBOX=1: the render hung inside the sandbox"
    );
    assert!(
        ok && out.contains("RENDERED"),
        "OD_CHROME_NO_SANDBOX=1 must render:\n{out}"
    );

    let log = tempfile::tempdir().unwrap();
    let args = log.path().join("args.txt");
    let (in_time, ok, out) = inner(
        &[
            ("OD_CHROME_NO_SANDBOX", "0"),
            ("OD_RECORD_ARGS", args.to_str().unwrap()),
        ],
        Duration::from_secs(150),
    );
    assert!(
        in_time,
        "without OD_CHROME_NO_SANDBOX the export must fail fast, not hang"
    );
    assert!(ok, "inner run failed:\n{out}");
    assert!(
        out.contains("RENDERED")
            || out.contains("FAILED: ") && out.contains("OD_CHROME_NO_SANDBOX"),
        "the error must name OD_CHROME_NO_SANDBOX:\n{out}"
    );
    // The product never turns Chrome's sandbox off by itself.
    let argv = std::fs::read_to_string(&args).unwrap();
    assert!(
        !argv.lines().any(|a| a.contains("no-sandbox")),
        "launched with --no-sandbox without the opt-in: {argv}"
    );
}

/// The render itself, run by [`rendering_inside_an_outer_sandbox`].
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inner_render() {
    if std::env::var_os(INNER).is_none() {
        return;
    }
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>Inside</h1>").unwrap();
    // OD_RECORD_ARGS: launch through a wrapper that writes Chrome's argv.
    let exe = match std::env::var("OD_RECORD_ARGS") {
        Ok(log) => {
            use std::os::unix::fs::PermissionsExt;
            let wrapper = dir.path().join("recording-chrome");
            let script = format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{log}'\nexec '{}' \"$@\"\n",
                chrome.display()
            );
            std::fs::write(&wrapper, script).unwrap();
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
            wrapper
        }
        Err(_) => chrome,
    };
    let renderer = CdpRenderer::with_executable(exe);
    match renderer
        .print_pdf(&RenderTarget::new(page), DeckMode::Page)
        .await
    {
        Ok(pdf) if pdf.starts_with(b"%PDF") => println!("RENDERED"),
        Ok(_) => println!("FAILED: not a PDF"),
        Err(e) => println!("FAILED: {e}"),
    }
}
