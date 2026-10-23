//! Track W3-content routes (spec 1.3, 1.4, 1.5), mounted with one `merge`
//! in [`crate::router`]:
//!
//! - `POST /api/design-systems/generate`, `GET /api/design-systems/jobs[/:id]`:
//!   an agent writes a new design system from a brief ([`design_systems`]).
//! - `GET /api/atoms`, `GET /api/plugins/:id/pipeline`: the atom catalogue
//!   and a plugin's validated pipeline ([`pipeline`]).
//! - `/api/media/tasks*`, `POST /api/projects/:id/media/tasks`: queued media
//!   generation with a concurrency limit, cancel and restore ([`media_tasks`]).
//! - `POST /api/projects/:id/export/pptx-editable` ([`pptx`]).

pub mod design_systems;
pub mod media_tasks;
pub mod pipeline;
pub mod pptx;

use crate::state::AppState;
use axum::{Extension, Router};
use od_export::Renderer;
use od_media::{EnvLookup, Limits};
use std::sync::Arc;

/// What the routes need besides [`AppState`]; tests swap in fakes.
#[derive(Clone)]
pub struct Deps {
    pub renderer: Arc<dyn Renderer>,
    /// Provider key lookup besides `media-config.json`.
    pub env: Arc<EnvLookup>,
    pub limits: Limits,
    /// Media generations running at once (`OD_MEDIA_CONCURRENCY`).
    pub media_concurrency: usize,
}

impl Default for Deps {
    fn default() -> Self {
        Self {
            renderer: Arc::new(od_render_cdp::CdpRenderer::new()),
            env: Arc::new(od_media::process_env),
            limits: Limits::default(),
            media_concurrency: std::env::var("OD_MEDIA_CONCURRENCY")
                .ok()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(od_media::tasks::DEFAULT_CONCURRENCY),
        }
    }
}

pub fn routes(state: &AppState) -> Router<AppState> {
    routes_with(state, Deps::default())
}

/// Builds the routes; the media queue restores persisted tasks here.
pub fn routes_with(state: &AppState, deps: Deps) -> Router<AppState> {
    let queue = media_tasks::open_queue(state, &deps);
    Router::new()
        .merge(design_systems::routes())
        .merge(pipeline::routes())
        .merge(media_tasks::routes().layer(Extension(queue)))
        .merge(pptx::routes().layer(Extension(Arc::clone(&deps.renderer))))
        .layer(Extension(Arc::new(design_systems::Jobs::default())))
}
