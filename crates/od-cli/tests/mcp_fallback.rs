//! `od mcp` picks its daemon like `<app> mcp` (`od_mcp::locate`): a
//! `--daemon-url` or `OD_DAEMON_URL` with no daemon behind it gives way to
//! the running app's in `<data>/daemon.lock`, and says so on stderr.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const OD: &str = env!("CARGO_BIN_EXE_od");

/// Runs `od <args>` on `data` (with `env` set), calls `list_projects` and
/// returns its result and the server's stderr.
fn list_projects(data: &Path, args: &[&str], env: Option<&str>) -> (Value, String) {
    let mut cmd = Command::new(OD);
    cmd.args(args)
        .env("OD_DATA_DIR", data)
        .env_remove("OD_DAEMON_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(url) = env {
        cmd.env("OD_DAEMON_URL", url);
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for msg in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "list_projects", "arguments": {}}}),
    ] {
        writeln!(stdin, "{msg}").unwrap();
    }
    stdin.flush().unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    let result = loop {
        let Ok(line) = rx.recv_timeout(Duration::from_secs(60)) else {
            let _ = child.kill();
            panic!("no list_projects answer within 60 s");
        };
        let msg: Value = serde_json::from_str(&line).unwrap();
        if msg["id"] == 2 {
            break msg["result"].clone();
        }
    };
    // Closing stdin ends the server; then its stderr is complete.
    drop(stdin);
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    child.wait().unwrap();
    assert_ne!(result["isError"], true, "{result}");
    (result, stderr)
}

/// A daemon that answers every request with `body` (JSON).
fn stub_daemon(body: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut conn in listener.incoming().map_while(Result::ok) {
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && conn.read(&mut byte).is_ok_and(|n| n == 1) {
                head.push(byte[0]);
            }
            let body = body.to_string();
            let _ = write!(
                conn,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// A loopback URL with nothing listening: a port that was free a moment ago.
fn dead_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

/// The running app, its URL in `data`'s `daemon.lock`.
fn running_app(data: &Path) -> String {
    let url = stub_daemon(json!({"projects": [{"id": "p-from-the-lock", "name": "Lock"}]}));
    std::fs::write(data.join("daemon.lock"), &url).unwrap();
    url
}

#[test]
fn a_dead_daemon_url_flag_falls_back_on_daemon_lock() {
    let data = tempfile::tempdir().unwrap();
    let lock = running_app(data.path());
    let dead = dead_url();
    let (result, stderr) = list_projects(data.path(), &["mcp", "--daemon-url", &dead], None);
    assert!(result.to_string().contains("p-from-the-lock"), "{result}");
    assert!(
        stderr.contains(&format!(
            "no daemon at {dead}; using the running app's {lock}"
        )),
        "{stderr}"
    );
}

#[test]
fn a_dead_od_daemon_url_falls_back_on_daemon_lock() {
    let data = tempfile::tempdir().unwrap();
    running_app(data.path());
    let (result, _) = list_projects(data.path(), &["mcp"], Some(&dead_url()));
    assert!(result.to_string().contains("p-from-the-lock"), "{result}");
}

#[test]
fn a_live_daemon_url_wins_over_daemon_lock() {
    let data = tempfile::tempdir().unwrap();
    running_app(data.path());
    let flag = stub_daemon(json!({"projects": [{"id": "p-from-the-flag", "name": "Flag"}]}));
    // The global flag before the subcommand, as `od --daemon-url X mcp`.
    let (result, stderr) = list_projects(data.path(), &["--daemon-url", &flag, "mcp"], None);
    assert!(result.to_string().contains("p-from-the-flag"), "{result}");
    assert!(!stderr.contains("no daemon at"), "{stderr}");
}

#[test]
fn without_a_url_od_mcp_reaches_the_running_app() {
    let data = tempfile::tempdir().unwrap();
    running_app(data.path());
    let (result, _) = list_projects(data.path(), &["mcp"], None);
    assert!(result.to_string().contains("p-from-the-lock"), "{result}");
}
