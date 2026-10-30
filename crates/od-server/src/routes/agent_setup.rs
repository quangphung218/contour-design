//! Agent connection setup (track C3): `POST /api/agents/:agentId/detect`
//! detects one agent again, so a setup dialog can check the user's own
//! install without rescanning every CLI. The original installs a bundled
//! DeepSeek Harness component here; this app does not ship it, and the
//! user installs `dsh` and its `open-design` profile themselves (see
//! [`od_runtime::dsh_companion`]). Mounted with one `merge` in
//! [`crate::router`].

use crate::error::ApiErr;
use crate::state::AppState;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/agents/{agent_id}/detect", post(detect))
}

async fn detect(State(state): State<AppState>, Path(agent_id): Path<String>) -> Response {
    let Some(def) = od_runtime::def::find(&agent_id) else {
        return ApiErr::not_found("Unknown agent.").into_response();
    };
    // The agent may have been upgraded: its next run probes its flags again.
    state.runs.engine.forget_capabilities();
    let engine = state.runs.engine.config();
    let agent = od_runtime::detect::detect_in(
        def,
        &engine.path,
        &engine.base_env,
        super::runs::byok_configured(&state),
    )
    .await;
    // Found here first (the user installed it from the setup dialog): setup
    // step 2, as in `GET /api/agents`. A saved API key is not an agent.
    if agent.available && def.id != od_runtime::def::BYOK.id {
        crate::routes::manage::usage::record_setup(&state, od_contracts::SetupStep::AgentFound)
            .await;
    }
    Json(json!({ "agent": agent })).into_response()
}
