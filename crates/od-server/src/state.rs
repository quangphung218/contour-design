use crate::error::{ApiErr, ApiResult};
use od_content::ContentStore;
use od_preview::{EditKeys, PreviewScopes, ProjectEvents};
use od_runs::Runs;
use od_runtime::{EngineConfig, RunEngine, ToolTokens};
use od_store::{Store, StoreError};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Default daemon port, shared with the CLI.
pub const DEFAULT_PORT: u16 = 7456;
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Daemon settings resolved at startup.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    /// Ports browsers may legitimately use as the origin (the bound port,
    /// plus `OD_WEB_PORT` when set). Filled in after binding.
    pub ports: Vec<u16>,
    /// Required when binding outside loopback.
    pub api_token: Option<String>,
    /// Extra browser origins from `OD_ALLOWED_ORIGINS`.
    pub allowed_origins: Vec<String>,
    pub resource_root: PathBuf,
    /// This daemon's base URL, exported to agents as `OD_DAEMON_URL`.
    pub daemon_url: Option<String>,
    /// `<exe> mcp` (Integrations ▸ Install) gets `--daemon-url` for this
    /// daemon. False in the desktop app: its port changes per launch, and
    /// its `mcp` finds the running app from `daemon.lock`.
    pub mcp_daemon_url: bool,
}

/// Run registry, engine and per-run tool tokens.
#[derive(Debug)]
pub struct RunContext {
    pub registry: Arc<Runs>,
    pub engine: RunEngine,
    pub tokens: ToolTokens,
    /// Runs canceled after registering but before their agent launched.
    pending_cancels: Mutex<HashSet<String>>,
}

impl RunContext {
    #[must_use]
    pub fn new(registry: Arc<Runs>, engine: EngineConfig) -> Self {
        Self {
            registry,
            engine: RunEngine::new(engine),
            tokens: ToolTokens::default(),
            pending_cancels: Mutex::default(),
        }
    }

    /// Cancel requests for runs not yet in the engine. Lock it around
    /// `engine.cancel` / `engine.launch` so neither side misses the other.
    pub fn pending_cancels(&self) -> MutexGuard<'_, HashSet<String>> {
        self.pending_cancels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Preview scopes and the per-project event hub (watchers and
/// `conversation-created`), shared by every route.
#[derive(Clone, Default)]
pub struct Preview {
    pub scopes: Arc<PreviewScopes>,
    pub edit_keys: Arc<EditKeys>,
    pub events: Arc<ProjectEvents>,
}

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub content: Arc<ContentStore>,
    pub config: Arc<ServerConfig>,
    pub runs: Arc<RunContext>,
    pub preview: Preview,
    /// Terminal tabs and the other platform routes' state.
    pub platform: crate::routes::platform::Platform,
}

impl AppState {
    /// Ends what the daemon started that would outlive it: the terminal
    /// tabs' shells and their jobs. Call once the server stopped serving.
    pub async fn shutdown(&self) {
        self.platform.terminals.close_all().await;
    }

    /// Runs blocking store work off the async runtime.
    pub async fn blocking<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
    {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
            .map_err(ApiErr::from)
    }
}
