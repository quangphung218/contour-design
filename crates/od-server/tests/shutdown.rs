//! Stopping the daemon (Ctrl-C) ends what its terminal tabs started, even
//! a job that ignores SIGHUP. Alone in its binary: it signals itself.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use futures_util::SinkExt;
use od_server::{ServeOptions, serve};
use od_store::DataDir;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

async fn wait_for_url(data: &Path) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = std::fs::read_to_string(data.join("daemon.lock")) {
            if text.contains("http://") {
                return text.trim().to_owned();
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "daemon never came up"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Pids whose command line is exactly `sleep <secs>`.
fn sleepers(secs: &str) -> Vec<String> {
    let out = std::process::Command::new("pgrep")
        .args(["-f", &format!("^sleep {secs}$")])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn ctrl_c_kills_terminal_jobs_that_ignore_sighup() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let daemon = tokio::spawn(serve(ServeOptions {
        host: "127.0.0.1".to_owned(),
        port: 0,
        data_dir: DataDir::new(&data),
        resource_root: tmp.path().join("no-content"),
        api_token: None,
        allowed_origins: Vec::new(),
    }));
    let base = wait_for_url(&data).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let project: Value = http
        .post(format!("{base}/api/projects"))
        .json(&json!({"name": "Shutdown"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project = project["project"]["id"].as_str().unwrap().to_owned();
    let term: Value = http
        .post(format!("{base}/api/projects/{project}/terminals"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tid = term["id"].as_str().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!(
        "{}/api/terminals/{tid}/ws",
        base.replace("http://", "ws://")
    ))
    .await
    .unwrap();
    let secs = (700_000 + std::process::id() % 100_000).to_string();
    let line = format!("sh -c \"trap '' HUP; exec sleep {secs}\"\r");
    ws.send(Message::Text(
        json!({"type": "input", "data": line}).to_string().into(),
    ))
    .await
    .unwrap();
    until("the job never started", || !sleepers(&secs).is_empty()).await;

    nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT).unwrap();
    let stopped = tokio::time::timeout(Duration::from_secs(15), daemon).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut left = sleepers(&secs);
    while !left.is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
        left = sleepers(&secs);
    }
    if !left.is_empty() {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .args(&left)
            .status();
    }
    assert!(stopped.is_ok(), "the daemon did not stop");
    assert!(left.is_empty(), "the job outlived the daemon: {left:?}");
}
