//! Client commands (`od media models`, …) take a URL someone gave (flag or
//! `OD_DAEMON_URL`) as is: only the default gives way to `daemon.lock`.
//! The fallback test skips itself when a daemon answers on the default
//! port, as the running app may on the machine running the tests.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

const OD: &str = env!("CARGO_BIN_EXE_od");

/// A loopback URL with nothing listening.
fn dead_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

fn media_models(data: &Path, args: &[&str], env: Option<&str>) -> Output {
    let mut cmd = Command::new(OD);
    cmd.args(args)
        .args(["media", "models"])
        .env("OD_DATA_DIR", data)
        .env_remove("OD_DAEMON_URL");
    if let Some(url) = env {
        cmd.env("OD_DAEMON_URL", url);
    }
    cmd.output().unwrap()
}

#[test]
fn a_given_url_is_not_replaced_by_daemon_lock() {
    let data = tempfile::tempdir().unwrap();
    // The lock names another dead daemon: reaching it would fail with its
    // URL instead of the given one.
    let lock = dead_url();
    std::fs::write(data.path().join("daemon.lock"), &lock).unwrap();
    let given = dead_url();
    for (args, env) in [
        (vec!["--daemon-url", given.as_str()], None),
        (vec![], Some(given.as_str())),
    ] {
        let out = media_models(data.path(), &args, env);
        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("cannot reach the daemon at {given}")),
            "{stderr}"
        );
        assert!(!stderr.contains(&lock), "{stderr}");
    }
}

/// A daemon that answers every request (`/api/health` included) with
/// `body` (JSON).
fn stub_daemon(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut conn in listener.incoming().map_while(Result::ok) {
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && conn.read(&mut byte).is_ok_and(|n| n == 1) {
                head.push(byte[0]);
            }
            let _ = write!(
                conn,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// Whether a daemon answers `/api/health` on the default URL.
fn default_daemon_answers() -> bool {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap()
        .get("http://127.0.0.1:7456/api/health")
        .send()
        .is_ok_and(|res| res.status().is_success())
}

#[test]
fn without_a_url_a_client_command_reaches_the_app_in_daemon_lock() {
    if default_daemon_answers() {
        eprintln!("a daemon answers on 127.0.0.1:7456: skipping the daemon.lock fallback test");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let lock = stub_daemon(r#"{"providers":[{"id":"from-the-lock","configured":true}]}"#);
    std::fs::write(data.path().join("daemon.lock"), format!("{lock}\n")).unwrap();
    let out = media_models(data.path(), &[], None);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stdout.contains("provider\tfrom-the-lock\tkey set"),
        "{stdout}"
    );
}
