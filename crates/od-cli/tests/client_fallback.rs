//! Client commands (`od media models`, …) take a URL someone gave (flag or
//! `OD_DAEMON_URL`) as is: only the default gives way to `daemon.lock`
//! (the fallback itself is unit-tested in `lib.rs`, since the default port
//! may be taken on the machine running the tests).

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};

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
