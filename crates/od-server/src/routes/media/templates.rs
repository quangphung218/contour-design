//! Media prompt templates (spec 1.4): `GET /api/prompt-templates` lists
//! them without the prompt text; `GET /api/prompt-templates/:surface/:id`
//! returns one with its prompt. Each carries its `source` (license and
//! author) for attribution.

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/prompt-templates", get(list))
        .route("/api/prompt-templates/{surface}/{id}", get(get_one))
}

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    surface: Option<String>,
}

pub async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    let content = state.content.snapshot();
    let templates: Vec<Value> = content
        .prompt_templates
        .iter()
        .filter(|t| q.surface.as_deref().is_none_or(|s| t.surface == s))
        .map(|t| {
            let mut v = json!(t);
            if let Some(o) = v.as_object_mut() {
                o.remove("prompt");
            }
            v
        })
        .collect();
    Json(json!({ "promptTemplates": templates }))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path((surface, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let content = state.content.snapshot();
    let t = content
        .prompt_templates
        .iter()
        .find(|t| t.surface == surface && t.id == id)
        .ok_or_else(|| ApiErr::not_found("prompt template not found"))?;
    Ok(Json(json!({ "promptTemplate": t })))
}
