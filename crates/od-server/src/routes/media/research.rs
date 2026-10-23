//! `POST /api/research/search` (track C3; the original's
//! `routes/media.ts`): shallow Tavily research with the key from Settings
//! ▸ Media, for agents (`od research search`) and the UI. Errors carry the
//! original's codes (`QUERY_REQUIRED`, `TAVILY_API_KEY_MISSING`, …).

use super::Deps;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Extension, Json, Router};
use od_media::{ConfigFile, ResearchRequest};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/research/search", post(search))
}

async fn search(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
    Json(body): Json<Value>,
) -> Response {
    let req: ResearchRequest = serde_json::from_value(body).unwrap_or_default();
    let config = ConfigFile::in_dir(state.store.data_dir().root());
    let http = match od_media::provider::http_client() {
        Ok(h) => h,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match od_media::research::search(&http, &config, deps.env.as_ref(), &req).await {
        Ok(findings) => Json(json!({ "findings": findings })).into_response(),
        Err(e) => (
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(json!({"error": {"code": e.code, "message": e.message}})),
        )
            .into_response(),
    }
}
