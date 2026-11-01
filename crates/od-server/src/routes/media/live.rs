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
        .route("/api/live-artifacts/{aid}/template", get(template))
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

/// One line per live create, update or refresh, so the agent tool, the
/// MCP/API and the UI paths can be counted: `op`, `via`
/// (`tool_token` for an agent run, `api` without one, `ui`), `source`
/// (the artifact's refresh source type, `none`, or `unknown` on failure)
/// and `result`.
fn logged(op: &str, via: &str, result: ApiResult<Json<Value>>) -> ApiResult<Json<Value>> {
    let (source, outcome) = outcome(op, &result);
    tracing::info!(op, via, source = %source, result = outcome, "live artifact");
    result
}

/// The `source` and `result` fields of [`logged`].
fn outcome(op: &str, result: &ApiResult<Json<Value>>) -> (String, &'static str) {
    match result {
        Ok(Json(v)) => (
            v.pointer("/artifact/document/sourceJson/type")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_owned(),
            "ok",
        ),
        Err(e) => (
            "unknown".to_owned(),
            match e.status {
                StatusCode::NOT_FOUND => "not_found",
                StatusCode::UNPROCESSABLE_ENTITY | StatusCode::CONFLICT if op == "refresh" => {
                    "refresh_failed"
                }
                StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => "validation_error",
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "unauthorized",
                _ => "error",
            },
        ),
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

/// CSP of the rendered page: no scripts, and nothing loaded from off the
/// daemon (an `<img src="https://…?x={{data.secret}}">` would carry data
/// out). Same fetch rules as the file preview, minus scripts.
pub const LIVE_PREVIEW_CSP: &str = "sandbox; default-src 'none'; \
img-src 'self' data: blob:; media-src 'self' data: blob:; font-src 'self' data:; \
style-src 'self' 'unsafe-inline'; script-src 'none'; connect-src 'none'; \
form-action 'none'; base-uri 'none'; object-src 'none'";

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
        HeaderValue::from_static(LIVE_PREVIEW_CSP),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// The template source, for the Studio's code view (`text/plain`).
pub async fn template(
    State(state): State<AppState>,
    Path(aid): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Response> {
    let (_, store) = open(&state, q.project_id).await?;
    let html = live(store, move |s| s.template_html(&aid)).await?;
    let mut res = html.into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
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
    let result = async {
        let (_, store) = open(&state, q.project_id).await?;
        let input = update_input(body.input)?;
        let artifact = live(store, move |s| {
            s.update(&aid, input, body.template_html.as_deref(), None)
        })
        .await?;
        Ok(Json(json!({ "artifact": artifact })))
    }
    .await;
    logged("update", "ui", result)
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
    let result = async {
        let (root, store) = open(&state, q.project_id).await?;
        let out = live(store, move |s| s.refresh(&root, &aid)).await?;
        Ok(Json(json!(out)))
    }
    .await;
    logged("refresh", "ui", result)
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
    let mut via = "api";
    let result = async {
        let caller = tool_caller(
            &state,
            &headers,
            "/api/tools/live-artifacts/create",
            body.project_id.as_deref(),
        )?;
        if caller.run_id.is_some() {
            via = "tool_token";
        }
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
    .await;
    logged("create", via, result)
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
    let mut via = "api";
    let result = async {
        let caller = tool_caller(
            &state,
            &headers,
            "/api/tools/live-artifacts/update",
            body.project_id.as_deref(),
        )?;
        if caller.run_id.is_some() {
            via = "tool_token";
        }
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
    .await;
    logged("update", via, result)
}

pub async fn tool_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<ToolBody>,
) -> ApiResult<Json<Value>> {
    let mut via = "api";
    let result = async {
        let caller = tool_caller(
            &state,
            &headers,
            "/api/tools/live-artifacts/refresh",
            body.project_id.as_deref(),
        )?;
        if caller.run_id.is_some() {
            via = "tool_token";
        }
        let aid = artifact_id(&body)?;
        let (root, store) = open(&state, caller.project_id).await?;
        let out = live(store, move |s| s.refresh(&root, &aid)).await?;
        Ok(Json(json!(out)))
    }
    .await;
    logged("refresh", via, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_fields_name_the_source_and_the_result() {
        let ok = Ok(Json(
            json!({"artifact": {"document": {"sourceJson": {"type": "local_file"}}}}),
        ));
        assert_eq!(outcome("create", &ok), ("local_file".to_owned(), "ok"));
        let plain = Ok(Json(json!({"artifact": {"document": {}}})));
        assert_eq!(outcome("update", &plain).0, "none");
        let invalid = || {
            ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::ValidationFailed,
                "x",
            )
        };
        assert_eq!(outcome("create", &Err(invalid())).1, "validation_error");
        assert_eq!(outcome("refresh", &Err(invalid())).1, "refresh_failed");
        let conflict = ApiErr::new(StatusCode::CONFLICT, ErrorCode::Conflict, "no source");
        assert_eq!(
            outcome("refresh", &Err(conflict)),
            ("unknown".to_owned(), "refresh_failed")
        );
        let gone = ApiErr::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, "x");
        assert_eq!(outcome("update", &Err(gone)).1, "not_found");
    }
}
