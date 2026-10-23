//! Manual edit (spec 1.2, track C1-preview). Mounted with one `merge` in
//! [`crate::router`]. Writes go through `POST source-edits` (attributes,
//! element HTML, removal) and undo/redo through a guarded version restore.
//!
//! - `POST /api/projects/:id/source-element`: one element as the source has
//!   it (attributes in order, its exact HTML, its text), for the panel.

use super::preview_tools::{blocking, patch_err};
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::post;
use od_contracts::{SourceElementRequest, SourceElementResponse};
use od_preview::source_patch::read_element;
use od_store::paths::is_html_name;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/projects/{id}/source-element", post(source_element))
}

async fn source_element(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<SourceElementRequest>,
) -> ApiResult<ApiJson<SourceElementResponse>> {
    if !is_html_name(&req.file) {
        return Err(ApiErr::bad_request("only HTML files have elements"));
    }
    let element = blocking(&state, move |s| {
        let (_, bytes) = s.read_file(&id, &req.file)?;
        let html = std::str::from_utf8(&bytes)
            .map_err(|_| ApiErr::bad_request("HTML file is not valid UTF-8"))?;
        read_element(
            html,
            &req.selector,
            req.expect_tag.as_deref(),
            req.expect_text.as_deref(),
        )
        .map_err(patch_err)
    })
    .await?;
    Ok(ApiJson(element))
}
