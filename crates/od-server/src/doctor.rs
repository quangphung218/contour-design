//! `od doctor` and Settings ▸ About ▸ Check setup: one line per condition
//! the main flow needs, each with how to fix it in one sentence.
//!
//! Every check takes what it looks at as an argument (a folder, a `PATH`,
//! a renderer, a free-space figure), so tests run it on a fake machine.
//! [`run_local`] gathers the real inputs. The CLI asks the daemon it found
//! to run them (`POST /api/doctor`, the daemon's own data folder) and runs
//! them itself only when no daemon answers; either way its own daemon check
//! ([`daemon_check`]) comes first. The daemon's route says it answers.
//!
//! Nothing is uploaded and nothing secret is printed: paths, versions and
//! sizes only. The macOS folder permission is only reported, never asked:
//! reading a protected folder would show the system dialog.

use od_export::{CaptureOptions, ImageFormat, RenderError, RenderTarget, Renderer};
use od_runtime::def::{RuntimeDef, StreamFormat};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long an agent CLI has to answer `--version`.
pub const AGENT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long Chrome has to start and show a blank page.
pub const CHROME_TIMEOUT: Duration = Duration::from_secs(60);
/// How long ffmpeg has to answer `-version`.
pub const FFMPEG_TIMEOUT: Duration = Duration::from_secs(10);
/// Free space below which the disk check fails.
pub const MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Works.
    Ok,
    /// Works, but something may get in the way (optional tool missing,
    /// a permission macOS may ask for). Does not fail the run.
    Warn,
    /// The main flow breaks here: `od doctor` exits 1.
    Fail,
}

impl Status {
    /// The mark printed before the line.
    #[must_use]
    pub fn mark(self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Warn => "!",
            Self::Fail => "✗",
        }
    }
}

/// One line of the report. `od doctor` reads it back from the daemon's
/// `POST /api/doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// Stable name: `daemon`, `data-dir`, `agent`, `chrome`, `ffmpeg`,
    /// `folder-access`, `disk`.
    pub id: Cow<'static, str>,
    pub status: Status,
    /// What was found, in one sentence.
    pub summary: String,
    /// How to fix it, in one sentence; `None` when nothing needs fixing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Check {
    fn ok(id: &'static str, summary: impl Into<String>) -> Self {
        Self {
            id: Cow::Borrowed(id),
            status: Status::Ok,
            summary: summary.into(),
            fix: None,
        }
    }

    fn warn(id: &'static str, summary: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            id: Cow::Borrowed(id),
            status: Status::Warn,
            summary: summary.into(),
            fix: Some(fix.into()),
        }
    }

    fn fail(id: &'static str, summary: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            id: Cow::Borrowed(id),
            status: Status::Fail,
            summary: summary.into(),
            fix: Some(fix.into()),
        }
    }
}

/// The whole report, as `od doctor --json` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Version of the program that ran the checks.
    pub version: String,
    pub platform: String,
    /// `false` when any check failed.
    pub ok: bool,
    pub checks: Vec<Check>,
}

impl Report {
    #[must_use]
    pub fn new(checks: Vec<Check>) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            ok: checks.iter().all(|c| c.status != Status::Fail),
            checks,
        }
    }

    /// One line per check, `✓ summary`, with `  fix: …` under a line that
    /// needs it.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.checks {
            out.push_str(c.status.mark());
            out.push(' ');
            out.push_str(&c.summary);
            out.push('\n');
            if let Some(fix) = &c.fix {
                out.push_str("  fix: ");
                out.push_str(fix);
                out.push('\n');
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Daemon

/// The daemon at `url` answered `version` (`None`: it did not answer).
/// `url` is `None` when no daemon recorded one.
#[must_use]
pub fn daemon_check(url: Option<&str>, answered: Option<&str>, expected: &str) -> Check {
    const ID: &str = "daemon";
    let Some(url) = url else {
        return Check::fail(
            ID,
            "No Contour daemon is running for this data folder.",
            "Open Contour, or run `od daemon start`.",
        );
    };
    match answered {
        None => Check::fail(
            ID,
            format!("The daemon recorded at {url} does not answer."),
            "Quit Contour and open it again, or run `od daemon start`.",
        ),
        Some(v) if v != expected => Check::fail(
            ID,
            format!("The daemon at {url} is version {v}, this od is {expected}."),
            "Quit Contour and open it again so the app and od are the same version.",
        ),
        Some(v) => Check::ok(ID, format!("Daemon {v} answers at {url}.")),
    }
}

// ---------------------------------------------------------------------------
// Data folder

/// Writes and removes a file in `root` (created if missing).
#[must_use]
pub fn data_dir_check(root: &Path) -> Check {
    const ID: &str = "data-dir";
    let probe = root.join(format!(".od-doctor-{}", std::process::id()));
    let written = std::fs::create_dir_all(root)
        .and_then(|()| std::fs::write(&probe, b"ok"))
        .and_then(|()| std::fs::remove_file(&probe));
    match written {
        Ok(()) => Check::ok(ID, format!("Data folder {} is writable.", root.display())),
        Err(e) => Check::fail(
            ID,
            format!("Cannot write to the data folder {}: {e}.", root.display()),
            "Give your user write access to that folder, or set OD_DATA_DIR to a folder you own.",
        ),
    }
}

// ---------------------------------------------------------------------------
// Agent CLI

/// The agent the user chose (`agentId` in app config) or, with none,
/// every agent with a CLI: found on `path` (or at `custom_bin`), and its
/// `--version` answers within `timeout`. A chosen id that is no agent fails.
pub async fn agent_check(
    configured: Option<&str>,
    path: &OsStr,
    custom_bin: impl Fn(&str) -> Option<PathBuf>,
    timeout: Duration,
) -> Check {
    const ID: &str = "agent";
    let chosen = configured.filter(|id| !id.is_empty() && *id != "auto");
    if let Some(id) = chosen {
        // An id no build knows (another build's agent, a typo): the runs
        // would refuse it, so no other agent may stand in for it (QA-118).
        let Some(def) = od_runtime::def::find(id) else {
            return Check::fail(
                ID,
                format!("The chosen agent `{id}` is not one this Contour knows."),
                "Pick an agent in Settings ▸ Agents.",
            );
        };
        if def.stream_format == StreamFormat::Byok {
            return Check::ok(ID, format!("{} uses an API key; no CLI needed.", def.name));
        }
        let bin = custom_bin(def.id).or_else(|| od_runtime::detect::resolve_bin(def, path));
        return match bin {
            None => Check::fail(
                ID,
                format!("{} (`{}`) is not on your login PATH.", def.name, def.bin),
                format!(
                    "Install {} and open a new terminal, or set its path in Settings ▸ Agents.",
                    def.name
                ),
            ),
            Some(bin) => version_check(def, &bin, path, timeout).await,
        };
    }
    let found: Vec<(&RuntimeDef, PathBuf)> = od_runtime::def::ALL
        .into_iter()
        .filter(|d| d.stream_format != StreamFormat::Byok)
        .filter_map(|d| {
            custom_bin(d.id)
                .or_else(|| od_runtime::detect::resolve_bin(d, path))
                .map(|b| (d, b))
        })
        .collect();
    if found.is_empty() {
        return Check::fail(
            ID,
            "No agent CLI is on your login PATH.",
            "Install Claude Code, Codex or another supported agent, then press Rescan in Settings ▸ Agents.",
        );
    }
    let checks = futures_util::future::join_all(
        found
            .iter()
            .map(|(def, bin)| version_check(def, bin, path, timeout)),
    )
    .await;
    // Any agent that answers is enough; else the first one's reason.
    checks
        .iter()
        .find(|c| c.status == Status::Ok)
        .or_else(|| checks.first())
        .cloned()
        .unwrap_or_else(|| Check::fail(ID, "No agent CLI answered.", "Reinstall your agent CLI."))
}

async fn version_check(def: &RuntimeDef, bin: &Path, path: &OsStr, timeout: Duration) -> Check {
    match od_runtime::detect::version_line(def, bin, path, timeout).await {
        Some(v) => Check::ok("agent", format!("{} {v} at {}.", def.name, bin.display())),
        None => Check::fail(
            "agent",
            format!(
                "{} at {} did not answer `--version` within {} s.",
                def.name,
                bin.display(),
                timeout.as_secs()
            ),
            format!(
                "Run `{} --version` in a terminal to see why; reinstall it or sign in again.",
                bin.display()
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// Chrome

/// `found` (from [`od_render_cdp::find_chrome`]) names a browser, and
/// `renderer` shows a blank page with it within `timeout`.
pub async fn chrome_check(
    found: Result<PathBuf, RenderError>,
    renderer: Arc<dyn Renderer>,
    timeout: Duration,
) -> Check {
    const ID: &str = "chrome";
    const FIX: &str =
        "Install Google Chrome, Edge or Chromium, or set OD_CHROME_PATH to its program.";
    let exe = match found {
        Ok(exe) => exe,
        Err(e) => {
            return Check::fail(
                ID,
                format!("No browser for PDF, image and PPTX export: {e}."),
                FIX,
            );
        }
    };
    let page = match blank_page() {
        Ok(page) => page,
        Err(e) => {
            return Check::fail(
                ID,
                format!("Cannot write a test page to the temp folder: {e}."),
                "Free some disk space or fix the permissions of the temp folder.",
            );
        }
    };
    let target = RenderTarget::new(page.path().join("blank.html"));
    let opts = CaptureOptions {
        format: ImageFormat::Png,
        full_page: false,
    };
    match tokio::time::timeout(timeout, renderer.capture_page(&target, opts)).await {
        Ok(Ok(_)) => Check::ok(
            ID,
            format!("{} opens a blank page for export.", exe.display()),
        ),
        Ok(Err(e)) => Check::fail(
            ID,
            format!("{} could not show a blank page: {e}.", exe.display()),
            "Open the browser once by hand (macOS may need to approve it), or point OD_CHROME_PATH to another one.",
        ),
        Err(_) => Check::fail(
            ID,
            format!(
                "{} did not show a blank page within {} s.",
                exe.display(),
                timeout.as_secs()
            ),
            "Open the browser once by hand (macOS may need to approve it), or point OD_CHROME_PATH to another one.",
        ),
    }
}

fn blank_page() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::Builder::new().prefix("od-doctor-").tempdir()?;
    std::fs::write(
        dir.path().join("blank.html"),
        "<!doctype html><title>od doctor</title>",
    )?;
    Ok(dir)
}

// ---------------------------------------------------------------------------
// ffmpeg

/// `found` (from [`od_render_cdp::ffmpeg::find_ffmpeg`]) names ffmpeg and
/// it answers `-version` within `timeout`. Only MP4 needs it: a missing
/// ffmpeg is a warning.
pub async fn ffmpeg_check(found: Result<PathBuf, RenderError>, timeout: Duration) -> Check {
    const ID: &str = "ffmpeg";
    let exe = match found {
        Ok(exe) => exe,
        Err(e) => {
            return Check::warn(
                ID,
                format!("No ffmpeg, so no MP4 export: {e}."),
                "Install ffmpeg (macOS: `brew install ffmpeg`) or set OD_FFMPEG_PATH.",
            );
        }
    };
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("-version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = match cmd.spawn() {
        Ok(child) => tokio::time::timeout(timeout, child.wait_with_output()).await,
        Err(e) => Ok(Err(e)),
    };
    match out {
        Ok(Ok(out)) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let line = text.lines().next().unwrap_or("ffmpeg");
            // "ffmpeg version 7.1 Copyright (c) …": the version is enough.
            let line = line.split(" Copyright").next().unwrap_or(line).trim();
            Check::ok(ID, format!("{line} at {}.", exe.display()))
        }
        Ok(Ok(out)) => Check::warn(
            ID,
            format!("{} -version exited with {}.", exe.display(), out.status),
            "Reinstall ffmpeg or point OD_FFMPEG_PATH to one that runs.",
        ),
        Ok(Err(e)) => Check::warn(
            ID,
            format!("Cannot run {}: {e}.", exe.display()),
            "Reinstall ffmpeg or point OD_FFMPEG_PATH to one that runs.",
        ),
        Err(_) => Check::warn(
            ID,
            format!(
                "{} did not answer -version within {} s.",
                exe.display(),
                timeout.as_secs()
            ),
            "Reinstall ffmpeg or point OD_FFMPEG_PATH to one that runs.",
        ),
    }
}

// ---------------------------------------------------------------------------
// macOS folder access

/// Folders macOS asks the user about before an app reads them.
const PROTECTED: [&str; 3] = ["Documents", "Desktop", "Downloads"];

/// Which of `folders` (the data folder, project locations) sit in a
/// folder macOS protects under `home`. Reports only: it never reads them,
/// since reading one would show the permission dialog.
#[must_use]
pub fn folder_access_check(home: Option<&Path>, folders: &[PathBuf]) -> Check {
    const ID: &str = "folder-access";
    let protected: Vec<&PathBuf> = match home {
        Some(home) => folders
            .iter()
            .filter(|f| PROTECTED.iter().any(|p| f.starts_with(home.join(p))))
            .collect(),
        None => Vec::new(),
    };
    if protected.is_empty() {
        return Check::ok(
            ID,
            "No project folder is in Documents, Desktop or Downloads; macOS asks nothing.",
        );
    }
    let list: Vec<String> = protected.iter().map(|p| p.display().to_string()).collect();
    Check::warn(
        ID,
        format!("macOS asks Contour before it opens {}.", list.join(", ")),
        "If projects there do not open, allow Contour in System Settings ▸ Privacy & Security ▸ Files and Folders.",
    )
}

// ---------------------------------------------------------------------------
// Disk

/// `free` bytes on the disk of `root` (`Err`: could not tell).
#[must_use]
pub fn disk_check(root: &Path, free: std::io::Result<u64>, min: u64) -> Check {
    const ID: &str = "disk";
    let mb = |b: u64| b / (1024 * 1024);
    match free {
        Ok(free) if free < min => Check::fail(
            ID,
            format!(
                "Only {} MB free on the disk of {} (needs {} MB).",
                mb(free),
                root.display(),
                mb(min)
            ),
            "Free some disk space: agents, exports and file history write there.",
        ),
        Ok(free) => Check::ok(
            ID,
            format!("{} MB free on the disk of {}.", mb(free), root.display()),
        ),
        Err(e) => Check::warn(
            ID,
            format!("Cannot tell the free space of {}: {e}.", root.display()),
            "Check the disk in Finder or Explorer; make sure it has at least 1 GB free.",
        ),
    }
}

// ---------------------------------------------------------------------------
// All of it

/// What [`run_local`] looks at.
pub struct Inputs {
    /// The data folder (`OD_DATA_DIR` or the platform's).
    pub data: od_store::DataDir,
    /// `PATH` agents are found on: the login shell's, as the app uses.
    pub agent_path: std::ffi::OsString,
    /// Renders the blank page.
    pub renderer: Arc<dyn Renderer>,
}

impl Inputs {
    /// This machine's: the data folder from the environment, the login
    /// shell's `PATH`, the Chrome renderer exports use.
    pub async fn from_env() -> Self {
        Self {
            data: od_store::DataDir::from_env(),
            agent_path: od_runtime::login_path::agent_path().await,
            renderer: Arc::new(od_render_cdp::CdpRenderer::new()),
        }
    }
}

/// Every check but the daemon's, in the order the main flow needs them.
pub async fn run_local(inputs: Inputs) -> Vec<Check> {
    let root = inputs.data.root().to_path_buf();
    let mut checks = vec![data_dir_check(&root)];
    let configured = inputs
        .data
        .app_setting("agentId")
        .and_then(|v| v.as_str().map(str::to_owned));
    checks.push(
        agent_check(
            configured.as_deref(),
            &inputs.agent_path,
            |id| crate::routes::manage::settings::custom_bin(&root, id),
            AGENT_TIMEOUT,
        )
        .await,
    );
    checks.push(
        chrome_check(
            od_render_cdp::find_chrome(),
            inputs.renderer,
            CHROME_TIMEOUT,
        )
        .await,
    );
    checks.push(ffmpeg_check(od_render_cdp::ffmpeg::find_ffmpeg(), FFMPEG_TIMEOUT).await);
    if cfg!(target_os = "macos") {
        let mut folders = vec![root.clone()];
        folders.extend(project_locations(&inputs.data));
        checks.push(folder_access_check(
            od_runtime::login_path::home_dir().as_deref(),
            &folders,
        ));
    }
    checks.push(disk_check(
        &root,
        fs4::available_space(&root),
        MIN_FREE_BYTES,
    ));
    checks
}

/// The folders of the project locations the user added (app config).
fn project_locations(data: &od_store::DataDir) -> Vec<PathBuf> {
    data.app_setting("projectLocations")
        .and_then(|v| {
            serde_json::from_value::<Vec<crate::routes::manage::locations::Location>>(v).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|l| PathBuf::from(l.path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::BoxFuture;
    use od_export::{DeckMode, FrameOptions, RenderResult, Slides};

    #[test]
    fn the_daemon_must_answer_with_this_version() {
        let none = daemon_check(None, None, "1.0.0");
        assert_eq!(none.status, Status::Fail);
        let silent = daemon_check(Some("http://127.0.0.1:9"), None, "1.0.0");
        assert_eq!(silent.status, Status::Fail);
        assert!(silent.summary.contains("127.0.0.1:9"), "{}", silent.summary);
        let old = daemon_check(Some("http://x"), Some("0.9.0"), "1.0.0");
        assert_eq!(old.status, Status::Fail);
        assert!(old.summary.contains("0.9.0") && old.summary.contains("1.0.0"));
        let same = daemon_check(Some("http://x"), Some("1.0.0"), "1.0.0");
        assert_eq!(same.status, Status::Ok);
        assert_eq!(same.fix, None);
    }

    #[test]
    fn the_data_folder_must_take_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = data_dir_check(&tmp.path().join("data"));
        assert_eq!(ok.status, Status::Ok, "{ok:?}");
        // Nothing left behind.
        assert_eq!(
            std::fs::read_dir(tmp.path().join("data")).unwrap().count(),
            0
        );
        // A file where the folder should be: no folder can be made there.
        let file = tmp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let bad = data_dir_check(&file.join("data"));
        assert_eq!(bad.status, Status::Fail, "{bad:?}");
        assert!(bad.fix.unwrap().contains("OD_DATA_DIR"));
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_data_folder_fails() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("data");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let check = data_dir_check(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(check.status, Status::Fail, "{check:?}");
    }

    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    fn path_of(dir: &Path) -> std::ffi::OsString {
        std::env::join_paths([dir, Path::new("/usr/bin"), Path::new("/bin")]).unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_chosen_agent_must_be_found_and_answer_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = path_of(tmp.path());
        let none = |_: &str| None;
        let missing = agent_check(Some("claude"), &path, none, AGENT_TIMEOUT).await;
        assert_eq!(missing.status, Status::Fail, "{missing:?}");
        assert!(missing.summary.contains("`claude`"), "{}", missing.summary);

        script(tmp.path(), "claude", "echo '2.1.0 (Claude Code)'");
        let ok = agent_check(Some("claude"), &path, none, AGENT_TIMEOUT).await;
        assert_eq!(ok.status, Status::Ok, "{ok:?}");
        assert!(ok.summary.contains("2.1.0"), "{}", ok.summary);

        // A CLI that hangs fails at the limit, not later.
        script(tmp.path(), "claude", "sleep 30");
        let started = std::time::Instant::now();
        let hung = agent_check(Some("claude"), &path, none, Duration::from_millis(500)).await;
        assert_eq!(hung.status, Status::Fail, "{hung:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(hung.summary.contains("--version"), "{}", hung.summary);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_custom_agent_path_wins_and_no_choice_takes_any_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let custom = script(elsewhere.path(), "my-claude", "echo 'custom 9'");
        let path = path_of(tmp.path());
        let by_settings = |id: &str| (id == "claude").then(|| custom.clone());
        let ok = agent_check(Some("claude"), &path, by_settings, AGENT_TIMEOUT).await;
        assert_eq!(ok.status, Status::Ok, "{ok:?}");
        assert!(ok.summary.contains("custom 9"), "{}", ok.summary);

        let none = |_: &str| None;
        let nothing = agent_check(None, &path, none, AGENT_TIMEOUT).await;
        assert_eq!(nothing.status, Status::Fail, "{nothing:?}");
        script(tmp.path(), "codex", "echo 'codex-cli 0.50.0'");
        let any = agent_check(Some("auto"), &path, none, AGENT_TIMEOUT).await;
        assert_eq!(any.status, Status::Ok, "{any:?}");
        assert!(any.summary.contains("codex-cli"), "{}", any.summary);
    }

    /// QA-118: an unknown `agentId` fails; it never falls back to any
    /// installed agent.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_chosen_agent_that_does_not_exist_fails() {
        let tmp = tempfile::tempdir().unwrap();
        script(tmp.path(), "claude", "echo '2.1.0 (Claude Code)'");
        let path = path_of(tmp.path());
        let check = agent_check(Some("no-such-agent"), &path, |_: &str| None, AGENT_TIMEOUT).await;
        assert_eq!(check.status, Status::Fail, "{check:?}");
        assert!(
            check.summary.contains("`no-such-agent`"),
            "{}",
            check.summary
        );
        assert!(!check.summary.contains("Claude"), "{}", check.summary);
        assert!(check.fix.is_some());
    }

    #[tokio::test]
    async fn an_api_key_agent_needs_no_cli() {
        let path = std::ffi::OsString::new();
        let check = agent_check(Some("byok"), &path, |_: &str| None, AGENT_TIMEOUT).await;
        assert_eq!(check.status, Status::Ok, "{check:?}");
    }

    /// Shows the page, fails, or never answers.
    enum Fake {
        Shows,
        Fails,
        Hangs,
    }

    impl Renderer for Fake {
        fn render_slides<'a>(
            &'a self,
            _: &'a RenderTarget,
            _: DeckMode,
        ) -> BoxFuture<'a, RenderResult<Slides>> {
            Box::pin(async { Err(RenderError::Unsupported("slides")) })
        }
        fn render_frames<'a>(
            &'a self,
            _: &'a RenderTarget,
            _: FrameOptions,
            _: &'a Path,
        ) -> BoxFuture<'a, RenderResult<od_export::FrameSet>> {
            Box::pin(async { Err(RenderError::Unsupported("frames")) })
        }
        fn print_pdf<'a>(
            &'a self,
            _: &'a RenderTarget,
            _: DeckMode,
        ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
            Box::pin(async { Err(RenderError::Unsupported("pdf")) })
        }
        fn capture_page<'a>(
            &'a self,
            target: &'a RenderTarget,
            _: CaptureOptions,
        ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
            Box::pin(async move {
                assert!(target.html_path.is_file(), "the test page is written");
                match self {
                    Self::Shows => Ok(b"png".to_vec()),
                    Self::Fails => Err(RenderError::Failed("the browser quit".into())),
                    Self::Hangs => std::future::pending().await,
                }
            })
        }
    }

    #[tokio::test]
    async fn chrome_must_be_found_and_show_a_blank_page_in_time() {
        let exe = || Ok(PathBuf::from("/Applications/Chrome"));
        let limit = Duration::from_millis(300);
        let ok = chrome_check(exe(), Arc::new(Fake::Shows), limit).await;
        assert_eq!(ok.status, Status::Ok, "{ok:?}");
        let failed = chrome_check(exe(), Arc::new(Fake::Fails), limit).await;
        assert_eq!(failed.status, Status::Fail);
        assert!(
            failed.summary.contains("the browser quit"),
            "{}",
            failed.summary
        );
        let hung = chrome_check(exe(), Arc::new(Fake::Hangs), limit).await;
        assert_eq!(hung.status, Status::Fail);
        assert!(hung.summary.contains("within"), "{}", hung.summary);
        let missing = chrome_check(
            Err(RenderError::Unavailable("no Chrome".into())),
            Arc::new(Fake::Shows),
            limit,
        )
        .await;
        assert_eq!(missing.status, Status::Fail);
        assert!(missing.fix.unwrap().contains("OD_CHROME_PATH"));
    }

    #[tokio::test]
    async fn a_missing_ffmpeg_only_warns() {
        let missing = ffmpeg_check(
            Err(RenderError::Unavailable("no ffmpeg".into())),
            FFMPEG_TIMEOUT,
        )
        .await;
        assert_eq!(missing.status, Status::Warn, "{missing:?}");
        let gone = ffmpeg_check(Ok(PathBuf::from("/no/such/ffmpeg")), FFMPEG_TIMEOUT).await;
        assert_eq!(gone.status, Status::Warn, "{gone:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_must_answer_version() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = script(tmp.path(), "ffmpeg", "echo 'ffmpeg version 7.1'");
        let check = ffmpeg_check(Ok(ok), FFMPEG_TIMEOUT).await;
        assert_eq!(check.status, Status::Ok, "{check:?}");
        assert!(
            check.summary.starts_with("ffmpeg version 7.1"),
            "{}",
            check.summary
        );
        let broken = script(tmp.path(), "ffmpeg2", "exit 1");
        assert_eq!(
            ffmpeg_check(Ok(broken), FFMPEG_TIMEOUT).await.status,
            Status::Warn
        );
        let hung = script(tmp.path(), "ffmpeg3", "sleep 30");
        let check = ffmpeg_check(Ok(hung), Duration::from_millis(300)).await;
        assert_eq!(check.status, Status::Warn, "{check:?}");
    }

    #[test]
    fn folders_macos_protects_are_reported_not_opened() {
        let home = Path::new("/Users/me");
        let clear = folder_access_check(
            Some(home),
            &[PathBuf::from(
                "/Users/me/Library/Application Support/Contour",
            )],
        );
        assert_eq!(clear.status, Status::Ok);
        let asked = folder_access_check(
            Some(home),
            &[
                PathBuf::from("/Users/me/Library/Application Support/Contour"),
                PathBuf::from("/Users/me/Documents/Designs"),
            ],
        );
        assert_eq!(asked.status, Status::Warn);
        assert!(
            asked.summary.contains("/Users/me/Documents/Designs"),
            "{}",
            asked.summary
        );
        assert!(asked.fix.unwrap().contains("Files and Folders"));
        // A folder merely named like one is not inside it.
        let near = folder_access_check(Some(home), &[PathBuf::from("/Users/me/Documents2")]);
        assert_eq!(near.status, Status::Ok);
    }

    #[test]
    fn a_nearly_full_disk_fails() {
        let root = Path::new("/data");
        assert_eq!(
            disk_check(root, Ok(5 << 30), MIN_FREE_BYTES).status,
            Status::Ok
        );
        let full = disk_check(root, Ok(100 << 20), MIN_FREE_BYTES);
        assert_eq!(full.status, Status::Fail);
        assert!(full.summary.contains("100 MB"), "{}", full.summary);
        let unknown = disk_check(root, Err(std::io::Error::other("no stat")), MIN_FREE_BYTES);
        assert_eq!(unknown.status, Status::Warn);
        let tmp = tempfile::tempdir().unwrap();
        let real = disk_check(tmp.path(), fs4::available_space(tmp.path()), 1);
        assert_eq!(real.status, Status::Ok, "{real:?}");
    }

    #[test]
    fn the_report_fails_only_on_a_failed_check() {
        let warn = Check::warn("ffmpeg", "no ffmpeg", "install it");
        let report = Report::new(vec![Check::ok("disk", "fine"), warn.clone()]);
        assert!(report.ok);
        assert_eq!(report.text(), "✓ fine\n! no ffmpeg\n  fix: install it\n");
        let report = Report::new(vec![warn, Check::fail("agent", "none", "install one")]);
        assert!(!report.ok);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["checks"][1]["status"], "fail");
        assert_eq!(json["checks"][1]["fix"], "install one");
        assert_eq!(json["ok"], false);
    }
}
