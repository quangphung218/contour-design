//! The command Integrations ▸ Install registers (`<app> mcp`) is an MCP
//! stdio server: it answers `initialize` and `tools/list` without opening a
//! window or starting a daemon, and without `--daemon-url` it reaches the
//! running app's daemon through `<data>/daemon.lock`.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Starts `<app> mcp <args>` on `data`, sends `initialize` and `requests`
/// (ids from 2), and returns the answers by id once all have come.
fn session(data: &Path, args: &[&str], requests: &[Value]) -> HashMap<u64, Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_od-desktop"))
        .arg("mcp")
        .args(args)
        .env("OD_DATA_DIR", data)
        .env_remove("OD_DAEMON_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut msgs = vec![
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    for (id, req) in (2u64..).zip(requests) {
        let mut req = req.clone();
        req["jsonrpc"] = json!("2.0");
        req["id"] = json!(id);
        msgs.push(req);
    }
    for msg in &msgs {
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
    let mut replies = HashMap::new();
    while replies.len() < 1 + requests.len() {
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

    // Closing stdin ends the server.
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "still running after stdin closed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    replies
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

#[test]
fn mcp_answers_initialize_and_tools_list_over_stdio() {
    let data = tempfile::tempdir().unwrap();
    let replies = session(
        data.path(),
        &["--daemon-url", "http://127.0.0.1:9"],
        &[json!({"method": "tools/list", "params": {}})],
    );
    assert_eq!(replies[&1]["result"]["serverInfo"]["name"], "contour");
    let tools = replies[&2]["result"]["tools"].as_array().unwrap();
    assert!(
        tools.iter().any(|t| t["name"] == "list_projects"),
        "{tools:?}"
    );
}

/// No `--daemon-url` (what Install registers in the app): the URL in
/// `daemon.lock` is the running app's daemon.
#[test]
fn without_a_flag_mcp_reaches_the_daemon_in_daemon_lock() {
    let data = tempfile::tempdir().unwrap();
    let url = stub_daemon(json!({"projects": [{"id": "p-from-the-lock", "name": "Lock"}]}));
    std::fs::write(data.path().join("daemon.lock"), &url).unwrap();
    let replies = session(
        data.path(),
        &[],
        &[json!({"method": "tools/call", "params": {"name": "list_projects", "arguments": {}}})],
    );
    let result = &replies[&2]["result"];
    assert_ne!(result["isError"], true, "{result}");
    assert!(result.to_string().contains("p-from-the-lock"), "{result}");
}
