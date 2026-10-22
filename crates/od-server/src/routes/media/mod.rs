//! Track W2-media routes (spec 1.3, 1.4, 1.5, 4.2), mounted with one
//! `merge` in [`crate::router`]:
//!
//! - `POST /api/projects/:id/export/mp4`: HyperFrames → MP4 ([`mp4`]).
//! - `/api/live-artifacts/*`, `/api/tools/live-artifacts/*` ([`live`]).
//! - `/api/plugins*` ([`plugins`]); runs apply them via `pluginId`.
//! - `/api/media/*`, `POST /api/projects/:id/media/generate` ([`generate`]).
//! - `/api/prompt-templates*` ([`templates`]).

pub mod generate;
pub mod live;
pub mod mp4;
pub mod plugins;
pub mod templates;

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::post;
use axum::{Extension, Router};
use od_contracts::ErrorCode;
use od_export::Renderer;
use od_media::{EnvLookup, Limits};
use std::sync::Arc;

/// What the routes depend on besides [`AppState`]; tests swap in fakes.
#[derive(Clone)]
pub struct Deps {
    pub renderer: Arc<dyn Renderer>,
    /// Where provider keys are looked up besides `media-config.json`.
    pub env: Arc<EnvLookup>,
    pub limits: Limits,
}

impl Default for Deps {
    /// Chrome renderer, the process environment, default limits.
    fn default() -> Self {
        Self {
            renderer: Arc::new(od_render_cdp::CdpRenderer::new()),
            env: Arc::new(od_media::process_env),
            limits: Limits::default(),
        }
    }
}

pub fn routes() -> Router<AppState> {
    routes_with(Deps::default())
}

pub fn routes_with(deps: Deps) -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/export/mp4", post(mp4::export_mp4))
        .layer(Extension(Arc::clone(&deps.renderer)))
        .merge(generate::routes().layer(Extension(deps)))
        .merge(live::routes())
        .merge(plugins::routes())
        .merge(templates::routes())
}

/// Who is calling a tool endpoint: the project (and run) from the tool
/// token, else the `projectId` the local caller named.
pub(crate) struct ToolCaller {
    pub(crate) project_id: Option<String>,
    pub(crate) run_id: Option<String>,
}

pub(crate) fn tool_caller(
    state: &AppState,
    headers: &HeaderMap,
    path: &str,
    supplied: Option<&str>,
) -> ApiResult<ToolCaller> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| t.starts_with("odt_"));
    let Some(token) = bearer else {
        return Ok(ToolCaller {
            project_id: supplied.map(str::to_owned),
            run_id: None,
        });
    };
    let grant = state.runs.tokens.authorize(token, path).ok_or_else(|| {
        ApiErr::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthorized,
            "invalid or expired tool token",
        )
    })?;
    if let (Some(s), Some(g)) = (supplied, grant.project_id.as_deref()) {
        if s != g {
            return Err(ApiErr::forbidden(
                "projectId is derived from the tool token",
            ));
        }
    }
    Ok(ToolCaller {
        project_id: grant.project_id.or_else(|| supplied.map(str::to_owned)),
        run_id: Some(grant.run_id),
    })
}
