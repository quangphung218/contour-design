//! Track W2-media routes (spec 1.3, 1.4, 4.2), mounted with one `merge` in
//! [`crate::router`]:
//!
//! - `POST /api/projects/:id/export/mp4`: HyperFrames → MP4 ([`mp4`]).
//! - `/api/live-artifacts/*`, `/api/tools/live-artifacts/*` ([`live`]).

pub mod live;
pub mod mp4;

use crate::state::AppState;
use axum::routing::post;
use axum::{Extension, Router};
use od_export::Renderer;
use std::sync::Arc;

/// Routes with the Chrome renderer.
pub fn routes() -> Router<AppState> {
    routes_with(Arc::new(od_render_cdp::CdpRenderer::new()))
}

/// Routes with any renderer (tests use a fake).
pub fn routes_with(renderer: Arc<dyn Renderer>) -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/export/mp4", post(mp4::export_mp4))
        .layer(Extension(renderer))
        .merge(live::routes())
}
