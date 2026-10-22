//! Live artifact routes (spec 1.5, 2.5): the UI/CLI family takes
//! `?projectId=`; the agent tool family (`/api/tools/live-artifacts/*`,
//! used by `od mcp live-artifacts`) takes the project from the run's
//! `OD_TOOL_TOKEN` when one is sent, else from `projectId`.

use super::tool_caller;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use od_contracts::ErrorCode;
use od_live::{CreateInput, LiveError, LiveStore, UpdateInput};
use od_store::files::ProjectRoot;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/live-artifacts", get(list))
        .route(
            "/api/live-artifacts/{aid}",
            get(get_one).patch(patch).delete(delete),
        )
        .route("/api/live-artifacts/{aid}/preview", get(preview))
        .route("/api/live-artifacts/{aid}/refreshes", get(refreshes))
        .route("/api/live-artifacts/{aid}/refresh", post(refresh))
        .route("/api/tools/live-artifacts/create", post(tool_create))
        .route("/api/tools/live-artifacts/list", get(tool_list))
        .route("/api/tools/live-artifacts/update", post(tool_update))
        .route("/api/tools/live-artifacts/refresh", post(tool_refresh))
}

impl From<LiveError> for ApiErr {
    fn from(e: LiveError) -> Self {
        let (status, code) = match &e {
            LiveError::NotFound => (StatusCode::NOT_FOUND, ErrorCode::NotFound),
            LiveError::Validation(_) | LiveError::Template(_) | LiveError::Source(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::ValidationFailed,
            ),
            LiveError::RefreshInProgress | LiveError::NotRefreshable(_) => {
                (StatusCode::CONFLICT, ErrorCode::Conflict)
            }
            LiveError::Corrupt(_) | LiveError::Io(_) => {
                tracing::error!(error = %e, "live artifact store error");
                (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError)
            }
        };
        ApiErr::new(status, code, e.to_string())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectQuery {
    #[serde(default)]
    project_id: Option<String>,
}

/// The project's root and its live store.
async fn open(state: &AppState, project_id: Option<String>) -> ApiResult<(ProjectRoot, LiveStore)> {
    let id = project_id
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| ApiErr::bad_request("projectId is required"))?;
    state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            let root = s.project_root(&project)?;
            let live = LiveStore::new(root.dir.clone(), project.id);
            Ok((root, live))
        })
        .await
}

/// Runs live-store work off the async runtime.
async fn live<T, F>(store: LiveStore, f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&LiveStore) -> od_live::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
        .map_err(ApiErr::from)
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    let (_, store) = open(&state, q.project_id).await?;
    let artifacts = live(store, |s| s.list()).await?;
    Ok(Json(json!({ "artifacts": artifacts })))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    let (_, store) = open(&state, q.project_id).await?;
    let artifact = live(store, move |s| s.get(&aid)).await?;
    Ok(Json(json!({ "artifact": artifact })))
}

/// The rendered page. Scripts are already refused at write time; the CSP
/// keeps it that way for anything a user edits by hand on disk.
pub async fn preview(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Response> {
    let (_, store) = open(&state, q.project_id).await?;
    let html = live(store, move |s| s.preview_html(&aid)).await?;
    let mut res = html.into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; script-src 'none'; object-src 'none'"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

pub async fn refreshes(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    let (_, store) = open(&state, q.project_id).await?;
    let refreshes = live(store, move |s| s.refreshes(&aid)).await?;
    Ok(Json(json!({ "refreshes": refreshes })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchBody {
    #[serde(flatten)]
    input: Value,
    #[serde(default)]
    template_html: Option<String>,
}

fn update_input(v: Value) -> ApiResult<UpdateInput> {
    serde_json::from_value(v).map_err(|e| {
        ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ValidationFailed,
            e.to_string(),
        )
    })
}

pub async fn patch(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
    ApiJson(body): ApiJson<PatchBody>,
) -> ApiResult<Json<Value>> {
    let (_, store) = open(&state, q.project_id).await?;
    let input = update_input(body.input)?;
    let artifact = live(store, move |s| {
        s.update(&aid, input, body.template_html.as_deref(), None)
    })
    .await?;
    Ok(Json(json!({ "artifact": artifact })))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    let (_, store) = open(&state, q.project_id).await?;
    live(store, move |s| s.delete(&aid)).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn refresh(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    let (root, store) = open(&state, q.project_id).await?;
    let out = live(store, move |s| s.refresh(&root, &aid)).await?;
    Ok(Json(json!(out)))
}

// ---------------------------------------------------------------------------
// Agent tool endpoints

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolBody {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    template_html: Option<String>,
    #[serde(default)]
    provenance_json: Option<Value>,
}

fn artifact_id(body: &ToolBody) -> ApiResult<String> {
    body.artifact_id
        .clone()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| ApiErr::bad_request("artifactId is required"))
}

pub async fn tool_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<ToolBody>,
) -> ApiResult<Json<Value>> {
    let caller = tool_caller(
        &state,
        &headers,
        "/api/tools/live-artifacts/create",
        body.project_id.as_deref(),
    )?;
    let input: CreateInput = serde_json::from_value(body.input.unwrap_or_else(|| json!({})))
        .map_err(|e| {
            ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::ValidationFailed,
                e.to_string(),
            )
        })?;
    let (_, store) = open(&state, caller.project_id).await?;
    let artifact = live(store, move |s| {
        s.create(
            input,
            body.template_html.as_deref(),
            body.provenance_json,
            caller.run_id,
        )
    })
    .await?;
    Ok(Json(json!({ "artifact": artifact })))
}

pub async fn tool_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Value>> {
    let caller = tool_caller(
        &state,
        &headers,
        "/api/tools/live-artifacts/list",
        q.project_id.as_deref(),
    )?;
    let (_, store) = open(&state, caller.project_id).await?;
    let artifacts = live(store, |s| s.list()).await?;
    Ok(Json(json!({ "artifacts": artifacts })))
}

pub async fn tool_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<ToolBody>,
) -> ApiResult<Json<Value>> {
    let caller = tool_caller(
        &state,
        &headers,
        "/api/tools/live-artifacts/update",
        body.project_id.as_deref(),
    )?;
    let aid = artifact_id(&body)?;
    let input = update_input(body.input.unwrap_or_else(|| json!({})))?;
    let (_, store) = open(&state, caller.project_id).await?;
    let artifact = live(store, move |s| {
        s.update(
            &aid,
            input,
            body.template_html.as_deref(),
            body.provenance_json,
        )
    })
    .await?;
    Ok(Json(json!({ "artifact": artifact })))
}

pub async fn tool_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<ToolBody>,
) -> ApiResult<Json<Value>> {
    let caller = tool_caller(
        &state,
        &headers,
        "/api/tools/live-artifacts/refresh",
        body.project_id.as_deref(),
    )?;
    let aid = artifact_id(&body)?;
    let (root, store) = open(&state, caller.project_id).await?;
    let out = live(store, move |s| s.refresh(&root, &aid)).await?;
    Ok(Json(json!(out)))
}
