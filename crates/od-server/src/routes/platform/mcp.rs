//! External MCP servers (spec 1.5 "MCP client"), backed by `od-mcp-client`:
//!
//! - `GET /api/mcp/servers`: the servers, env and header values masked,
//!   plus whether OAuth is connected (never the token).
//! - `PUT /api/mcp/servers` `{servers}` replaces the list; masked values
//!   keep what was stored. Tokens of removed servers are dropped.
//! - `POST /api/mcp/servers/:sid/test` connects and lists tools.
//! - `POST /api/mcp/servers/:sid/oauth/start` `{open?}` → `{authorizeUrl}`;
//!   with `open: true` (desktop) the daemon opens it in the system browser.
//! - `GET /api/mcp/oauth/callback` is the loopback redirect target.
//! - `DELETE /api/mcp/servers/:sid/oauth` signs out.
//!
//! [`for_agent`] turns the enabled servers into the spawning agent's own
//! config format.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Router};
use od_mcp_client::client::ToolInfo;
use od_mcp_client::oauth::{self, Flows};
use od_mcp_client::{McpConfig, McpError, McpServer, OAuthStatus, TokenStore, config, wiring};
use od_runtime::McpLaunch;
use od_runtime::def::McpInjection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

pub const CALLBACK_PATH: &str = "/api/mcp/oauth/callback";

/// One token store per data dir, so every writer shares its lock.
fn token_store(data_root: &FsPath) -> Arc<TokenStore> {
    static STORES: LazyLock<Mutex<HashMap<PathBuf, Arc<TokenStore>>>> =
        LazyLock::new(Mutex::default);
    let path = od_mcp_client::tokens_path(data_root);
    let mut stores = STORES.lock().unwrap_or_else(PoisonError::into_inner);
    Arc::clone(
        stores
            .entry(path.clone())
            .or_insert_with(|| Arc::new(TokenStore::new(path))),
    )
}

/// State of the MCP routes.
#[derive(Clone)]
pub struct Mcp {
    config_path: PathBuf,
    tokens: Arc<TokenStore>,
    flows: Arc<Flows>,
    http: reqwest::Client,
    /// Serializes read-modify-write of the config file.
    write: Arc<tokio::sync::Mutex<()>>,
}

impl Mcp {
    /// `None` when no HTTP client can be built (TLS backend failure).
    #[must_use]
    pub fn new(state: &AppState) -> Option<Self> {
        let root = state.store.data_dir().root();
        wiring::wipe_run_configs(root);
        let http = oauth::http_client()
            .inspect_err(|e| tracing::error!(error = %e, "MCP client disabled"))
            .ok()?;
        Some(Self {
            config_path: od_mcp_client::config_path(root),
            tokens: token_store(root),
            flows: Arc::default(),
            http,
            write: Arc::default(),
        })
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mcp/servers", get(list).put(replace))
        .route("/api/mcp/servers/{sid}/test", post(test))
        .route("/api/mcp/servers/{sid}/oauth/start", post(oauth_start))
        .route("/api/mcp/servers/{sid}/oauth", delete(oauth_forget))
        .route(CALLBACK_PATH, get(oauth_callback))
}

fn api_err(e: McpError) -> ApiErr {
    match e {
        McpError::Invalid(m) => ApiErr::bad_request(m),
        McpError::Io(err) => {
            tracing::error!(error = %err, "MCP config I/O");
            ApiErr::internal("could not read or write the MCP config")
        }
        other => ApiErr::new(
            StatusCode::BAD_GATEWAY,
            od_contracts::ErrorCode::UpstreamUnavailable,
            other.to_string(),
        ),
    }
}

async fn load(mcp: &Mcp) -> ApiResult<McpConfig> {
    let path = mcp.config_path.clone();
    tokio::task::spawn_blocking(move || config::load(&path))
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?
        .map_err(api_err)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    #[serde(flatten)]
    pub server: McpServer,
    pub oauth: OAuthStatus,
}

#[derive(Debug, Serialize)]
pub struct ServersResponse {
    pub servers: Vec<ServerView>,
}

fn view(mcp: &Mcp, cfg: &McpConfig) -> ServersResponse {
    ServersResponse {
        servers: cfg
            .servers
            .iter()
            .map(|s| ServerView {
                server: s.masked(),
                oauth: mcp.tokens.status(&s.id),
            })
            .collect(),
    }
}

async fn list(Extension(mcp): Extension<Mcp>) -> ApiResult<ApiJson<ServersResponse>> {
    let cfg = load(&mcp).await?;
    Ok(ApiJson(view(&mcp, &cfg)))
}

async fn replace(
    Extension(mcp): Extension<Mcp>,
    ApiJson(mut next): ApiJson<McpConfig>,
) -> ApiResult<ApiJson<ServersResponse>> {
    let _guard = mcp.write.lock().await;
    let old = load(&mcp).await?;
    next.unmask(&old).map_err(api_err)?;
    next.validate().map_err(api_err)?;
    let (path, tokens, cfg) = (
        mcp.config_path.clone(),
        Arc::clone(&mcp.tokens),
        next.clone(),
    );
    // A token is kept only for a server still at the URL it was given for.
    tokio::task::spawn_blocking(move || {
        config::save(&path, &cfg)?;
        tokens.retain(|id| {
            cfg.find(id)
                .is_some_and(|s| old.find(id).is_some_and(|o| o.url == s.url))
        })
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(api_err)?;
    tracing::info!(servers = next.servers.len(), "MCP servers saved");
    Ok(ApiJson(view(&mcp, &next)))
}

async fn server(mcp: &Mcp, sid: &str) -> ApiResult<McpServer> {
    load(mcp)
        .await?
        .find(sid)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("MCP server not found"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResponse {
    pub ok: bool,
    pub tools: Vec<ToolInfo>,
    pub needs_auth: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

async fn test(
    Extension(mcp): Extension<Mcp>,
    Path(sid): Path<String>,
) -> ApiResult<ApiJson<TestResponse>> {
    let s = server(&mcp, &sid).await?;
    let bearer = oauth::access_token(&mcp.http, &mcp.tokens, &s.id, od_runs::now_ms()).await;
    let res = od_mcp_client::client::list_tools(&s, &mcp.http, bearer.as_deref()).await;
    Ok(ApiJson(match res {
        Ok(tools) => TestResponse {
            ok: true,
            tools,
            needs_auth: false,
            error: None,
        },
        Err(e) => TestResponse {
            ok: false,
            tools: Vec::new(),
            needs_auth: matches!(e, McpError::NeedsAuth { .. }),
            error: Some(e.to_string()),
        },
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartResponse {
    pub authorize_url: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthStartRequest {
    #[serde(default)]
    open: bool,
}

async fn oauth_start(
    State(state): State<AppState>,
    Extension(mcp): Extension<Mcp>,
    Path(sid): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<ApiJson<OAuthStartResponse>> {
    let req: OAuthStartRequest = if body.is_empty() {
        OAuthStartRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiErr::bad_request(e.to_string()))?
    };
    let s = server(&mcp, &sid).await?;
    if s.auth != od_mcp_client::AuthMode::Oauth {
        return Err(ApiErr::bad_request("this server is not set to OAuth"));
    }
    let base = state
        .config
        .daemon_url
        .as_deref()
        .ok_or_else(|| ApiErr::internal("daemon url unknown"))?;
    let redirect_uri = format!("{}{CALLBACK_PATH}", base.trim_end_matches('/'));
    // A 401 from the server may point at its metadata.
    let hint = match od_mcp_client::client::list_tools(&s, &mcp.http, None).await {
        Err(McpError::NeedsAuth { resource_metadata }) => resource_metadata,
        _ => None,
    };
    let authorize_url = oauth::begin(
        &mcp.http,
        &s,
        &mcp.tokens,
        &mcp.flows,
        &redirect_uri,
        hint.as_deref(),
    )
    .await
    .map_err(api_err)?;
    tracing::info!(server = %s.id, "MCP OAuth started");
    if req.open {
        // `begin` already checked the URL is https (or loopback http).
        let program = crate::routes::handoff::opener();
        let mut child = tokio::process::Command::new(&program)
            .arg(&authorize_url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| ApiErr::internal(format!("cannot run {program}: {e}")))?;
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
    }
    Ok(ApiJson(OAuthStartResponse { authorize_url }))
}

async fn oauth_forget(
    Extension(mcp): Extension<Mcp>,
    Path(sid): Path<String>,
) -> ApiResult<ApiJson<od_contracts::OkResponse>> {
    let tokens = Arc::clone(&mcp.tokens);
    tokio::task::spawn_blocking(move || tokens.remove(&sid))
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?
        .map_err(api_err)?;
    Ok(ApiJson(od_contracts::OkResponse { ok: true }))
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// A fixed page: nothing from the query is echoed back.
fn callback_page(status: StatusCode, ok: bool) -> Response {
    let (title, body) = if ok {
        (
            "Connected",
            "The MCP server is connected. You can close this tab and return to Nét.",
        )
    } else {
        (
            "Sign-in failed",
            "The MCP server could not be connected. Return to Nét and try again.",
        )
    };
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title>\
         <body style=\"font:16px system-ui;margin:3rem\"><h1>{title}</h1><p>{body}</p>"
    );
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

async fn oauth_callback(
    Extension(mcp): Extension<Mcp>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    // The state is the CSRF check: unknown, expired or reused → refused.
    let Some(flow) = q.state.as_deref().and_then(|s| mcp.flows.take(s)) else {
        tracing::warn!("MCP OAuth callback with an unknown state");
        return callback_page(StatusCode::BAD_REQUEST, false);
    };
    let (Some(code), None) = (q.code.as_deref(), q.error.as_deref()) else {
        tracing::warn!(server = %flow.server_id, "MCP OAuth denied or without a code");
        return callback_page(StatusCode::BAD_REQUEST, false);
    };
    match oauth::finish(&mcp.http, &flow, code, &mcp.tokens, od_runs::now_ms()).await {
        Ok(()) => {
            tracing::info!(server = %flow.server_id, "MCP OAuth connected");
            callback_page(StatusCode::OK, true)
        }
        Err(e) => {
            tracing::warn!(server = %flow.server_id, error = %e, "MCP OAuth token exchange failed");
            callback_page(StatusCode::BAD_GATEWAY, false)
        }
    }
}

/// What one run passes to its agent.
#[derive(Debug, Default)]
pub struct AgentWiring {
    pub launch: McpLaunch,
    pub env: Vec<(OsString, OsString)>,
    /// Per-run config file to delete when the run ends.
    pub file: Option<PathBuf>,
}

/// The enabled servers in the format of `format`'s agent. Problems are
/// logged and the run goes on without external servers.
pub async fn for_agent(state: &AppState, injection: McpInjection, run_id: &str) -> AgentWiring {
    let root = state.store.data_dir().root().to_path_buf();
    let cfg = match config::load(&od_mcp_client::config_path(&root)) {
        Ok(cfg) if cfg.servers.iter().any(|s| s.enabled) => cfg,
        Ok(_) => return AgentWiring::default(),
        Err(e) => {
            tracing::warn!(error = %e, "ignoring the MCP config");
            return AgentWiring::default();
        }
    };
    let tokens = token_store(&root);
    let mut bearer = HashMap::new();
    if let Ok(http) = oauth::http_client() {
        for s in cfg
            .servers
            .iter()
            .filter(|s| s.enabled && s.auth == od_mcp_client::AuthMode::Oauth)
        {
            if let Some(t) = oauth::access_token(&http, &tokens, &s.id, od_runs::now_ms()).await {
                bearer.insert(s.id.clone(), t);
            }
        }
    }
    let built = wiring::build(&cfg.servers, &bearer);
    let mut out = AgentWiring::default();
    match injection {
        McpInjection::ClaudeConfig => {
            if let Some(doc) = &built.claude {
                match wiring::write_run_config(&root, run_id, doc) {
                    Ok(path) => {
                        out.launch.args = vec![
                            "--mcp-config".to_owned(),
                            path.to_string_lossy().into_owned(),
                        ];
                        out.file = Some(path);
                    }
                    Err(e) => tracing::warn!(error = %e, "writing the run's MCP config"),
                }
            }
        }
        McpInjection::Acp => out.launch.acp_servers = built.acp,
        McpInjection::OpenCodeEnv(var) => {
            if let Some(doc) = &built.opencode {
                out.env.push((var.into(), doc.to_string().into()));
            }
        }
        McpInjection::None => {}
    }
    out
}
