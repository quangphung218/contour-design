//! A browser that cannot start must give a clear error quickly, and the
//! product must never answer that by turning Chrome's sandbox off itself.
//! The "browsers" here are scripts that record their arguments: one exits
//! at once, one hangs without ever opening DevTools. No Chrome needed.
//! Only the hang is tried again (a cold first start can be slow).

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::{CdpRenderer, NO_SANDBOX_ENV};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// A fake browser running `body` after writing its arguments to `args.txt`
/// and a line per launch to `launches.txt`.
fn fake_browser(dir: &Path, body: &str) -> std::path::PathBuf {
    let exe = dir.join("fake-chrome");
    let log = dir.join("args.txt");
    std::fs::write(
        &exe,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\necho x >> '{}'\n{body}\n",
            log.display(),
            dir.join("launches.txt").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

async fn fails_fast_without_dropping_the_sandbox(body: &str, limit: Duration, launches: usize) {
    if std::env::var(NO_SANDBOX_ENV).as_deref() == Ok("1") {
        eprintln!("skipping: {NO_SANDBOX_ENV}=1 is set for this run");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_browser(dir.path(), body);
    let page = dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>x</h1>").unwrap();

    let started = Instant::now();
    let result = tokio::time::timeout(
        limit + Duration::from_secs(30),
        CdpRenderer::with_executable(&exe).print_pdf(&RenderTarget::new(page), DeckMode::Page),
    )
    .await
    .expect("the export hung instead of failing");
    let took = started.elapsed();

    let err = result.expect_err("a browser that cannot start cannot render");
    assert!(
        took <= limit,
        "took {took:?} to fail (limit {limit:?}): {err}"
    );
    assert!(
        err.to_string().contains("cannot start"),
        "the error says what went wrong: {err}"
    );
    let launched = std::fs::read_to_string(dir.path().join("launches.txt")).unwrap();
    assert_eq!(launched.lines().count(), launches, "browser launches");
    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    assert!(!args.is_empty(), "the fake browser was launched");
    assert!(
        !args.lines().any(|a| a.contains("no-sandbox")),
        "the product turned Chrome's sandbox off by itself: {args}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_browser_that_exits_at_once_fails_within_seconds() {
    fails_fast_without_dropping_the_sandbox(
        "echo 'cannot start' >&2; exit 1",
        Duration::from_secs(10),
        1,
    )
    .await;
}

/// No DevTools URL ever: the launch timeout (30 s) ends each of two tries.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_browser_that_never_opens_devtools_fails_after_two_launch_timeouts() {
    fails_fast_without_dropping_the_sandbox("exec sleep 600", Duration::from_secs(70), 2).await;
}
