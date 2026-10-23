//! Open Design desktop shell (spec 5.1, 5.3).
//!
//! Runs `od-server` inside this process on a random loopback port and points
//! the window at it, so the UI talks to the same HTTP + SSE API as the CLI.
//! No sidecar, no IPC for data: only the native folder picker goes through
//! Tauri (`pick_folder` command and the File menu).

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
use std::sync::Arc;
use tauri::ipc::CapabilityBuilder;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

const OPEN_FOLDER_MENU_ID: &str = "open-folder";

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
            od_runtime::EngineConfig::from_env(),
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

/// File ▸ Open Folder…: import the folder and open its Studio.
fn open_folder_from_menu(app: &AppHandle) {
    let handle = app.clone();
    app.dialog().file().pick_folder(move |picked| {
        let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
            return;
        };
        let daemon = handle.state::<Daemon>();
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
        .permission("dialog:default")
}

/// The main window stays on the daemon's origin; anything else (a link in
/// a design, a redirect) is refused rather than loaded with the app's IPC.
fn allowed_navigation(url: &Url, origin: &str) -> bool {
    url.origin().ascii_serialization() == origin
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
    Ok(menu)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![pick_folder])
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id() == OPEN_FOLDER_MENU_ID {
                open_folder_from_menu(app);
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let daemon = match tauri::async_runtime::block_on(start_daemon(resource_root(&handle)))
            {
                Ok(daemon) => daemon,
                Err(e) => {
                    // Typically a CLI daemon already owns the data dir.
                    handle
                        .dialog()
                        .message(format!("{e:#}"))
                        .title("Open Design")
                        .blocking_show();
                    return Err(e.into());
                }
            };
            let origin = daemon.url.origin().ascii_serialization();
            app.add_capability(ui_capability(&origin))?;
            let nav_origin = origin.clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(daemon.url.clone()))
                .on_navigation(move |url| {
                    let ok = allowed_navigation(url, &nav_origin);
                    if !ok {
                        tracing::warn!(%url, "blocked a navigation away from the app");
                    }
                    ok
                })
                .title("Open Design")
                .inner_size(1360.0, 860.0)
                .min_inner_size(900.0, 600.0)
                .build()?;
            app.manage(daemon);
            // SIGTERM (logout, `kill`) and Ctrl-C quit through `RunEvent::Exit`,
            // which ends the terminal tabs' shells.
            tauri::async_runtime::spawn(async move {
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
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_stays_on_the_daemon_origin() {
        let origin = "http://127.0.0.1:50123";
        let ok = |u: &str| allowed_navigation(&Url::parse(u).unwrap(), origin);
        assert!(ok("http://127.0.0.1:50123/projects/p1"));
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
