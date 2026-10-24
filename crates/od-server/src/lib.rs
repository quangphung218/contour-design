//! HTTP API of the open-design-rs daemon.
//!
//! Binds loopback by default. Binding elsewhere requires `OD_API_TOKEN`.
//! Browser requests pass the origin guard (spec 2.2).

pub mod error;
pub mod guard;
pub mod lock;
pub mod routes;
pub mod state;

use anyhow::Context;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use od_content::{ContentRoots, ContentStore, RESOURCE_ROOT_ENV};
use od_store::{DataDir, Store};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use lock::{DaemonLock, LockError};
pub use routes::platform::Platform;
pub use state::{AppState, DEFAULT_HOST, DEFAULT_PORT, Preview, RunContext, ServerConfig};

/// Request bodies up to 200 MiB (largest expected design asset).
const MAX_BODY_BYTES: usize = 200 * 1024 * 1024;

/// Builds the API router over `state`.
pub fn router(state: AppState) -> Router {
    use routes::{content, conversations, files, projects, raw, runs, system};
    let api = Router::new()
        .route("/api/health", get(system::health))
        .route("/api/version", get(system::version))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route(
            "/api/projects/{id}",
            get(projects::get)
                .patch(projects::update)
                .delete(projects::delete),
        )
        .route("/api/projects/{id}/duplicate", post(projects::duplicate))
        .route(
            "/api/projects/{id}/files",
            get(files::list).post(files::write),
        )
        .route(
            "/api/projects/{id}/files/{*path}",
            get(files::get).post(files::post).delete(files::delete),
        )
        .route("/api/projects/{id}/upload", post(files::upload))
        .route("/api/projects/{id}/raw/{*path}", get(raw::raw))
        .route("/api/projects/{id}/react/{*path}", get(raw::react))
        .route("/api/skills", get(content::list_skills))
        .route("/api/skills/{id}", get(content::get_skill))
        .route("/api/design-templates", get(content::list_design_templates))
        .route(
            "/api/design-templates/{id}",
            get(content::get_design_template),
        )
        .route(
            "/api/design-templates/{id}/example",
            get(content::get_design_template_example),
        )
        .route("/api/design-systems", get(content::list_design_systems))
        .route("/api/design-systems/{id}", get(content::get_design_system))
        .route("/api/craft", get(content::list_craft))
        .route("/api/craft/{id}", get(content::get_craft))
        .route(
            "/api/projects/{id}/conversations",
            get(conversations::list).post(conversations::create),
        )
        .route(
            "/api/projects/{id}/conversations/{cid}/messages",
            get(conversations::messages),
        )
        .route(
            "/api/projects/{id}/conversations/{cid}/messages/{mid}",
            axum::routing::put(conversations::put_message),
        )
        .route("/api/agents", get(runs::agents))
        .route("/api/runs", get(runs::list).post(runs::create))
        .route("/api/runs/{id}", get(runs::get))
        .route("/api/runs/{id}/events", get(runs::events))
        .route("/api/runs/{id}/cancel", post(runs::cancel))
        .route("/api/runs/{id}/steer", post(runs::steer))
        .merge(routes::ui::routes())
        .merge(routes::export::routes())
        .merge(routes::file_ops::routes())
        .merge(routes::media::routes())
        .merge(routes::comments::routes())
        .merge(routes::handoff::routes())
        .merge(routes::agent_w3::routes())
        .merge(routes::chat_ops::routes())
        .merge(routes::preview_tools::routes())
        .merge(routes::w3::routes(&state))
        .merge(routes::library::routes())
        .merge(routes::manage::routes())
        .merge(routes::manual_edit::routes())
        .merge(routes::c2::routes())
        .merge(routes::agent_setup::routes())
        .merge(routes::genui::routes());
    let api = routes::preview::mount(api, state.preview.clone());
    let api = routes::platform::mount(api, &state);
    api.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        guard::origin_guard,
    ))
    .layer(axum::middleware::from_fn_with_state(
        state.clone(),
        guard::token_guard,
    ))
    .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
    .with_state(state)
}

/// Finds bundled content: `OD_RESOURCE_ROOT`, then `content/` next to the
/// executable (or one level up), the working directory, and the source tree.
#[must_use]
pub fn resolve_resource_root() -> PathBuf {
    if let Some(root) = std::env::var_os(RESOURCE_ROOT_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("content"));
        candidates.push(exe_dir.join("../content"));
        candidates.push(exe_dir.join("../../content"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("content"));
    }
    let source_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    candidates
        .into_iter()
        .find(|c| c.join("skills").is_dir())
        .unwrap_or(source_tree)
}

/// Options for [`serve`].
#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub host: String,
    pub port: u16,
    pub data_dir: DataDir,
    pub resource_root: PathBuf,
    pub api_token: Option<String>,
    pub allowed_origins: Vec<String>,
}

impl ServeOptions {
    /// Defaults from the environment: `OD_DATA_DIR`, `OD_RESOURCE_ROOT`,
    /// `OD_API_TOKEN`, `OD_ALLOWED_ORIGINS`.
    #[must_use]
    pub fn from_env(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            data_dir: DataDir::from_env(),
            resource_root: resolve_resource_root(),
            api_token: std::env::var("OD_API_TOKEN").ok().filter(|t| !t.is_empty()),
            allowed_origins: std::env::var("OD_ALLOWED_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(|o| o.trim().trim_end_matches('/').to_owned())
                .filter(|o| !o.is_empty())
                .collect(),
        }
    }
}

fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Restores runs from `<data>/runs`: in-flight runs of a crashed daemon
/// are ended as failed, their leftover process groups killed, and their
/// assistant messages marked failed.
///
/// Takes the data dir's [`DaemonLock`]: only the sole daemon on a folder may
/// treat its running runs as orphans.
///
/// # Errors
/// When the runs folder cannot be listed.
pub async fn open_runs(store: &Store, _lock: &DaemonLock) -> anyhow::Result<Arc<od_runs::Runs>> {
    let dir = store.data_dir().runs_dir();
    let (runs, report) = tokio::task::spawn_blocking(move || od_runs::Runs::load(dir))
        .await?
        .context("loading runs")?;
    for (run_id, orphan) in &report.interrupted {
        if let Some(orphan) = orphan {
            if od_runtime::process::reap_orphan(orphan.pgid, orphan.bin.as_deref()) {
                tracing::warn!(run = %run_id, pgid = orphan.pgid, "killed an orphaned agent");
            }
        }
    }
    let failed = store.fail_interrupted_messages()?;
    tracing::info!(
        runs = report.loaded,
        interrupted = report.interrupted.len(),
        messages_failed = failed,
        "restored runs"
    );
    Ok(Arc::new(runs))
}

/// Resolves on Ctrl-C, or on Unix on SIGINT or SIGTERM (what launchd,
/// systemd and `kill` send). The handlers are in place once this returns.
///
/// # Errors
/// When a signal handler cannot be installed.
pub fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = ()>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut int = signal(SignalKind::interrupt())?;
        let mut term = signal(SignalKind::terminate())?;
        Ok(async move {
            tokio::select! {
                _ = int.recv() => {}
                _ = term.recv() => {}
            }
        })
    }
    #[cfg(not(unix))]
    Ok(async {
        let _ = tokio::signal::ctrl_c().await;
    })
}

/// Opens the store, indexes content, binds and serves until Ctrl-C or
/// SIGTERM, then ends the terminal tabs' shells.
///
/// # Errors
/// Refuses a non-loopback bind without `OD_API_TOKEN`; store, bind and I/O
/// errors.
pub async fn serve(opts: ServeOptions) -> anyhow::Result<()> {
    if !is_loopback_host(&opts.host) && opts.api_token.is_none() {
        anyhow::bail!("binding to {} requires OD_API_TOKEN", opts.host);
    }
    let started = std::time::Instant::now();
    let mut lock = DaemonLock::acquire(opts.data_dir.root())?;
    let data_dir = opts.data_dir.clone();
    let store = tokio::task::spawn_blocking(move || Store::open(data_dir))
        .await?
        .context("opening the data store")?;
    let roots = ContentRoots::bundled(&opts.resource_root).with_user_root(opts.data_dir.root());
    let content = tokio::task::spawn_blocking(move || ContentStore::load(roots)).await?;

    let listener = tokio::net::TcpListener::bind((opts.host.as_str(), opts.port))
        .await
        .with_context(|| format!("binding {}:{}", opts.host, opts.port))?;
    let local = listener.local_addr()?;
    // Before the daemon is announced: a signal right after is not missed.
    let stop = shutdown_signal()?;
    lock.record_url(&format!("http://{local}"))
        .context("recording the daemon url")?;
    let runs = open_runs(&store, &lock).await?;
    let mut ports = vec![local.port()];
    if let Some(web) = std::env::var("OD_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
    {
        if web != local.port() {
            ports.push(web);
        }
    }
    let state = AppState {
        store: Arc::new(store),
        content: Arc::new(content),
        config: Arc::new(ServerConfig {
            host: opts.host.clone(),
            ports,
            api_token: opts.api_token,
            allowed_origins: opts.allowed_origins,
            resource_root: opts.resource_root,
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
    tracing::info!(
        url = %format!("http://{local}"),
        data_dir = %opts.data_dir.root().display(),
        startup_ms = started.elapsed().as_millis(),
        "od daemon listening"
    );
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(stop)
        .await;
    state.shutdown().await;
    served?;
    Ok(())
}
