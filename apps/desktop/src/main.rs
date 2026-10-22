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
    AppState, Preview, RunContext, ServerConfig, open_runs, resolve_resource_root, router,
};
use od_store::{DataDir, Store};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

const OPEN_FOLDER_MENU_ID: &str = "open-folder";

/// The in-process daemon.
struct Daemon {
    state: AppState,
    url: Url,
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
    let store = {
        let data_dir = data_dir.clone();
        tokio::task::spawn_blocking(move || Store::open(data_dir))
            .await?
            .context("opening the data store")?
    };
    let roots = ContentRoots::bundled(&resource_root).with_user_root(data_dir.root());
    let content = tokio::task::spawn_blocking(move || ContentStore::load(roots)).await?;
    let runs = open_runs(&store).await?;
    // Port 0: a fresh loopback port per launch, so a CLI daemon on 7456 or a
    // second window never collides.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let local = listener.local_addr()?;
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
    Ok(Daemon { state, url })
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
            let daemon = tauri::async_runtime::block_on(start_daemon(resource_root(&handle)))?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(daemon.url.clone()))
                .title("Open Design")
                .inner_size(1360.0, 860.0)
                .min_inner_size(900.0, 600.0)
                .build()?;
            app.manage(daemon);
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("running the desktop app")
}
