//! `<app> daemon start`: the installed app's binary runs the `od` daemon
//! headless (no Tauri, no window), on its own data dir, beside the app in
//! use. Runs the real `od-desktop` binary: on a CI runner without a display
//! a window would fail to open, so this also shows none is built.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const APP: &str = env!("CARGO_BIN_EXE_od-desktop");
const WITHIN: Duration = Duration::from_secs(30);

/// Kills the daemon when a test fails before stopping it.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon_start(data: &Path) -> Command {
    daemon_start_from(Path::new(APP), data)
}

/// `<exe> daemon start --port 0` on `data`, with none of the `od` env.
fn daemon_start_from(exe: &Path, data: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(["daemon", "start", "--port", "0"])
        .env("OD_DATA_DIR", data)
        .env("RUST_LOG", "warn")
        .env_remove("OD_PORT")
        .env_remove("OD_HOST")
        .env_remove("OD_RESOURCE_ROOT")
        .env_remove("OD_LOG_FILE")
        .env_remove("OD_API_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    cmd
}

/// The URL the daemon recorded in `<data>/daemon.lock`.
fn lock_url(data: &Path, child: &mut Child) -> String {
    let deadline = Instant::now() + WITHIN;
    while Instant::now() < deadline {
        if let Ok(url) = std::fs::read_to_string(data.join("daemon.lock")) {
            let url = url.trim();
            if url.starts_with("http://") {
                return url.to_owned();
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the daemon exited first: {status}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no URL in {}", data.join("daemon.lock").display());
}

/// `GET <url><path>`: status line and body.
fn get(url: &str, path: &str) -> (String, String) {
    let host = url.trim_start_matches("http://").trim_end_matches('/');
    let mut stream = TcpStream::connect(host).unwrap();
    stream.set_read_timeout(Some(WITHIN)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (head.lines().next().unwrap().to_owned(), body.to_owned())
}

/// Ends the daemon the way `kill` or Ctrl-C does where the platform has a
/// signal for it (Windows: `TerminateProcess`, as Task Manager).
fn stop(daemon: &mut Daemon) -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        let sent = Command::new("kill")
            .args(["-TERM", &daemon.0.id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
    }
    #[cfg(not(unix))]
    daemon.0.kill().unwrap();
    let deadline = Instant::now() + WITHIN;
    loop {
        if let Some(status) = daemon.0.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "the daemon did not stop");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn daemon_start_runs_headless_on_its_own_data_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let mut daemon = Daemon(daemon_start(&data).stderr(Stdio::null()).spawn().unwrap());
    let url = lock_url(&data, &mut daemon.0);
    assert_ne!(
        url, "http://127.0.0.1:7456",
        "--port 0 binds a port of its own"
    );

    let (status, body) = get(&url, "/api/health");
    assert!(status.contains(" 200 "), "{status}: {body}");
    let health: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(health["ok"], true);
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
    // The bundled skills are served (the source tree's, in a test build).
    let (status, body) = get(&url, "/api/skills");
    assert!(status.contains(" 200 "), "{status}");
    assert!(body.contains("\"id\""), "no skills: {body}");

    // The window's path logs to `<data>/logs/`; `od daemon start` only
    // with --log-file.
    assert!(!data.join("logs").exists(), "the app's window path ran");

    let status = stop(&mut daemon);
    #[cfg(unix)]
    assert!(status.success(), "SIGTERM ends it cleanly: {status}");
    #[cfg(not(unix))]
    let _ = status;
    // The data dir is free again.
    od_server::DaemonLock::acquire(&data).unwrap();
}

/// The binary inside an installed app's layout serves the app's bundled
/// content, which `od`'s own lookup (next to the binary) misses on macOS
/// and in the Linux packages.
#[test]
fn daemon_start_serves_the_content_of_the_installed_app() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let (exe, resources) = if cfg!(target_os = "macos") {
        let contents = tmp.path().join("Contour.app/Contents");
        (
            contents.join("MacOS/od-desktop"),
            contents.join("Resources"),
        )
    } else if cfg!(windows) {
        (tmp.path().join("od-desktop.exe"), tmp.path().to_path_buf())
    } else {
        let usr = tmp.path().join("usr");
        (usr.join("bin/od-desktop"), usr.join("lib/Contour"))
    };
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    // A link: the debug binary is large. Same volume (the target dir).
    if std::fs::hard_link(APP, &exe).is_err() {
        std::fs::copy(APP, &exe).unwrap();
    }
    let skill = resources.join("content/skills/bundle-only");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: bundle-only\ndescription: Only in the bundle.\n---\nBody",
    )
    .unwrap();
    let data = tmp.path().join("data");
    // Not the source tree's `content/` through the working directory.
    let mut daemon = Daemon(
        daemon_start_from(&exe, &data)
            .current_dir(tmp.path())
            .env_remove("APPDIR")
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let url = lock_url(&data, &mut daemon.0);
    let (status, body) = get(&url, "/api/skills");
    assert!(status.contains(" 200 "), "{status}");
    assert!(
        body.contains("bundle-only"),
        "not the bundle's skills: {body}"
    );
}

/// A data dir another daemon holds (the open app's, without `OD_DATA_DIR`)
/// is refused with the holder's URL; nothing opens instead.
#[test]
fn daemon_start_refuses_a_data_dir_in_use() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let mut first = Daemon(daemon_start(&data).stderr(Stdio::null()).spawn().unwrap());
    let url = lock_url(&data, &mut first.0);

    // The window's path would show a dialog and wait: give it a deadline.
    let mut second = Daemon(daemon_start(&data).stderr(Stdio::piped()).spawn().unwrap());
    let deadline = Instant::now() + WITHIN;
    let status = loop {
        if let Some(status) = second.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the second daemon did not exit");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stderr = String::new();
    second
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("already running"), "{stderr}");
    assert!(stderr.contains(&url), "{stderr}");
}
