//! Contour desktop shell (spec 5.1, 5.3).
//!
//! Runs `od-server` inside this process on a random loopback port and points
//! the window at it, so the UI talks to the same HTTP + SSE API as the CLI.
//! No sidecar, no IPC for data: only the native folder picker, the window
//! theme and the menus go through Tauri. The window opens on a splash page
//! (`static/index.html`) while the daemon starts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// macOS only; the pure parts also build for tests everywhere.
#[cfg(any(target_os = "macos", test))]
mod js_dialogs;
mod mcp;
mod updater;
#[cfg(any(target_os = "macos", test))]
mod web_process;

use anyhow::Context;
use od_content::{ContentRoots, ContentStore};
use od_server::routes::ui::import_folder;
use od_server::{
    AppState, DaemonLock, Platform, Preview, RunContext, ServerConfig, open_runs,
    resolve_resource_root, router,
};
use od_store::{DataDir, Store};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::CapabilityBuilder;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{
    AppHandle, Manager, RunEvent, Runtime, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;

const OPEN_FOLDER_MENU_ID: &str = "open-folder";
const SHORTCUTS_MENU_ID: &str = "help-shortcuts";
const PALETTE_MENU_ID: &str = "help-palette";
const SETTINGS_MENU_ID: &str = "app-settings";
const DATA_FOLDER_MENU_ID: &str = "help-data-folder";
const DOCS_MENU_ID: &str = "help-docs";
const REPORT_MENU_ID: &str = "help-report-issue";
const UPDATES_MENU_ID: &str = "help-check-updates";
/// View ▸ Reload on macOS: the page reloads through its leave guard, or
/// the shell restarts it when it hangs the window (QA-067).
const RELOAD_MENU_ID: &str = "view-reload";
/// File ▸ Close Window on macOS: ours, so it has no ⌘W ([`menu_spec`]).
const CLOSE_WINDOW_MENU_ID: &str = "close-window";
/// Whether closing the main window only hides it: macOS, where the app
/// stays in the Dock with its runs going and the Dock icon shows it again
/// (QA-043). Elsewhere closing the window quits, as before.
const HIDE_ON_CLOSE: bool = cfg!(target_os = "macos");
/// Display name (window title, dialogs); `tauri.conf.json` `productName`.
const APP_NAME: &str = "Contour";
/// `<app> daemon …`: `od daemon start` / `status`, without the window.
const DAEMON_COMMAND: &str = "daemon";

/// `<app> mcp …`: the MCP stdio server, without the window.
const MCP_COMMAND: &str = "mcp";

/// The arguments after subcommand `name` when `od` would run `name` for
/// `args` ([`od_cli::subcommand`]: past `od`'s global flags).
fn subcommand_args<'a>(
    args: &'a [std::ffi::OsString],
    name: &str,
) -> Option<&'a [std::ffi::OsString]> {
    let command = od_cli::subcommand(args).filter(|c| *c == name)?;
    // The very argument `subcommand` returned, not an earlier flag value
    // that happens to read `name` (`--daemon-url mcp`).
    let at = args
        .iter()
        .position(|a| std::ptr::eq(a.as_os_str(), command))?;
    Some(&args[at + 1..])
}

/// The arguments for [`mcp::run`] when `args` runs `mcp`: `od`'s
/// `--daemon-url` given before `mcp`, then the arguments after it, so
/// `<app> --daemon-url URL mcp` reaches that daemon as `od` would, and a
/// `--daemon-url` after `mcp` still wins (`mcp::run` keeps the last).
/// The other global flag, `--json`, means nothing to the MCP server.
fn mcp_args(args: &[std::ffi::OsString]) -> Option<Vec<std::ffi::OsString>> {
    let rest = subcommand_args(args, MCP_COMMAND)?;
    // Before `mcp` there are only `od`'s global flags and their values.
    let mut before = args[..args.len() - rest.len() - 1].iter();
    let mut out = Vec::new();
    while let Some(arg) = before.next() {
        if arg == "--daemon-url" {
            out.push(arg.clone());
            out.extend(before.next().cloned());
        } else if arg.to_str().is_some_and(|a| a.starts_with("--daemon-url=")) {
            out.push(arg.clone());
        }
    }
    out.extend(rest.iter().cloned());
    Some(out)
}

/// The in-process daemon.
struct Daemon {
    state: AppState,
    url: Url,
    /// Released on exit ([`before_exit`]), before a restart opens the next
    /// copy, which takes it again.
    lock: Mutex<Option<DaemonLock>>,
}

/// Bundled content: `Resources/content` in a packaged app, else the usual
/// lookup (`OD_RESOURCE_ROOT`, next to the binary, source tree).
fn resource_root(app: &AppHandle) -> PathBuf {
    bundled_content(app.path().resource_dir().ok()).unwrap_or_else(resolve_resource_root)
}

/// `content/` in the app's resource folder (`Contents/Resources` on macOS,
/// next to the binary on Windows, `/usr/lib/Contour` in the Linux
/// packages), when it holds the skills.
fn bundled_content(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
    resource_dir
        .map(|d| d.join("content"))
        .filter(|d| d.join("skills").is_dir())
}

/// The content `<app> daemon start` serves: `OD_RESOURCE_ROOT` when set
/// (left to the CLI's own lookup), else the bundle's.
fn daemon_resource_root(
    env: Option<std::ffi::OsString>,
    resource_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    if env.is_some_and(|v| !v.is_empty()) {
        return None;
    }
    bundled_content(resource_dir)
}

async fn start_daemon(data_dir: DataDir, resource_root: PathBuf) -> anyhow::Result<Daemon> {
    let mut lock = DaemonLock::acquire(data_dir.root())?;
    let store = {
        let data_dir = data_dir.clone();
        tokio::task::spawn_blocking(move || Store::open(data_dir))
            .await?
            .context("opening the data store")?
    };
    let roots = ContentRoots::bundled(&resource_root).with_user_root(data_dir.root());
    let content = tokio::task::spawn_blocking(move || ContentStore::load(roots)).await?;
    let engine = od_runtime::EngineConfig::from_env().with_login_path().await;
    // Port 0: a fresh loopback port per launch, so a CLI daemon on 7456 never
    // collides on the port (the data dir lock above refuses a second daemon).
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let local = listener.local_addr()?;
    lock.record_url(&format!("http://{local}"))?;
    let runs = open_runs(&store, &lock).await?;
    let state = AppState {
        store: Arc::new(store),
        content: Arc::new(content),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![local.port()],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root,
            daemon_url: Some(format!("http://{local}")),
            mcp_daemon_url: false,
        }),
        runs: Arc::new(RunContext::new(Arc::clone(&runs), engine)),
        preview: Preview::default(),
        platform: Platform::from_env(),
    };
    let _flusher = runs.spawn_flusher();
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "daemon stopped");
        }
    });
    let url = Url::parse(&format!("http://{local}/"))?;
    tracing::info!(%url, data_dir = %data_dir.root().display(), "in-process daemon listening");
    Ok(Daemon {
        state,
        url,
        lock: Mutex::new(Some(lock)),
    })
}

/// Native folder picker for the UI's "Open folder…" button.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let path = rx.await.ok().flatten()?.into_path().ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Native window theme follows the app's (Settings ▸ Appearance), so the
/// title bar and the window chrome match: `light`, `dark`, or `system`.
#[tauri::command]
fn set_theme(window: tauri::WebviewWindow, theme: String) {
    let _ = window.set_theme(native_theme(&theme));
}

fn native_theme(theme: &str) -> Option<tauri::Theme> {
    match theme {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    }
}

/// The UI sends the menu's words in its language (QA-049).
#[tauri::command]
fn set_menu_labels(app: AppHandle, labels: MenuLabels) {
    // Sent as the app's shell shows (od-ui `Shelled`), which then takes
    // `od:menu`: View ▸ Reload may ask this page from now on.
    page_listens(&app);
    match build_menu(&app, &labels.or_english()) {
        Ok(menu) => {
            if let Err(e) = app.set_menu(menu) {
                tracing::warn!(error = %e, "cannot set the menu");
            }
        }
        Err(e) => tracing::warn!(error = %e, "cannot build the menu"),
    }
}

/// A web page for a new tab: the app's own `_blank` links and "Open in new
/// tab", or a link a design's user clicked (the preview bridge's
/// `od:open-link`, QA-054). The window opens no new window itself
/// (QA-061), so the UI asks for the system browser here. Web addresses
/// only.
#[tauri::command]
fn open_link(opened: tauri::State<'_, Arc<LastOpened>>, url: String) -> Result<(), String> {
    let url = link_to_open(&url).ok_or_else(|| "not a web address".to_owned())?;
    opened.open(&url);
    Ok(())
}

/// The UI opened another design (QA-064) or reloaded the preview
/// (QA-068): the Studio changes its route or its frame, not the page, so
/// the dialogs the user blocked would stay blocked. Dialogs show again. Nothing to do off macOS,
/// where the webview draws its own dialogs.
#[tauri::command]
fn reset_dialog_gate(window: tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    if let Err(e) = window.run_on_main_thread(js_dialogs::reset_all) {
        tracing::warn!(error = %e, "cannot reset the dialog gate");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// How long WebKit gets to say the web content process ended before
/// [`restart_page`] loads the page itself.
#[cfg(any(target_os = "macos", test))]
const RESTART_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// After [`restart_page`] loaded its page itself, WebKit silent for
/// [`RESTART_GRACE`], how long WebKit's late word that the old web content
/// process ended is still taken for that restart rather than a crash.
#[cfg(any(target_os = "macos", test))]
const LATE_END_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

/// What WebKit's word that the web content process ended calls for
/// ([`Restarts::ended`]).
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq)]
enum ProcessEnded {
    /// [`restart_page`] ended it: load the page it left.
    Load(Url),
    /// [`restart_page`] ended it, and already loaded its page when WebKit
    /// stayed silent: nothing to do.
    LateRestart,
    /// It ended by itself ([`Crashes`]).
    Crashed,
}

/// The main window's restarts ([`restart_page`]): the page to load once
/// the web content process is gone, taken when WebKit says it ended
/// ([`on_page_terminated`]) or, WebKit silent, after [`RESTART_GRACE`].
/// Nothing there (a crash) reloads the page, as Tauri does by default.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default)]
struct Restarts {
    next: Option<Url>,
    /// When the grace timer loaded the page of a restart whose process
    /// end WebKit had not reported yet.
    loaded_early: Option<std::time::Instant>,
}

#[cfg(any(target_os = "macos", test))]
impl Restarts {
    /// A restart that loads `then` began.
    fn begin(&mut self, then: Url) {
        self.next = Some(then);
        self.loaded_early = None;
    }

    /// [`RESTART_GRACE`] is over at `now`: the page to load, if WebKit has
    /// not said the process ended. `killed`: the process was asked to
    /// end, so WebKit may still say so, late.
    fn grace_over(&mut self, killed: bool, now: std::time::Instant) -> Option<Url> {
        let next = self.next.take();
        if next.is_some() && killed {
            self.loaded_early = Some(now);
        }
        next
    }

    /// WebKit says the web content process ended, at `now`.
    fn ended(&mut self, now: std::time::Instant) -> ProcessEnded {
        if let Some(url) = self.next.take() {
            self.loaded_early = None;
            return ProcessEnded::Load(url);
        }
        match self.loaded_early.take() {
            Some(at) if now.saturating_duration_since(at) < LATE_END_WINDOW => {
                ProcessEnded::LateRestart
            }
            _ => ProcessEnded::Crashed,
        }
    }
}

/// Runs `f` on the app's [`Restarts`], if it keeps any.
#[cfg(any(target_os = "macos", test))]
fn with_restarts<R: Runtime, T>(
    app: &AppHandle<R>,
    f: impl FnOnce(&mut Restarts) -> T,
) -> Option<T> {
    app.try_state::<Mutex<Restarts>>()
        .and_then(|restarts| restarts.lock().ok().map(|mut restarts| f(&mut restarts)))
}

/// The query the Studio reads on load: the shell stopped the design's
/// preview (od-ui `desktop::preview_was_stopped`).
#[cfg(any(target_os = "macos", test))]
const PREVIEW_STOPPED: (&str, &str) = ("preview", "stopped");

/// The query key that says why ([`StopReason`]; od-ui
/// `desktop::stop_reason`).
#[cfg(any(target_os = "macos", test))]
const STOP_REASON_KEY: &str = "why";

/// Why the shell stopped the design's preview, so the Studio says the
/// right thing (QA-071).
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    /// The design hung the window in a dialog loop ([`js_dialogs`]).
    Dialogs,
    /// The page did not answer View ▸ Reload and the user chose Reload.
    Hung,
    /// The web content process crashed twice in a row ([`Crashes`]).
    Crashed,
}

#[cfg(any(target_os = "macos", test))]
impl StopReason {
    /// Its value in the query.
    fn as_query(self) -> &'static str {
        match self {
            Self::Dialogs => "dialogs",
            Self::Hung => "hung",
            Self::Crashed => "crashed",
        }
    }
}

/// Where the app opens again after a design hung or crashed it (QA-067):
/// the same page with the preview stopped and why
/// (`?preview=stopped&why=…`). Present mode goes back to that design's
/// Studio, which says why and can run it again; it would only loop again.
#[cfg(any(target_os = "macos", test))]
fn stopped_preview_url(url: &Url, why: StopReason) -> Url {
    let mut url = url.clone();
    let presented = url
        .path()
        .strip_prefix("/present/")
        .and_then(|rest| rest.split('/').next())
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    if let Some(id) = presented {
        url.set_path(&format!("/projects/{id}"));
    }
    let (key, value) = PREVIEW_STOPPED;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != key && k != STOP_REASON_KEY)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair(key, value)
        .append_pair(STOP_REASON_KEY, why.as_query());
    url
}

/// Runs `task` on the main thread once the WebKit call running now has
/// returned. WebKit's delegate callbacks (a JavaScript panel, the web
/// content process ending) run on the main thread, where
/// `run_on_main_thread` runs its task at once, inside that call; and the
/// call may itself sit inside a Tauri dispatcher call: `restart_page`'s
/// `with_webview` ends the process, and WebKit says so before returning,
/// while Tauri holds the window's lock that `navigate`, `reload`, `url`
/// and `with_webview` take again (QA-070, a deadlock). A thread of its
/// own first, so the task waits for the main thread's next turn.
#[cfg(any(target_os = "macos", test))]
fn off_webkit_call<R: Runtime>(app: &AppHandle<R>, task: fn(&AppHandle<R>)) {
    let app = app.clone();
    std::thread::spawn(move || {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || task(&handle));
    });
}

/// [`js_dialogs`]'s gate saw a design hang the window in a dialog loop:
/// [`stop_hung_page`], off WebKit's call ([`off_webkit_call`]).
// WebKit callback: no Tauri dispatcher call here (QA-070).
#[cfg(any(target_os = "macos", test))]
fn on_hung<R: Runtime>(app: &AppHandle<R>) {
    off_webkit_call(app, stop_hung_page);
}

/// A design hung the window in a dialog loop ([`js_dialogs`]'s gate saw
/// it): end the web content process and open the app again on the same
/// page, the preview stopped.
#[cfg(any(target_os = "macos", test))]
fn stop_hung_page<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    match shown_url(&window) {
        Some(url) => restart_page(app, stopped_preview_url(&url, StopReason::Dialogs)),
        None => tracing::warn!("cannot tell which page hung"),
    }
}

/// How long the page gets to answer View ▸ Reload ([`reload_ack`])
/// before the shell asks the user whether to restart it.
#[cfg(target_os = "macos")]
const RELOAD_ACK_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// After Wait, how long until the shell asks again about a page that
/// still has not answered.
#[cfg(target_os = "macos")]
const RELOAD_ASK_AGAIN: std::time::Duration = std::time::Duration::from_secs(5);

/// View ▸ Reload asks the page first, so its leave guard can ask about
/// work not saved yet (a manual edit not applied); only a page that does
/// not answer is taken for hung.
#[derive(Debug, Default)]
struct ReloadAsk {
    last: u64,
    open: Option<u64>,
    /// The page shown takes `od:menu` (its shell said so, through
    /// [`set_menu_labels`]); the splash, and the app before its shell
    /// shows, do not.
    listening: bool,
}

impl ReloadAsk {
    /// A new question; its token. `None` when the page shown does not
    /// take `od:menu`: it could never answer, so nothing is asked, and the
    /// shell reloads it itself ([`reload_unheard`]).
    fn ask(&mut self) -> Option<u64> {
        if !self.listening {
            return None;
        }
        self.last = self.last.wrapping_add(1);
        self.open = Some(self.last);
        Some(self.last)
    }

    /// The page shown takes `od:menu` from now on.
    fn listens(&mut self) {
        self.listening = true;
    }

    /// The page answered: it reloads, or asks the user, by itself.
    fn answered(&mut self) {
        self.open = None;
    }

    /// A page started loading (its own reload, a navigation, a restart):
    /// the question was for the page before, which is gone, so nothing
    /// is left to ask about it.
    fn page_loaded(&mut self) {
        self.open = None;
        self.listening = false;
    }

    /// Whether question `token` is still open: the page has not answered
    /// it, and no newer question replaced it.
    #[cfg(any(target_os = "macos", test))]
    fn is_open(&self, token: u64) -> bool {
        self.open == Some(token)
    }

    /// Whether question `token` is still open (then closed now): the
    /// page did not answer it. Only macOS restarts a page that does not.
    #[cfg(any(target_os = "macos", test))]
    fn unanswered(&mut self, token: u64) -> bool {
        let open = self.open == Some(token);
        if open {
            self.open = None;
        }
        open
    }
}

/// The page took View ▸ Reload (`od:menu` `reload`): it reloads itself,
/// after its leave guard asks if work would be lost.
#[tauri::command]
fn reload_ack(asks: tauri::State<'_, Mutex<ReloadAsk>>) {
    if let Ok(mut asks) = asks.lock() {
        asks.answered();
    }
}

/// A page started loading in the main window (its own reload, a
/// navigation, a restart): no Reload question is left about the page
/// gone, and the new one takes no `od:menu` until its shell says so.
fn page_started<R: Runtime>(app: &impl Manager<R>) {
    if let Some(asks) = app.try_state::<Mutex<ReloadAsk>>()
        && let Ok(mut asks) = asks.lock()
    {
        asks.page_loaded();
    }
}

/// The page shown takes `od:menu` ([`set_menu_labels`]).
fn page_listens<R: Runtime>(app: &impl Manager<R>) {
    if let Some(asks) = app.try_state::<Mutex<ReloadAsk>>()
        && let Ok(mut asks) = asks.lock()
    {
        asks.listens();
    }
}

/// The last page the main window started or finished loading
/// (`on_page_load`), kept aside: once its web content process ended,
/// WebKit has no address to give ([`shown_url`], QA-073).
#[derive(Debug, Default)]
struct LoadedPage(Mutex<Option<Url>>);

/// The main window started or finished loading `url` ([`LoadedPage`]).
fn page_loaded_at<R: Runtime>(app: &impl Manager<R>, url: &Url) {
    if let Some(page) = app.try_state::<LoadedPage>()
        && let Ok(mut page) = page.0.lock()
    {
        *page = Some(url.clone());
    }
}

/// The page `window` shows: WebKit's own address while its web content
/// process lives, else the last page it loaded ([`LoadedPage`]). Never
/// wry's `url()` on macOS: once the process ended WebKit's address is
/// nil, and wry unwraps it and panics the app (QA-073). A dispatcher
/// call: on the main thread, never inside a WebKit call (QA-070).
fn shown_url<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<Url> {
    #[cfg(target_os = "macos")]
    let live = {
        let found = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&found);
        if let Err(e) = window.with_webview(move |webview| {
            let url = web_process::page_url(webview.inner()).and_then(|u| Url::parse(&u).ok());
            if let Ok(mut slot) = slot.lock() {
                *slot = url;
            }
        }) {
            tracing::warn!(error = %e, "cannot reach the webview");
        }
        found.lock().ok().and_then(|mut found| found.take())
    };
    #[cfg(not(target_os = "macos"))]
    // url() allowed: WebView2 and WebKitGTK return an error, never panic.
    let live = window.url().ok();
    live.or_else(|| {
        window
            .try_state::<LoadedPage>()
            .and_then(|page| page.0.lock().ok().and_then(|page| page.clone()))
    })
}

/// The main window's `on_page_load` for a page that started loading: the
/// Reload question ([`page_started`]) and, on macOS, the dialogs the user
/// blocked on the page gone show again (QA-064).
// WebKit callback: no Tauri dispatcher call here (QA-070).
fn on_page_started<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    page_started(window);
    #[cfg(target_os = "macos")]
    js_dialogs::page_loaded_main();
}

/// QA's test hook for the crash loop (docs/qa/test-plan.md): set to `1`,
/// each load of a design's page with its preview running ends the window's
/// own web content process, as a design that crashes it would. Only a debug
/// build or a QA copy (bundle id ending `.qa`) reads it
/// ([`qa_crash_web_enabled`]).
#[cfg(target_os = "macos")]
const QA_CRASH_WEB_ENV: &str = "OD_QA_CRASH_WEB";

/// Whether the crash-loop test hook is on: [`QA_CRASH_WEB_ENV`] is `1`
/// (`value`), in a debug build (`debug`) or a copy whose bundle id
/// (`bundle_id`) ends `.qa`. A release Contour never crashes itself,
/// whatever its environment.
#[cfg(any(target_os = "macos", test))]
fn qa_crash_web_enabled(value: Option<&str>, debug: bool, bundle_id: Option<&str>) -> bool {
    value == Some("1") && (debug || bundle_id.is_some_and(|id| id.ends_with(".qa")))
}

/// Whether the crash-loop test hook ends the web content process for
/// `url`, a page that finished loading: a design's Studio or Present page
/// whose preview runs. Not once the shell stopped the preview
/// ([`stopped_preview_url`]), so the loop ends where a real one would.
#[cfg(any(target_os = "macos", test))]
fn qa_crashes_page(url: &Url) -> bool {
    let design = ["/projects/", "/present/"].iter().any(|prefix| {
        url.path()
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty())
    });
    let (key, value) = PREVIEW_STOPPED;
    let stopped = url.query_pairs().any(|(k, v)| k == key && v == value);
    design && !stopped
}

/// The main window finished loading `url`: with the crash-loop test hook
/// on ([`qa_crash_web_enabled`]) and a design's preview running there,
/// its web content process ends, off WebKit's call.
// WebKit callback: no Tauri dispatcher call here (QA-070).
#[cfg(target_os = "macos")]
fn qa_crash_web<R: Runtime>(window: &tauri::WebviewWindow<R>, url: &Url) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    let enabled = *ENABLED.get_or_init(|| {
        let value = std::env::var(QA_CRASH_WEB_ENV).ok();
        let bundle_id = web_process::bundle_id();
        let on = qa_crash_web_enabled(
            value.as_deref(),
            cfg!(debug_assertions),
            bundle_id.as_deref(),
        );
        if on {
            tracing::warn!("QA test hook: a design's page crashes its web content process");
        }
        on
    });
    if !enabled || !qa_crashes_page(url) {
        return;
    }
    off_webkit_call(window.app_handle(), |app| {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        if let Err(e) = window.with_webview(|webview| {
            if web_process::kill(webview.inner()) {
                tracing::warn!("QA test hook: ended the web content process");
            } else {
                tracing::warn!("QA test hook: this WebKit cannot end the web content process");
            }
        }) {
            tracing::warn!(error = %e, "cannot reach the webview");
        }
    });
}

/// The splash: the bundled page shown until the daemon listens.
fn is_splash(url: &Url) -> bool {
    url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost")
}

/// Where View ▸ Reload goes on a page that does not take `od:menu`
/// (`shown`): the splash opens the app once the daemon listens (`daemon`),
/// so a reload racing the daemon's own `navigate` does not leave the
/// window on the splash; `None` reloads the page shown.
fn unheard_reload_target(shown: &Url, daemon: Option<&Url>) -> Option<Url> {
    if is_splash(shown) {
        daemon.cloned()
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
use web_process::NotRespondingChoice;

/// What the shell does about a View ▸ Reload the page has not answered.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotAnswered {
    /// End the web content process; open the page with the design's
    /// preview stopped (the design is what hangs it).
    Restart,
    /// Ask again after [`RELOAD_ASK_AGAIN`].
    AskAgainLater,
    /// The page answered after all, or a newer Reload took over.
    Nothing,
}

/// The step after asking the user (`choice`; `None` when no question
/// could show, a dialog being up), the question still `open` or not. A
/// page that is only busy may answer while the user decides: then nothing
/// happens, whatever the choice (the alert closes itself then, QA-072,
/// and a Reload pressed just before is a no-op); Reload restarts only a
/// page still silent.
#[cfg(any(target_os = "macos", test))]
fn after_not_answering(open: bool, choice: Option<NotRespondingChoice>) -> NotAnswered {
    match (open, choice) {
        (false, _) => NotAnswered::Nothing,
        (true, Some(NotRespondingChoice::Reload)) => NotAnswered::Restart,
        (true, Some(NotRespondingChoice::Wait | NotRespondingChoice::Closed) | None) => {
            NotAnswered::AskAgainLater
        }
    }
}

/// View ▸ Reload (⌘R): the page reloads itself through its leave guard
/// (`od:menu` `reload`). A page that does not answer within
/// [`RELOAD_ACK_WAIT`] may be busy or hung: the shell asks the user, from
/// the native side, whether to wait or restart it ([`check_reload`]).
fn reload_from_menu(app: &AppHandle) {
    let Some(asked) = app
        .try_state::<Mutex<ReloadAsk>>()
        .and_then(|asks| asks.lock().ok().map(|mut asks| asks.ask()))
    else {
        return;
    };
    let Some(token) = asked else {
        reload_unheard(app);
        return;
    };
    ui_action(app, "reload");
    #[cfg(target_os = "macos")]
    check_reload(app.clone(), token, RELOAD_ACK_WAIT);
    #[cfg(not(target_os = "macos"))]
    let _ = token;
}

/// View ▸ Reload on a page that does not take `od:menu` (the splash, the
/// app before its shell shows): nothing to ask it, nor any work to lose,
/// so the shell loads it again at once ([`unheard_reload_target`]).
fn reload_unheard(app: &AppHandle) {
    show_main(app);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let daemon = app.try_state::<Daemon>().map(|daemon| daemon.url.clone());
    let target =
        shown_url(&window).and_then(|shown| unheard_reload_target(&shown, daemon.as_ref()));
    let loaded = match target {
        Some(url) => window.navigate(url),
        None => window.reload(),
    };
    if let Err(e) = loaded {
        tracing::warn!(error = %e, "cannot reload the page");
    }
}

/// After `wait`, if the page has still not answered Reload question
/// `token`, asks the user "This page isn't responding": Wait asks again
/// later, Reload ends its web content process and opens the page with the
/// preview stopped, so a design that hangs it does not run again.
#[cfg(target_os = "macos")]
fn check_reload(app: AppHandle, token: u64, wait: std::time::Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let open = move |app: &AppHandle| {
                app.try_state::<Mutex<ReloadAsk>>()
                    .and_then(|asks| asks.lock().ok().map(|asks| asks.is_open(token)))
                    .unwrap_or(false)
            };
            if !open(&handle) {
                return;
            }
            let asked = handle.clone();
            let choice = web_process::ask_not_responding(move || open(&asked));
            match after_not_answering(open(&handle), choice) {
                NotAnswered::Nothing => {
                    if choice == Some(NotRespondingChoice::Reload) {
                        tracing::info!("the page answered Reload first: not restarting it");
                    }
                }
                NotAnswered::AskAgainLater => check_reload(handle, token, RELOAD_ASK_AGAIN),
                NotAnswered::Restart => {
                    if let Some(asks) = handle.try_state::<Mutex<ReloadAsk>>()
                        && let Ok(mut asks) = asks.lock()
                    {
                        asks.unanswered(token);
                    }
                    tracing::warn!("the page did not answer Reload: restarting it");
                    if let Some(window) = handle.get_webview_window("main") {
                        match shown_url(&window) {
                            Some(url) => {
                                restart_page(&handle, stopped_preview_url(&url, StopReason::Hung))
                            }
                            None => tracing::warn!("cannot tell which page hung"),
                        }
                    }
                }
            }
        });
    });
}

/// Web content crashes within this long of each other make a loop: a
/// design that crashes the process as it loads would reload for ever.
#[cfg(any(target_os = "macos", test))]
const CRASH_LOOP_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// What a crash of the web content process does ([`Crashes`]).
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterCrash {
    /// Reload the page shown.
    Reload,
    /// The second crash in a row: open the page with the design's preview
    /// stopped ([`stopped_preview_url`]).
    StopPreview,
    /// Crashing even so: leave the window as it is (⌘R, ⌘Q still work).
    GiveUp,
}

/// Recent crashes of the web content process, [`CRASH_LOOP_WINDOW`] apart
/// at most.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default)]
struct Crashes {
    last: Option<std::time::Instant>,
    in_a_row: u32,
}

#[cfg(any(target_os = "macos", test))]
impl Crashes {
    /// The web content process crashed at `now`.
    fn crashed(&mut self, now: std::time::Instant) -> AfterCrash {
        let again = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < CRASH_LOOP_WINDOW);
        self.in_a_row = if again {
            self.in_a_row.saturating_add(1)
        } else {
            1
        };
        self.last = Some(now);
        match self.in_a_row {
            1 => AfterCrash::Reload,
            2 => AfterCrash::StopPreview,
            _ => AfterCrash::GiveUp,
        }
    }
}

/// Ends the main window's web content process and loads `then` in a new
/// one ([`on_page_terminated`]). On the main thread.
#[cfg(any(target_os = "macos", test))]
fn restart_page<R: Runtime>(app: &AppHandle<R>, then: Url) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // The page is replaced: no Reload question is left about it.
    page_started(app);
    with_restarts(app, |restarts| restarts.begin(then));
    let killed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&killed);
    if let Err(e) = window.with_webview(move |webview| {
        #[cfg(target_os = "macos")]
        flag.store(web_process::kill(webview.inner()), Ordering::SeqCst);
        #[cfg(not(target_os = "macos"))]
        let _ = (flag, webview);
    }) {
        tracing::warn!(error = %e, "cannot reach the webview");
    }
    // WebKit may not say the process ended (or this WebKit cannot end
    // it): load the page anyway.
    let killed = killed.load(Ordering::SeqCst);
    let wait = if killed {
        RESTART_GRACE
    } else {
        std::time::Duration::ZERO
    };
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || load_after_grace(&app, killed));
    });
}

/// [`RESTART_GRACE`] is over: loads the page [`restart_page`] left, if
/// WebKit has not said the process ended (which loads it).
#[cfg(any(target_os = "macos", test))]
fn load_after_grace<R: Runtime>(app: &AppHandle<R>, killed: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let now = std::time::Instant::now();
    if let Some(url) = with_restarts(app, |r| r.grace_over(killed, now)).flatten()
        && let Err(e) = window.navigate(url)
    {
        tracing::warn!(error = %e, "cannot load the app again");
    }
}

/// `on_web_content_process_terminate`: a page's dialog or file panel
/// still up closes (nobody waits for it), then [`on_page_terminated`].
/// WebKit calls it inside [`restart_page`]'s `with_webview`, which holds
/// the window's lock (QA-070): only state kept aside
/// ([`js_dialogs::process_ended_main`]), never Tauri's dispatcher.
// WebKit callback: no Tauri dispatcher call here (QA-070).
#[cfg(target_os = "macos")]
fn on_web_process_ended<R: Runtime>(webview: &tauri::Webview<R>) {
    js_dialogs::process_ended_main();
    on_page_terminated(webview.app_handle());
}

/// WebKit says the main window's web content process ended
/// ([`page_terminated`]), off WebKit's call ([`off_webkit_call`]): when
/// [`restart_page`] ended it, WebKit says so inside its `with_webview`.
// WebKit callback: no Tauri dispatcher call here (QA-070).
#[cfg(any(target_os = "macos", test))]
fn on_page_terminated<R: Runtime>(app: &AppHandle<R>) {
    off_webkit_call(app, page_terminated);
}

/// The main window's web content process ended: loads the page
/// [`restart_page`] left; or nothing, its page loaded already; or, a
/// crash, reloads the page shown ([`Crashes`]).
#[cfg(any(target_os = "macos", test))]
fn page_terminated<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let now = std::time::Instant::now();
    let ended = with_restarts(app, |r| r.ended(now)).unwrap_or(ProcessEnded::Crashed);
    let loaded = match ended {
        ProcessEnded::Load(url) => window.navigate(url),
        ProcessEnded::LateRestart => {
            tracing::info!("the web content process ended after its restart");
            return;
        }
        ProcessEnded::Crashed => {
            let after = app
                .try_state::<Mutex<Crashes>>()
                .and_then(|c| c.lock().ok().map(|mut c| c.crashed(now)))
                .unwrap_or(AfterCrash::Reload);
            tracing::warn!(?after, "the web content process ended by itself");
            match after {
                AfterCrash::Reload => window.reload(),
                // The process is gone: never wry's `url()` (QA-073).
                AfterCrash::StopPreview => match shown_url(&window) {
                    Some(url) => window.navigate(stopped_preview_url(&url, StopReason::Crashed)),
                    None => {
                        tracing::warn!("cannot tell which page crashed");
                        return;
                    }
                },
                AfterCrash::GiveUp => return,
            }
        }
    };
    if let Err(e) = loaded {
        tracing::warn!(error = %e, "cannot load the app again");
    }
}

/// The last page handed to the system browser and when. One click can ask
/// twice for the same page (a `_blank` link whose click handler also calls
/// `window.open`, which the preview bridge passes on too); the second
/// opening of the same page within [`SAME_OPEN_WINDOW`] is dropped, so one
/// click opens one tab.
#[derive(Default)]
struct LastOpened(Mutex<Option<(Url, std::time::Instant)>>);

const SAME_OPEN_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

impl LastOpened {
    /// Opens `url` in the browser unless it was just opened.
    fn open(&self, url: &Url) {
        let fresh = match self.0.lock() {
            Ok(mut last) => first_opening(&mut last, url, std::time::Instant::now()),
            Err(_) => true,
        };
        if fresh {
            open_in_browser(url);
        }
    }
}

/// Whether opening `url` at `now` is not a repeat of `last`; records it.
fn first_opening(
    last: &mut Option<(Url, std::time::Instant)>,
    url: &Url,
    now: std::time::Instant,
) -> bool {
    let repeat = last
        .as_ref()
        .is_some_and(|(u, at)| u == url && now.saturating_duration_since(*at) < SAME_OPEN_WINDOW);
    if !repeat {
        *last = Some((url.clone(), now));
    }
    !repeat
}

fn link_to_open(url: &str) -> Option<Url> {
    Url::parse(url).ok().filter(opens_in_browser)
}

/// Export in the app: the native Save dialog for the file the page made
/// (request body, name in header `x-file-name`, URI-encoded), so the user
/// picks the place and the toast names the file really written (QA-048).
/// `None` when cancelled. The path comes from the dialog, never the page.
#[tauri::command]
async fn save_file(
    app: AppHandle,
    request: tauri::ipc::Request<'_>,
) -> Result<Option<String>, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the file's bytes".to_owned());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .map(|n| file_name_only(&n))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "export".to_owned());
    let mut dialog = app.dialog().file().set_file_name(&name);
    if let Ok(downloads) = app.path().download_dir() {
        dialog = dialog.set_directory(downloads);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.save_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// `encodeURIComponent`'s inverse; a bad escape stays as it is.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The last path component: a suggested name, not a place.
fn file_name_only(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Shows the main window again (Dock icon, a menu action while hidden).
fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The app's event loop:
/// - the main window's close button (and File ▸ Close Window) on macOS
///   hides the window; the app and its runs keep going ([`HIDE_ON_CLOSE`]).
///   Quit (⌘Q) does not come through here.
/// - the Dock icon shows it again.
/// - on exit, running agents and the terminal tabs' shells, which would
///   outlive the app, stop ([`before_exit`]).
fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    match event {
        RunEvent::Exit => before_exit(app),
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if HIDE_ON_CLOSE && label == "main" => {
            api.prevent_close();
            if let Some(window) = app.get_webview_window(label) {
                hide_leaving_fullscreen(window);
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => show_main(app),
        _ => {}
    }
}

/// Hides `window`, out of full screen first: a full-screen window hidden
/// as it is leaves its Space behind, black, and the Dock brings it back
/// into that Space (the original's `hideWindowExitingFullscreen`). Tauri
/// has no "left full screen" event: wait for the flag to drop, then for
/// the animation to end, off the event loop.
fn hide_leaving_fullscreen<R: Runtime>(window: tauri::WebviewWindow<R>) {
    if !window.is_fullscreen().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let _ = window.set_fullscreen(false);
    std::thread::spawn(move || {
        let step = std::time::Duration::from_millis(100);
        for _ in 0..30 {
            if !window.is_fullscreen().unwrap_or(false) {
                break;
            }
            std::thread::sleep(step);
        }
        // macOS's leave-full-screen animation outlasts the flag.
        std::thread::sleep(std::time::Duration::from_millis(FULLSCREEN_SETTLE_MS));
        let _ = window.hide();
    });
}

/// How long the window gets after leaving full screen before it hides.
const FULLSCREEN_SETTLE_MS: u64 = 800;

/// Menu actions the UI performs: `window` event `od:menu` with `detail`.
fn ui_action(app: &AppHandle, action: &str) {
    show_main(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval(format!(
            "window.dispatchEvent(new CustomEvent('od:menu', {{ detail: {action:?} }}))"
        ));
    }
}

/// File ▸ Open Folder…: import the folder and open its Studio.
fn open_folder_from_menu(app: &AppHandle) {
    let handle = app.clone();
    app.dialog().file().pick_folder(move |picked| {
        let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
            return;
        };
        let Some(daemon) = handle.try_state::<Daemon>() else {
            return;
        };
        let target = match import_folder(&daemon.state.store, &path, None) {
            Ok((project, _)) => daemon.url.join(&format!("projects/{}", project.id)),
            Err(e) => {
                handle
                    .dialog()
                    .message(format!("Could not open {}: {e}", path.display()))
                    .title("Open Folder")
                    .show(|_| {});
                return;
            }
        };
        if let (Ok(url), Some(window)) = (target, handle.get_webview_window("main")) {
            show_main(&handle);
            let _ = window.navigate(url);
        }
    });
}

/// IPC (the folder picker) for the main window, only while it shows the
/// daemon's exact origin. The IPC origin is the requesting document's
/// `Origin`, so sandboxed preview iframes (`Origin: null`) and pages of
/// other loopback ports never match. Replaces a static
/// `http://127.0.0.1:*` capability, which every local server shared.
/// The app's config, assets and ACL (`build.rs`), for any runtime: one
/// expansion, as it embeds `Info.plist` on macOS (tests use the mock runtime).
fn app_context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

fn ui_capability(origin: &str) -> CapabilityBuilder {
    let capability = CapabilityBuilder::new("daemon-ui")
        .window("main")
        .local(false)
        .remote(format!("{origin}/*"))
        .permission("core:default")
        .permission("core:window:allow-start-dragging")
        .permission("dialog:default");
    APP_COMMAND_PERMISSIONS
        .iter()
        .fold(capability, |c, p| c.permission(*p))
}

/// `allow-<command>` for each app command (`build.rs` generates them): the
/// daemon's page is a remote origin to Tauri, which refuses it any app
/// command without one.
const APP_COMMAND_PERMISSIONS: [&str; 11] = [
    "allow-pick-folder",
    "allow-set-theme",
    "allow-set-menu-labels",
    "allow-save-file",
    "allow-update-support",
    "allow-install-update",
    "allow-cancel-update",
    "allow-restart-app",
    "allow-open-link",
    "allow-reset-dialog-gate",
    "allow-reload-ack",
];

/// The main window stays on the daemon's origin (and the bundled splash
/// shown before it); anything else (a link in a design, a redirect) is
/// refused rather than loaded with the app's IPC. A frame's own document
/// (`about:srcdoc`, `about:blank`) never leaves the app and keeps the
/// preview's CSP, so a design's `<iframe srcdoc>` loads (QA-062).
fn allowed_navigation(url: &Url, origin: Option<&str>) -> bool {
    let splash = is_splash(url);
    let frame_document = url.scheme() == "about" && matches!(url.path(), "srcdoc" | "blank");
    splash || frame_document || origin.is_some_and(|o| url.origin().ascii_serialization() == o)
}

/// A page link meant for a new window (`target="_blank"`, `window.open`):
/// only web addresses leave the app, for the system browser.
fn opens_in_browser(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// Hands `url` to the system opener (`open`, `xdg-open`, `explorer`).
fn open_in_browser(url: &Url) {
    let program = od_server::routes::handoff::opener();
    match std::process::Command::new(&program)
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        // Reaped off the UI thread: the opener may outlive the call.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => tracing::warn!(%url, error = %e, %program, "cannot open the link"),
    }
}

/// The native menu's words. English unless the UI sends its language's
/// (`set_menu_labels`, after Settings ▸ Language); a label it leaves out
/// stays English.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MenuLabels {
    about: String,
    services: String,
    hide: String,
    hide_others: String,
    show_all: String,
    quit: String,
    settings: String,
    file: String,
    close_window: String,
    edit: String,
    undo: String,
    redo: String,
    cut: String,
    copy: String,
    paste: String,
    select_all: String,
    view: String,
    reload: String,
    fullscreen: String,
    project: String,
    open_folder: String,
    window: String,
    minimize: String,
    maximize: String,
    help: String,
    documentation: String,
    check_updates: String,
    report_issue: String,
    data_folder: String,
    shortcuts: String,
    palette: String,
}

impl Default for MenuLabels {
    fn default() -> Self {
        let s = str::to_owned;
        Self {
            about: format!("About {APP_NAME}"),
            services: s("Services"),
            hide: format!("Hide {APP_NAME}"),
            hide_others: s("Hide Others"),
            show_all: s("Show All"),
            quit: format!("Quit {APP_NAME}"),
            settings: s("Settings…"),
            file: s("File"),
            close_window: s("Close Window"),
            edit: s("Edit"),
            undo: s("Undo"),
            redo: s("Redo"),
            cut: s("Cut"),
            copy: s("Copy"),
            paste: s("Paste"),
            select_all: s("Select All"),
            view: s("View"),
            reload: s("Reload"),
            fullscreen: s("Toggle Full Screen"),
            project: s("Project"),
            open_folder: s("Open Folder…"),
            window: s("Window"),
            minimize: s("Minimize"),
            maximize: s("Zoom"),
            help: s("Help"),
            documentation: s("Documentation"),
            check_updates: s("Check for Updates…"),
            report_issue: s("Report Issue"),
            data_folder: s("Show Data Folder"),
            shortcuts: s("Keyboard Shortcuts"),
            palette: s("Command Palette…"),
        }
    }
}

impl MenuLabels {
    /// Each label the UI left empty takes the English one.
    fn or_english(mut self) -> Self {
        let en = Self::default();
        for (mine, english) in self.fields_mut().into_iter().zip(en.into_fields()) {
            if mine.trim().is_empty() {
                *mine = english;
            }
        }
        self
    }

    fn fields_mut(&mut self) -> [&mut String; 31] {
        [
            &mut self.about,
            &mut self.services,
            &mut self.hide,
            &mut self.hide_others,
            &mut self.show_all,
            &mut self.quit,
            &mut self.settings,
            &mut self.file,
            &mut self.close_window,
            &mut self.edit,
            &mut self.undo,
            &mut self.redo,
            &mut self.cut,
            &mut self.copy,
            &mut self.paste,
            &mut self.select_all,
            &mut self.view,
            &mut self.reload,
            &mut self.fullscreen,
            &mut self.project,
            &mut self.open_folder,
            &mut self.window,
            &mut self.minimize,
            &mut self.maximize,
            &mut self.help,
            &mut self.documentation,
            &mut self.check_updates,
            &mut self.report_issue,
            &mut self.data_folder,
            &mut self.shortcuts,
            &mut self.palette,
        ]
    }

    fn into_fields(mut self) -> [String; 31] {
        self.fields_mut().map(std::mem::take)
    }
}

/// A native item the OS carries out (Copy, Hide, Minimize…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Native {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    CloseWindow,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    Maximize,
}

/// One row of a menu: ours (an id for `on_menu_event`), the OS's, or a line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Item {
        id: &'static str,
        text: String,
        accelerator: Option<&'static str>,
    },
    Native(Native, String),
    Separator,
}

/// A top-level menu: its title and rows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    /// Tauri's id for Window and Help, which macOS treats as such.
    id: Option<&'static str>,
    title: String,
    entries: Vec<Entry>,
}

/// The menu bar for `os` (`std::env::consts::OS`), in `l`'s words: the
/// layout of Tauri's default menu plus Project, Settings and Help's items.
///
/// On macOS, Close Window has no ⌘W: the Studio closes a tab on ⌘W (the
/// shortcuts dialog), and the menu's key took it first and closed the last
/// window, which quit the app with its runs (QA-042). The original's
/// Electron menu has no ⌘W either. The red button still hides the window.
fn menu_spec(l: &MenuLabels, os: &str) -> Vec<Section> {
    let mac = os == "macos";
    let item = |id, text: &String, accelerator| Entry::Item {
        id,
        text: text.clone(),
        accelerator,
    };
    let native = |n, text: &String| Entry::Native(n, text.clone());
    let close_window = if mac {
        item(CLOSE_WINDOW_MENU_ID, &l.close_window, None)
    } else {
        // Alt+F4 elsewhere, which no page shortcut uses.
        native(Native::CloseWindow, &l.close_window)
    };
    let mut bar = Vec::new();
    if mac {
        bar.push(Section {
            id: None,
            title: APP_NAME.to_owned(),
            entries: vec![
                native(Native::About, &l.about),
                Entry::Separator,
                item(SETTINGS_MENU_ID, &l.settings, Some("CmdOrCtrl+,")),
                Entry::Separator,
                native(Native::Services, &l.services),
                Entry::Separator,
                native(Native::Hide, &l.hide),
                native(Native::HideOthers, &l.hide_others),
                native(Native::ShowAll, &l.show_all),
                Entry::Separator,
                native(Native::Quit, &l.quit),
            ],
        });
    }
    // Linux has no File menu in Tauri's default either.
    if os != "linux" {
        let mut entries = vec![close_window.clone()];
        if !mac {
            entries.push(native(Native::Quit, &l.quit));
        }
        bar.push(Section {
            id: None,
            title: l.file.clone(),
            entries,
        });
    }
    bar.push(Section {
        id: None,
        title: l.edit.clone(),
        entries: vec![
            native(Native::Undo, &l.undo),
            native(Native::Redo, &l.redo),
            Entry::Separator,
            native(Native::Cut, &l.cut),
            native(Native::Copy, &l.copy),
            native(Native::Paste, &l.paste),
            native(Native::SelectAll, &l.select_all),
        ],
    });
    if mac {
        bar.push(Section {
            id: None,
            title: l.view.clone(),
            entries: vec![
                item(RELOAD_MENU_ID, &l.reload, Some("CmdOrCtrl+R")),
                Entry::Separator,
                native(Native::Fullscreen, &l.fullscreen),
            ],
        });
    }
    let mut project = vec![item(
        OPEN_FOLDER_MENU_ID,
        &l.open_folder,
        Some("CmdOrCtrl+O"),
    )];
    if !mac {
        project.push(item(SETTINGS_MENU_ID, &l.settings, Some("CmdOrCtrl+,")));
    }
    bar.push(Section {
        id: None,
        title: l.project.clone(),
        entries: project,
    });
    let mut window = vec![
        native(Native::Minimize, &l.minimize),
        native(Native::Maximize, &l.maximize),
    ];
    if mac {
        window.push(Entry::Separator);
    }
    window.push(close_window);
    bar.push(Section {
        id: Some(tauri::menu::WINDOW_SUBMENU_ID),
        title: l.window.clone(),
        entries: window,
    });
    // Documentation opens the guide bundled in the UI; Report Issue copies
    // a diagnostics summary and opens the log folder (no network); Check
    // for Updates asks GitHub Releases, only when chosen.
    let mut help = Vec::new();
    if !mac {
        help.extend([native(Native::About, &l.about), Entry::Separator]);
    }
    help.extend([
        item(DOCS_MENU_ID, &l.documentation, None),
        Entry::Separator,
        item(UPDATES_MENU_ID, &l.check_updates, None),
        item(REPORT_MENU_ID, &l.report_issue, None),
        item(DATA_FOLDER_MENU_ID, &l.data_folder, None),
        item(SHORTCUTS_MENU_ID, &l.shortcuts, Some("CmdOrCtrl+/")),
        item(PALETTE_MENU_ID, &l.palette, None),
    ]);
    bar.push(Section {
        id: Some(tauri::menu::HELP_SUBMENU_ID),
        title: l.help.clone(),
        entries: help,
    });
    bar
}

/// The menu bar of [`menu_spec`] for this OS.
fn build_menu<R: Runtime>(app: &AppHandle<R>, labels: &MenuLabels) -> tauri::Result<Menu<R>> {
    use tauri::menu::{AboutMetadata, IsMenuItem, MenuItemKind, PredefinedMenuItem as P};
    let pkg = app.package_info();
    let about = AboutMetadata {
        name: Some(APP_NAME.to_owned()),
        version: Some(pkg.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        authors: app.config().bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };
    let menu = Menu::new(app)?;
    for section in menu_spec(labels, std::env::consts::OS) {
        let mut items: Vec<MenuItemKind<R>> = Vec::new();
        for entry in section.entries {
            let item = match entry {
                Entry::Separator => MenuItemKind::Predefined(P::separator(app)?),
                Entry::Item {
                    id,
                    text,
                    accelerator,
                } => MenuItemKind::MenuItem(MenuItem::with_id(app, id, text, true, accelerator)?),
                Entry::Native(native, text) => {
                    let t = Some(text.as_str());
                    MenuItemKind::Predefined(match native {
                        Native::About => P::about(app, t, Some(about.clone()))?,
                        Native::Services => P::services(app, t)?,
                        Native::Hide => P::hide(app, t)?,
                        Native::HideOthers => P::hide_others(app, t)?,
                        Native::ShowAll => P::show_all(app, t)?,
                        Native::Quit => P::quit(app, t)?,
                        Native::CloseWindow => P::close_window(app, t)?,
                        Native::Undo => P::undo(app, t)?,
                        Native::Redo => P::redo(app, t)?,
                        Native::Cut => P::cut(app, t)?,
                        Native::Copy => P::copy(app, t)?,
                        Native::Paste => P::paste(app, t)?,
                        Native::SelectAll => P::select_all(app, t)?,
                        Native::Fullscreen => P::fullscreen(app, t)?,
                        Native::Minimize => P::minimize(app, t)?,
                        Native::Maximize => P::maximize(app, t)?,
                    })
                }
            };
            items.push(item);
        }
        let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
        // macOS lists the open windows in the menu with Window's id, and
        // puts its search field in Help's.
        let submenu = match section.id {
            Some(id) => Submenu::with_id_and_items(app, id, &section.title, true, &refs)?,
            None => Submenu::with_items(app, &section.title, true, &refs)?,
        };
        menu.append(&submenu)?;
    }
    Ok(menu)
}

/// Help ▸ Show Data Folder: the data dir in the OS file manager.
fn show_data_folder(app: &AppHandle) {
    let Some(daemon) = app.try_state::<Daemon>() else {
        return;
    };
    let dir = daemon.state.store.data_dir().root().to_path_buf();
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(&dir).spawn() {
        tracing::warn!(error = %e, dir = %dir.display(), "could not show the data folder");
    }
}

fn main() -> anyhow::Result<()> {
    // `<app> mcp`: the stdio server an agent starts (Integrations ▸ Install),
    // without the window, the daemon or the app's log in `<data>/logs/`.
    // `"$OD_NODE_BIN" "$OD_BIN" media …` from an agent: both variables
    // name this binary (it is the `od` of the app's runs).
    let all = od_cli::strip_self_arg(std::env::args_os().collect());
    let args = &all[1.min(all.len())..];
    // `od`'s global flags may come first (`<app> --json mcp`).
    if let Some(rest) = mcp_args(args) {
        return mcp::run(&rest);
    }
    // `<app> media generate …` and the other client commands: the CLI,
    // without the window, against the running app's daemon. `od`'s global
    // flags may come first (`<app> --json daemon status`).
    let command = od_cli::subcommand(args);
    if command.is_some_and(|a| od_cli::CLIENT_COMMANDS.iter().any(|c| a == *c)) {
        // The CLI's own code: 2 (task still running) and 66 stay as they are.
        std::process::exit(i32::from(od_cli::main_code(all)));
    }
    let context = app_context();
    // `<app> daemon start [--port 0] …`: the `od` daemon, headless, for QA
    // and agents testing the installed app beside the one in use (its own
    // `OD_DATA_DIR`). No Tauri: no window, no Dock icon (no
    // `NSApplication`). Without `OD_DATA_DIR` it is the app's data dir,
    // whose `daemon.lock` refuses a second daemon while the app is open.
    if command.is_some_and(|a| a == DAEMON_COMMAND) {
        let resource_dir =
            tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default())
                .ok();
        let root = daemon_resource_root(
            std::env::var_os(od_content::RESOURCE_ROOT_ENV),
            resource_dir,
        );
        let app = od_cli::AppDaemon {
            resource_root: root,
            // The agents the app's window finds, not the terminal's `PATH`.
            login_path: true,
        };
        std::process::exit(i32::from(od_cli::main_code_with(all, app)));
    }
    // Opened from Finder the app has no stdout or stderr: the log and any
    // panic also go to `<data>/logs/` (Help ▸ Report Issue reads it).
    let data_dir = od_server::logs::with_console(DataDir::from_env);
    od_server::logs::init(Some(data_dir.root()));

    let opened = Arc::new(LastOpened::default());
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Its key: `plugins.updater` in tauri.conf.json. Its own commands
        // stay ungranted; Update now goes through `updater::install_update`.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateSlot::default())
        .manage(opened)
        .manage(Mutex::new(ReloadAsk::default()))
        .manage(LoadedPage::default());
    // The page loads again once its web content process ends: after a
    // design hung it (QA-067), View ▸ Reload, or a crash.
    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(Mutex::new(Restarts::default()))
        .manage(Mutex::new(Crashes::default()))
        .on_web_content_process_terminate(on_web_process_ended);
    builder
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            set_theme,
            set_menu_labels,
            save_file,
            updater::update_support,
            updater::install_update,
            updater::cancel_update,
            updater::restart_app,
            open_link,
            reset_dialog_gate,
            reload_ack
        ])
        .menu(|app| build_menu(app, &MenuLabels::default()))
        .on_menu_event(|app, event| match event.id().as_ref() {
            CLOSE_WINDOW_MENU_ID => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
            }
            OPEN_FOLDER_MENU_ID => open_folder_from_menu(app),
            RELOAD_MENU_ID => reload_from_menu(app),
            SHORTCUTS_MENU_ID => ui_action(app, "shortcuts"),
            PALETTE_MENU_ID => ui_action(app, "palette"),
            SETTINGS_MENU_ID => ui_action(app, "settings"),
            DATA_FOLDER_MENU_ID => show_data_folder(app),
            DOCS_MENU_ID => ui_action(app, "docs"),
            REPORT_MENU_ID => ui_action(app, "report-issue"),
            UPDATES_MENU_ID => ui_action(app, "check-updates"),
            _ => {}
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            // The daemon's origin, once it listens; until then only the
            // splash may load.
            let origin: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
            let nav_origin = Arc::clone(&origin);
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .on_navigation(move |url| {
                        let ok = allowed_navigation(url, nav_origin.get().map(String::as_str));
                        if !ok {
                            tracing::warn!(%url, "blocked a navigation away from the app");
                        }
                        ok
                    })
                    // The app never opens a second webview, nor the browser
                    // from here: the request does not say which frame asked,
                    // and a design's `window.open` with no click opened the
                    // browser on every load (QA-061). The app's own links go
                    // through `open_link`, a design's through the preview
                    // bridge's keyed `od:open-link`, which needs a real click.
                    .on_new_window(move |url, _| {
                        tracing::warn!(%url, "blocked a new window");
                        tauri::webview::NewWindowResponse::Deny
                    })
                    // A new page may show dialogs again though the user
                    // blocked the last page's (QA-064).
                    .on_page_load(|window, payload| {
                        // Its address once its web process ended (QA-073).
                        page_loaded_at(&window, payload.url());
                        if payload.event() == tauri::webview::PageLoadEvent::Started {
                            on_page_started(&window);
                        }
                        // QA's crash-loop test hook; off unless asked.
                        #[cfg(target_os = "macos")]
                        if payload.event() == tauri::webview::PageLoadEvent::Finished {
                            qa_crash_web(&window, payload.url());
                        }
                    })
                    .title(APP_NAME)
                    .inner_size(1360.0, 860.0)
                    .min_inner_size(900.0, 600.0)
                    // Files dropped on the window reach the page as HTML5 drops
                    // (files panel, Markdown editor) instead of Tauri events.
                    .disable_drag_drop_handler();
            // The UI draws its own title strip under the traffic lights
            // (od-ui `data-shell="mac"`, dragged via data-tauri-drag-region).
            #[cfg(target_os = "macos")]
            let window = window
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true);
            let window = window.build()?;
            // A page's `alert`, `confirm` and `prompt` (QA-064); a page
            // hung in a dialog loop is stopped (QA-067).
            #[cfg(target_os = "macos")]
            {
                let hung = handle.clone();
                if let Err(e) = window.with_webview(move |webview| {
                    js_dialogs::install(webview.inner(), move || on_hung(&hung));
                }) {
                    tracing::warn!(error = %e, "no JavaScript dialogs");
                }
                // ⌘Q and ⌘R work while a page hangs the window (QA-067).
                web_process::menu_keys_first();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = window;
            let root = resource_root(&handle);
            tauri::async_runtime::spawn(async move {
                let daemon = match start_daemon(data_dir, root).await {
                    Ok(daemon) => daemon,
                    Err(e) => {
                        // Typically a CLI daemon already owns the data dir.
                        handle
                            .dialog()
                            .message(format!("{e:#}"))
                            .title(APP_NAME)
                            .blocking_show();
                        handle.exit(1);
                        return;
                    }
                };
                let daemon_origin = daemon.url.origin().ascii_serialization();
                if let Err(e) = handle.add_capability(ui_capability(&daemon_origin)) {
                    tracing::error!(error = %e, "no IPC capability for the UI");
                }
                let _ = origin.set(daemon_origin);
                let url = daemon.url.clone();
                handle.manage(daemon);
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.navigate(url);
                }
                // SIGTERM (logout, `kill`) and Ctrl-C quit through
                // `RunEvent::Exit`, which stops running agents and ends the
                // terminal tabs' shells.
                match od_server::shutdown_signal() {
                    Ok(stop) => {
                        stop.await;
                        handle.exit(0);
                    }
                    Err(e) => tracing::warn!(error = %e, "no SIGTERM handler"),
                }
            });
            Ok(())
        })
        .build(context)
        .context("building the desktop app")?
        .run(|app, event| on_run_event(app, &event));
    Ok(())
}

/// Quitting, restarting after an update, or the Windows installer taking
/// over: stops running agents (as Stop does) and the terminal tabs' shells,
/// which would outlive the app, and releases the data folder's lock.
pub(crate) fn before_exit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(daemon) = app.try_state::<Daemon>() {
        tauri::async_runtime::block_on(daemon.state.shutdown());
        if let Ok(mut lock) = daemon.lock.lock() {
            lock.take();
        }
    }
    remove_webview_temp(&std::env::temp_dir());
}

/// The webview (WebKit on macOS) keeps caches in `<tmp>/<executable
/// name>`; nothing there outlives the app.
fn remove_webview_temp(tmp: &std::path::Path) {
    let Some(name) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(std::ffi::OsStr::to_owned))
    else {
        return;
    };
    let dir = tmp.join(name);
    if dir.is_dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invokes `cmd` from a page at `page` in the app built with this
    /// crate's ACL (`build.rs`), the UI capability granted to `origin`.
    fn invoke_from(origin: &str, page: &str, cmd: &str) -> Result<(), String> {
        let context: tauri::Context<tauri::test::MockRuntime> = app_context();
        let app = tauri::test::mock_builder()
            // The ACL goes by command name, before any handler: this one
            // stands in for the shell's, which need the real runtime.
            .invoke_handler(|invoke| {
                invoke.resolver.resolve("ready");
                true
            })
            .build(context)
            .unwrap();
        app.add_capability(ui_capability(origin)).unwrap();
        let url: tauri::Url = page.parse().unwrap();
        let webview = tauri::WebviewWindowBuilder::new(
            &app,
            "main",
            tauri::WebviewUrl::External(url.clone()),
        )
        .build()
        .unwrap();
        tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url,
                body: tauri::ipc::InvokeBody::default(),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    #[test]
    fn the_daemon_ui_may_call_the_app_commands() {
        // The UI comes from the daemon's loopback origin, a remote origin to
        // Tauri: without the app's ACL every app command was refused, so
        // Check for Updates never offered Update now.
        let origin = "http://127.0.0.1:43210";
        for cmd in [
            "pick_folder",
            "set_theme",
            "set_menu_labels",
            "save_file",
            "update_support",
            "install_update",
            "cancel_update",
            "restart_app",
            "open_link",
            "reset_dialog_gate",
            "reload_ack",
        ] {
            assert_eq!(
                invoke_from(origin, &format!("{origin}/studio"), cmd),
                Ok(()),
                "{cmd}"
            );
        }
        // Only the app's commands.
        assert!(invoke_from(origin, &format!("{origin}/"), "no_such_command").is_err());
        // Another loopback port gets nothing.
        assert!(invoke_from(origin, "http://127.0.0.1:5000/", "update_support").is_err());
    }

    #[test]
    fn the_webview_temp_folder_goes_on_exit() {
        let tmp = std::env::temp_dir().join(format!("od-desktop-test-{}", std::process::id()));
        let name = std::env::current_exe()
            .unwrap()
            .file_stem()
            .unwrap()
            .to_owned();
        std::fs::create_dir_all(tmp.join(&name).join("WebKit")).unwrap();
        std::fs::create_dir_all(tmp.join("other")).unwrap();
        remove_webview_temp(&tmp);
        assert!(!tmp.join(&name).exists());
        assert!(tmp.join("other").exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_headless_daemon_serves_the_bundled_content() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().to_path_buf();
        assert_eq!(daemon_resource_root(None, Some(bundle.clone())), None);
        std::fs::create_dir_all(bundle.join("content/skills")).unwrap();
        assert_eq!(
            daemon_resource_root(None, Some(bundle.clone())),
            Some(bundle.join("content"))
        );
        assert_eq!(
            daemon_resource_root(Some("".into()), Some(bundle.clone())),
            Some(bundle.join("content"))
        );
        // OD_RESOURCE_ROOT wins: the CLI's lookup reads it.
        assert_eq!(daemon_resource_root(Some("/x".into()), Some(bundle)), None);
        assert_eq!(daemon_resource_root(None, None), None);
    }

    #[test]
    fn mcp_runs_past_the_global_flags() {
        let args = |a: &[&str]| a.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        let rest = |a: &[&str]| {
            mcp_args(&args(a)).map(|r| {
                r.iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(rest(&["mcp"]), Some(vec![]));
        assert_eq!(
            rest(&["mcp", "--daemon-url", "http://x"]),
            Some(vec!["--daemon-url".into(), "http://x".into()])
        );
        assert_eq!(rest(&["--json", "mcp"]), Some(vec![]));
        assert_eq!(
            rest(&["--daemon-url", "http://x", "mcp", "--daemon-url=http://y"]),
            Some(vec![
                "--daemon-url".into(),
                "http://x".into(),
                "--daemon-url=http://y".into()
            ])
        );
        // A flag's value is not the command.
        assert_eq!(rest(&["--daemon-url", "mcp", "project", "list"]), None);
        assert_eq!(
            rest(&["--daemon-url", "mcp", "mcp", "--daemon-url=http://y"]),
            Some(vec![
                "--daemon-url".into(),
                "mcp".into(),
                "--daemon-url=http://y".into()
            ])
        );
        assert_eq!(rest(&["project", "mcp"]), None);
        // `od`'s `--daemon-url` before `mcp` goes along; one after it wins
        // (`mcp::run` keeps the last).
        assert_eq!(
            rest(&["--daemon-url", "http://q", "--json", "mcp"]),
            Some(vec!["--daemon-url".into(), "http://q".into()])
        );
        assert_eq!(
            rest(&["--daemon-url=http://q", "mcp", "--daemon-url", "http://y"]),
            Some(vec![
                "--daemon-url=http://q".into(),
                "--daemon-url".into(),
                "http://y".into()
            ])
        );
        assert_eq!(rest(&["--json"]), None);
        assert_eq!(rest(&[]), None);
    }

    #[test]
    fn native_theme_follows_the_app() {
        assert_eq!(native_theme("dark"), Some(tauri::Theme::Dark));
        assert_eq!(native_theme("light"), Some(tauri::Theme::Light));
        assert_eq!(native_theme("system"), None);
    }

    #[test]
    fn navigation_stays_on_the_daemon_origin() {
        let origin = "http://127.0.0.1:50123";
        let ok = |u: &str| allowed_navigation(&Url::parse(u).unwrap(), Some(origin));
        assert!(ok("http://127.0.0.1:50123/projects/p1"));
        // The splash, before and after the daemon is up.
        assert!(ok("tauri://localhost/index.html"));
        assert!(allowed_navigation(
            &Url::parse("http://tauri.localhost/index.html").unwrap(),
            None
        ));
        assert!(!allowed_navigation(
            &Url::parse("http://127.0.0.1:50123/").unwrap(),
            None
        ));
        assert!(ok("http://127.0.0.1:50123/api/projects/p1/raw/index.html"));
        // QA-062: an `<iframe srcdoc>` in a design loads `about:srcdoc`, an
        // empty frame `about:blank`; neither leaves the app.
        assert!(ok("about:srcdoc"));
        assert!(ok("about:blank"));
        assert!(allowed_navigation(
            &Url::parse("about:srcdoc").unwrap(),
            None
        ));
        for bad in [
            "about:config",
            "about:srcdoc/x",
            "http://127.0.0.1:7456/",
            "http://localhost:50123/",
            "https://127.0.0.1:50123/",
            "https://evil.example/",
            "file:///etc/passwd",
        ] {
            assert!(!ok(bad), "{bad}");
        }
    }

    /// Apache-2.0 §4(a)/(d) and the MIT/MPL notices: the app ships LICENSE,
    /// NOTICE and THIRD-PARTY-LICENSES next to `content/`, where the daemon
    /// serves them from (`GET /api/licenses/:name`).
    #[test]
    fn the_bundle_ships_the_license_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("tauri.conf.json")).unwrap())
                .unwrap();
        let resources = conf["bundle"]["resources"].as_object().unwrap();
        assert_eq!(resources["../../content/"], "content/");
        for name in od_server::routes::licenses::FILES {
            let source = format!("../../{name}");
            assert_eq!(
                resources.get(&source).and_then(|v| v.as_str()),
                Some(name),
                "{name}"
            );
            let text = std::fs::read_to_string(dir.join(&source)).unwrap();
            assert!(!text.trim().is_empty(), "{name} is empty");
        }
    }

    /// Every row of the menu bar, with its section's title.
    fn rows(bar: &[Section]) -> Vec<(&str, &Entry)> {
        bar.iter()
            .flat_map(|s| s.entries.iter().map(move |e| (s.title.as_str(), e)))
            .collect()
    }

    #[test]
    fn cmd_w_reaches_the_page_on_macos() {
        // QA-042: the native Close Window took ⌘W before the Studio could
        // close a tab, closed the last window and quit with the runs.
        let bar = menu_spec(&MenuLabels::default(), "macos");
        for (menu, entry) in rows(&bar) {
            assert_ne!(
                entry,
                &Entry::Native(Native::CloseWindow, "Close Window".into()),
                "{menu}: the OS item carries ⌘W"
            );
            if let Entry::Item { accelerator, .. } = entry {
                assert_ne!(*accelerator, Some("CmdOrCtrl+W"), "{menu}");
            }
        }
        // Close Window is still there, ours, without a key: File and Window.
        let ours = Entry::Item {
            id: CLOSE_WINDOW_MENU_ID,
            text: "Close Window".into(),
            accelerator: None,
        };
        let closes: Vec<_> = rows(&bar)
            .into_iter()
            .filter(|(_, e)| **e == ours)
            .map(|(m, _)| m)
            .collect();
        assert_eq!(closes, ["File", "Window"]);
        // Quit stays: ⌘Q quits.
        assert!(
            rows(&bar)
                .iter()
                .any(|(_, e)| matches!(e, Entry::Native(Native::Quit, _)))
        );
    }

    #[test]
    fn close_window_stays_native_off_macos() {
        // Alt+F4 there; no page shortcut uses it.
        for os in ["windows", "linux"] {
            let bar = menu_spec(&MenuLabels::default(), os);
            assert!(
                rows(&bar)
                    .iter()
                    .any(|(_, e)| matches!(e, Entry::Native(Native::CloseWindow, _))),
                "{os}"
            );
            assert!(
                !rows(&bar).iter().any(
                    |(_, e)| matches!(e, Entry::Item { id, .. } if *id == CLOSE_WINDOW_MENU_ID)
                ),
                "{os}"
            );
        }
        let titles = |os| {
            menu_spec(&MenuLabels::default(), os)
                .into_iter()
                .map(|s| s.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles("macos"),
            [
                "Contour", "File", "Edit", "View", "Project", "Window", "Help"
            ]
        );
        assert_eq!(
            titles("windows"),
            ["File", "Edit", "Project", "Window", "Help"]
        );
        assert_eq!(titles("linux"), ["Edit", "Project", "Window", "Help"]);
    }

    #[test]
    fn view_reload_is_native_on_macos() {
        // QA-067: ⌘R reloads from the native side; a hung page cannot
        // swallow it.
        let bar = menu_spec(&MenuLabels::default(), "macos");
        let reload = Entry::Item {
            id: RELOAD_MENU_ID,
            text: "Reload".into(),
            accelerator: Some("CmdOrCtrl+R"),
        };
        let found: Vec<_> = rows(&bar)
            .into_iter()
            .filter(|(_, e)| **e == reload)
            .map(|(m, _)| m)
            .collect();
        assert_eq!(found, ["View"]);
        let labels: MenuLabels =
            serde_json::from_value(serde_json::json!({ "reload": "Tải lại" })).unwrap();
        let bar = menu_spec(&labels.or_english(), "macos");
        assert!(rows(&bar).iter().any(|(_, e)| matches!(
            e,
            Entry::Item { id, text, .. } if *id == RELOAD_MENU_ID && text == "Tải lại"
        )));
    }

    #[test]
    fn a_lone_crash_reloads() {
        let t0 = std::time::Instant::now();
        let mut crashes = Crashes::default();
        assert_eq!(crashes.crashed(t0), AfterCrash::Reload);
        // Long after: a crash of its own, not a loop.
        assert_eq!(crashes.crashed(t0 + CRASH_LOOP_WINDOW), AfterCrash::Reload);
    }

    #[test]
    fn a_second_crash_soon_after_stops_the_preview() {
        // A design that crashes the process as it loads reloaded for ever.
        let t0 = std::time::Instant::now();
        let mut crashes = Crashes::default();
        assert_eq!(crashes.crashed(t0), AfterCrash::Reload);
        let soon = t0 + CRASH_LOOP_WINDOW - std::time::Duration::from_millis(1);
        assert_eq!(crashes.crashed(soon), AfterCrash::StopPreview);
        // Crashing even with the preview stopped: no more loads.
        assert_eq!(crashes.crashed(soon), AfterCrash::GiveUp);
        assert_eq!(crashes.crashed(soon), AfterCrash::GiveUp);
        // Quiet for a while: a new crash reloads again.
        let later = soon + CRASH_LOOP_WINDOW;
        assert_eq!(crashes.crashed(later), AfterCrash::Reload);
    }

    /// A [`ReloadAsk`] for a page that takes `od:menu`.
    fn listening() -> ReloadAsk {
        let mut asks = ReloadAsk::default();
        asks.listens();
        asks
    }

    #[test]
    fn a_page_deaf_to_the_menu_is_reloaded_without_asking() {
        // The splash, or the app before its shell shows, cannot answer
        // View ▸ Reload: asking would say "This page isn't responding"
        // about a page that is fine.
        let mut asks = ReloadAsk::default();
        assert_eq!(asks.ask(), None);
        asks.listens();
        let token = asks.ask().unwrap();
        assert!(asks.is_open(token));
        // A new page listens only once its shell says so.
        asks.page_loaded();
        assert_eq!(asks.ask(), None);
    }

    #[test]
    fn reloading_the_splash_opens_the_app_once_the_daemon_listens() {
        let url = |s: &str| Url::parse(s).unwrap();
        let daemon = url("http://127.0.0.1:5173/");
        for splash in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html",
        ] {
            assert_eq!(
                unheard_reload_target(&url(splash), Some(&daemon)),
                Some(daemon.clone())
            );
            assert_eq!(unheard_reload_target(&url(splash), None), None);
        }
        // The app itself, not shown yet: reloaded where it is.
        assert_eq!(
            unheard_reload_target(&url("http://127.0.0.1:5173/projects/p1"), Some(&daemon)),
            None
        );
    }

    #[test]
    fn webkit_late_word_after_a_restart_is_not_a_crash() {
        // WebKit said nothing within RESTART_GRACE, so the grace timer
        // loaded the page; WebKit saying the old process ended after that
        // is the same restart: no second load, no crash counted.
        let then = Url::parse("http://127.0.0.1:5173/projects/p1?preview=stopped").unwrap();
        let t0 = std::time::Instant::now();
        let mut restarts = Restarts::default();
        restarts.begin(then.clone());
        assert_eq!(restarts.grace_over(true, t0), Some(then));
        let late = t0 + LATE_END_WINDOW - std::time::Duration::from_millis(1);
        assert_eq!(restarts.ended(late), ProcessEnded::LateRestart);
        // Once: the next end is the new page's own.
        assert_eq!(restarts.ended(late), ProcessEnded::Crashed);
    }

    #[test]
    fn a_process_end_long_after_a_restart_is_a_crash() {
        let then = Url::parse("http://127.0.0.1:5173/").unwrap();
        let t0 = std::time::Instant::now();
        let mut restarts = Restarts::default();
        restarts.begin(then.clone());
        assert_eq!(restarts.grace_over(true, t0), Some(then.clone()));
        assert_eq!(restarts.ended(t0 + LATE_END_WINDOW), ProcessEnded::Crashed);
        // The process was never asked to end (this WebKit cannot): any
        // end after is a crash.
        restarts.begin(then.clone());
        assert_eq!(restarts.grace_over(false, t0), Some(then));
        assert_eq!(restarts.ended(t0), ProcessEnded::Crashed);
    }

    #[test]
    fn webkit_word_in_time_loads_the_restarts_page_once() {
        let then = Url::parse("http://127.0.0.1:5173/").unwrap();
        let t0 = std::time::Instant::now();
        let mut restarts = Restarts::default();
        restarts.begin(then.clone());
        assert_eq!(restarts.ended(t0), ProcessEnded::Load(then));
        // The grace timer finds nothing left to load.
        assert_eq!(restarts.grace_over(true, t0), None);
        assert_eq!(restarts.ended(t0), ProcessEnded::Crashed);
    }

    /// An app on the mock runtime keeping the shell's Reload, restart and
    /// crash state, its main window showing `page`, loaded through
    /// `on_page_load` ([`LoadedPage`]).
    fn shell_app(
        page: &str,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        tauri::WebviewWindow<tauri::test::MockRuntime>,
    ) {
        let context: tauri::Context<tauri::test::MockRuntime> = app_context();
        let app = tauri::test::mock_builder()
            .manage(Mutex::new(ReloadAsk::default()))
            .manage(Mutex::new(Restarts::default()))
            .manage(Mutex::new(Crashes::default()))
            .manage(LoadedPage::default())
            .build(context)
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(
            &app,
            "main",
            tauri::WebviewUrl::External(page.parse().unwrap()),
        )
        .build()
        .unwrap();
        page_loaded_at(&window, &page.parse().unwrap());
        (app, window)
    }

    /// Whether the app's Reload question `token` is still open.
    fn question_open(app: &tauri::App<tauri::test::MockRuntime>, token: u64) -> bool {
        app.state::<Mutex<ReloadAsk>>()
            .lock()
            .unwrap()
            .is_open(token)
    }

    #[test]
    fn a_page_load_closes_the_reload_question_and_stops_listening() {
        let (app, window) = shell_app("http://127.0.0.1:5173/");
        page_listens(&window);
        let token = app
            .state::<Mutex<ReloadAsk>>()
            .lock()
            .unwrap()
            .ask()
            .unwrap();
        on_page_started(&window);
        assert!(!question_open(&app, token));
        assert_eq!(app.state::<Mutex<ReloadAsk>>().lock().unwrap().ask(), None);
    }

    #[test]
    fn a_hung_page_restarts_on_its_studio_with_the_preview_stopped() {
        // The dialog gate's callback, through to the page loaded again.
        let (app, window) = shell_app("http://127.0.0.1:5173/present/p1");
        page_listens(&window);
        let token = app
            .state::<Mutex<ReloadAsk>>()
            .lock()
            .unwrap()
            .ask()
            .unwrap();
        on_hung(app.handle());
        let wanted = "http://127.0.0.1:5173/projects/p1?preview=stopped&why=dialogs";
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while window.url().unwrap().as_str() != wanted && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(window.url().unwrap().as_str(), wanted);
        assert!(
            !question_open(&app, token),
            "the restart closes the question"
        );
    }

    #[test]
    fn webkit_late_word_after_a_restart_leaves_the_page_alone() {
        let (app, window) = shell_app("http://127.0.0.1:5173/projects/p1");
        let then = Url::parse("http://127.0.0.1:5173/projects/p1?preview=stopped").unwrap();
        with_restarts(app.handle(), |r| {
            r.begin(then.clone());
            r.grace_over(true, std::time::Instant::now())
        });
        window.navigate(then.clone()).unwrap();
        page_terminated(app.handle());
        assert_eq!(window.url().unwrap(), then);
        let crashes = app.state::<Mutex<Crashes>>();
        assert_eq!(crashes.lock().unwrap().in_a_row, 0, "no crash counted");
        // A crash after it is counted.
        page_terminated(app.handle());
        assert_eq!(crashes.lock().unwrap().in_a_row, 1);
    }

    #[test]
    fn an_answered_reload_is_left_to_the_page() {
        let mut asks = listening();
        let token = asks.ask().unwrap();
        asks.answered();
        assert!(!asks.unanswered(token), "the page answered: no restart");
    }

    #[test]
    fn an_unanswered_reload_restarts_the_page_once() {
        let mut asks = listening();
        let token = asks.ask().unwrap();
        assert!(asks.unanswered(token));
        assert!(!asks.unanswered(token), "once");
        // An old question's timer does not act on a newer one.
        let first = asks.ask().unwrap();
        let second = asks.ask().unwrap();
        assert!(!asks.unanswered(first));
        assert!(asks.unanswered(second));
    }

    #[test]
    fn a_page_silent_after_reload_is_restarted_only_on_the_users_word() {
        use NotAnswered::{AskAgainLater, Nothing, Restart};
        use NotRespondingChoice::{Closed, Reload, Wait};
        // A busy page is not killed behind the user's back: Wait, or no
        // question shown (a dialog is up), asks again later.
        assert_eq!(after_not_answering(true, Some(Wait)), AskAgainLater);
        assert_eq!(after_not_answering(true, None), AskAgainLater);
        assert_eq!(after_not_answering(true, Some(Reload)), Restart);
        // The page answered while the user decided: its leave guard has
        // the reload now, whatever the choice.
        assert_eq!(after_not_answering(false, Some(Reload)), Nothing);
        assert_eq!(after_not_answering(false, Some(Wait)), Nothing);
        assert_eq!(after_not_answering(false, None), Nothing);
        // The page answered, so the shell closed the alert (QA-072): a
        // Reload on the stale question, or the alert closing, does nothing.
        assert_eq!(after_not_answering(false, Some(Closed)), Nothing);
        // Closed with the question still open (no path does it): never a
        // restart without the user's word.
        assert_eq!(after_not_answering(true, Some(Closed)), AskAgainLater);
    }

    #[test]
    fn the_crash_hook_is_off_unless_qa_asks() {
        let qa = Some("dev.contour.desktop.qa");
        let release = Some("dev.contour.desktop");
        assert!(qa_crash_web_enabled(Some("1"), false, qa));
        assert!(qa_crash_web_enabled(Some("1"), true, release));
        // A release Contour never crashes itself.
        assert!(!qa_crash_web_enabled(Some("1"), false, release));
        assert!(!qa_crash_web_enabled(Some("1"), false, None));
        assert!(!qa_crash_web_enabled(
            Some("1"),
            false,
            Some("dev.contour.qa.desktop")
        ));
        // Not asked.
        assert!(!qa_crash_web_enabled(None, true, qa));
        assert!(!qa_crash_web_enabled(Some("0"), true, qa));
        assert!(!qa_crash_web_enabled(Some(""), true, qa));
    }

    #[test]
    fn the_crash_hook_ends_only_a_running_design_preview() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(qa_crashes_page(&url("http://127.0.0.1:5/projects/p1")));
        assert!(qa_crashes_page(&url("http://127.0.0.1:5/present/p1?x=1")));
        // The loop ends where a real one does: the preview stopped.
        let stopped =
            stopped_preview_url(&url("http://127.0.0.1:5/projects/p1"), StopReason::Crashed);
        assert!(!qa_crashes_page(&stopped));
        // No design: home, the project list, the splash.
        assert!(!qa_crashes_page(&url("http://127.0.0.1:5/")));
        assert!(!qa_crashes_page(&url("http://127.0.0.1:5/projects/")));
        assert!(!qa_crashes_page(&url("tauri://localhost/index.html")));
    }

    #[test]
    fn a_new_page_closes_the_reload_question() {
        // The page reloaded some other way (its own reload, a navigation,
        // a restart): the shell must not ask about it any more.
        let mut asks = listening();
        let token = asks.ask().unwrap();
        asks.page_loaded();
        assert!(!asks.is_open(token));
        assert!(!asks.unanswered(token), "no restart either");
        assert_eq!(asks.ask(), None, "the new page has not said it listens");
    }

    #[test]
    fn a_reload_question_stays_open_until_answered_or_replaced() {
        let mut asks = listening();
        let first = asks.ask().unwrap();
        assert!(asks.is_open(first));
        assert!(asks.is_open(first), "looking does not close it");
        let second = asks.ask().unwrap();
        assert!(!asks.is_open(first), "a newer Reload takes over");
        assert!(asks.is_open(second));
        asks.answered();
        assert!(!asks.is_open(second));
    }

    #[test]
    fn a_hung_design_reopens_its_studio_with_the_preview_stopped() {
        let url = |s: &str| Url::parse(s).unwrap();
        let base = "http://127.0.0.1:5173";
        let dialogs = StopReason::Dialogs;
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/projects/p1")), dialogs).as_str(),
            format!("{base}/projects/p1?preview=stopped&why=dialogs")
        );
        // Other query kept, the mark not doubled, the fragment kept.
        assert_eq!(
            stopped_preview_url(
                &url(&format!(
                    "{base}/projects/p1?a=1&preview=stopped&why=hung#x"
                )),
                dialogs
            )
            .as_str(),
            format!("{base}/projects/p1?a=1&preview=stopped&why=dialogs#x")
        );
        // Present mode would loop again: back to the design's Studio.
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/present/p1?slide=3")), dialogs).as_str(),
            format!("{base}/projects/p1?slide=3&preview=stopped&why=dialogs")
        );
    }

    #[test]
    fn the_address_says_why_the_preview_stopped() {
        // QA-071: a busy loop with no dialog is not "kept opening dialogs".
        let page = Url::parse("http://127.0.0.1:5173/projects/p1").unwrap();
        let why = |reason| {
            stopped_preview_url(&page, reason)
                .query_pairs()
                .find(|(k, _)| k == STOP_REASON_KEY)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(why(StopReason::Dialogs).as_deref(), Some("dialogs"));
        assert_eq!(why(StopReason::Hung).as_deref(), Some("hung"));
        assert_eq!(why(StopReason::Crashed).as_deref(), Some("crashed"));
    }

    #[test]
    fn a_crash_loop_stops_the_preview_saying_it_crashed() {
        let (app, window) = shell_app("http://127.0.0.1:5173/projects/p1");
        page_terminated(app.handle());
        page_terminated(app.handle());
        assert_eq!(
            window.url().unwrap().as_str(),
            "http://127.0.0.1:5173/projects/p1?preview=stopped&why=crashed"
        );
    }

    #[test]
    fn a_crash_loop_stops_the_page_last_loaded_not_wrys_address() {
        // QA-073: the process is gone, so WebKit has no address (wry's
        // `url()` panics); the page `on_page_load` saw last is the one.
        let (app, window) = shell_app("http://127.0.0.1:5173/projects/p1");
        page_loaded_at(
            &window,
            &Url::parse("http://127.0.0.1:5173/present/p2").unwrap(),
        );
        page_terminated(app.handle());
        page_terminated(app.handle());
        assert_eq!(
            window.url().unwrap().as_str(),
            "http://127.0.0.1:5173/projects/p2?preview=stopped&why=crashed"
        );
    }

    #[test]
    fn a_crash_loop_with_no_page_known_leaves_the_window() {
        let (app, window) = shell_app("http://127.0.0.1:5173/projects/p1");
        *app.state::<LoadedPage>().0.lock().unwrap() = None;
        page_terminated(app.handle());
        page_terminated(app.handle());
        assert_eq!(
            window.url().unwrap().as_str(),
            "http://127.0.0.1:5173/projects/p1"
        );
        let crashes = app.state::<Mutex<Crashes>>();
        assert_eq!(crashes.lock().unwrap().in_a_row, 2);
    }

    #[test]
    fn a_page_load_is_kept_as_the_page_shown() {
        let (app, window) = shell_app("http://127.0.0.1:5173/");
        let next = Url::parse("http://127.0.0.1:5173/projects/p1").unwrap();
        page_loaded_at(&window, &next);
        assert_eq!(shown_url(&window), Some(next));
        drop(app);
    }

    #[test]
    fn webkit_saying_the_process_ended_inside_a_held_lock_does_not_deadlock() {
        // QA-070: `restart_page`'s `with_webview` holds Tauri's window
        // lock while WebKit ends the process and says so, on the main
        // thread, where `run_on_main_thread` runs at once (as the mock
        // runtime does). A lock the work takes, held across the callback,
        // stands in for it: the work must wait for the call to return.
        let (app, window) = shell_app("http://127.0.0.1:5173/projects/p1");
        let then =
            Url::parse("http://127.0.0.1:5173/projects/p1?preview=stopped&why=hung").unwrap();
        with_restarts(app.handle(), |r| r.begin(then.clone()));
        let handle = app.handle().clone();
        let (done, returned) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let restarts = handle.state::<Mutex<Restarts>>();
            let held = restarts.lock().unwrap();
            on_page_terminated(&handle);
            drop(held);
            let _ = done.send(());
        });
        assert!(
            returned
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok(),
            "WebKit's callback ran the work inside its call and deadlocked"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while window.url().unwrap() != then && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            window.url().unwrap(),
            then,
            "the restart's page loads after"
        );
    }

    #[test]
    fn the_menu_speaks_the_ui_language() {
        // QA-049: the UI sends its words; a missing one stays English.
        let labels: MenuLabels = serde_json::from_value(serde_json::json!({
            "file": "Tệp",
            "openFolder": "Mở thư mục…",
            "reportIssue": "Báo lỗi",
            "closeWindow": "",
        }))
        .unwrap();
        let bar = menu_spec(&labels.or_english(), "macos");
        let titles: Vec<_> = bar.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Tệp"));
        let texts: Vec<String> = rows(&bar)
            .into_iter()
            .filter_map(|(_, e)| match e {
                Entry::Item { text, .. } | Entry::Native(_, text) => Some(text.clone()),
                Entry::Separator => None,
            })
            .collect();
        for want in ["Mở thư mục…", "Báo lỗi", "Close Window", "Paste"] {
            assert!(texts.iter().any(|t| t == want), "{want}");
        }
        // Window and Help keep Tauri's ids, which macOS needs.
        let ids: Vec<_> = bar.iter().filter_map(|s| s.id).collect();
        assert_eq!(
            ids,
            [tauri::menu::WINDOW_SUBMENU_ID, tauri::menu::HELP_SUBMENU_ID]
        );
    }

    #[test]
    fn closing_the_window_hides_it_on_macos() {
        // QA-043: the red button quit the app and stopped its runs.
        let context: tauri::Context<tauri::test::MockRuntime> = app_context();
        let app = tauri::test::mock_builder().build(context).unwrap();
        let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::App("index.html".into()))
            .build()
            .unwrap();
        let kept = Arc::new(Mutex::new(None::<bool>));
        let seen = Arc::clone(&kept);
        app.run(move |handle, event| match event {
            RunEvent::Ready => window.close().unwrap(),
            RunEvent::WindowEvent { .. } => on_run_event(handle, &event),
            RunEvent::MainEventsCleared => {
                let mut seen = seen.lock().unwrap();
                if seen.is_none() {
                    *seen = Some(handle.get_webview_window("main").is_some());
                    // Ends the mock event loop.
                    window.destroy().unwrap();
                }
            }
            _ => {}
        });
        // Without the hide the window is gone and the loop ends before it
        // ever goes idle.
        let kept = *kept.lock().unwrap();
        if HIDE_ON_CLOSE {
            assert_eq!(kept, Some(true));
        } else {
            assert_eq!(kept, None);
        }
    }

    #[test]
    fn the_save_dialog_suggests_only_a_name() {
        assert_eq!(percent_decode("index%20(1).pdf"), "index (1).pdf");
        assert_eq!(percent_decode("ghi-ch%C3%BA.md"), "ghi-chú.md");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(file_name_only("../../etc/passwd"), "passwd");
        assert_eq!(file_name_only(r"..\x.png"), "x.png");
        assert_eq!(file_name_only("deck.pptx"), "deck.pptx");
    }

    #[test]
    fn one_click_opens_one_tab() {
        let a = Url::parse("https://example.com/p1").unwrap();
        let b = Url::parse("https://example.com/p2").unwrap();
        let t0 = std::time::Instant::now();
        let ms = std::time::Duration::from_millis;
        let mut last = None;
        assert!(first_opening(&mut last, &a, t0));
        // The same page again right away: the other path of the same click.
        assert!(!first_opening(&mut last, &a, t0 + ms(300)));
        // Another page is opened.
        assert!(first_opening(&mut last, &b, t0 + ms(400)));
        assert!(first_opening(&mut last, &a, t0 + ms(500)));
        // The same page clicked again later opens again.
        assert!(first_opening(&mut last, &a, t0 + ms(1600)));
    }

    #[test]
    fn open_link_takes_web_addresses_only() {
        assert_eq!(
            link_to_open("https://example.com/p1").map(String::from),
            Some("https://example.com/p1".to_owned())
        );
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "not a url", ""] {
            assert!(link_to_open(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn only_web_links_open_in_the_browser() {
        let opens = |u: &str| opens_in_browser(&Url::parse(u).unwrap());
        assert!(opens("https://code.claude.com/docs/en/setup"));
        assert!(opens("https://www.google.com/chrome/"));
        assert!(opens(
            "http://127.0.0.1:50123/api/projects/p1/raw/index.html"
        ));
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "blob:http://127.0.0.1:50123/x",
            "tauri://localhost/index.html",
            "mailto:a@b.c",
            "vscode://file/x",
        ] {
            assert!(!opens(bad), "{bad}");
        }
    }

    /// The line over a function WebKit calls synchronously, maybe inside a
    /// Tauri dispatcher call that holds the window's lock (QA-070).
    const WEBKIT_CALLBACK: &str = "// WebKit callback:";

    /// Tauri calls that go through the window's dispatcher: inside a WebKit
    /// callback they lock the window again (QA-070). A page-load event's
    /// own `payload.url()` is not one. `shown_url(` wraps `with_webview`.
    const DISPATCHER_CALLS: [&str; 12] = [
        "with_webview(",
        "navigate(",
        "run_on_main_thread(",
        "eval(",
        "reload(",
        ".url()",
        "shown_url(",
        ".emit",
        ".set_",
        ".show(",
        ".hide(",
        ".close(",
    ];

    /// The shell's sources with WebKit callbacks.
    const SOURCES: [(&str, &str); 3] = [
        ("main.rs", include_str!("main.rs")),
        ("js_dialogs.rs", include_str!("js_dialogs.rs")),
        ("web_process.rs", include_str!("web_process.rs")),
    ];

    /// `source` without its comments and with its string and char literals
    /// emptied: braces and words in them count for nothing.
    fn code_only(source: &str) -> String {
        let chars: Vec<char> = source.chars().collect();
        let mut out = String::with_capacity(source.len());
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '/' if chars.get(i + 1) == Some(&'/') => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                '"' => {
                    i += 1;
                    while i < chars.len() && chars[i] != '"' {
                        i += if chars[i] == '\\' { 2 } else { 1 };
                    }
                    out.push_str("\"\"");
                }
                // A char literal (`'x'`, `'\n'`), not a lifetime.
                '\'' if chars.get(i + 2) == Some(&'\'') || chars.get(i + 1) == Some(&'\\') => {
                    i += 1;
                    while i < chars.len() && chars[i] != '\'' {
                        i += if chars[i] == '\\' { 2 } else { 1 };
                    }
                    out.push_str("' '");
                }
                c => out.push(c),
            }
            i += 1;
        }
        out
    }

    /// The index of the bracket closing the one `code` opens at `open`.
    fn closing(code: &str, open: usize) -> usize {
        let (up, down) = match &code[open..=open] {
            "(" => ('(', ')'),
            _ => ('{', '}'),
        };
        let mut depth = 0usize;
        for (i, c) in code[open..].char_indices() {
            if c == up {
                depth += 1;
            } else if c == down {
                depth -= 1;
                if depth == 0 {
                    return open + i;
                }
            }
        }
        panic!("unbalanced {up} at {open}");
    }

    /// `code` without what it hands to `off_webkit_call`, which runs after
    /// WebKit's call has returned.
    fn outside_off_webkit_call(code: &str) -> String {
        let mut code = code.to_owned();
        while let Some(at) = code.find("off_webkit_call(") {
            let open = at + "off_webkit_call".len();
            let close = closing(&code, open);
            code.replace_range(at..=close, "handed_off");
        }
        code
    }

    /// The dispatcher calls `code` makes itself ([`DISPATCHER_CALLS`]).
    fn dispatcher_calls(code: &str) -> Vec<&'static str> {
        let code = outside_off_webkit_call(code).replace("payload.url()", "payload_url");
        DISPATCHER_CALLS
            .into_iter()
            .filter(|call| code.contains(call))
            .collect()
    }

    /// Each function of `source` under a [`WEBKIT_CALLBACK`] line: its
    /// name and its body (code only).
    fn webkit_callbacks(source: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            offset += line.len();
            if !line.trim_start().starts_with(WEBKIT_CALLBACK) {
                continue;
            }
            let rest = &source[offset..];
            let at = rest.find("fn ").expect("a function under the line");
            let code = code_only(&rest[at..]);
            let name: String = code[3..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let open = code.find('{').expect("a body");
            let body = code[open..=closing(&code, open)].to_owned();
            found.push((name, body));
        }
        found
    }

    #[test]
    fn webkit_callbacks_never_call_tauris_dispatcher() {
        // QA-070, and the terminate hook again in #138: `with_webview`
        // inside a callback WebKit makes inside `restart_page`'s
        // `with_webview` locks the window twice and hangs the app.
        let mut names = Vec::new();
        for (file, source) in SOURCES {
            for (name, body) in webkit_callbacks(source) {
                let calls = dispatcher_calls(&body);
                assert!(
                    calls.is_empty(),
                    "{file} `{name}` runs inside a WebKit call and calls {calls:?}: \
                     hand it to `off_webkit_call` or use state kept aside"
                );
                names.push(name);
            }
        }
        for name in [
            "on_web_process_ended",
            "on_page_terminated",
            "on_page_started",
            "qa_crash_web",
            "on_hung",
            "run_panel",
            "run_alert",
            "run_confirm",
            "run_prompt",
            "run_open_panel",
        ] {
            assert!(
                names.iter().any(|n| n == name),
                "`{name}` lost its `{WEBKIT_CALLBACK}` line"
            );
        }
    }

    #[test]
    fn the_windows_webkit_hooks_never_call_tauris_dispatcher() {
        let code = code_only(include_str!("main.rs"));
        for hook in [
            ".on_web_content_process_terminate(",
            ".on_page_load(",
            ".on_navigation(",
            ".on_new_window(",
        ] {
            let at = code.find(hook).unwrap_or_else(|| panic!("no {hook}"));
            let open = at + hook.len() - 1;
            let args = &code[open..=closing(&code, open)];
            let calls = dispatcher_calls(args);
            assert!(
                calls.is_empty(),
                "{hook} calls {calls:?} inside WebKit's call"
            );
        }
    }

    #[test]
    fn a_process_end_and_a_page_load_reach_the_pages_dialogs() {
        // Not covered by any test that runs WebKit: the wiring itself.
        let code = code_only(include_str!("main.rs"));
        assert!(code.contains(".on_web_content_process_terminate(on_web_process_ended)"));
        let callbacks = webkit_callbacks(include_str!("main.rs"));
        let body = |name: &str| {
            callbacks
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, body)| body.clone())
                .unwrap_or_default()
        };
        assert!(body("on_web_process_ended").contains("js_dialogs::process_ended_main()"));
        assert!(body("on_page_started").contains("js_dialogs::page_loaded_main()"));
        // Both name the window `install` kept: without it they do nothing.
        let dialogs = code_only(include_str!("js_dialogs.rs"));
        let at = dialogs.find("fn install(").expect("js_dialogs::install");
        let open = at + dialogs[at..].find('{').expect("a body");
        assert!(dialogs[open..=closing(&dialogs, open)].contains("MAIN.set("));
        // The page a crash loop stops is the one `on_page_load` kept (QA-073).
        let hook = code.find(".on_page_load(").expect("on_page_load");
        let open = hook + ".on_page_load".len();
        assert!(
            code[open..=closing(&code, open)].contains("page_loaded_at(&window, payload.url())")
        );
        let shipped = code.split("\nmod tests").next().unwrap_or_default();
        assert!(shipped.contains(".manage(LoadedPage::default())"));
    }

    /// The line over (or the end of) a line that may read an address
    /// through wry's `url()`, saying why it cannot panic there.
    const URL_ALLOWED: &str = "// url() allowed:";

    /// The lines of `source`, its tests left out, that read a window's or
    /// webview's address through wry's `url()` with no [`URL_ALLOWED`]
    /// note. A page-load event's own `payload.url()` is not one.
    fn wry_url_reads(source: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut allowed = false;
        for line in source.lines() {
            if line.trim_start().starts_with("mod tests") {
                break;
            }
            let code = code_only(line).replace("payload.url()", "");
            if code.contains(".url()") && !allowed && !line.contains(URL_ALLOWED) {
                found.push(line.trim().to_owned());
            }
            allowed = line.trim_start().starts_with(URL_ALLOWED);
        }
        found
    }

    #[test]
    fn the_shell_never_reads_an_address_through_wry() {
        // QA-073: once the web content process ended, WKWebView's address
        // is nil and wry's `url()` unwraps it: the app panics and quits.
        // Read it through `shown_url`.
        for (file, source) in [
            ("main.rs", include_str!("main.rs")),
            ("js_dialogs.rs", include_str!("js_dialogs.rs")),
            ("web_process.rs", include_str!("web_process.rs")),
            ("updater.rs", include_str!("updater.rs")),
            ("mcp.rs", include_str!("mcp.rs")),
        ] {
            let reads = wry_url_reads(source);
            assert!(
                reads.is_empty(),
                "{file} reads an address through wry's `url()`: {reads:?}; \
                 use `shown_url`, or say why it cannot panic ({URL_ALLOWED})"
            );
        }
    }

    #[test]
    fn the_url_check_sees_wrys_url_not_payloads() {
        let source = "
            fn a() { w.url(); }
            fn b() { payload.url(); }
            // w.url() in a comment is fine
            fn c() { tracing::warn!(\"x.url()\"); }
            // url() allowed: not macOS
            fn d() { w.url(); }
            fn e() { webview
                .url(); }
            #[cfg(test)]
            mod tests { fn t() { w.url(); } }
        ";
        assert_eq!(wry_url_reads(source), ["fn a() { w.url(); }", ".url(); }"]);
    }

    #[test]
    fn the_check_sees_dispatcher_calls_not_handed_off() {
        let source = "
            // WebKit callback: test
            fn hook(w: &W) {
                // w.with_webview(|_| {}) in a comment is fine
                tracing::warn!(\"navigate(\");
                off_webkit_call(app, |app| { w.navigate(u); w.with_webview(f); });
                let c = '{';
                if x { w.reload(); }
            }
            fn after() { w.with_webview(f); }
        ";
        let callbacks = webkit_callbacks(source);
        assert_eq!(callbacks.len(), 1);
        assert_eq!(callbacks[0].0, "hook");
        assert_eq!(dispatcher_calls(&callbacks[0].1), ["reload("]);
        assert!(dispatcher_calls("{ payload.url(); handed(); }").is_empty());
    }
}
