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
/// View ▸ Reload on macOS: loads the app again from the native side, even
/// when a page hangs the window (QA-067).
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

/// The page to load once the window's web content process is gone: set
/// before [`restart_page`] ends it, taken when WebKit says it ended
/// ([`on_page_terminated`]). Nothing there (a crash) reloads the page, as
/// Tauri does by default.
#[cfg(target_os = "macos")]
#[derive(Default)]
struct AfterRestart(Mutex<Option<Url>>);

/// How long WebKit gets to say the web content process ended before
/// [`restart_page`] loads the page itself.
#[cfg(target_os = "macos")]
const RESTART_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// The query the Studio reads on load: the design's preview was stopped
/// because it kept opening dialogs (od-ui `desktop::preview_was_stopped`).
#[cfg(any(target_os = "macos", test))]
const PREVIEW_STOPPED: (&str, &str) = ("preview", "stopped");

/// Where the app opens again after a design hung it in a dialog loop
/// (QA-067): the same page with the preview stopped
/// (`?preview=stopped`). Present mode goes back to that design's Studio,
/// which says why and can run it again; it would only loop again.
#[cfg(any(target_os = "macos", test))]
fn stopped_preview_url(url: &Url) -> Url {
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
        .filter(|(k, _)| k != key)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair(key, value);
    url
}

/// A design hung the window in a dialog loop ([`js_dialogs`]'s gate saw
/// it): end the web content process and open the app again on the same
/// page, the preview stopped.
#[cfg(target_os = "macos")]
fn stop_hung_page(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    match window.url() {
        Ok(url) => restart_page(app, stopped_preview_url(&url)),
        Err(e) => tracing::warn!(error = %e, "cannot read the page's address"),
    }
}

/// View ▸ Reload (⌘R): the same page, loaded again from the native side.
/// It ends the web content process first, so it works on a page that
/// hangs the window, which a plain reload would wait on.
fn reload_from_menu(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window("main") {
        match window.url() {
            Ok(url) => restart_page(app, url),
            Err(e) => tracing::warn!(error = %e, "cannot read the page's address"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Ends the main window's web content process and loads `then` in a new
/// one ([`on_page_terminated`]). On the main thread.
#[cfg(target_os = "macos")]
fn restart_page(app: &AppHandle, then: Url) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Some(slot) = app.try_state::<AfterRestart>() {
        if let Ok(mut next) = slot.0.lock() {
            *next = Some(then);
        }
    }
    let killed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&killed);
    if let Err(e) = window.with_webview(move |webview| {
        flag.store(web_process::kill(webview.inner()), Ordering::SeqCst);
    }) {
        tracing::warn!(error = %e, "cannot reach the webview");
    }
    // WebKit may not say the process ended (or this WebKit cannot end
    // it): load the page anyway.
    let wait = if killed.load(Ordering::SeqCst) {
        RESTART_GRACE
    } else {
        std::time::Duration::ZERO
    };
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || load_after_restart(&app, false));
    });
}

/// WebKit says the main window's web content process ended: load the page
/// [`restart_page`] left, or reload the one shown (a crash).
#[cfg(target_os = "macos")]
fn on_page_terminated(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || load_after_restart(&handle, true));
}

/// Loads the page [`restart_page`] left, if still there; else, when
/// `crashed`, reloads the page shown.
#[cfg(target_os = "macos")]
fn load_after_restart(app: &AppHandle, crashed: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let next = app
        .try_state::<AfterRestart>()
        .and_then(|slot| slot.0.lock().ok().and_then(|mut next| next.take()));
    let loaded = match next {
        Some(url) => window.navigate(url),
        None if crashed => window.reload(),
        None => return,
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
const APP_COMMAND_PERMISSIONS: [&str; 10] = [
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
];

/// The main window stays on the daemon's origin (and the bundled splash
/// shown before it); anything else (a link in a design, a redirect) is
/// refused rather than loaded with the app's IPC. A frame's own document
/// (`about:srcdoc`, `about:blank`) never leaves the app and keeps the
/// preview's CSP, so a design's `<iframe srcdoc>` loads (QA-062).
fn allowed_navigation(url: &Url, origin: Option<&str>) -> bool {
    let splash = url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost");
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
        .manage(opened);
    // The page loads again once its web content process ends: after a
    // design hung it (QA-067), View ▸ Reload, or a crash.
    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(AfterRestart::default())
        .on_web_content_process_terminate(|webview| on_page_terminated(webview.app_handle()));
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
            reset_dialog_gate
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
                        #[cfg(target_os = "macos")]
                        if payload.event() == tauri::webview::PageLoadEvent::Started {
                            let _ = window.with_webview(|webview| {
                                js_dialogs::page_loaded(webview.inner());
                            });
                        }
                        #[cfg(not(target_os = "macos"))]
                        let _ = (window, payload);
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
                let on_hung = handle.clone();
                if let Err(e) = window.with_webview(move |webview| {
                    js_dialogs::install(webview.inner(), move || {
                        let app = on_hung.clone();
                        let _ = on_hung.run_on_main_thread(move || stop_hung_page(&app));
                    });
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
    fn a_hung_design_reopens_its_studio_with_the_preview_stopped() {
        let url = |s: &str| Url::parse(s).unwrap();
        let base = "http://127.0.0.1:5173";
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/projects/p1"))).as_str(),
            format!("{base}/projects/p1?preview=stopped")
        );
        // Other query kept, the mark not doubled, the fragment kept.
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/projects/p1?a=1&preview=stopped#x")))
                .as_str(),
            format!("{base}/projects/p1?a=1&preview=stopped#x")
        );
        // Present mode would loop again: back to the design's Studio.
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/present/p1?slide=3"))).as_str(),
            format!("{base}/projects/p1?slide=3&preview=stopped")
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
}
