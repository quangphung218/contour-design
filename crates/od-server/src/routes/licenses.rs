//! Settings ▸ About ▸ Licenses: `GET /api/licenses/:name` returns one of the
//! license files as plain text. They sit next to the content root: the repo
//! root in a source checkout, `Resources/` in the packaged app
//! (`apps/desktop/tauri.conf.json` bundles them there).

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};

/// The files the app ships and serves, by name.
pub const FILES: [&str; 3] = ["LICENSE", "NOTICE", "THIRD-PARTY-LICENSES"];

pub async fn get(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<Response> {
    let name = FILES
        .into_iter()
        .find(|f| *f == name)
        .ok_or_else(|| ApiErr::not_found("unknown license file"))?;
    let dir = state
        .config
        .resource_root
        .parent()
        .ok_or_else(|| ApiErr::not_found("license file not found"))?;
    let text = tokio::fs::read_to_string(dir.join(name))
        .await
        .map_err(|_| ApiErr::not_found("license file not found"))?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        text,
    )
        .into_response())
}
