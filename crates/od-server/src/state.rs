use crate::error::{ApiErr, ApiResult};
use od_content::ContentStore;
use od_store::{Store, StoreError};
use std::path::PathBuf;
use std::sync::Arc;

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
}

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub content: Arc<ContentStore>,
    pub config: Arc<ServerConfig>,
}

impl AppState {
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
