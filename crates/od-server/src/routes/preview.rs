//! Preview routes (spec 4.1), backed by `od-preview`:
//!
//! - `GET /api/projects/:id/preview-url?file=` mints a short-lived scope.
//! - `GET /api/projects/:id/preview/<scope>/<path>` serves project files
//!   under the strict preview CSP.
//! - `GET /api/projects/:id/raw/<path>?bridge=1` (and the scoped route with
//!   the same query) returns HTML ≤ 2 MiB with the bridge injected; `raw`
//!   also gets a `<base href>` pointing at the project's live scope.
//!   `raw?bridge=1&key=<id>` also hands the bridge a fresh edit key the
//!   first time `<id>` is served; `POST /api/projects/:id/edit-keys/<id>`
//!   gives it to the host, once (see `od_preview::edit_key`).
//! - `GET /api/projects/:id/events` streams `ready`, `file-changed` and
//!   `conversation-created` over SSE.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::raw;
use crate::state::AppState;
pub use crate::state::Preview;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, middleware};
use od_contracts::ErrorCode;
use od_preview::{
    MAX_INJECT_BYTES, PREVIEW_CSP, PREVIEW_IFRAME_SANDBOX, PreviewUrlResponse, ProjectEvent,
    encode_path, encode_segment, inject_bridge_keyed, is_edit_key_id, is_scope_token,
};
use od_store::usage::UsageMark;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

/// Reports whether a response carries the bridge: `injected` or `skipped`.
pub const BRIDGE_HEADER: &str = "x-od-bridge";
const KEEPALIVE: Duration = Duration::from_secs(25);

/// Adds the preview routes to `api` and the bridge rewrite for `raw`.
pub fn mount(api: Router<AppState>, state: &AppState) -> Router<AppState> {
    api.route("/api/projects/{id}/preview-url", get(preview_url))
        .route("/api/projects/{id}/preview/{scope}/{*path}", get(scoped))
        .route("/api/projects/{id}/events", get(events))
        .route("/api/projects/{id}/edit-keys/{kid}", post(claim_edit_key))
        .layer(middleware::from_fn_with_state(state.clone(), raw_bridge))
        .layer(Extension(state.preview.clone()))
}

fn wants_bridge(query: Option<&str>) -> bool {
    query.is_some_and(|q| q.split('&').any(|kv| kv == "bridge=1" || kv == "bridge"))
}

/// The load id in `key=<id>`, if well formed.
fn edit_key_id(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .find_map(|kv| kv.strip_prefix("key="))
        .filter(|id| is_edit_key_id(id))
}

/// Whether the host page started this load, so it may get an edit key. A
/// sandboxed page navigating itself has an opaque origin: its requests say
/// `Sec-Fetch-Site: cross-site`. Without the header (an older engine, the
/// CLI) the load counts as the host's.
fn host_load(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .is_none_or(|site| matches!(site.as_bytes(), b"same-origin" | b"none"))
}

/// Injected bytes differ from the file, so file validators and ranges no
/// longer apply.
fn strip_validators(headers: &mut HeaderMap) {
    for name in [
        header::IF_NONE_MATCH,
        header::IF_MODIFIED_SINCE,
        header::RANGE,
        header::IF_RANGE,
    ] {
        headers.remove(name);
    }
}

fn is_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim_start().to_ascii_lowercase().starts_with("text/html"))
}

/// Splits `/api/projects/<id>/raw/<rest>` into `(id, rest)` (still encoded).
fn raw_target(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/api/projects/")?;
    let (id, rest) = rest.split_once('/')?;
    let file = rest.strip_prefix("raw/")?;
    (!id.is_empty() && !file.is_empty()).then_some((id, file))
}

/// Rewrites a 200 HTML response to carry the bridge (and the edit key
/// `issue` gives, asked only when the bridge goes in). Larger documents
/// and other responses pass through.
async fn with_bridge(
    res: Response,
    base_href: Option<&str>,
    issue: impl FnOnce() -> Option<String>,
) -> Response {
    if res.status() != StatusCode::OK || !is_html(res.headers()) {
        return res;
    }
    let declared = res
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_none_or(|len| len > MAX_INJECT_BYTES) {
        let mut res = res;
        res.headers_mut()
            .insert(BRIDGE_HEADER, HeaderValue::from_static("skipped"));
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_INJECT_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => return ApiErr::internal(format!("reading HTML: {err}")).into_response(),
    };
    let html = match inject_bridge_keyed(&bytes, base_href, issue().as_deref()) {
        Ok(html) => html,
        Err(err) => {
            tracing::warn!(error = %err, "bridge injection failed; serving the file as is");
            parts
                .headers
                .insert(BRIDGE_HEADER, HeaderValue::from_static("skipped"));
            return Response::from_parts(parts, Body::from(bytes));
        }
    };
    let headers = &mut parts.headers;
    for name in [header::ETAG, header::LAST_MODIFIED, header::ACCEPT_RANGES] {
        headers.remove(name);
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(html.len()));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(BRIDGE_HEADER, HeaderValue::from_static("injected"));
    Response::from_parts(parts, Body::from(html))
}

/// Middleware on `raw`: `?bridge=1` turns the file into a srcDoc-ready
/// document whose relative URLs resolve under the project's live scope.
/// The first such page the host loads is the project's first preview
/// (Settings ▸ About ▸ Your usage).
async fn raw_bridge(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    if req.method() != Method::GET || !wants_bridge(req.uri().query()) {
        return next.run(req).await;
    }
    let Some((id, file)) = raw_target(req.uri().path()) else {
        return next.run(req).await;
    };
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    // Project ids are URL-safe (`is_safe_id`), so the encoded form is the id.
    let scope = state.preview.scopes.acquire(id);
    let base_href = if dir.is_empty() {
        format!("/api/projects/{id}/preview/{}/", scope.token)
    } else {
        format!("/api/projects/{id}/preview/{}/{dir}/", scope.token)
    };
    // A page that names a fresh id on each of its own loads would get a
    // key each time and slip past the bridge's reload guard.
    let host = host_load(req.headers());
    let kid = edit_key_id(req.uri().query())
        .filter(|_| host)
        .map(str::to_owned);
    let id = id.to_owned();
    strip_validators(req.headers_mut());
    let res = next.run(req).await;
    if host && res.status() == StatusCode::OK && is_html(res.headers()) {
        let project = id.clone();
        if let Err(err) = state
            .blocking(move |s| s.mark_usage(&project, UsageMark::FirstPreview))
            .await
        {
            tracing::warn!(error = %err.message, "recording the first preview failed");
        }
    }
    let preview = &state.preview;
    with_bridge(res, Some(&base_href), || {
        kid.and_then(|kid| preview.edit_keys.issue(&id, &kid))
    })
    .await
}

#[derive(Debug, serde::Serialize)]
pub struct EditKeyResponse {
    key: String,
}

/// The host takes the key its load was given. The origin guard refuses
/// this POST from sandboxed frames (`Origin: null`).
pub async fn claim_edit_key(
    Extension(preview): Extension<Preview>,
    Path((id, kid)): Path<(String, String)>,
) -> ApiResult<Response> {
    let key = preview
        .edit_keys
        .claim(&id, &kid)
        .ok_or_else(|| ApiErr::not_found("edit key not found, expired or already taken"))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        ApiJson(EditKeyResponse { key }),
    )
        .into_response())
}

#[derive(Debug, serde::Deserialize)]
pub struct PreviewUrlQuery {
    file: Option<String>,
}

pub async fn preview_url(
    State(state): State<AppState>,
    Extension(preview): Extension<Preview>,
    Path(id): Path<String>,
    Query(query): Query<PreviewUrlQuery>,
) -> ApiResult<Response> {
    let file = query.file.unwrap_or_else(|| "index.html".to_owned());
    let project_id = id.clone();
    let (meta, _) = state
        .blocking(move |s| s.stat_file(&project_id, &file))
        .await?;
    let scope = preview.scopes.mint(&id);
    let body = PreviewUrlResponse {
        url: format!(
            "/api/projects/{}/preview/{}/{}",
            encode_segment(&id),
            scope.token,
            encode_path(&meta.name)
        ),
        file: meta.name,
        csp: PREVIEW_CSP.to_owned(),
        iframe_sandbox: PREVIEW_IFRAME_SANDBOX.to_owned(),
        opaque_origin: true,
        expires_at: scope.expires_at,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], ApiJson(body)).into_response())
}

pub async fn scoped(
    State(state): State<AppState>,
    Extension(preview): Extension<Preview>,
    Path((id, scope, path)): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    mut headers: HeaderMap,
) -> ApiResult<Response> {
    if !is_scope_token(&scope) {
        return Err(ApiErr::bad_request("invalid preview scope"));
    }
    if !preview.scopes.resolve(&id, &scope) {
        return Err(ApiErr::not_found("preview scope not found or expired"));
    }
    let bridge = wants_bridge(query.as_deref());
    if bridge {
        strip_validators(&mut headers);
    }
    let mut res = raw::raw(State(state), Path((id, path)), headers).await?;
    if bridge {
        res = with_bridge(res, None, || None).await;
    }
    let headers = res.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PREVIEW_CSP),
    );
    Ok(res)
}

pub async fn events(
    State(state): State<AppState>,
    Extension(preview): Extension<Preview>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let project_id = id.clone();
    let root = state
        .blocking(move |s| {
            let project = s.get_project(&project_id)?;
            s.project_root(&project)
        })
        .await?;
    let hub = Arc::clone(&preview.events);
    let project_id = id.clone();
    let sub = tokio::task::spawn_blocking(move || hub.subscribe(&project_id, &root.dir))
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ApiErr::new(
                    StatusCode::CONFLICT,
                    ErrorCode::Conflict,
                    "project folder is missing",
                )
            } else {
                ApiErr::internal(format!("watching project: {e}"))
            }
        })?;

    let ready = ProjectEvent::Ready { project_id: id };
    let stream = futures_util::stream::unfold(
        (Some(ready), sub, 0_u64),
        |(pending, mut sub, sent)| async move {
            let event = match pending {
                Some(event) => event,
                None => sub.recv().await?,
            };
            let id = sent + 1;
            let sse = Event::default()
                .id(id.to_string())
                .event(event.name())
                .json_data(&event)
                .ok()?;
            Some((Ok::<_, Infallible>(sse), (None, sub, id)))
        },
    );
    let mut res = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(KEEPALIVE))
        .into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_query_and_raw_paths() {
        assert!(wants_bridge(Some("v=3&bridge=1")));
        assert!(wants_bridge(Some("bridge")));
        assert!(!wants_bridge(Some("bridge=0")));
        assert!(!wants_bridge(None));
        assert_eq!(
            raw_target("/api/projects/p1/raw/deck/index.html"),
            Some(("p1", "deck/index.html"))
        );
        assert_eq!(raw_target("/api/projects/p1/files/a.html"), None);
        assert_eq!(raw_target("/api/projects/p1/raw/"), None);
    }
}
