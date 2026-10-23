//! Agent connection setup (track C3; the original's
//! `POST /api/agents/:agentId/companion/install`): installs the bundled
//! DeepSeek Harness `open-design` profile component with the user's own
//! `dsh` (see [`od_runtime::dsh_companion`]). Mounted with one `merge` in
//! [`crate::router`].

use crate::error::ApiErr;
use crate::state::AppState;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use od_runtime::def::DSH;
use od_runtime::dsh_companion::{self, SetupError};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/agents/{agent_id}/companion/install", post(install))
}

/// One setup at a time: a second click waits for the first to finish.
fn setup_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// The bundle folder: `OD_DSH_RUNTIME_DIR` (for builds that ship it
/// elsewhere), else `<resources>/agent-runtimes/deepseek-harness`.
fn bundle_dir(state: &AppState) -> std::path::PathBuf {
    std::env::var_os("OD_DSH_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map_or_else(
            || state.config.resource_root.join(dsh_companion::BUNDLE_DIR),
            std::path::PathBuf::from,
        )
}

async fn install(State(state): State<AppState>, Path(agent_id): Path<String>) -> Response {
    if agent_id != DSH.id {
        return ApiErr::bad_request("This agent has no Open Design connection component.")
            .into_response();
    }
    let _one = setup_lock().lock().await;
    let engine = state.runs.engine.config();
    let bin = od_runtime::detect::resolve_bin(&DSH, &engine.path);
    let result = dsh_companion::install(
        bin.as_deref(),
        &bundle_dir(&state),
        &engine.path,
        &engine.base_env,
    )
    .await;
    match result {
        Ok((action, version)) => {
            let agent =
                od_runtime::detect::detect_in(&DSH, &engine.path, &engine.base_env, false).await;
            Json(json!({
                "ok": true,
                "action": action.as_str(),
                "agent": agent,
                "packageVersion": version,
            }))
            .into_response()
        }
        Err(e) => {
            let status = if e == SetupError::AgentNotInstalled {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            ApiErr::new(status, e.code(), e.to_string()).into_response()
        }
    }
}
