//! The switches Chrome actually receives. chromiumoxide prefixes every key
//! with `--`, so `.arg("--no-first-run")` used to reach Chrome as
//! `----no-first-run`, which Chrome ignores: the host-resolver rules that
//! keep renders off the network were never applied.
//! A wrapper records its arguments, then runs the real Chrome.
//! Needs Chrome like `chrome.rs`; `OD_SKIP_CHROME_TESTS=1` skips it.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::{CdpRenderer, chrome_sandbox_works, find_chrome};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

async fn launch_args(no_sandbox: bool) -> Vec<String> {
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("recording-chrome");
    let log = dir.path().join("args.txt");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            chrome.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>Flags</h1>").unwrap();
    let mut renderer = CdpRenderer::with_executable(&wrapper);
    if no_sandbox || !chrome_sandbox_works(&chrome) {
        renderer = renderer.without_chrome_sandbox();
    }
    let pdf = renderer
        .print_pdf(&RenderTarget::new(page), DeckMode::Page)
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    read_lines(&log)
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chrome_gets_the_switches_as_written() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        return;
    }
    let args = launch_args(false).await;
    for want in [
        "--host-resolver-rules=MAP * ~NOTFOUND , EXCLUDE localhost",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-sync",
        "--disable-component-update",
        "--disable-domain-reliability",
        "--metrics-recording-only",
    ] {
        assert!(
            args.iter().any(|a| a == want),
            "{want} missing from {args:?}"
        );
    }
    assert!(
        !args.iter().any(|a| a.starts_with("---")),
        "a switch Chrome ignores: {args:?}"
    );

    let args = launch_args(true).await;
    assert!(args.iter().any(|a| a == "--no-sandbox"), "{args:?}");
}
