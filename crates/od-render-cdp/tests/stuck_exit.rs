//! A browser that does not exit after `Browser.close` must not hang the
//! export: the Atlat gate once saw `od export --format pdf` wait the CLI's
//! full 600 s while the daemon sat in `Session::close` waiting for Chrome.
//!
//! The "browser" here is a wrapper that starts the real Chrome in the
//! background, then ignores SIGTERM and never exits on its own.
//! Needs Chrome like `chrome.rs`; `OD_SKIP_CHROME_TESTS=1` skips it.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::find_chrome;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_browser_that_never_exits_does_not_hang_the_export() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("stuck-chrome");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ntrap '' TERM\n'{}' \"$@\" &\nwhile :; do sleep 1; done\n",
            chrome.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>Stuck</h1>").unwrap();

    let renderer = common::renderer(&wrapper);
    let started = Instant::now();
    let pdf = tokio::time::timeout(
        Duration::from_secs(90),
        renderer.print_pdf(&RenderTarget::new(page), DeckMode::Page),
    )
    .await
    .expect("the export finished instead of waiting for the browser forever")
    .unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    eprintln!("finished in {:?}", started.elapsed());
    // Killed, not left running: no process runs the wrapper any more.
    let left = std::process::Command::new("pgrep")
        .args(["-f", &wrapper.display().to_string()])
        .output()
        .unwrap();
    assert!(
        left.stdout.is_empty(),
        "the stuck browser is still running: {}",
        String::from_utf8_lossy(&left.stdout)
    );
}
