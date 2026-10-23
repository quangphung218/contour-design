//! Atoms and plugin pipelines (spec 1.4).
//!
//! - `GET /api/atoms`: the first-party atom catalogue.
//! - `GET /api/plugins/:id/pipeline`: the plugin's pipeline, validated
//!   (422 with the reason when it does not parse), plus the devloop cap.

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use od_content::atoms::ATOMS;
use od_content::pipeline::{Pipeline, max_iterations_from_env};
use od_contracts::ErrorCode;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/atoms", get(atoms))
        .route("/api/plugins/{id}/pipeline", get(pipeline))
}

pub async fn atoms() -> Json<Value> {
    Json(json!({ "atoms": ATOMS }))
}

pub async fn pipeline(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let content = state.content.snapshot();
    let plugin = content
        .find_plugin(&id)
        .ok_or_else(|| ApiErr::not_found(format!("unknown plugin {id:?}")))?;
    let Some(raw) = &plugin.pipeline else {
        return Ok(Json(json!({ "pluginId": plugin.id, "pipeline": null })));
    };
    let parsed = Pipeline::parse(raw).map_err(|e| {
        ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ValidationFailed,
            e.to_string(),
        )
    })?;
    Ok(Json(json!({
        "pluginId": plugin.id,
        "pipeline": parsed,
        "maxIterations": max_iterations_from_env(),
    })))
}
