//! A rendered page loads nothing but itself: no other `file://` URL (a
//! local file must not end up in an exported PNG/PDF) and no network, IP
//! literals included.
//!
//! Needs Chrome (see `chrome.rs`); `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_export::image::decode_rgb;
use od_export::{CaptureOptions, RenderTarget, Renderer};
use od_render_cdp::{CdpRenderer, find_chrome};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn center(png: &[u8]) -> [u8; 3] {
    let rgb = decode_rgb(png).unwrap();
    let i = ((rgb.height as usize / 2) * rgb.width as usize + rgb.width as usize / 2) * 3;
    [rgb.pixels[i], rgb.pixels[i + 1], rgb.pixels[i + 2]]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_cannot_frame_local_files_or_reach_ip_literals() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = CdpRenderer::with_executable(find_chrome().expect("Chrome is required"));

    // A file outside the export: solid red if it ever shows.
    let outside = tempfile::tempdir().unwrap();
    let local = outside.path().join("local.html");
    std::fs::write(
        &local,
        "<body style='margin:0;background:rgb(255,0,0)'></body>",
    )
    .unwrap();
    let local_url = format!("file://{}", local.canonicalize().unwrap().display());

    // A loopback listener addressed by IP literal (no resolver involved).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    {
        let hits = Arc::clone(&hits);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                hits.fetch_add(1, Ordering::SeqCst);
                drop(conn); // answer nothing, close at once
            }
        });
    }

    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(
        &page,
        format!(
            "<!doctype html><body style='margin:0;background:rgb(0,0,255)'>\
<iframe src='{local_url}' style='border:0;width:100vw;height:100vh'></iframe>\
<img src='http://127.0.0.1:{port}/ping.png'></body>"
        ),
    )
    .unwrap();
    let png = renderer
        .capture_page(&RenderTarget::new(page), CaptureOptions::default())
        .await
        .unwrap();
    let px = center(&png);
    // Blocked, the frame shows Chrome's grey error page instead.
    let red = px[0] > 200 && px[1] < 60 && px[2] < 60;
    assert!(!red, "the local file was rendered: {px:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the page reached 127.0.0.1:{port}"
    );
}
