//! HTTP API of the open-design-rs daemon.
//!
//! Binds loopback by default. Binding elsewhere requires `OD_API_TOKEN`.
//! Browser requests pass the origin guard (spec 2.2).

pub mod error;
pub mod guard;
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

pub use state::{AppState, DEFAULT_HOST, DEFAULT_PORT, ServerConfig};

/// Request bodies up to 200 MiB (largest expected design asset).
const MAX_BODY_BYTES: usize = 200 * 1024 * 1024;

/// Builds the API router over `state`.
pub fn router(state: AppState) -> Router {
    use routes::{content, files, projects, raw, system};
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
        .route("/api/skills", get(content::list_skills))
        .route("/api/skills/{id}", get(content::get_skill))
        .route("/api/design-templates", get(content::list_design_templates))
        .route(
            "/api/design-templates/{id}",
            get(content::get_design_template),
        )
        .route("/api/design-systems", get(content::list_design_systems))
        .route("/api/design-systems/{id}", get(content::get_design_system))
        .route("/api/craft", get(content::list_craft))
        .route("/api/craft/{id}", get(content::get_craft))
        .merge(routes::ui::routes());
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

/// Opens the store, indexes content, binds and serves until Ctrl-C.
///
/// # Errors
/// Refuses a non-loopback bind without `OD_API_TOKEN`; store, bind and I/O
/// errors.
pub async fn serve(opts: ServeOptions) -> anyhow::Result<()> {
    if !is_loopback_host(&opts.host) && opts.api_token.is_none() {
        anyhow::bail!("binding to {} requires OD_API_TOKEN", opts.host);
    }
    let started = std::time::Instant::now();
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
        }),
    };
    tracing::info!(
        url = %format!("http://{local}"),
        data_dir = %opts.data_dir.root().display(),
        startup_ms = started.elapsed().as_millis(),
        "od daemon listening"
    );
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
