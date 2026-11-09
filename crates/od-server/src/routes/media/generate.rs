//! Media generation (spec 1.5): the user's own provider keys in
//! `<data>/media-config.json`, generated files written into the project.
//!
//! - `GET/PUT /api/media/config`: masked read; write from Settings.
//! - `GET /api/media/models`: catalogue and which providers have a key.
//! - `POST /api/projects/:id/media/generate`: one image, video or audio.
//! - `POST /api/tools/media/generate`: same, project from `OD_TOOL_TOKEN`.
//!
//! Generation is synchronous: the response arrives when the file is
//! written (fal video can take minutes).

use super::{Deps, tool_caller};
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use od_contracts::ErrorCode;
use od_media::config::{ConfigError, StoredConfig};
use od_media::{ConfigFile, GenerateRequest, MediaError};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/media/config", get(get_config).put(put_config))
        .route("/api/media/models", get(models))
        .route("/api/projects/{id}/media/generate", post(generate))
        .route("/api/tools/media/generate", post(tool_generate))
}

fn config_file(state: &AppState) -> ConfigFile {
    ConfigFile::in_dir(state.store.data_dir().root())
}

impl From<MediaError> for ApiErr {
    fn from(e: MediaError) -> Self {
        let (status, code) = match &e {
            MediaError::BadRequest(_) | MediaError::NoCredential { .. } => {
                (StatusCode::BAD_REQUEST, ErrorCode::BadRequest)
            }
            MediaError::Upstream(_) => (StatusCode::BAD_GATEWAY, ErrorCode::UpstreamUnavailable),
            MediaError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, ErrorCode::UpstreamUnavailable),
        };
        ApiErr::new(status, code, e.to_string())
    }
}

impl From<ConfigError> for ApiErr {
    fn from(e: ConfigError) -> Self {
        match e {
            ConfigError::Invalid(m) => ApiErr::bad_request(m),
            ConfigError::Io(e) => ApiErr::internal(e.to_string()),
        }
    }
}

pub async fn get_config(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
) -> Json<Value> {
    Json(config_file(&state).masked(deps.env.as_ref()))
}

pub async fn put_config(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let file = config_file(&state);
    let file2 = file.clone();
    let dropped = tokio::task::spawn_blocking(move || write_config(&file2, &body, || {}))
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))??;
    let mut v = file.masked(deps.env.as_ref());
    // A key from the environment still configures the provider, so its
    // dropped stored key needs nothing from the user.
    let dropped: Vec<String> = dropped
        .into_iter()
        .filter(|id| v["providers"][id.as_str()]["source"] != "env")
        .collect();
    v["keyDropped"] = json!(dropped);
    Ok(Json(v))
}

/// Writes `body` and returns [`dropped_keys`]. This route is the only
/// writer of the file, and its lock holds the reads before and after the
/// write, so another PUT cannot land between them and show up as a
/// dropped key here. `after_read` runs after the first read (tests race
/// another PUT there).
fn write_config(
    file: &ConfigFile,
    body: &Value,
    after_read: impl FnOnce(),
) -> Result<Vec<String>, ConfigError> {
    static PUT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PUT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let prior = file.read();
    after_read();
    file.write(body)?;
    Ok(dropped_keys(body, &prior, &file.read()))
}

/// Providers whose saved key the PUT asked to keep but the daemon dropped,
/// because the base URL changed (QA-069): Settings asks for them again.
fn dropped_keys(body: &Value, prior: &StoredConfig, next: &StoredConfig) -> Vec<String> {
    let has_key =
        |c: &StoredConfig, id: &str| c.providers.get(id).is_some_and(|p| !p.api_key.is_empty());
    let Some(asked) = body.get("providers").and_then(Value::as_object) else {
        return Vec::new();
    };
    asked
        .iter()
        .filter(|(_, e)| e.get("preserveApiKey") == Some(&Value::Bool(true)))
        .filter(|(_, e)| {
            e.get("apiKey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .is_empty()
        })
        .filter(|(id, _)| has_key(prior, id) && !has_key(next, id))
        .map(|(id, _)| id.clone())
        .collect()
}

pub async fn models(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
) -> Json<Value> {
    let mut v = od_media::models_json(&config_file(&state), deps.env.as_ref());
    // Rendered on this machine from a composition the agent wrote, so not
    // a choice for the prompt-based pickers that read `models`.
    v["localModels"] = json!([crate::routes::w3::motion::catalogue_entry()]);
    Json(v)
}

async fn run(
    state: &AppState,
    deps: &Deps,
    project_id: String,
    req: GenerateRequest,
    run_id: Option<&str>,
) -> ApiResult<Json<Value>> {
    // The project must exist before a paid provider call.
    let pid = project_id.clone();
    state.blocking(move |s| s.get_project(&pid)).await?;
    let surface = serde_json::to_value(req.surface)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    // Track C2: the run's chat shows this generation as it goes.
    let mut progress =
        crate::routes::c2::media::Progress::start(state, run_id, &surface, &req.model);
    let res = async {
        let http = od_media::provider::http_client()?;
        let generated = od_media::generate(
            &http,
            &config_file(state),
            deps.env.as_ref(),
            &req,
            deps.limits,
        )
        .await?;
        progress.line(format!("Writing {}", generated.name));
        Ok::<_, ApiErr>(generated)
    }
    .await;
    let generated = match res {
        Ok(g) => g,
        Err(e) => {
            progress.failed(&e.message);
            return Err(e);
        }
    };
    let note = generated.note.clone();
    let (name, bytes) = (generated.name.clone(), generated.bytes);
    let file = match state
        .blocking(move |s| s.write_file(&project_id, &name, &bytes, true, None))
        .await
    {
        Ok(f) => f,
        Err(e) => {
            progress.failed(&e.message);
            return Err(e);
        }
    };
    progress.done(&file.name);
    tracing::info!(model = %generated.model, file = %file.name, "media generated");
    Ok(Json(json!({
        "file": file,
        "name": file.name,
        "size": file.size,
        "mime": file.mime,
        "kind": file.kind,
        "model": generated.model,
        "surface": generated.surface,
        "provider": generated.provider,
        "providerNote": note,
    })))
}

fn parse(body: Value) -> ApiResult<GenerateRequest> {
    serde_json::from_value(body).map_err(|e| {
        ApiErr::bad_request(format!(
            "surface must be image, video or audio and model is required: {e}"
        ))
    })
}

pub async fn generate(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let req = parse(body)?;
    run(&state, &deps, id, req, None).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolGenerate {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(flatten)]
    rest: Value,
}

pub async fn tool_generate(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<ToolGenerate>,
) -> ApiResult<Json<Value>> {
    let caller = tool_caller(
        &state,
        &headers,
        "/api/tools/media/generate",
        body.project_id.as_deref(),
    )?;
    let project = caller
        .project_id
        .ok_or_else(|| ApiErr::bad_request("projectId is required"))?;
    let req = parse(body.rest)?;
    run(&state, &deps, project, req, caller.run_id.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Tab A saves OpenAI unchanged with the key left empty while tab B
    /// clears the configuration (spec 3.1, QA-069 follow-up). Whichever
    /// goes first, A's URL did not change, so A reports no dropped key.
    #[test]
    fn a_racing_clear_is_not_reported_as_a_dropped_key() {
        let tmp = tempfile::tempdir().unwrap();
        let file = ConfigFile::in_dir(tmp.path());
        let url = "http://127.0.0.1:9/v1";
        file.write(&json!({ "providers": { "openai": { "apiKey": "sk-1234", "baseUrl": url } } }))
            .unwrap();
        let keep = json!({ "providers": { "openai": { "apiKey": "", "preserveApiKey": true, "baseUrl": url } } });
        let (done, finished) = mpsc::channel();
        std::thread::scope(|s| {
            let dropped = write_config(&file, &keep, || {
                let file = file.clone();
                s.spawn(move || {
                    write_config(&file, &json!({ "providers": {} }), || {}).unwrap();
                    done.send(()).unwrap();
                });
                // Without the lock the clear lands here; with it, it waits.
                let _ = finished.recv_timeout(Duration::from_millis(300));
            })
            .unwrap();
            assert!(dropped.is_empty(), "{dropped:?}");
        });
    }
}
