//! The command Integrations ▸ Install registers (`<app> mcp --daemon-url …`)
//! is an MCP stdio server: it answers `initialize` and `tools/list` without
//! opening a window or starting a daemon.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn mcp_answers_initialize_and_tools_list_over_stdio() {
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_od-desktop"))
        .args(["mcp", "--daemon-url", "http://127.0.0.1:9"])
        // No daemon.lock there: the flag's URL is used.
        .env("OD_DATA_DIR", data.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for msg in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
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
    let mut replies = std::collections::HashMap::new();
    while replies.len() < 2 {
        let Ok(line) = rx.recv_timeout(Duration::from_secs(60)) else {
            let _ = child.kill();
            panic!("no JSON-RPC answer within 60 s; got {replies:?}");
        };
        let msg: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout is not JSON-RPC ({e}): {line}"));
        if let Some(id) = msg["id"].as_u64() {
            replies.insert(id, msg);
        }
    }
    assert_eq!(replies[&1]["result"]["serverInfo"]["name"], "contour");
    let tools = replies[&2]["result"]["tools"].as_array().unwrap();
    assert!(
        tools.iter().any(|t| t["name"] == "list_projects"),
        "{tools:?}"
    );

    // Closing stdin ends the server.
    drop(stdin);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "still running after stdin closed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
