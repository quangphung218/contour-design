//! Plugin presets (spec 1.4): bundled `plugins/_official` atoms and
//! scenarios plus user plugins, read-only.
//!
//! - `GET  /api/plugins[?kind=scenario&includeHidden=1]`: list.
//! - `GET  /api/plugins/:id`: one plugin, with its `SKILL.md` body.
//! - `POST /api/plugins/:id/apply` `{inputs, locale}`: validate inputs and
//!   return the snapshot (with the prompt block) a run would get.
//!
//! A run applies a plugin through `pluginId` / `pluginInputs` on
//! `POST /api/runs` ([`applied_plugin`]).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use od_content::ContentIndex;
use od_content::plugins::{AppliedPlugin, ApplyError};
use od_contracts::{ChatRequest, ErrorCode};
use serde::Deserialize;
use serde_json::{Map, Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/plugins", get(list))
        .route("/api/plugins/{id}", get(info))
        .route("/api/plugins/{id}/apply", post(apply))
}

fn not_found(id: &str) -> ApiErr {
    ApiErr::new(
        StatusCode::NOT_FOUND,
        ErrorCode::NotFound,
        format!("unknown plugin {id:?}"),
    )
}

impl From<ApplyError> for ApiErr {
    fn from(e: ApplyError) -> Self {
        ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ValidationFailed,
            e.to_string(),
        )
    }
}

/// The plugin a run asked for, applied; `None` without `pluginId`.
///
/// # Errors
/// 404 for an unknown plugin, 422 for missing required inputs.
pub(crate) fn applied_plugin(
    content: &ContentIndex,
    req: &ChatRequest,
) -> ApiResult<Option<AppliedPlugin>> {
    let Some(id) = req
        .plugin_id
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
    else {
        return Ok(None);
    };
    let plugin = content.find_plugin(id).ok_or_else(|| not_found(id))?;
    let inputs = req.plugin_inputs.clone().unwrap_or_default();
    Ok(Some(plugin.apply(&inputs, req.locale.as_deref())?))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    include_hidden: Option<String>,
}

pub async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    let content = state.content.snapshot();
    let hidden = matches!(q.include_hidden.as_deref(), Some("1" | "true"));
    let plugins: Vec<&od_content::PluginEntry> = content
        .plugins
        .iter()
        .filter(|p| hidden || !p.hidden)
        .filter(|p| q.kind.as_deref().is_none_or(|k| p.kind == k))
        .collect();
    Json(json!({ "plugins": plugins }))
}

pub async fn info(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let content = state.content.snapshot();
    let plugin = content.find_plugin(&id).ok_or_else(|| not_found(&id))?;
    let mut v = json!(plugin);
    v["body"] = json!(plugin.body);
    v["manifest"] = Value::Object(plugin.manifest.clone());
    Ok(Json(json!({ "plugin": v })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBody {
    #[serde(default)]
    inputs: Map<String, Value>,
    #[serde(default)]
    locale: Option<String>,
}

pub async fn apply(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<ApplyBody>,
) -> ApiResult<Json<Value>> {
    let content = state.content.snapshot();
    let plugin = content.find_plugin(&id).ok_or_else(|| not_found(&id))?;
    let applied = plugin.apply(&body.inputs, body.locale.as_deref())?;
    Ok(Json(json!({
        "query": applied.query,
        "inputs": plugin.inputs,
        "projectMetadata": {
            "name": applied.plugin_title,
            "taskKind": applied.task_kind,
            "skillId": applied.skill_ids.first(),
            "designSystemId": applied.design_system_id,
            "craftRequires": applied.craft_requires,
        },
        "capabilitiesRequired": applied.capabilities_required,
        "appliedPlugin": applied,
    })))
}
