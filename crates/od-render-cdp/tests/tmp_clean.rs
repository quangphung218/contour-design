//! Rendering leaves nothing in TMPDIR: the browser profile (`od-chrome-*`)
//! goes with the browser, also when a render fails after Chrome started or
//! its future is dropped midway. One exception, on macOS: a render dropped
//! while LaunchServices starts its browser keeps that profile, marked
//! `od-opening` and with nothing running on it, for a later sweep (a
//! browser starting late would write a removed one back unmarked). A
//! browser that comes up on it after the process quit is ended by the next
//! render, which the outer test runs as a second process.
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
    // Removing a profile ends Chrome's helpers found with `ps`, which is
    // setuid root on macOS and cannot run inside sandbox-exec (the pre-merge
    // check); a helper still exiting there may write the folder back.
    if std::process::Command::new("ps")
        .args(["-p", "1"])
        .output()
        .is_err()
    {
        eprintln!("skipping: ps cannot run in this sandbox");
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
    let _reap = Reap(private.clone());
    let inner = |what: &str| {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["inner_renders", "--exact", "--nocapture"])
            .env("TMPDIR", &private)
            .env(INNER, what)
            .status()
            .unwrap();
        assert!(status.success(), "the {what} render(s) failed");
    };
    inner("renders");
    // A browser LaunchServices starts late for the dropped render comes up
    // after its process quit, in a second or two: the next render runs once
    // it has (or after 10 s, when none came).
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while cfg!(target_os = "macos")
        && marked(&private).iter().any(|p| users(p) == 0)
        && std::time::Instant::now() < until
    {
        std::thread::sleep(Duration::from_millis(100));
    }
    inner("next");
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
            .map(|e| e.unwrap().path())
            .filter(|p| !(cfg!(target_os = "macos") && given_up_while_opening(p)))
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        left.sort();
        if left == expected {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(left, expected, "left in TMPDIR");
}

/// A profile kept because its render was dropped while LaunchServices
/// started the browser: marked, and nothing runs on it.
fn given_up_while_opening(profile: &std::path::Path) -> bool {
    if !profile.join("od-opening").exists() {
        return false;
    }
    assert_eq!(
        users(profile),
        0,
        "a browser still runs on {}",
        profile.display()
    );
    true
}

/// Kills, when dropped, whatever still runs on a profile in the test's own
/// temp dir: a failed run leaves no browser behind once that dir is gone.
struct Reap(std::path::PathBuf);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = std::process::Command::new("pkill")
            .args(["-9", "-f", "--"])
            .arg(format!("--user-data-dir={}", self.0.display()))
            .status();
    }
}

/// The `od-opening` profiles in `tmp`.
fn marked(tmp: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(tmp)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("od-opening").exists())
        .collect()
}

/// How many processes run with `profile` as their profile.
fn users(profile: &std::path::Path) -> usize {
    let needle = format!("--user-data-dir={}", profile.display());
    let ps = std::process::Command::new("ps")
        .args(["-A", "-ww", "-o", "args="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&ps.stdout)
        .lines()
        .filter(|l| l.contains(&needle))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inner_renders() {
    let Some(inner) = std::env::var_os(INNER) else {
        return;
    };
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>Hi</h1>").unwrap();
    let target = RenderTarget::new(page);

    let shot = renderer
        .capture_page(&target, od_export::CaptureOptions::default())
        .await;
    assert!(shot.is_ok(), "{shot:?}");
    if inner == "next" {
        return; // the next render after the process below quit
    }
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
