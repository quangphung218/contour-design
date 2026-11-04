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
    daemon_start_from(Path::new(APP), data, &[])
}

/// `<exe> <global flags> daemon start --port 0` on `data`, with none of
/// the `od` env.
fn daemon_start_from(exe: &Path, data: &Path, global: &[&str]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(global)
        .args(["daemon", "start", "--port", "0"])
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
    // `od`'s global flags may come before `daemon` (it was the window).
    let mut daemon = Daemon(
        daemon_start_from(Path::new(APP), &data, &["--json"])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
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
        daemon_start_from(&exe, &data, &[])
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

/// The agents are found as the app's window finds them: on the login
/// shell's `PATH`, not only the terminal's (a QA agent's `PATH` is often
/// cut down). Unix: Windows reads the registry instead of a shell.
#[cfg(unix)]
#[test]
fn daemon_start_finds_the_agents_on_the_login_shells_path() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let agents = tmp.path().join("login-bin");
    std::fs::create_dir_all(&agents).unwrap();
    let script = |path: &Path, body: &str| {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(&agents.join("claude"), "echo 9.9.9");
    // `<shell> -ilc 'printf …'`: answers with a PATH holding `claude`.
    let shell = tmp.path().join("login-shell");
    script(
        &shell,
        &format!("printf '__OD_PATH__%s__OD_PATH__' '{}'", agents.display()),
    );
    // A new script's first exec can take seconds on a loaded Mac (it is
    // scanned), past the daemon's 3 s login shell limit; on Linux it can
    // fail with ETXTBSY while a fork still holds it. Run both once first,
    // as `od-runtime`'s login_path tests do.
    warm_up(&shell);
    warm_up(&agents.join("claude"));
    let data = tmp.path().join("data");
    let mut daemon = Daemon(
        daemon_start(&data)
            .env("SHELL", &shell)
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", tmp.path())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let url = lock_url(&data, &mut daemon.0);
    let (status, body) = get(&url, "/api/agents");
    assert!(status.contains(" 200 "), "{status}");
    let res: serde_json::Value = serde_json::from_str(&body).unwrap();
    let claude = res["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "claude")
        .unwrap();
    assert_eq!(
        claude["path"].as_str(),
        Some(agents.join("claude").to_str().unwrap()),
        "{claude}"
    );
}

/// Runs `script` once until it starts (see its caller).
#[cfg(unix)]
fn warm_up(script: &Path) {
    for _ in 0..100 {
        let started = Command::new(script)
            .args(["-ilc", "true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if started.is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
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

/// Waits for `child` to exit on its own: the window's path would not.
fn exits_within(child: &mut Daemon) -> std::process::ExitStatus {
    let deadline = Instant::now() + WITHIN;
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "still running: the window opened"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `<exe> <args>` on `data` with piped stdout and stderr.
fn client(data: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(APP);
    cmd.args(args)
        .env("OD_DATA_DIR", data)
        .env("RUST_LOG", "warn")
        .env_remove("OD_DAEMON_URL")
        .env_remove("OD_API_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn read_all(pipe: Option<impl Read>) -> String {
    let mut text = String::new();
    pipe.unwrap().read_to_string(&mut text).unwrap();
    text
}

/// A client command after `od`'s global flags runs the CLI and exits
/// (it opened the window).
#[test]
fn a_client_command_after_the_global_flags_runs_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let mut daemon = Daemon(daemon_start(&data).stderr(Stdio::null()).spawn().unwrap());
    let url = lock_url(&data, &mut daemon.0);

    let mut cli = Daemon(
        client(&data, &["--daemon-url", &url, "--json", "project", "list"])
            .spawn()
            .unwrap(),
    );
    let status = exits_within(&mut cli);
    let stdout = read_all(cli.0.stdout.take());
    let stderr = read_all(cli.0.stderr.take());
    assert!(status.success(), "{status}: {stderr}");
    let list: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout}"));
    assert!(list.is_array() || list.is_object(), "{list}");
    assert!(!data.join("logs").exists(), "the app's window path ran");
}

/// `<app> --json mcp` is the MCP server too: it answers `initialize` and
/// exits when stdin closes (it opened the window).
#[test]
fn mcp_after_the_global_flags_runs_the_server() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let mut daemon = Daemon(daemon_start(&data).stderr(Stdio::null()).spawn().unwrap());
    lock_url(&data, &mut daemon.0);

    let mut mcp = Daemon(
        client(&data, &["--json", "mcp"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stdin = mcp.0.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"test","version":"1"}}}}}}"#
    )
    .unwrap();
    drop(stdin);
    let status = exits_within(&mut mcp);
    let stdout = read_all(mcp.0.stdout.take());
    let stderr = read_all(mcp.0.stderr.take());
    assert!(status.success(), "{status}: {stderr}");
    let line = stdout.lines().next().unwrap_or_default();
    let reply: serde_json::Value = serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("stdout is not JSON-RPC ({e}): {stdout}"));
    assert_eq!(reply["id"], 1, "{reply}");
    assert!(reply["result"]["serverInfo"].is_object(), "{reply}");
    assert!(!data.join("logs").exists(), "the app's window path ran");
}
