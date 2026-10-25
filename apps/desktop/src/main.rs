//! Contour desktop shell (spec 5.1, 5.3).
//!
//! Runs `od-server` inside this process on a random loopback port and points
//! the window at it, so the UI talks to the same HTTP + SSE API as the CLI.
//! No sidecar, no IPC for data: only the native folder picker, the window
//! theme and the menus go through Tauri. The window opens on a splash page
//! (`static/index.html`) while the daemon starts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
use std::sync::{Arc, OnceLock};
use tauri::ipc::CapabilityBuilder;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

const OPEN_FOLDER_MENU_ID: &str = "open-folder";
const SHORTCUTS_MENU_ID: &str = "help-shortcuts";
const PALETTE_MENU_ID: &str = "help-palette";
const SETTINGS_MENU_ID: &str = "app-settings";
const DATA_FOLDER_MENU_ID: &str = "help-data-folder";
const DOCS_MENU_ID: &str = "help-docs";
const REPORT_MENU_ID: &str = "help-report-issue";
/// Display name (window title, dialogs); `tauri.conf.json` `productName`.
const APP_NAME: &str = "Contour";

/// The in-process daemon.
struct Daemon {
    state: AppState,
    url: Url,
    _lock: DaemonLock,
}

/// Bundled content: `Resources/content` in a packaged app, else the usual
/// lookup (`OD_RESOURCE_ROOT`, next to the binary, source tree).
fn resource_root(app: &AppHandle) -> PathBuf {
    app.path()
        .resource_dir()
        .ok()
        .map(|d| d.join("content"))
        .filter(|d| d.join("skills").is_dir())
        .unwrap_or_else(resolve_resource_root)
}

async fn start_daemon(resource_root: PathBuf) -> anyhow::Result<Daemon> {
    let data_dir = DataDir::from_env();
    let mut lock = DaemonLock::acquire(data_dir.root())?;
    let store = {
        let data_dir = data_dir.clone();
        tokio::task::spawn_blocking(move || Store::open(data_dir))
            .await?
            .context("opening the data store")?
    };
    let roots = ContentRoots::bundled(&resource_root).with_user_root(data_dir.root());
    let content = tokio::task::spawn_blocking(move || ContentStore::load(roots)).await?;
    let agent_path = od_runtime::login_path::agent_path().await;
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
        }),
        runs: Arc::new(RunContext::new(
            Arc::clone(&runs),
            od_runtime::EngineConfig {
                path: agent_path,
                login_shell: od_runtime::login_path::default_shell(),
                ..od_runtime::EngineConfig::from_env()
            },
        )),
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
        _lock: lock,
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

/// Menu actions the UI performs: `window` event `od:menu` with `detail`.
fn ui_action(app: &AppHandle, action: &str) {
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
            let _ = window.navigate(url);
        }
    });
}

/// IPC (the folder picker) for the main window, only while it shows the
/// daemon's exact origin. The IPC origin is the requesting document's
/// `Origin`, so sandboxed preview iframes (`Origin: null`) and pages of
/// other loopback ports never match. Replaces a static
/// `http://127.0.0.1:*` capability, which every local server shared.
fn ui_capability(origin: &str) -> CapabilityBuilder {
    CapabilityBuilder::new("daemon-ui")
        .window("main")
        .local(false)
        .remote(format!("{origin}/*"))
        .permission("core:default")
        .permission("core:window:allow-start-dragging")
        .permission("dialog:default")
}

/// The main window stays on the daemon's origin (and the bundled splash
/// shown before it); anything else (a link in a design, a redirect) is
/// refused rather than loaded with the app's IPC.
fn allowed_navigation(url: &Url, origin: Option<&str>) -> bool {
    let splash = url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost");
    splash || origin.is_some_and(|o| url.origin().ascii_serialization() == o)
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let open = MenuItem::with_id(
        app,
        OPEN_FOLDER_MENU_ID,
        "Open Folder…",
        true,
        Some("CmdOrCtrl+O"),
    )?;
    let project = Submenu::with_items(app, "Project", true, &[&open])?;
    menu.append(&project)?;
    let settings = MenuItem::with_id(
        app,
        SETTINGS_MENU_ID,
        "Settings…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let shortcuts = MenuItem::with_id(
        app,
        SHORTCUTS_MENU_ID,
        "Keyboard Shortcuts",
        true,
        Some("CmdOrCtrl+/"),
    )?;
    let palette = MenuItem::with_id(app, PALETTE_MENU_ID, "Command Palette…", true, None::<&str>)?;
    let data = MenuItem::with_id(
        app,
        DATA_FOLDER_MENU_ID,
        "Show Data Folder",
        true,
        None::<&str>,
    )?;
    // Help ▸ Documentation opens the guide bundled in the UI; Report Issue
    // copies a diagnostics summary and opens the log folder (no network).
    let docs = MenuItem::with_id(app, DOCS_MENU_ID, "Documentation", true, None::<&str>)?;
    let report = MenuItem::with_id(app, REPORT_MENU_ID, "Report Issue", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    // macOS: Settings goes in the app menu, after About.
    #[cfg(target_os = "macos")]
    if let Some(app_menu) = menu.items()?.first().and_then(|i| i.as_submenu().cloned()) {
        app_menu.insert(&settings, 2)?;
    }
    #[cfg(not(target_os = "macos"))]
    project.append(&settings)?;
    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[&docs, &separator, &report, &data, &shortcuts, &palette],
    )?;
    menu.append(&help)?;
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![pick_folder, set_theme])
        .menu(build_menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_FOLDER_MENU_ID => open_folder_from_menu(app),
            SHORTCUTS_MENU_ID => ui_action(app, "shortcuts"),
            PALETTE_MENU_ID => ui_action(app, "palette"),
            SETTINGS_MENU_ID => ui_action(app, "settings"),
            DATA_FOLDER_MENU_ID => show_data_folder(app),
            DOCS_MENU_ID => ui_action(app, "docs"),
            REPORT_MENU_ID => ui_action(app, "report-issue"),
            _ => {}
        })
        .setup(|app| {
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
            window.build()?;
            let root = resource_root(&handle);
            tauri::async_runtime::spawn(async move {
                let daemon = match start_daemon(root).await {
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
                // `RunEvent::Exit`, which ends the terminal tabs' shells.
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
        .build(tauri::generate_context!())
        .context("building the desktop app")?
        .run(|app, event| {
            // The terminal tabs' shells would outlive the app.
            if let RunEvent::Exit = event {
                if let Some(daemon) = app.try_state::<Daemon>() {
                    tauri::async_runtime::block_on(daemon.state.shutdown());
                }
                remove_webview_temp(&std::env::temp_dir());
            }
        });
    Ok(())
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
        for bad in [
            "http://127.0.0.1:7456/",
            "http://localhost:50123/",
            "https://127.0.0.1:50123/",
            "https://evil.example/",
            "file:///etc/passwd",
        ] {
            assert!(!ok(bad), "{bad}");
        }
    }
}
