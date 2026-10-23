//! `POST /api/projects/:id/export/pptx-editable` (spec 1.3): the deck's
//! slides measured by the renderer become native text boxes, shapes and
//! pictures. Same body as the other `export/*` routes.

use crate::routes::export::{
    ExportErr, ExportResult, attachment, internal, parse_body, resolve_entry, scratch,
};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::{Extension, Router};
use od_contracts::ErrorCode;
use od_export::Renderer;
use std::sync::Arc;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/projects/{id}/export/pptx-editable",
        post(export_pptx_editable),
    )
}

pub async fn export_pptx_editable(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let req = parse_body(&body)?;
    let entry = resolve_entry(&state, id, &req).await?;
    let scratch = scratch(&state, &entry, &req).await?;
    let layouts = renderer.extract_layouts(&scratch.target).await?;
    let title = req
        .title
        .clone()
        .unwrap_or_else(|| entry.project.name.clone());
    let pptx = tokio::task::spawn_blocking(move || {
        od_export::pptx_editable::layouts_to_pptx(&layouts, &title)
    })
    .await
    .map_err(internal)?
    .map_err(|e| {
        ExportErr::new(
            StatusCode::BAD_GATEWAY,
            ErrorCode::UpstreamUnavailable,
            e.to_string(),
        )
    })?;
    Ok(attachment(
        od_export::pptx::PPTX_MIME,
        &entry.filename(req.title.as_deref(), "pptx"),
        pptx,
    ))
}
