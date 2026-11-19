//! No export waits forever on the page or on a browser that is gone.
//! chromiumoxide 0.9.1 waits forever for a page promise that never settles
//! and for every call once Chrome exits (PR #269 probes), so each step
//! after the page opens is bounded (`STEP_TIMEOUT`) and a lost connection
//! fails the step waiting at once.
//!
//! Needs Chrome like `chrome.rs`; `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::{CaptureOptions, DeckMode, FrameOptions, RenderError, RenderTarget, Renderer};
use od_render_cdp::find_chrome;
use std::time::{Duration, Instant};

/// A page that stops animation frames: settling waits two of them, so its
/// promise never settles (an agent-written page can do this by mistake).
const NO_FRAMES: &str = "<!doctype html><html><head><script>\
window.requestAnimationFrame = function () { return 0; };\
</script></head><body><section class=\"slide\" data-composition-id=\"main\" data-duration=\"1\">\
<h1>Frozen</h1></section></body></html>";

fn chrome() -> Option<std::path::PathBuf> {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return None;
    }
    Some(find_chrome().expect("Chrome/Edge/Chromium is required for these tests"))
}

/// One export of kind `name`, its output dropped.
async fn export(
    renderer: &od_render_cdp::CdpRenderer,
    target: &RenderTarget,
    name: &str,
    frames_dir: &std::path::Path,
) -> Result<(), RenderError> {
    match name {
        "pdf" => renderer.print_pdf(target, DeckMode::Auto).await.map(drop),
        "png" => renderer
            .capture_page(target, CaptureOptions::default())
            .await
            .map(drop),
        "slides" => renderer
            .render_slides(target, DeckMode::Auto)
            .await
            .map(drop),
        "layouts" => renderer.extract_layouts(target).await.map(drop),
        _ => {
            let options = FrameOptions {
                fps: 1,
                duration_ms: 1000,
                require_timeline: false,
            };
            renderer
                .render_frames(target, options, frames_dir)
                .await
                .map(drop)
        }
    }
}

/// Every export kind ends with an error naming the step within the step
/// limit, instead of waiting on the page forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_that_never_settles_fails_every_export_in_time() {
    let Some(exe) = chrome() else { return };
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("index.html");
    std::fs::write(&page, NO_FRAMES).unwrap();
    let target = RenderTarget::new(page);
    let limit = Duration::from_secs(2);
    let renderer = common::renderer(&exe).with_step_timeout(limit);
    let frames_dir = dir.path().join("frames");

    for name in ["pdf", "png", "slides", "layouts", "frames"] {
        let export = export(&renderer, &target, name, &frames_dir);
        let started = Instant::now();
        // Opening (up to a few seconds) + the step limit + closing.
        let result = tokio::time::timeout(Duration::from_secs(60), export)
            .await
            .unwrap_or_else(|_| panic!("{name}: the export still waits after 60 s"));
        let took = started.elapsed();
        let Err(RenderError::Failed(msg)) = result else {
            panic!("{name}: {result:?}");
        };
        assert!(
            msg.contains("gave no answer within 2 s while waiting for the page to load"),
            "{name}: {msg}"
        );
        eprintln!("{name}: failed in {took:?}: {msg}");
    }
}

/// Chrome killed in the middle of an export: the step waiting fails at
/// once (not after the step limit) and says the browser quit.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_browser_killed_during_an_export_fails_it_at_once() {
    use std::os::unix::fs::PermissionsExt;
    let Some(exe) = chrome() else { return };
    let dir = tempfile::tempdir().unwrap();
    // `exec` keeps the pid: the one written is the browser's own.
    let pid_file = dir.path().join("chrome.pid");
    let wrapper = dir.path().join("chrome-with-pid");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec '{}' \"$@\"\n",
            pid_file.display(),
            exe.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let page = dir.path().join("index.html");
    std::fs::write(&page, NO_FRAMES).unwrap();
    let target = RenderTarget::new(page);
    // The default limit (a minute): only the lost connection can end it soon.
    let renderer = common::renderer(&wrapper);

    let export = tokio::spawn(async move { renderer.print_pdf(&target, DeckMode::Auto).await });
    let deadline = Instant::now() + Duration::from_secs(30);
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
        {
            break pid;
        }
        assert!(Instant::now() < deadline, "the browser never started");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    // Let it open the page and start settling (opening takes 1–2 s).
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!export.is_finished(), "the export ended before the kill");
    let killed = Instant::now();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(20), export)
        .await
        .expect("the export still waits 20 s after the browser was killed")
        .unwrap();
    let took = killed.elapsed();
    // Settling, as a rule; still opening the page on a slow machine.
    let msg = match result {
        Err(RenderError::Failed(msg)) if msg.contains("the browser quit while") => msg,
        Err(RenderError::Unavailable(msg)) if msg.contains("quit before it showed the page") => msg,
        other => panic!("{other:?}"),
    };
    eprintln!("failed {took:?} after the kill: {msg}");
}
