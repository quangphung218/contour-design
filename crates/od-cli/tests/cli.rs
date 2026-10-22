//! End to end: start the real `od` binary as a daemon, drive it with the
//! client subcommands.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use serde_json::Value;
use std::net::TcpListener;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const OD: &str = env!("CARGO_BIN_EXE_od");

struct Daemon {
    child: Child,
    url: String,
    _tmp: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `bare`: run `od` with no subcommand (port from `OD_PORT`).
fn start(bare: bool) -> Daemon {
    let tmp = tempfile::tempdir().unwrap();
    let resources = tmp.path().join("resources");
    std::fs::create_dir_all(resources.join("skills/demo")).unwrap();
    std::fs::write(
        resources.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo skill.\n---\nBody",
    )
    .unwrap();
    let port = free_port();
    let mut cmd = Command::new(OD);
    if !bare {
        cmd.args(["daemon", "start", "--port", &port.to_string()]);
    }
    let child = cmd
        .env("OD_PORT", port.to_string())
        .env("OD_DATA_DIR", tmp.path().join("data"))
        .env("OD_RESOURCE_ROOT", &resources)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let daemon = Daemon {
        child,
        url: format!("http://127.0.0.1:{port}"),
        _tmp: tmp,
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if od(&daemon, &["daemon", "status"]).status.success() {
            return daemon;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("daemon did not answer on {}", daemon.url);
}

fn od(d: &Daemon, args: &[&str]) -> Output {
    Command::new(OD)
        .arg("--daemon-url")
        .arg(&d.url)
        .args(args)
        .output()
        .unwrap()
}

fn od_json(d: &Daemon, args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    let out = od(d, &all);
    assert!(
        out.status.success(),
        "od {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn project_and_files_commands_round_trip() {
    let d = start(false);
    let created = od_json(
        &d,
        &["project", "create", "--name", "CLI Demo", "--kind", "deck"],
    );
    let id = created["project"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["project"]["metadata"]["kind"], "deck");

    let list = od_json(&d, &["project", "list"]);
    assert_eq!(list["projects"][0]["id"], id.as_str());
    let info = od_json(&d, &["project", "info", &id]);
    assert_eq!(info["project"]["name"], "CLI Demo");

    let wrote = od_json(
        &d,
        &[
            "files",
            "write",
            &id,
            "deck/index.html",
            "--content",
            "<h1>Deck</h1>",
        ],
    );
    assert_eq!(wrote["file"]["name"], "deck/index.html");
    let files = od_json(&d, &["files", "list", &id]);
    assert_eq!(files["files"][0]["name"], "deck/index.html");
    let read = od(&d, &["files", "read", &id, "deck/index.html"]);
    assert_eq!(read.stdout, b"<h1>Deck</h1>");

    // Binary content survives (base64 on the wire).
    let bin = d._tmp.path().join("blob.bin");
    std::fs::write(&bin, [0u8, 159, 146, 150, 255]).unwrap();
    od_json(
        &d,
        &[
            "files",
            "write",
            &id,
            "blob.bin",
            "--from",
            bin.to_str().unwrap(),
        ],
    );
    let out_path = d._tmp.path().join("copy.bin");
    let read = od(
        &d,
        &[
            "files",
            "read",
            &id,
            "blob.bin",
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(read.status.success());
    assert_eq!(std::fs::read(&out_path).unwrap(), [0u8, 159, 146, 150, 255]);

    let dup = od_json(&d, &["project", "duplicate", &id, "--name", "Copy"]);
    assert_eq!(dup["project"]["name"], "Copy");
    assert_eq!(dup["copiedFiles"].as_array().unwrap().len(), 2);

    od_json(&d, &["files", "delete", &id, "blob.bin"]);
    let missing = od(&d, &["files", "read", &id, "blob.bin"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("404"));

    let traversal = od(&d, &["files", "read", &id, "../../app.sqlite"]);
    assert!(!traversal.status.success());

    od_json(&d, &["project", "delete", &id]);
    let gone = od(&d, &["project", "info", &id]);
    assert!(String::from_utf8_lossy(&gone.stderr).contains("PROJECT_NOT_FOUND"));

    // Human output.
    let plain = od(&d, &["project", "list"]);
    assert!(String::from_utf8_lossy(&plain.stdout).contains("Copy"));
}

#[test]
fn bare_od_starts_the_daemon() {
    let d = start(true);
    let health = od_json(&d, &["daemon", "status"]);
    assert_eq!(health["ok"], true);
}

#[test]
fn unreachable_daemon_is_a_clear_error() {
    let port = free_port();
    let out = Command::new(OD)
        .args([
            "--daemon-url",
            &format!("http://127.0.0.1:{port}"),
            "project",
            "list",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot reach the daemon"));
}

#[test]
fn non_loopback_bind_requires_token() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(OD)
        .args([
            "daemon",
            "start",
            "--host",
            "0.0.0.0",
            "--port",
            &free_port().to_string(),
        ])
        .env("OD_DATA_DIR", tmp.path())
        .env_remove("OD_API_TOKEN")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("OD_API_TOKEN"));
}
