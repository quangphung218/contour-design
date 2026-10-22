//! Routes the UI needs beyond the core API (spec 1.1, 1.5):
//!
//! - `GET/PUT /api/app-config`: app settings (agent, model,
//!   `agentPermissionMode`, theme, onboarding), a JSON object in
//!   `<data>/app-config.json`.
//! - `POST /api/import/folder`: a project pointing at an existing folder.
//! - Everything else that is not `/api/*`: the static `od-ui` bundle, with
//!   `index.html` as the SPA fallback.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use od_contracts::{CreateProjectRequest, CreateProjectResponse, Project, ProjectMetadata};
use od_store::{Store, StoreError};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Overrides where the built UI bundle (`trunk build` output) is served from.
pub const UI_DIST_ENV: &str = "OD_UI_DIST";

const APP_CONFIG_FILE: &str = "app-config.json";

/// Known settings and their allowed values. Unknown keys are stored as-is so
/// newer UIs can add settings without a daemon change.
const ENUM_KEYS: [(&str, &[&str]); 2] = [
    ("agentPermissionMode", &["auto", "ask"]),
    ("theme", &["system", "light", "dark"]),
];

/// Serialises read-modify-write of `app-config.json` within this process.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// UI routes, serving the bundle found by [`resolve_ui_dist`].
pub fn routes() -> Router<AppState> {
    routes_with_dist(resolve_ui_dist())
}

/// UI routes serving the bundle in `dist`.
pub fn routes_with_dist(dist: PathBuf) -> Router<AppState> {
    let dist = Arc::new(dist);
    Router::new()
        .route("/api/app-config", get(get_config).put(put_config))
        .route("/api/import/folder", post(import_folder_route))
        .fallback(move |req: Request| serve_static(Arc::clone(&dist), req))
}

/// `OD_UI_DIST`, then `ui/` next to the executable (or one level up), then
/// `crates/od-ui/dist` in the source tree.
#[must_use]
pub fn resolve_ui_dist() -> PathBuf {
    if let Some(dir) = std::env::var_os(UI_DIST_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let source_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../od-ui/dist");
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .into_iter()
        .flat_map(|exe| {
            [
                exe.join("ui"),
                exe.join("../ui"),
                exe.join("../Resources/ui"),
            ]
        })
        .find(|c| c.join("index.html").is_file())
        .unwrap_or(source_tree)
}

// ---------------------------------------------------------------- app config

fn config_path(store: &Store) -> PathBuf {
    store.data_dir().root().join(APP_CONFIG_FILE)
}

fn read_config(path: &Path) -> Result<Map<String, Value>, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(
            serde_json::from_slice::<Map<String, Value>>(&bytes).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "app-config.json is not a JSON object; starting fresh");
                Map::new()
            }),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.into()),
    }
}

/// Checks enum-valued settings. `null` is always allowed (it clears).
fn validate_patch(patch: &Map<String, Value>) -> Result<(), String> {
    for (key, allowed) in ENUM_KEYS {
        match patch.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::String(v)) if allowed.contains(&v.as_str()) => {}
            Some(other) => {
                return Err(format!(
                    "{key} must be one of {}, got {other}",
                    allowed.join(", ")
                ));
            }
        }
    }
    Ok(())
}

/// Shallow merge: `null` removes a key, anything else replaces it.
fn merge_patch(config: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (key, value) in patch {
        if value.is_null() {
            config.remove(&key);
        } else {
            config.insert(key, value);
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

async fn get_config(State(state): State<AppState>) -> ApiResult<ApiJson<Value>> {
    let config = state
        .blocking(|s| {
            let _guard = CONFIG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            read_config(&config_path(s))
        })
        .await?;
    Ok(ApiJson(Value::Object(config)))
}

async fn put_config(
    State(state): State<AppState>,
    ApiJson(patch): ApiJson<Map<String, Value>>,
) -> ApiResult<ApiJson<Value>> {
    validate_patch(&patch).map_err(ApiErr::bad_request)?;
    let config = state
        .blocking(move |s| {
            let _guard = CONFIG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let path = config_path(s);
            let mut config = read_config(&path)?;
            merge_patch(&mut config, patch);
            let bytes = serde_json::to_vec_pretty(&config)?;
            write_atomic(&path, &bytes)?;
            Ok(config)
        })
        .await?;
    Ok(ApiJson(Value::Object(config)))
}

// ------------------------------------------------------------- folder import

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportFolderRequest {
    base_dir: String,
    #[serde(default)]
    name: Option<String>,
}

/// Creates a project whose files live in `dir` (never copied, spec 1.1).
///
/// # Errors
/// [`StoreError::BadRequest`] when `dir` is not an absolute existing folder,
/// is the filesystem root, or lies inside the data directory.
pub fn import_folder(
    store: &Store,
    dir: &Path,
    name: Option<String>,
) -> Result<(Project, String), StoreError> {
    let bad = |m: &str| StoreError::BadRequest(m.to_owned());
    if !dir.is_absolute() {
        return Err(bad("baseDir must be an absolute path"));
    }
    let dir = dir
        .canonicalize()
        .map_err(|_| bad("baseDir does not exist"))?;
    if !dir.is_dir() {
        return Err(bad("baseDir is not a folder"));
    }
    if dir.parent().is_none() {
        return Err(bad("refusing to import the filesystem root"));
    }
    let data_root = store
        .data_dir()
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.data_dir().root().to_path_buf());
    if dir.starts_with(&data_root) || data_root.starts_with(&dir) {
        return Err(bad("baseDir overlaps the data directory"));
    }
    let name = name
        .filter(|n| !n.trim().is_empty())
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Imported folder".to_owned());
    store.create_project(CreateProjectRequest {
        name,
        metadata: Some(ProjectMetadata {
            base_dir: Some(dir.to_string_lossy().into_owned()),
            imported_from: Some("folder".to_owned()),
            ..ProjectMetadata::default()
        }),
        ..CreateProjectRequest::default()
    })
}

async fn import_folder_route(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<ImportFolderRequest>,
) -> ApiResult<ApiJson<CreateProjectResponse>> {
    let (project, conversation_id) = state
        .blocking(move |s| import_folder(s, Path::new(&req.base_dir), req.name))
        .await?;
    Ok(ApiJson(CreateProjectResponse {
        project,
        conversation_id,
    }))
}

// -------------------------------------------------------------- static files

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Maps a request path to a file under `dist`, refusing anything but plain
/// non-hidden segments.
fn dist_file(dist: &Path, uri_path: &str) -> Option<PathBuf> {
    let rel = Path::new(uri_path.trim_start_matches('/'));
    let mut out = dist.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(seg) if !seg.to_string_lossy().starts_with('.') => out.push(seg),
            _ => return None,
        }
    }
    Some(out)
}

async fn serve_static(dist: Arc<PathBuf>, req: Request) -> Response {
    let path = req.uri().path();
    if path == "/api" || path.starts_with("/api/") {
        return ApiErr::not_found("no such API route").into_response();
    }
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let index = dist.join("index.html");
    let file = dist_file(&dist, path)
        .filter(|f| f.is_file())
        .unwrap_or_else(|| index.clone());
    let is_index = file == index;
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let mut res = Response::new(Body::from(bytes));
            let headers = res.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(&file)),
            );
            // trunk fingerprints assets, so only index.html may change.
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(if is_index {
                    "no-cache"
                } else {
                    "public, max-age=31536000, immutable"
                }),
            );
            res
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!(
                "UI bundle not found in {}. Build it with `trunk build` in crates/od-ui, or set {UI_DIST_ENV}.",
                dist.display()
            ),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => Map::new(),
        }
    }

    #[test]
    fn patch_validation() {
        assert!(validate_patch(&obj(json!({"agentPermissionMode": "ask"}))).is_ok());
        assert!(validate_patch(&obj(json!({"agentPermissionMode": null}))).is_ok());
        assert!(validate_patch(&obj(json!({"agentPermissionMode": "yolo"}))).is_err());
        assert!(validate_patch(&obj(json!({"theme": 3}))).is_err());
        assert!(validate_patch(&obj(json!({"model": "anything"}))).is_ok());
    }

    #[test]
    fn merge_null_removes() {
        let mut c = obj(json!({"agentId": "claude", "model": "opus"}));
        merge_patch(&mut c, obj(json!({"model": null, "theme": "dark"})));
        assert_eq!(
            Value::Object(c),
            json!({"agentId": "claude", "theme": "dark"})
        );
    }

    #[test]
    fn dist_paths_stay_inside() {
        let d = Path::new("/srv/dist");
        assert_eq!(dist_file(d, "/app.js"), Some(d.join("app.js")));
        assert_eq!(dist_file(d, "/"), Some(d.to_path_buf()));
        assert_eq!(dist_file(d, "/../etc/passwd"), None);
        assert_eq!(dist_file(d, "/.git/config"), None);
    }
}
