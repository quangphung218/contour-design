//! The daemon stops at once while a tab of the app is open: the event
//! streams every screen keeps open (memory, the shown project's files) end
//! on the shutdown signal instead of holding it for the whole drain
//! (QA-092: 5 s on every quit, update and restart).

#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

/// Opens the event stream at `path` and waits for its first event.
async fn open_stream(d: &common::Daemon, path: &str) -> reqwest::Response {
    let mut res = d
        .http
        .get(format!("{}{path}", d.base))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "{path}: {}", res.status());
    let first = tokio::time::timeout(Duration::from_secs(10), res.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!first.is_empty(), "{path}: the first event");
    res
}

#[tokio::test]
async fn the_daemon_stops_at_once_with_the_app_s_event_streams_open() {
    let mut d = common::daemon().await;
    let (project, _) = d.project().await;
    let memory = open_stream(&d, "/api/memory/events").await;
    let files = open_stream(&d, &format!("/api/projects/{project}/events")).await;
    let t0 = Instant::now();
    d.stop_serving().await.unwrap();
    let took = t0.elapsed();
    assert!(
        took < Duration::from_secs(2),
        "the daemon took {took:?} to stop with two event streams open"
    );
    drop((memory, files));
}
