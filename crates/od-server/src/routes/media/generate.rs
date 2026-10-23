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
use od_media::config::ConfigError;
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
    tokio::task::spawn_blocking(move || file2.write(&body))
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))??;
    Ok(Json(file.masked(deps.env.as_ref())))
}

pub async fn models(
    State(state): State<AppState>,
    Extension(deps): Extension<Deps>,
) -> Json<Value> {
    Json(od_media::models_json(
        &config_file(&state),
        deps.env.as_ref(),
    ))
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
