//! [`Renderer`] over the Chrome DevTools Protocol.
//!
//! Drives the Chrome, Edge or Chromium already installed (decision 2: never
//! download a browser). Lookup order: `OD_CHROME_PATH`, then the standard
//! install locations for the OS, then `PATH`. Each operation launches a
//! fresh headless browser with a throwaway profile and DNS for everything
//! but localhost disabled, loads a file from disk, and closes it again.
//! The page may load nothing but that file (`data:`/`blob:` aside): every
//! other request, `file://` or network, is failed through CDP `Fetch`.
//!
//! On macOS a browser inside an `.app` bundle is started through
//! LaunchServices, not as our child (`launch_services`): macOS would charge
//! what Chrome and its updater do to Contour (App Management prompts).

pub mod ffmpeg;
pub mod hyperframes_cli;
#[cfg(target_os = "macos")]
mod launch_services;
mod script;

use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams, RequestPattern,
};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams, PrintToPdfParams, Viewport,
};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::error::CdpError;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use od_export::hyperframes::{MAX_FPS, frame_count};
use od_export::pptx_editable::SlideLayout;
use od_export::renderer::{MAX_PAGE_HEIGHT, frame_file_name};
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, ImageFormat, RenderError, RenderResult,
    RenderTarget, Renderer, Slides,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Environment variable naming the browser executable.
pub const CHROME_PATH_ENV: &str = "OD_CHROME_PATH";

/// `1` launches Chrome with `--no-sandbox`, the only way that happens.
///
/// Security: this removes Chrome's isolation of the agent-written HTML it
/// renders. It exists for processes that already run inside a sandbox
/// Chrome cannot nest its own in (macOS `sandbox-exec`, some containers),
/// where Chrome cannot render at all. The product never sets it or falls
/// back to it: a browser that cannot start or render is an error
/// (`scripts/tests/chrome_sandbox_test.sh`, `tests/launch_failure.rs`).
pub const NO_SANDBOX_ENV: &str = "OD_CHROME_NO_SANDBOX";

/// Longest wait for a launched browser to show the page. Normally under two
/// seconds; past this the browser is taken as unable to render here.
const OPEN_TIMEOUT: Duration = Duration::from_secs(90);

/// Browser launches tried when DevTools does not answer within the launch
/// timeout ([`LAUNCH_TIMEOUT`] each).
const LAUNCH_ATTEMPTS: u32 = 2;
/// Longest wait for one launched browser to open DevTools.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest wait for a start through LaunchServices (`open`, DevTools,
/// connecting) before the browser starts as our child instead. It normally
/// takes 1–7 s. A failed start that took this long counts as one of the
/// [`LAUNCH_ATTEMPTS`] ([`spent_a_launch`]), so the worst case (this,
/// ending it, one child launch) leaves over 30 s of [`OPEN_TIMEOUT`] to
/// open the page.
#[cfg(target_os = "macos")]
const LAUNCH_SERVICES_TIMEOUT: Duration = Duration::from_secs(20);
/// What [`OPEN_TIMEOUT`] keeps, past every launch, for opening the page.
#[cfg(target_os = "macos")]
const PAGE_MARGIN: Duration = Duration::from_secs(15);
/// Longest wait chromiumoxide gives command chains (navigation). A single
/// command (`Page::execute`: print, capture) stops at its own fixed
/// [`COMMAND_TIMEOUT`] whatever this says.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// chromiumoxide 0.9.1's own fixed wait for one command
/// (`handler::REQUEST_TIMEOUT`, `CommandFuture`); its error is
/// `CdpError::Timeout`.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest wait for one step once the page is open: settling, showing a
/// slide, measuring, printing, capturing, one MP4 frame. chromiumoxide
/// waits forever for a page promise that never settles (a page that stops
/// animation frames) and, without [`Session::open`]'s handler loop ending,
/// for every call once the browser is gone; so each step is bounded here.
///
/// Measured (macOS, debug build, a 60-slide 1920×1080 deck with blur):
/// printing 13.9 s, a full-page capture 3.9 s, a slide capture 0.25 s,
/// settling 0.1 s. The longest wait a page script makes on purpose is the
/// film timeline's 15 s (`script::frame_ready`); settling waits images at
/// most 5 s. 60 s is four times that, and twice [`COMMAND_TIMEOUT`], so a
/// slow print or capture still ends with chromiumoxide's error first.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(60);
/// Milliseconds that replace [`STEP_TIMEOUT`], read by [`CdpRenderer::new`].
/// For tests that need a page that never settles to fail fast (the QA
/// e2e); the product never sets it.
pub const STEP_TIMEOUT_ENV: &str = "OD_RENDER_STEP_TIMEOUT_MS";

/// Switches every render passes on top of chromiumoxide's defaults, as
/// (key, value) without `--`: chromiumoxide adds it (`--x` would reach
/// Chrome as `----x`, which it ignores; tests/launch_flags.rs).
const RENDER_SWITCHES: &[(&str, Option<&str>)] = &[
    // No outbound network: only localhost resolves.
    (
        "host-resolver-rules",
        Some("MAP * ~NOTFOUND , EXCLUDE localhost"),
    ),
    ("no-first-run", None),
    ("no-default-browser-check", None),
    ("disable-sync", None),
    ("disable-component-update", None),
    ("disable-domain-reliability", None),
    ("metrics-recording-only", None),
];

/// Why a launch failed: only a timeout is worth one more try.
enum LaunchError {
    Timeout(RenderError),
    Other(RenderError),
}

/// Smallest and largest slide edge we trust from measurement.
const SLIDE_MIN_PX: u32 = 320;
const SLIDE_MAX_PX: u32 = 7_680;
/// Whole frame capture must finish within 5 minutes (original limit).
pub const FRAME_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long a closed browser gets to exit before it is killed.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Most frames one capture writes (one hour at 30 fps).
pub const MAX_FRAMES: u64 = 108_000;
/// Deck PDF page width (spec 4.2: 13.333 × 7.5 in).
const DECK_PAGE_WIDTH_IN: f64 = 13.333;
const CSS_PX_PER_IN: f64 = 96.0;

/// Finds a Chromium-family browser: `OD_CHROME_PATH`, standard locations,
/// then `PATH`.
///
/// # Errors
/// [`RenderError::Unavailable`] with a message naming what to install or set.
pub fn find_chrome() -> RenderResult<PathBuf> {
    if let Some(path) = std::env::var_os(CHROME_PATH_ENV).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(RenderError::Unavailable(format!(
                "{CHROME_PATH_ENV} points to {}, which does not exist",
                path.display()
            )))
        };
    }
    candidates()
        .into_iter()
        .find(|p| p.is_file())
        .or_else(find_on_path)
        .ok_or_else(|| {
            RenderError::Unavailable(format!(
                "this export needs Chrome, Edge or Chromium; install one or set {CHROME_PATH_ENV}"
            ))
        })
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(target_os = "macos") {
        let apps = [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Chromium.app/Contents/MacOS/Chromium",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ];
        for app in apps {
            out.push(Path::new("/Applications").join(app));
            if let Some(home) = std::env::var_os("HOME") {
                out.push(Path::new(&home).join("Applications").join(app));
            }
        }
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                out.push(base.join(r"Google\Chrome\Application\chrome.exe"));
                out.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
                out.push(base.join(r"Chromium\Application\chrome.exe"));
            }
        }
    } else {
        for p in [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/microsoft-edge",
            "/snap/bin/chromium",
        ] {
            out.push(PathBuf::from(p));
        }
    }
    out
}

fn find_on_path() -> Option<PathBuf> {
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "chrome",
        "msedge",
    ];
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        names.iter().find_map(|n| {
            let candidate = dir.join(if cfg!(windows) {
                format!("{n}.exe")
            } else {
                (*n).to_owned()
            });
            candidate.is_file().then_some(candidate)
        })
    })
}

/// Chrome-backed renderer. Cheap to construct; the browser is found and
/// launched per operation.
#[derive(Debug, Clone, Default)]
pub struct CdpRenderer {
    /// Fixed executable; `None` runs [`find_chrome`] on each call.
    executable: Option<PathBuf>,
    /// Launch with `--no-sandbox` ([`NO_SANDBOX_ENV`]).
    no_sandbox: bool,
    /// Replaces [`STEP_TIMEOUT`] ([`STEP_TIMEOUT_ENV`], [`Self::with_step_timeout`]).
    step_timeout: Option<Duration>,
}

impl CdpRenderer {
    /// Finds Chrome on each call; [`NO_SANDBOX_ENV`] is read here.
    #[must_use]
    pub fn new() -> Self {
        Self {
            executable: None,
            no_sandbox: no_sandbox_from_env(),
            step_timeout: step_timeout_from_env(),
        }
    }

    /// A fixed browser executable; [`NO_SANDBOX_ENV`] is read here.
    #[must_use]
    pub fn with_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: Some(path.into()),
            no_sandbox: no_sandbox_from_env(),
            step_timeout: step_timeout_from_env(),
        }
    }

    /// Launches without Chrome's own sandbox, as [`NO_SANDBOX_ENV`] does.
    /// For tests and embedders that already sandbox the process; product
    /// code never calls it (see [`NO_SANDBOX_ENV`] for the risk).
    #[must_use]
    pub fn without_chrome_sandbox(mut self) -> Self {
        self.no_sandbox = true;
        self
    }

    /// Bounds each step after the page opened by `limit` instead of
    /// [`STEP_TIMEOUT`]. For tests.
    #[must_use]
    pub fn with_step_timeout(mut self, limit: Duration) -> Self {
        self.step_timeout = Some(limit);
        self
    }

    fn executable(&self) -> RenderResult<PathBuf> {
        match &self.executable {
            Some(p) => Ok(p.clone()),
            None => find_chrome(),
        }
    }

    async fn open(&self, target: &RenderTarget) -> RenderResult<Session> {
        let exe = self.executable()?;
        let opening = Session::open(&exe, target, self.no_sandbox);
        match tokio::time::timeout(OPEN_TIMEOUT, opening).await {
            Ok(session) => session.map(|mut s| {
                s.step_timeout = self.step_timeout.unwrap_or(STEP_TIMEOUT);
                s
            }),
            Err(_) => Err(not_shown(
                &exe,
                &format!("did not show the page within {} s", OPEN_TIMEOUT.as_secs()),
            )),
        }
    }
}

/// A browser at `exe` that started but `why` (did not show the page):
/// inside another sandbox it cannot start its own, and either hangs or
/// quits. The error says where the way out is.
fn not_shown(exe: &Path, why: &str) -> RenderError {
    RenderError::Unavailable(format!(
        "{} started but {why}. Inside another sandbox \
         (sandbox-exec, a container) Chrome cannot start its own; only there, \
         {NO_SANDBOX_ENV}=1 runs it without (this removes Chrome's isolation of the page)",
        exe.display(),
    ))
}

fn no_sandbox_from_env() -> bool {
    std::env::var(NO_SANDBOX_ENV).as_deref() == Ok("1")
}

fn step_timeout_from_env() -> Option<Duration> {
    std::env::var(STEP_TIMEOUT_ENV)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
}

/// Whether Chrome at `exe` can start its own sandbox in this process's
/// environment. It cannot inside another sandbox (macOS `sandbox-exec`),
/// where it prints "Failed to initialize sandbox" and never renders. Runs
/// one headless `about:blank` (about a second). Only tests call it, to
/// decide on [`CdpRenderer::without_chrome_sandbox`]; the product never
/// probes and never falls back to running without the sandbox.
#[must_use]
pub fn chrome_sandbox_works(exe: &Path) -> bool {
    use std::io::Read;
    // A Profile, not a bare temp dir: Chrome's helpers outlive it briefly
    // and would write the folder back after it was removed.
    let Ok(profile) = Profile::new() else {
        return false;
    };
    let Ok(tmp) = profile.tmp() else {
        return false;
    };
    let mut cmd = std::process::Command::new(exe);
    if let Some(tmp) = tmp {
        cmd.env("TMPDIR", tmp);
    }
    let Ok(mut child) = cmd
        .args([
            "--headless=new",
            "--no-first-run",
            "--disable-gpu",
            "--dump-dom",
            "about:blank",
        ])
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let exited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    exited && !err.contains("Failed to initialize sandbox")
}

fn failed(context: &str) -> impl Fn(chromiumoxide::error::CdpError) -> RenderError + '_ {
    move |e| RenderError::Failed(format!("{context}: {e}"))
}

/// One headless browser with one page, closed on [`Session::close`].
struct Session {
    browser: Browser,
    handler: tokio::task::JoinHandle<()>,
    /// Set when the connection to the browser is lost (it quit or was
    /// killed), before the calls waiting fail ([`unless_gone`]).
    gone: Arc<AtomicBool>,
    /// Bound of each [`Session::step`].
    step_timeout: Duration,
    page: Page,
    /// Answers paused requests: only the target file goes through.
    gate: Option<tokio::task::JoinHandle<()>>,
    width: u32,
    height: u32,
    /// Started through LaunchServices: not our child, so closing waits for
    /// its processes by profile rather than by `wait`.
    detached: bool,
    /// Dropped last: after the browser, so its processes are gone first.
    profile: Profile,
}

/// How a start through LaunchServices went ([`Session::launch_detached`]).
#[cfg(target_os = "macos")]
enum Detached {
    Started(Box<Launched>),
    /// Not tried: the browser is no bundle LaunchServices can open now.
    Skipped,
    /// Tried and failed, which took up to [`LAUNCH_SERVICES_TIMEOUT`];
    /// with why, when known (the browser's own last line included).
    Failed(Option<RenderError>),
}

/// A browser just started and connected to.
struct Launched {
    browser: Browser,
    handler: chromiumoxide::Handler,
    profile: Profile,
    detached: bool,
}

/// Prefix of browser profile folders in the temp dir.
const PROFILE_PREFIX: &str = "od-chrome-";
/// A profile folder without a browser lock this old is left over.
#[cfg(unix)]
const STALE_PROFILE: Duration = Duration::from_secs(60 * 60);
/// Marks a profile a start through LaunchServices is under way on, from
/// before `open` until the browser is connected to; it holds the pid of
/// the process starting it. A mark left ([`Profile::abandon`], a cancelled
/// render, a process that quit or was killed meanwhile) means whatever
/// runs on the profile is a browser started for nobody: LaunchServices may
/// start it well after `open` is gone. [`sweep_stale_profiles`] ends it.
#[cfg(unix)]
const OPENING: &str = "od-opening";
/// The mark #235 left on a profile given up on, without a pid: read as an
/// [`OPENING`] mark whose process is gone.
#[cfg(unix)]
const ABANDONED: &str = "od-abandoned";
/// How long a profile given up on is watched for a browser starting late,
/// and how old its [`OPENING`] mark is before any sweep ends what runs on
/// it and removes it. LaunchServices may still start a browser well after
/// `open` was killed (a Gatekeeper check of a large bundle on a slow Mac).
#[cfg(unix)]
const ABANDONED_WATCH: Duration = Duration::from_secs(120);
/// How often an abandoned profile is checked during [`ABANDONED_WATCH`].
#[cfg(target_os = "macos")]
const ABANDONED_POLL: Duration = Duration::from_secs(1);

/// A throwaway browser profile. Removed only once no process started with
/// it is left: a Chrome helper still exiting would write it back (the
/// browser is killed on drop, not waited for).
struct Profile {
    dir: Option<tempfile::TempDir>,
    /// Marked [`OPENING`]: dropped so, the start was given up on midway.
    #[cfg(target_os = "macos")]
    opening: bool,
}

impl Profile {
    fn new() -> std::io::Result<Self> {
        sweep_stale_profiles(&std::env::temp_dir());
        let dir = tempfile::Builder::new().prefix(PROFILE_PREFIX).tempdir()?;
        Ok(Self {
            dir: Some(dir),
            #[cfg(target_os = "macos")]
            opening: false,
        })
    }

    fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .map_or(Path::new(""), tempfile::TempDir::path)
    }

    /// Chrome's `TMPDIR` on Linux, inside the profile: there Chrome puts
    /// its singleton socket folder (`com.google.Chrome.*`) and shared-memory
    /// files in the temp dir and leaves them when killed; this way they go
    /// with the profile. macOS leaves nothing there, and its temp path is
    /// long enough that a deeper socket could pass the 104-byte limit.
    fn tmp(&self) -> std::io::Result<Option<PathBuf>> {
        if !cfg!(target_os = "linux") {
            return Ok(None);
        }
        let tmp = self.path().join("tmp");
        std::fs::create_dir_all(&tmp)?;
        Ok(Some(tmp))
    }

    /// Marks the profile [`OPENING`], before `open` runs: from here on it
    /// is given up on unless [`Self::opened`] clears the mark, whether this
    /// drops, the render is cancelled or the process ends.
    #[cfg(target_os = "macos")]
    fn mark_opening(&mut self) -> std::io::Result<()> {
        std::fs::write(self.path().join(OPENING), std::process::id().to_string())?;
        self.opening = true;
        Ok(())
    }

    /// Clears the [`OPENING`] mark: the browser on the profile is ours.
    #[cfg(target_os = "macos")]
    fn opened(&mut self) -> std::io::Result<()> {
        std::fs::remove_file(self.path().join(OPENING))?;
        self.opening = false;
        Ok(())
    }

    /// Ends a profile LaunchServices failed to start a browser on, now
    /// (blocks up to 3 s, as dropping does) and for [`ABANDONED_WATCH`]
    /// after ([`watch_abandoned`]). The folder stays with its [`OPENING`]
    /// mark: removed, it would come back unmarked with a browser starting
    /// late; the first sweep after the watch removes it.
    #[cfg(target_os = "macos")]
    fn abandon(mut self) {
        let Some(dir) = self.dir.take() else {
            return;
        };
        let path = dir.keep();
        end_profile_users(&path);
        watch_abandoned(path);
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let Some(dir) = self.dir.take() else {
            return;
        };
        // A start through LaunchServices dropped midway (a render cancelled
        // while it waits for `open`): kept and watched as if abandoned, from
        // another thread, as this may run on the async runtime.
        #[cfg(target_os = "macos")]
        if self.opening {
            watch_abandoned(dir.keep());
            return;
        }
        end_profile_users(dir.path());
        let path = dir.path().to_path_buf();
        if dir.close().is_err() || path.exists() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Kills whatever runs on the abandoned profile at `path` every
/// [`ABANDONED_POLL`] for [`ABANDONED_WATCH`], on a thread of its own: a
/// browser LaunchServices starts late. Past that, or once this process is
/// gone, the next sweep (in any process) does it. The first check waits a
/// poll: the child launch that follows a failed start runs meanwhile, and a
/// `ps` beside it slowed it enough to lose what a browser exiting at once
/// printed (chromiumoxide reads its stderr in a race with its exit).
#[cfg(target_os = "macos")]
fn watch_abandoned(path: PathBuf) {
    let watched = path.clone();
    let watch = std::thread::Builder::new()
        .name("od-abandoned-profile".to_owned())
        .spawn(move || {
            let until = std::time::Instant::now() + ABANDONED_WATCH;
            while std::time::Instant::now() < until {
                std::thread::sleep(ABANDONED_POLL);
                end_profile_users(&watched);
            }
        });
    if let Err(e) = watch {
        tracing::warn!(error = %e, "cannot watch an abandoned browser profile; the next render ends what starts on it");
        end_profile_users(&path);
    }
}

/// The [`OPENING`] mark on the profile `dir` (or #235's [`ABANDONED`]),
/// `None` when it has none.
#[cfg(unix)]
struct OpeningMark {
    /// The process that marked it still runs: its start may be under way.
    writer_runs: bool,
    age: Duration,
}

#[cfg(unix)]
impl OpeningMark {
    fn of(dir: &Path) -> Option<Self> {
        let (mark, writer) = match std::fs::read_to_string(dir.join(OPENING)) {
            Ok(pid) => (dir.join(OPENING), pid.trim().parse::<i32>().ok()),
            Err(_) if dir.join(OPENING).exists() => (dir.join(OPENING), None),
            Err(_) if dir.join(ABANDONED).exists() => (dir.join(ABANDONED), None),
            Err(_) => return None,
        };
        let writer_runs = writer.filter(|pid| *pid > 0).is_some_and(|pid| {
            !matches!(
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
                Err(nix::errno::Errno::ESRCH)
            )
        });
        let age = std::fs::metadata(&mark)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .unwrap_or(Duration::ZERO);
        Some(Self { writer_runs, age })
    }

    /// Its start was given up on: what runs on the profile is left over.
    fn given_up(&self) -> bool {
        !self.writer_runs || self.age > ABANDONED_WATCH
    }

    /// Old enough that no browser starts on the profile any more.
    fn expired(&self) -> bool {
        self.age > ABANDONED_WATCH
    }
}

/// Pids of the processes started with `--user-data-dir=<dir>` (Chrome and
/// every helper it spawns); none when `ps` cannot run.
#[cfg(unix)]
fn profile_users(dir: &Path) -> Vec<i32> {
    profile_users_checked(dir).unwrap_or_default()
}

/// [`profile_users`], `None` when `ps` cannot run (it is setuid root on
/// macOS and fails inside `sandbox-exec`).
#[cfg(unix)]
fn profile_users_checked(dir: &Path) -> Option<Vec<i32>> {
    let flag = format!("--user-data-dir={}", dir.display());
    let out = std::process::Command::new("ps")
        .args(["-A", "-ww", "-o", "pid=,args="])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains(&flag))
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .collect(),
    )
}

/// Kills what still runs with `dir` as its profile and waits (≤ 3 s) for
/// it to be gone.
#[cfg(unix)]
fn end_profile_users(dir: &Path) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    for _ in 0..60 {
        let users = profile_users(dir);
        if users.is_empty() {
            return;
        }
        for pid in users {
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(not(unix))]
fn end_profile_users(_dir: &Path) {}

/// Removes profile folders a render killed with its process left in `tmp`:
/// no process uses them and their lock names a dead browser, or they have
/// no lock and are older than [`STALE_PROFILE`]. On one marked
/// [`OPENING`] whose start was given up on ([`OpeningMark::given_up`]) it
/// ends whatever runs, a browser that started late; the folder goes once
/// the mark is older than [`ABANDONED_WATCH`].
#[cfg(unix)]
fn sweep_stale_profiles(tmp: &Path) {
    let Ok(entries) = std::fs::read_dir(tmp) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let dir = entry.path();
        let is_profile = entry
            .file_name()
            .to_string_lossy()
            .starts_with(PROFILE_PREFIX)
            && entry.file_type().is_ok_and(|t| t.is_dir());
        if !is_profile {
            continue;
        }
        if let Some(mark) = OpeningMark::of(&dir) {
            if mark.given_up() {
                end_profile_users(&dir);
            }
            if mark.expired() {
                let _ = std::fs::remove_dir_all(&dir);
            }
            continue;
        }
        if !profile_users(&dir).is_empty() {
            continue;
        }
        // `SingletonLock` → `<host>-<pid>` of the browser using it.
        let stale = match std::fs::read_link(dir.join("SingletonLock")) {
            Ok(target) => target
                .to_string_lossy()
                .rsplit('-')
                .next()
                .and_then(|p| p.parse::<i32>().ok())
                .is_some_and(|pid| {
                    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err()
                }),
            Err(_) => entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age > STALE_PROFILE),
        };
        if stale {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(not(unix))]
fn sweep_stale_profiles(_tmp: &Path) {}

/// The child's error, with why LaunchServices failed first when the child's
/// says nothing more: chromiumoxide 0.9 can lose a child's stderr when the
/// child exits at once, and LaunchServices' error carries the browser's own
/// last line (`od-stderr.log`).
fn with_detached_reason(child: RenderError, detached: Option<RenderError>) -> RenderError {
    match (child, detached) {
        (RenderError::Unavailable(m), Some(first)) => {
            RenderError::Unavailable(format!("{m}; through LaunchServices first: {first}"))
        }
        (child, _) => child,
    }
}

/// Whether a failed start through LaunchServices that took `took` counts
/// as one of the [`LAUNCH_ATTEMPTS`]: when what it left of [`OPEN_TIMEOUT`]
/// cannot hold all of them and [`PAGE_MARGIN`]. `open` failing at once
/// leaves the child launch both.
#[cfg(target_os = "macos")]
fn spent_a_launch(took: Duration) -> bool {
    took + LAUNCH_TIMEOUT * LAUNCH_ATTEMPTS + PAGE_MARGIN > OPEN_TIMEOUT
}

impl Session {
    async fn open(exe: &Path, target: &RenderTarget, no_sandbox: bool) -> RenderResult<Self> {
        let Launched {
            browser,
            mut handler,
            profile,
            detached,
        } = Self::launch(exe, no_sandbox).await?;
        let gone = Arc::new(AtomicBool::new(false));
        let lost = Arc::clone(&gone);
        // In the export's span: a lost connection logs the request's id.
        let handler = tokio::spawn(tracing::Instrument::in_current_span(async move {
            drive(&mut handler, &lost).await;
        }));
        // Chrome that cannot start its sandbox quits here (#231).
        let quit = Arc::clone(&gone);
        let quit = |e: RenderError| {
            if quit.load(Ordering::Acquire) {
                not_shown(exe, "quit before it showed the page")
            } else {
                e
            }
        };
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(failed("opening a page"))
            .map_err(quit)?;
        let mut session = Self {
            page,
            browser,
            handler,
            gone,
            step_timeout: STEP_TIMEOUT,
            gate: None,
            width: target.width.max(1),
            height: target.height.max(1),
            detached,
            profile,
        };
        session
            .resize(session.width, session.height)
            .await
            .map_err(quit)?;
        let url = file_url(&target.html_path)?;
        session.gate = Some(session.only_allow(url.clone()).await.map_err(quit)?);
        session
            .page
            .goto(url)
            .await
            .map_err(failed("loading the page"))
            .map_err(quit)?;
        Ok(session)
    }

    /// Starts the browser at `exe` on a fresh profile and connects to it:
    /// every render's browser starts here. On macOS a browser in an `.app`
    /// bundle goes through LaunchServices first ([`Self::launch_detached`]);
    /// otherwise, or when that fails, it is spawned as our child by
    /// chromiumoxide.
    async fn launch(exe: &Path, no_sandbox: bool) -> RenderResult<Launched> {
        // A cold first start (Windows scanning a fresh profile) can miss the
        // launch timeout once; a second launch with a new profile is warm.
        let mut attempt = 1;
        // Why LaunchServices failed: chromiumoxide can lose a child's stderr
        // when the child exits at once, so the child's error may not say why.
        #[allow(unused_mut)]
        let mut detached_error: Option<RenderError> = None;
        #[cfg(target_os = "macos")]
        {
            let open = Path::new(launch_services::OPEN);
            let started = std::time::Instant::now();
            match Self::launch_detached(open, LAUNCH_SERVICES_TIMEOUT, exe, no_sandbox).await {
                Detached::Started(launched) => return Ok(*launched),
                // It took a launch's time: the child launch left gets all of its own.
                Detached::Failed(why) => {
                    if spent_a_launch(started.elapsed()) {
                        attempt += 1;
                    }
                    detached_error = why;
                }
                Detached::Skipped => {}
            }
        }
        loop {
            match Self::launch_child(exe, no_sandbox).await {
                Err(LaunchError::Timeout(_)) if attempt < LAUNCH_ATTEMPTS => attempt += 1,
                Err(LaunchError::Timeout(e) | LaunchError::Other(e)) => {
                    return Err(with_detached_reason(e, detached_error));
                }
                Ok(launched) => return Ok(launched),
            }
        }
    }

    /// The start through LaunchServices ([`launch_services`], `open` being
    /// the one at `open`), within `timeout`. Neither skipping it nor its failure fails an
    /// export: the caller starts a child instead. Its cost there is that
    /// macOS may charge the child's doings to us (App Management).
    #[cfg(target_os = "macos")]
    async fn launch_detached(
        open: &Path,
        timeout: Duration,
        exe: &Path,
        no_sandbox: bool,
    ) -> Detached {
        let Some(app) = launch_services::app_bundle(exe) else {
            return Detached::Skipped;
        };
        if launch_services::awaits_approval(&app) {
            tracing::warn!(
                app = %app.display(),
                "the browser is quarantined and its first launch was never approved (opened from Finder), so LaunchServices would wait for that; it starts as our child instead"
            );
            return Detached::Skipped;
        }
        let Ok(mut profile) = Profile::new() else {
            return Detached::Skipped;
        };
        let Ok(tmp) = profile.tmp() else {
            return Detached::Skipped;
        };
        // Before `open`: a browser it starts for nobody (this future dropped,
        // the process gone) is found by the mark and ended.
        if let Err(e) = profile.mark_opening() {
            tracing::warn!(error = %e, "cannot mark the browser profile, so the browser starts as our child, not through LaunchServices");
            return Detached::Skipped;
        }
        match launch_services::launch(
            open,
            &app,
            exe,
            profile.path(),
            tmp.as_deref(),
            no_sandbox,
            timeout,
            REQUEST_TIMEOUT,
        )
        .await
        {
            Ok(Some((browser, handler))) => {
                if let Err(e) = profile.opened() {
                    // Left marked, a sweep would end it in the middle of a render.
                    tracing::warn!(error = %e, "cannot clear the browser profile's mark; the browser starts as our child instead");
                    drop((browser, handler));
                    let _ = tokio::task::spawn_blocking(move || profile.abandon()).await;
                    return Detached::Failed(None);
                }
                Detached::Started(Box::new(Launched {
                    browser,
                    handler,
                    profile,
                    detached: true,
                }))
            }
            Ok(None) => {
                // Nothing was opened; a mark left only keeps the folder a while.
                let _ = profile.opened();
                tracing::warn!(
                    app = %app.display(),
                    "`ps` cannot run (inside another sandbox?), so the browser starts as our child, not through LaunchServices"
                );
                Detached::Skipped
            }
            Err(LaunchError::Timeout(e) | LaunchError::Other(e)) => {
                tracing::warn!(
                    app = %app.display(),
                    error = %e,
                    "starting the browser through LaunchServices failed; it starts as our child instead"
                );
                // Ends what LaunchServices started, or starts late (waits up to 3 s).
                let _ = tokio::task::spawn_blocking(move || profile.abandon()).await;
                Detached::Failed(Some(e))
            }
        }
    }

    /// The browser at `exe` spawned as our child by chromiumoxide.
    async fn launch_child(exe: &Path, no_sandbox: bool) -> Result<Launched, LaunchError> {
        let profile = Profile::new()
            .map_err(|e| RenderError::Failed(format!("creating a browser profile: {e}")))
            .map_err(LaunchError::Other)?;
        let tmp = profile
            .tmp()
            .map_err(|e| RenderError::Failed(format!("creating a browser profile: {e}")))
            .map_err(LaunchError::Other)?;
        let mut config = BrowserConfig::builder();
        if no_sandbox {
            config = config.no_sandbox();
        }
        if let Some(tmp) = tmp {
            config = config.env("TMPDIR", tmp.to_string_lossy());
        }
        for (key, value) in RENDER_SWITCHES {
            config = match value {
                Some(value) => config.arg((*key, *value)),
                None => config.arg(*key),
            };
        }
        let config = config
            .chrome_executable(exe)
            .new_headless_mode()
            .user_data_dir(profile.path())
            .viewport(None)
            .launch_timeout(LAUNCH_TIMEOUT)
            .request_timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| RenderError::Unavailable(format!("browser config: {e}")))
            .map_err(LaunchError::Other)?;
        match Browser::launch(config).await {
            Ok((browser, handler)) => Ok(Launched {
                browser,
                handler,
                profile,
                detached: false,
            }),
            Err(e) => {
                let err = RenderError::Unavailable(format!("cannot start {}: {e}", exe.display()));
                Err(match e {
                    chromiumoxide::error::CdpError::LaunchTimeout(_) => LaunchError::Timeout(err),
                    _ => LaunchError::Other(err),
                })
            }
        }
    }

    /// Pauses every request and lets only `allowed` (the page itself)
    /// continue, so a bundle cannot pull in local files (`<iframe
    /// src="file:///…">`) or reach the network by IP literal.
    async fn only_allow(&self, allowed: String) -> RenderResult<tokio::task::JoinHandle<()>> {
        let mut paused = self
            .page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(failed("intercepting requests"))?;
        self.page
            .execute(
                EnableParams::builder()
                    .pattern(RequestPattern::builder().url_pattern("*").build())
                    .build(),
            )
            .await
            .map_err(failed("intercepting requests"))?;
        let page = self.page.clone();
        Ok(tokio::spawn(tracing::Instrument::in_current_span(
            async move {
                while let Some(ev) = paused.next().await {
                    let id = ev.request_id.clone();
                    let res = if same_file_url(&ev.request.url, &allowed) {
                        page.execute(ContinueRequestParams::new(id)).await.map(drop)
                    } else {
                        tracing::warn!(url = %ev.request.url, "render blocked a request");
                        page.execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                            .await
                            .map(drop)
                    };
                    if res.is_err() {
                        break; // the page is gone
                    }
                }
            },
        )))
    }

    async fn resize(&mut self, width: u32, height: u32) -> RenderResult<()> {
        self.width = width;
        self.height = height;
        let resizing = self.page.execute(SetDeviceMetricsOverrideParams::new(
            i64::from(width),
            i64::from(height),
            1.0,
            false,
        ));
        self.step("setting the viewport", resizing).await?;
        Ok(())
    }

    /// Runs one CDP call, `what` naming it in errors, within
    /// [`Self::step_timeout`]; fails at once when the browser is gone.
    async fn step<T>(
        &self,
        what: &str,
        call: impl std::future::Future<Output = Result<T, CdpError>>,
    ) -> RenderResult<T> {
        let started = std::time::Instant::now();
        let limit = self.step_timeout;
        let result = unless_gone(&self.gone, what, async {
            match tokio::time::timeout(limit, call).await {
                Ok(answer) => answered(what, answer),
                Err(_) => Err(RenderError::Failed(format!(
                    "the page gave no answer within {} s while {what}; try again, and if it fails again, a script on the page may be holding it up",
                    limit.as_secs_f64().ceil()
                ))),
            }
        })
        .await;
        tracing::debug!(
            step = what,
            ms = started.elapsed().as_millis(),
            ok = result.is_ok(),
            "render step"
        );
        result
    }

    /// Runs `js` in the page, awaiting its promise, as one [`Self::step`].
    async fn eval(&self, what: &str, js: &str) -> RenderResult<Value> {
        let params = EvaluateParams::builder()
            .expression(js)
            .await_promise(true)
            .return_by_value(true)
            .build()
            .map_err(RenderError::Failed)?;
        let result = self
            .step(what, self.page.evaluate_expression(params))
            .await?;
        Ok(result.value().cloned().unwrap_or(Value::Null))
    }

    /// Waits for fonts, images and two frames, finishing or leaving
    /// `animations`; reports slides and size.
    async fn settle(&self, animations: script::Animations) -> RenderResult<PageInfo> {
        let v = self
            .eval("waiting for the page to load", &script::settle(animations))
            .await?;
        let num = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(PageInfo {
            slides: usize::try_from(num("slides")).unwrap_or(0),
            slide_width: clamp(num("w")),
            slide_height: clamp(num("h")),
            scroll_height: clamp(num("scrollHeight")),
        })
    }

    async fn screenshot(
        &self,
        clip: Viewport,
        format: ImageFormat,
        beyond: bool,
    ) -> RenderResult<Vec<u8>> {
        let mut params = CaptureScreenshotParams::builder()
            .clip(clip)
            .from_surface(true)
            .capture_beyond_viewport(beyond);
        params = match format {
            ImageFormat::Png => params.format(CaptureScreenshotFormat::Png),
            ImageFormat::Jpeg => params.format(CaptureScreenshotFormat::Jpeg).quality(92),
            ImageFormat::Webp => params.format(CaptureScreenshotFormat::Webp).quality(92),
        };
        let shot = self
            .step("capturing", self.page.execute(params.build()))
            .await?;
        let data: &str = shot.result.data.as_ref();
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| RenderError::Failed(format!("decoding the screenshot: {e}")))
    }

    fn viewport_clip(&self, y: u32, height: u32) -> Viewport {
        Viewport {
            x: 0.0,
            y: f64::from(y),
            width: f64::from(self.width),
            height: f64::from(height),
            scale: 1.0,
        }
    }

    /// Adopts the deck's authored slide size when it is plausible.
    async fn fit_to_slides(&mut self, info: &PageInfo) -> RenderResult<()> {
        let plausible = |n: u32| (SLIDE_MIN_PX..=SLIDE_MAX_PX).contains(&n);
        if plausible(info.slide_width)
            && plausible(info.slide_height)
            && (info.slide_width, info.slide_height) != (self.width, self.height)
        {
            self.resize(info.slide_width, info.slide_height).await?;
        }
        Ok(())
    }

    /// Asks the browser to quit and waits for it, at most [`CLOSE_TIMEOUT`]:
    /// waiting without a bound once held an export until the CLI gave up
    /// after 600 s. A browser still running then is killed when `self`
    /// drops (chromiumoxide spawns its child with `kill_on_drop`; the
    /// profile ends every process started with it).
    async fn close(mut self) {
        if let Some(gate) = self.gate.take() {
            gate.abort();
        }
        let quit = async {
            let _ = self.browser.close().await;
            let _ = self.browser.wait().await;
            if self.detached {
                wait_profile_unused(self.profile.path()).await;
            }
        };
        if tokio::time::timeout(CLOSE_TIMEOUT, quit).await.is_err() {
            tracing::warn!("the browser did not exit after closing; killing it");
        }
        self.handler.abort();
    }
}

/// Drives chromiumoxide's handler `events` until the connection to the
/// browser is lost or its stream ends, then sets `lost`. chromiumoxide
/// reports a lost connection once and then goes quiet, leaving every call
/// in flight or after it waiting forever; the caller then drops the
/// handler, which fails them all. A stream that ends (`None`) leaves the
/// same calls with no answer, so it counts as lost too. chromiumoxide
/// 0.9.1 does not end it on a WebSocket Close frame (its handler stays
/// pending): that case still ends at [`STEP_TIMEOUT`].
async fn drive(
    events: &mut (impl futures_util::Stream<Item = Result<(), CdpError>> + Unpin),
    lost: &AtomicBool,
) {
    while let Some(event) = events.next().await {
        if let Err(e @ (CdpError::Ws(_) | CdpError::Io(_))) = event {
            tracing::warn!(error = %e, "lost the connection to the browser");
            break;
        }
    }
    lost.store(true, Ordering::Release);
}

/// The answer to the CDP call `what`, chromiumoxide's own timeout (a
/// print or capture past [`COMMAND_TIMEOUT`]) said in the user's words.
fn answered<T>(what: &str, answer: Result<T, CdpError>) -> RenderResult<T> {
    match answer {
        Err(CdpError::Timeout) => Err(RenderError::Failed(format!(
            "the browser gave no answer within {} s while {what}; try again",
            COMMAND_TIMEOUT.as_secs()
        ))),
        answer => answer.map_err(failed(what)),
    }
}

/// `call`, its error saying the browser quit when the connection to it
/// was lost (`gone`). The handler task ends then, and dropping the handler
/// fails every call in flight or after at once ("oneshot canceled").
async fn unless_gone<T>(
    gone: &AtomicBool,
    what: &str,
    call: impl std::future::Future<Output = RenderResult<T>>,
) -> RenderResult<T> {
    call.await.map_err(|e| {
        if gone.load(Ordering::Acquire) {
            RenderError::Failed(format!("the browser quit while {what}; try again"))
        } else {
            e
        }
    })
}

/// Waits until no process runs with `dir` as its profile (or `ps` cannot
/// tell): `wait` for a browser that is not our child.
#[cfg(unix)]
async fn wait_profile_unused(dir: &Path) {
    loop {
        let d = dir.to_path_buf();
        let users = tokio::task::spawn_blocking(move || profile_users(&d))
            .await
            .unwrap_or_default();
        if users.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(not(unix))]
async fn wait_profile_unused(_dir: &Path) {}

#[derive(Debug, Clone, Copy)]
struct PageInfo {
    slides: usize,
    slide_width: u32,
    slide_height: u32,
    scroll_height: u32,
}

fn file_url(path: &Path) -> RenderResult<String> {
    let abs = std::fs::canonicalize(path)
        .map_err(|e| RenderError::Failed(format!("{}: {e}", path.display())))?;
    Ok(file_url_of(&abs.to_string_lossy()))
}

/// The `file://` URL of an absolute path. On Windows `canonicalize` gives a
/// verbatim path (`\\?\C:\…`, `\\?\UNC\server\share\…`), which no browser
/// opens as a URL (`file:////%3F/C:/…`): the prefix goes first.
fn file_url_of(abs: &str) -> String {
    let unverbatim = if let Some(unc) = abs.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        abs.strip_prefix(r"\\?\").unwrap_or(abs).to_owned()
    };
    let mut url = String::from("file://");
    let s = unverbatim.replace('\\', "/");
    if !s.starts_with('/') {
        url.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// `url` names the same file as `allowed` (fragment and percent-encoding
/// differences aside).
fn same_file_url(url: &str, allowed: &str) -> bool {
    let strip = |u: &str| percent_decode(u.split('#').next().unwrap_or_default());
    strip(url) == strip(allowed)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn is_deck(mode: DeckMode, info: &PageInfo) -> RenderResult<bool> {
    match mode {
        DeckMode::Deck if info.slides == 0 => Err(RenderError::NotADeck),
        DeckMode::Deck => Ok(true),
        DeckMode::Page => Ok(false),
        DeckMode::Auto => Ok(info.slides > 0),
    }
}

impl CdpRenderer {
    async fn slides(&self, target: &RenderTarget, mode: DeckMode) -> RenderResult<Slides> {
        let mut s = self.open(target).await?;
        let result = async {
            let info = s.settle(script::Animations::Finish).await?;
            if is_deck(mode, &info)? {
                s.fit_to_slides(&info).await?;
                let mut images = Vec::with_capacity(info.slides);
                for i in 0..info.slides {
                    s.eval(
                        &format!("showing slide {}", i + 1),
                        &script::show_slide(i, s.width, s.height),
                    )
                    .await?;
                    images.push(
                        s.screenshot(s.viewport_clip(0, s.height), ImageFormat::Png, false)
                            .await?,
                    );
                }
                Ok(Slides {
                    deck: true,
                    width: s.width,
                    height: s.height,
                    images,
                })
            } else {
                let total = info.scroll_height.clamp(1, MAX_PAGE_HEIGHT);
                let mut images = Vec::new();
                let mut y = 0;
                while y < total {
                    let h = s.height.min(total - y);
                    images.push(
                        s.screenshot(s.viewport_clip(y, h), ImageFormat::Png, true)
                            .await?,
                    );
                    y += h;
                }
                Ok(Slides {
                    deck: false,
                    width: s.width,
                    height: s.height,
                    images,
                })
            }
        }
        .await;
        s.close().await;
        result
    }

    async fn pdf(&self, target: &RenderTarget, mode: DeckMode) -> RenderResult<Vec<u8>> {
        let mut s = self.open(target).await?;
        let result = async {
            let info = s.settle(script::Animations::Finish).await?;
            let params = if is_deck(mode, &info)? {
                s.fit_to_slides(&info).await?;
                s.eval(
                    "laying out the slides for print",
                    &script::print_slides(s.width, s.height),
                )
                .await?;
                let css_width = DECK_PAGE_WIDTH_IN * CSS_PX_PER_IN;
                let scale = (css_width / f64::from(s.width)).clamp(0.1, 2.0);
                PrintToPdfParams::builder()
                    .paper_width(DECK_PAGE_WIDTH_IN)
                    .paper_height(DECK_PAGE_WIDTH_IN * f64::from(s.height) / f64::from(s.width))
                    .scale(scale)
                    .margin_top(0.0)
                    .margin_bottom(0.0)
                    .margin_left(0.0)
                    .margin_right(0.0)
                    .print_background(true)
                    .prefer_css_page_size(false)
                    .build()
            } else {
                PrintToPdfParams::builder().print_background(true).build()
            };
            s.step("printing", s.page.pdf(params)).await
        }
        .await;
        s.close().await;
        result
    }

    async fn capture(
        &self,
        target: &RenderTarget,
        options: CaptureOptions,
    ) -> RenderResult<Vec<u8>> {
        let s = self.open(target).await?;
        let result = async {
            let info = s.settle(script::Animations::Finish).await?;
            let height = if options.full_page {
                info.scroll_height.clamp(1, MAX_PAGE_HEIGHT)
            } else {
                s.height
            };
            s.screenshot(
                s.viewport_clip(0, height),
                options.format,
                options.full_page,
            )
            .await
        }
        .await;
        s.close().await;
        result
    }
}

impl CdpRenderer {
    async fn frames(
        &self,
        target: &RenderTarget,
        options: FrameOptions,
        out_dir: &Path,
    ) -> RenderResult<FrameSet> {
        if options.fps == 0 || options.fps > MAX_FPS {
            return Err(RenderError::Failed(format!(
                "frame rate must be 1–{MAX_FPS}, got {}",
                options.fps
            )));
        }
        tokio::fs::create_dir_all(out_dir)
            .await
            .map_err(|e| RenderError::Failed(format!("{}: {e}", out_dir.display())))?;
        let s = self.open(target).await?;
        let result = tokio::time::timeout(FRAME_CAPTURE_TIMEOUT, async {
            s.settle(script::Animations::Play).await?;
            let meta = s
                .eval(
                    "waiting for the film's timeline",
                    &script::frame_ready(options.require_timeline),
                )
                .await?;
            if options.require_timeline && meta.get("rootTimeline") != Some(&Value::Bool(true)) {
                let id = meta.get("rootId").and_then(Value::as_str).unwrap_or("main");
                return Err(RenderError::Failed(format!(
                    "window.__timelines.{id} was never registered: a script failed, was blocked or never ran, so the film would not move. Check the page's scripts (local files only; nothing is downloaded)"
                )));
            }
            let duration_ms = if options.duration_ms > 0 {
                options.duration_ms
            } else {
                let secs = meta.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
                if !(secs.is_finite() && secs > 0.0) {
                    return Err(RenderError::Failed(
                        "the composition declares no data-duration and registers no timeline in window.__timelines"
                            .to_owned(),
                    ));
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, finite
                let ms = (secs * 1000.0).round() as u64;
                ms
            };
            let count = frame_count(duration_ms, options.fps);
            if count > MAX_FRAMES {
                return Err(RenderError::Failed(format!(
                    "{count} frames exceed the {MAX_FRAMES} frame limit"
                )));
            }
            let count = u32::try_from(count).unwrap_or(u32::MAX);
            for i in 0..count {
                s.eval(
                    &format!("showing frame {}", i + 1),
                    &script::seek_frame(f64::from(i) / f64::from(options.fps)),
                )
                .await?;
                let png = s
                    .screenshot(s.viewport_clip(0, s.height), ImageFormat::Png, false)
                    .await?;
                let path = out_dir.join(frame_file_name(i));
                tokio::fs::write(&path, png)
                    .await
                    .map_err(|e| RenderError::Failed(format!("{}: {e}", path.display())))?;
            }
            Ok(FrameSet {
                count,
                fps: options.fps,
                width: s.width,
                height: s.height,
            })
        })
        .await
        .unwrap_or_else(|_| {
            Err(RenderError::Failed(format!(
                "frame capture timed out after {} minutes",
                FRAME_CAPTURE_TIMEOUT.as_secs() / 60
            )))
        });
        s.close().await;
        result
    }

    async fn layouts(&self, target: &RenderTarget) -> RenderResult<Vec<SlideLayout>> {
        let mut s = self.open(target).await?;
        let result =
            async {
                let info = s.settle(script::Animations::Finish).await?;
                is_deck(DeckMode::Deck, &info)?;
                s.fit_to_slides(&info).await?;
                let mut out = Vec::with_capacity(info.slides);
                for i in 0..info.slides {
                    s.eval(
                        &format!("showing slide {}", i + 1),
                        &script::show_slide(i, s.width, s.height),
                    )
                    .await?;
                    let raw = s
                        .eval(
                            &format!("measuring slide {}", i + 1),
                            od_export::pptx_editable::LAYOUT_SCRIPT,
                        )
                        .await?;
                    out.push(serde_json::from_value(raw).map_err(|e| {
                        RenderError::Failed(format!("measuring slide {}: {e}", i + 1))
                    })?);
                }
                Ok(out)
            }
            .await;
        s.close().await;
        result
    }

    /// Loads `target`, waits for it to settle, and returns the value of `js`
    /// (awaited if it is a promise). For diagnostics and tests.
    ///
    /// # Errors
    /// Browser or script errors.
    pub async fn evaluate(&self, target: &RenderTarget, js: &str) -> RenderResult<Value> {
        let s = self.open(target).await?;
        let result = async {
            s.settle(script::Animations::Finish).await?;
            s.eval("running page script", js).await
        }
        .await;
        s.close().await;
        result
    }
}

impl Renderer for CdpRenderer {
    fn render_slides<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(self.slides(target, mode))
    }

    fn render_frames<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: FrameOptions,
        out_dir: &'a Path,
    ) -> BoxFuture<'a, RenderResult<FrameSet>> {
        Box::pin(self.frames(target, options, out_dir))
    }

    fn print_pdf<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(self.pdf(target, mode))
    }

    fn capture_page<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(self.capture(target, options))
    }

    fn extract_layouts<'a>(
        &'a self,
        target: &'a RenderTarget,
    ) -> BoxFuture<'a, RenderResult<Vec<SlideLayout>>> {
        Box::pin(self.layouts(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_that_says_nothing_keeps_why_launch_services_failed() {
        let child = RenderError::Unavailable("cannot start x: stderr: BrowserStderr(\"\")".into());
        let first = RenderError::Unavailable(
            "cannot start x: it exited before opening DevTools (boom: no display)".into(),
        );
        let e = with_detached_reason(child, Some(first)).to_string();
        assert!(e.starts_with("cannot start x"), "{e}");
        assert!(e.contains("boom: no display"), "{e}");
        let plain = with_detached_reason(RenderError::Unavailable("cannot start x".into()), None);
        assert_eq!(plain.to_string(), "cannot start x");
        let other = with_detached_reason(
            RenderError::NotADeck,
            Some(RenderError::Unavailable("y".into())),
        );
        assert!(matches!(other, RenderError::NotADeck));
    }

    #[tokio::test]
    async fn the_browser_counts_as_gone_once_its_connection_is_lost_or_ends() {
        let invalid = || {
            CdpError::InvalidMessage("x".into(), serde_json::from_str::<Value>("{").unwrap_err())
        };
        // Other errors keep the connection.
        let lost = AtomicBool::new(false);
        let mut events = futures_util::stream::iter(vec![Ok(()), Err(invalid()), Ok(())])
            .chain(futures_util::stream::pending());
        let waited = tokio::time::timeout(Duration::from_millis(50), drive(&mut events, &lost));
        assert!(waited.await.is_err(), "stopped on a message error");
        assert!(!lost.load(Ordering::Acquire));
        // A connection error stops at once, before what follows.
        let lost = AtomicBool::new(false);
        let mut events = futures_util::stream::iter(vec![
            Ok(()),
            Err(CdpError::Io(std::io::ErrorKind::ConnectionReset.into())),
        ])
        .chain(futures_util::stream::pending());
        tokio::time::timeout(Duration::from_secs(5), drive(&mut events, &lost))
            .await
            .expect("went on after the connection was lost");
        assert!(lost.load(Ordering::Acquire));
        // The stream ending is lost too.
        let lost = AtomicBool::new(false);
        let mut events = futures_util::stream::iter(vec![Ok(())]);
        drive(&mut events, &lost).await;
        assert!(lost.load(Ordering::Acquire));
    }

    #[test]
    fn a_command_past_chromiumoxides_wait_says_so_in_words() {
        let e = answered::<()>("printing", Err(CdpError::Timeout)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "render failed: the browser gave no answer within 30 s while printing; try again"
        );
        let other = answered::<()>("capturing", Err(CdpError::NotFound)).unwrap_err();
        assert_eq!(
            other.to_string(),
            "render failed: capturing: Requested value not found."
        );
    }

    #[test]
    fn file_urls_are_percent_encoded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a b#.html");
        std::fs::write(&file, "").unwrap();
        let url = file_url(&file).unwrap();
        assert!(url.starts_with("file:///"), "{url}");
        assert!(url.ends_with("/a%20b%23.html"), "{url}");
    }

    #[test]
    fn windows_verbatim_paths_become_plain_file_urls() {
        assert_eq!(
            file_url_of(r"\\?\C:\Users\u\AppData\Roaming\contour\projects\p 1\index.html"),
            "file:///C:/Users/u/AppData/Roaming/contour/projects/p%201/index.html"
        );
        assert_eq!(
            file_url_of(r"\\?\UNC\server\share\index.html"),
            "file:////server/share/index.html"
        );
        assert_eq!(
            file_url_of(r"C:\work\index.html"),
            "file:///C:/work/index.html"
        );
        assert_eq!(file_url_of("/tmp/index.html"), "file:///tmp/index.html");
    }

    #[test]
    fn deck_mode_decision() {
        let page = PageInfo {
            slides: 0,
            slide_width: 0,
            slide_height: 0,
            scroll_height: 10,
        };
        let deck = PageInfo { slides: 3, ..page };
        assert!(matches!(
            is_deck(DeckMode::Deck, &page),
            Err(RenderError::NotADeck)
        ));
        assert!(!is_deck(DeckMode::Auto, &page).unwrap());
        assert!(is_deck(DeckMode::Auto, &deck).unwrap());
        assert!(!is_deck(DeckMode::Page, &deck).unwrap());
    }

    /// A start through LaunchServices that failed at once (`open` erred)
    /// leaves the child launch both attempts; one that ran to its deadline
    /// takes one, so the child's 30 s still fit the 90 s an export waits.
    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_slow_launch_services_failure_costs_a_child_attempt() {
        assert!(!spent_a_launch(Duration::from_millis(300)));
        assert!(!spent_a_launch(Duration::from_secs(15)));
        assert!(spent_a_launch(LAUNCH_SERVICES_TIMEOUT));
        // What the worst case still leaves the page, either way.
        for took in [
            Duration::from_secs(15),
            LAUNCH_SERVICES_TIMEOUT + Duration::from_secs(3),
        ] {
            let children = if spent_a_launch(took) {
                LAUNCH_ATTEMPTS - 1
            } else {
                LAUNCH_ATTEMPTS
            };
            assert!(
                took + LAUNCH_TIMEOUT * children + PAGE_MARGIN <= OPEN_TIMEOUT,
                "{took:?}"
            );
        }
    }

    /// A quarantined bundle never approved is not handed to LaunchServices,
    /// which would wait for that approval (`open` hung 90 s on the GitHub
    /// macOS runners, where Homebrew Cask leaves Chrome so). Nothing runs:
    /// an unsigned bundle there would wait for Gatekeeper too, either way.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_quarantined_bundle_skips_launch_services() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Quarantined Browser.app");
        let exe = app.join("Contents/MacOS/Quarantined Browser");
        let ran = dir.path().join("ran");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        xattr::set(
            &app,
            "com.apple.quarantine",
            b"0181;6af5c21a;Homebrew Cask;",
        )
        .unwrap();

        let started = std::time::Instant::now();
        let open = Path::new(launch_services::OPEN);
        assert!(matches!(
            Session::launch_detached(open, LAUNCH_SERVICES_TIMEOUT, &exe, false).await,
            Detached::Skipped
        ));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(!ran.exists(), "it was started");
    }

    /// The installed Chrome starts through LaunchServices: its browser
    /// process is launchd's child, not ours. Skipped where that cannot be
    /// (no Chrome, no `ps`, a Chrome never approved); `OD_EXPECT_LAUNCH_SERVICES=1`
    /// turns a skip into a failure (ci.yml `chrome-macos`, after approving
    /// the runner's Chrome).
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_installed_chrome_starts_through_launch_services() {
        let expected = std::env::var("OD_EXPECT_LAUNCH_SERVICES").as_deref() == Ok("1");
        let skip = |why: &str| {
            assert!(!expected, "OD_EXPECT_LAUNCH_SERVICES=1 but {why}");
            eprintln!("skipping: {why}");
        };
        if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
            return skip("OD_SKIP_CHROME_TESTS=1");
        }
        let Ok(exe) = find_chrome() else {
            return skip("no Chrome");
        };
        let Some(app) = launch_services::app_bundle(&exe) else {
            return skip("Chrome is not in an .app bundle");
        };
        if launch_services::awaits_approval(&app) {
            return skip("Chrome is quarantined and was never approved");
        }
        if profile_users_checked(Path::new("/nowhere")).is_none() {
            return skip("`ps` cannot run here");
        }

        let Detached::Started(launched) = Session::launch_detached(
            Path::new(launch_services::OPEN),
            LAUNCH_SERVICES_TIMEOUT,
            &exe,
            no_sandbox_from_env(),
        )
        .await
        else {
            panic!("LaunchServices did not start Chrome within the deadline");
        };
        assert!(launched.detached);
        assert!(
            !launched.profile.path().join(OPENING).exists(),
            "a started browser's profile is still marked: a sweep would end it mid-render"
        );
        let users = profile_users(launched.profile.path());
        let ppids: Vec<String> = users
            .iter()
            .filter_map(|pid| {
                let out = std::process::Command::new("ps")
                    .args(["-o", "ppid=", "-p", &pid.to_string()])
                    .output()
                    .ok()?;
                Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
            })
            .collect();
        assert!(
            ppids.contains(&"1".to_owned()),
            "no process of launchd: {ppids:?}"
        );
        let ours = std::process::id().to_string();
        assert!(!ppids.contains(&ours), "Chrome is our child: {ppids:?}");
        drop(launched.browser);
        let profile = launched.profile;
        tokio::task::spawn_blocking(move || drop(profile))
            .await
            .unwrap();
    }

    /// A process standing in for a browser on `profile` (only its arguments
    /// matter), killed and reaped when dropped.
    #[cfg(unix)]
    struct Stand(std::process::Child);

    #[cfg(unix)]
    impl Stand {
        fn on(profile: &Path) -> Self {
            Self(
                std::process::Command::new("perl")
                    .args(["-e", "sleep 60", "--"])
                    .arg(format!("--user-data-dir={}", profile.display()))
                    .spawn()
                    .unwrap(),
            )
        }

        /// Whether it was killed within 5 s.
        fn killed(&mut self) -> bool {
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < until {
                if self.0.try_wait().unwrap().is_some() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            false
        }
    }

    #[cfg(unix)]
    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A profile marked [`OPENING`] by `writer`, the mark `age` old.
    #[cfg(unix)]
    fn marked(tmp: &Path, name: &str, writer: u32, age: Duration) -> PathBuf {
        let dir = tmp.join(format!("{PROFILE_PREFIX}{name}"));
        std::fs::create_dir(&dir).unwrap();
        let mark = dir.join(OPENING);
        std::fs::write(&mark, writer.to_string()).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&mark)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - age)
            .unwrap();
        dir
    }

    /// A pid no process has now.
    #[cfg(unix)]
    fn exited_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    /// What runs on a profile marked [`OPENING`] is a browser LaunchServices
    /// started for a launch given up on, once the process that marked it is
    /// gone (it quit, or was killed, while waiting for `open`) or the mark
    /// is older than [`ABANDONED_WATCH`]: the next render's sweep ends it.
    /// The folder goes only with an old mark: removed earlier, a browser
    /// starting later would write it back unmarked. A launch still going
    /// (its process alive, the mark new) and a profile in use are left alone.
    #[cfg(unix)]
    #[test]
    fn the_sweep_ends_what_runs_on_a_profile_given_up_on() {
        if profile_users_checked(Path::new("/nowhere")).is_none() {
            return eprintln!("skipping: `ps` cannot run here");
        }
        let tmp = tempfile::tempdir().unwrap();
        let (ours, new, old) = (
            std::process::id(),
            Duration::ZERO,
            ABANDONED_WATCH + Duration::from_secs(60),
        );
        let quit = marked(tmp.path(), "quit", exited_pid(), new);
        let opening = marked(tmp.path(), "opening", ours, new);
        let watched_out = marked(tmp.path(), "watched-out", ours, old);
        let quit_long_ago = marked(tmp.path(), "quit-long-ago", exited_pid(), old);
        // As #235 marked a profile given up on: no pid, so no process left.
        let legacy = tmp.path().join(format!("{PROFILE_PREFIX}legacy"));
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("od-abandoned"), "").unwrap();
        let in_use = tmp.path().join(format!("{PROFILE_PREFIX}in-use"));
        std::fs::create_dir(&in_use).unwrap();
        let all = [
            &quit,
            &opening,
            &watched_out,
            &quit_long_ago,
            &legacy,
            &in_use,
        ];
        let mut stands: Vec<Stand> = all.iter().map(|d| Stand::on(d)).collect();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while all.iter().any(|d| profile_users(d).is_empty()) && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }

        sweep_stale_profiles(tmp.path());

        let [quit_s, opening_s, watched_s, long_ago_s, legacy_s, in_use_s] = &mut stands[..] else {
            unreachable!()
        };
        assert!(quit_s.killed(), "the browser of a launch that quit ran on");
        assert!(quit.join(OPENING).exists(), "a new mark was removed");
        assert!(watched_s.killed(), "the browser past the watch ran on");
        assert!(!watched_out.exists(), "an old marked profile was kept");
        assert!(long_ago_s.killed() && !quit_long_ago.exists());
        assert!(legacy_s.killed(), "the browser on a #235 profile ran on");
        assert!(legacy.exists(), "a new #235 mark was removed");
        for (stand, dir) in [(opening_s, &opening), (in_use_s, &in_use)] {
            assert!(
                stand.0.try_wait().unwrap().is_none(),
                "{} was killed",
                dir.display()
            );
            assert!(dir.exists(), "{} was removed", dir.display());
        }
    }

    /// A stand-in `open` that hangs and queues a start on the profile it is
    /// given 3 s later, as LaunchServices may after `open` is gone: as
    /// (`open`, the file it writes the profile to, the file the late start
    /// touches). The queued start carries the profile in no argument until
    /// it `exec`s, so ending the profile before then cannot find it.
    #[cfg(target_os = "macos")]
    fn late_open(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let (profile_file, late) = (dir.join("profile"), dir.join("late"));
        let open = dir.join("open");
        std::fs::write(
            &open,
            format!(
                r#"#!/bin/sh
for a in "$@"; do case "$a" in --user-data-dir=*) udd="$a";; esac; done
printf '%s' "${{udd#--user-data-dir=}}" > '{profile}'
UDD="$udd" LATE='{late}' /bin/sh -c 'sleep 3; touch "$LATE"; exec /usr/bin/perl -e "sleep 120" -- "$UDD"' >/dev/null 2>&1 &
exec sleep 60
"#,
                profile = profile_file.display(),
                late = late.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        (open, profile_file, late)
    }

    /// An empty bundle `X.app` in `dir`, out of quarantine: its executable.
    #[cfg(target_os = "macos")]
    fn bundle(dir: &Path) -> PathBuf {
        let exe = dir.join("X.app/Contents/MacOS/X");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        launch_services::unquarantine(&dir.join("X.app"));
        exe
    }

    /// After the late start [`late_open`] queued came: whether a process
    /// still runs on `profile` 5 s later. Kills it either way.
    #[cfg(target_os = "macos")]
    async fn late_start_left_running(late: &Path, profile: &Path) -> bool {
        let needle = format!("--user-data-dir={}", profile.display());
        let running = |needle: &str| {
            !std::process::Command::new("pgrep")
                .args(["-f", "--", needle])
                .output()
                .unwrap()
                .stdout
                .is_empty()
        };
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !late.exists() && std::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(late.exists(), "the late start never came");
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while running(&needle) && std::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let left = running(&needle);
        let _ = std::process::Command::new("pkill")
            .args(["-9", "-f", "--", &needle])
            .status();
        left
    }

    /// LaunchServices may start the browser after `open` was killed at the
    /// deadline (its request stays queued): one that comes up then, on the
    /// profile given up on, is ended too. The stand-in `open` hangs past
    /// the 1 s deadline.
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_browser_that_launch_services_starts_late_is_ended() {
        let dir = tempfile::tempdir().unwrap();
        let exe = bundle(dir.path());
        let (open, profile_file, late) = late_open(dir.path());

        let started = std::time::Instant::now();
        match Session::launch_detached(&open, Duration::from_secs(1), &exe, false).await {
            Detached::Failed(_) => {}
            Detached::Skipped => return eprintln!("skipping: `ps` cannot run here"),
            Detached::Started(_) => panic!("a hung `open` connected"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        let profile = PathBuf::from(std::fs::read_to_string(&profile_file).unwrap());
        assert!(
            !late_start_left_running(&late, &profile).await,
            "the browser that started late was left running"
        );
        assert!(
            profile.join(OPENING).exists(),
            "the profile was not kept, marked, for the next sweep"
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// A render cancelled while it waits for `open` (a media task's Cancel,
    /// an export whose client went away) drops the launch midway: what
    /// LaunchServices starts on its profile afterwards is ended as well,
    /// and the profile stays marked for the next sweep.
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_render_cancelled_while_launch_services_opens_ends_the_late_browser() {
        if profile_users_checked(Path::new("/nowhere")).is_none() {
            return eprintln!("skipping: `ps` cannot run here");
        }
        let dir = tempfile::tempdir().unwrap();
        let exe = bundle(dir.path());
        let (open, profile_file, late) = late_open(dir.path());

        let mut launch = Box::pin(Session::launch_detached(
            &open,
            Duration::from_secs(20),
            &exe,
            false,
        ));
        // Cancelled once `open` runs, as a user would while it waits.
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !std::fs::read_to_string(&profile_file).is_ok_and(|p| !p.is_empty()) {
            assert!(std::time::Instant::now() < until, "`open` never ran");
            let step = tokio::time::timeout(Duration::from_millis(50), &mut launch).await;
            assert!(step.is_err(), "the launch ended before it was cancelled");
        }
        drop(launch);
        let profile = PathBuf::from(std::fs::read_to_string(&profile_file).unwrap());
        assert!(
            !late_start_left_running(&late, &profile).await,
            "the browser that started after the cancel was left running"
        );
        assert!(
            profile.join(OPENING).exists(),
            "the profile was not kept, marked, for the next sweep"
        );
        let _ = std::fs::remove_dir_all(&profile);
    }
}
