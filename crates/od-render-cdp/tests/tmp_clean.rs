//! Rendering leaves nothing in TMPDIR: the browser profile (`od-chrome-*`)
//! goes with the browser, also when a render fails after Chrome started or
//! its future is dropped midway.
//!
//! The outer test runs this binary again with `TMPDIR` pointed at a fresh
//! folder (the inner test does the renders), then counts what is left.
//! Needs Chrome like `chrome.rs`; `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::find_chrome;
use std::time::Duration;

const INNER: &str = "OD_TMP_CLEAN_INNER";

#[test]
fn renders_leave_tmpdir_empty() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1")
        || std::env::var_os(INNER).is_some()
    {
        return;
    }
    let outer = tempfile::tempdir().unwrap();
    let private = outer.path().join("tmp");
    std::fs::create_dir(&private).unwrap();
    // What a render killed with its process left behind: a profile whose
    // Chrome is gone (its lock names a dead pid), and one that never got
    // as far as a lock, long ago.
    #[cfg(unix)]
    {
        let dead = private.join("od-chrome-dead01");
        std::fs::create_dir_all(dead.join("Default")).unwrap();
        std::os::unix::fs::symlink("somehost-999999", dead.join("SingletonLock")).unwrap();
        let old = private.join("od-chrome-old001");
        std::fs::create_dir(&old).unwrap();
        let long_ago = std::time::SystemTime::now() - Duration::from_secs(3 * 3600);
        std::fs::File::open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        // A live one (its lock names this process) is not touched.
        let live = private.join("od-chrome-live01");
        std::fs::create_dir(&live).unwrap();
        std::os::unix::fs::symlink(
            format!("somehost-{}", std::process::id()),
            live.join("SingletonLock"),
        )
        .unwrap();
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["inner_renders", "--exact", "--nocapture"])
        .env("TMPDIR", &private)
        .env(INNER, "1")
        .status()
        .unwrap();
    assert!(status.success(), "the renders failed");
    // Chrome's helpers may still be on their way out.
    let expected: &[&str] = if cfg!(unix) {
        &["od-chrome-live01"]
    } else {
        &[]
    };
    let mut left = Vec::new();
    for _ in 0..50 {
        left = std::fs::read_dir(&private)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        if left == expected {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(left, expected, "left in TMPDIR");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inner_renders() {
    if std::env::var_os(INNER).is_none() {
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>Hi</h1>").unwrap();
    let target = RenderTarget::new(page);

    let shot = renderer
        .capture_page(&target, od_export::CaptureOptions::default())
        .await;
    assert!(shot.is_ok(), "{shot:?}");
    // Fails after Chrome started: the page file is gone.
    let missing = RenderTarget::new(dir.path().join("missing.html"));
    assert!(renderer.print_pdf(&missing, DeckMode::Auto).await.is_err());
    // Given up on midway (a request timed out, the client went away).
    let dropped = tokio::time::timeout(
        Duration::from_millis(400),
        renderer.print_pdf(&target, DeckMode::Auto),
    )
    .await;
    drop(dropped);
    drop(dir);
}
