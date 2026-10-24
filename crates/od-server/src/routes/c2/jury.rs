//! Design Jury ▸ Interrupt (the original's critique interrupt handler):
//! `POST /api/projects/:id/critique/:run_id/interrupt`.
//!
//! Stops the run, as the original aborts the agent: the jury's best closed
//! round ships as the fallback (`od_runtime::critique`). Answers 202
//! `{runId, accepted, prevStatus}`: `running` when this call stopped it,
//! `interrupted` when it was already stopped; 409 when the run ended some
//! other way; 404 for a run of another project.

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use od_contracts::{ErrorCode, RunStatus};
use serde_json::json;

pub async fn interrupt(
    State(state): State<AppState>,
    Path((project_id, run_id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let run = state
        .runs
        .registry
        .get(&run_id)
        .filter(|r| r.meta().project_id.as_deref() == Some(project_id.as_str()))
        .ok_or_else(|| ApiErr::not_found("run not found"))?;
    let prev = match run.status() {
        RunStatus::Running => {
            let mut pending = state.runs.pending_cancels();
            if !state.runs.engine.cancel(&run_id) {
                // Registered but not launched yet: the launch step ends it.
                pending.insert(run_id.clone());
            }
            "running"
        }
        RunStatus::Canceled => "interrupted",
        other => {
            let current = serde_json::to_value(other).unwrap_or_default();
            return Ok((
                StatusCode::CONFLICT,
                axum::Json(json!({ "error": {
                    "code": ErrorCode::Conflict,
                    "message": "the run already ended",
                    "currentStatus": current,
                } })),
            )
                .into_response());
        }
    };
    Ok((
        StatusCode::ACCEPTED,
        axum::Json(json!({ "runId": run_id, "accepted": true, "prevStatus": prev })),
    )
        .into_response())
}
