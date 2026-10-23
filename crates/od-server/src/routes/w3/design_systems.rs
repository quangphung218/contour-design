//! Creating a design system from a brief (spec 1.4): a generation job runs
//! a coding agent in a staging folder; when it succeeds, the package is
//! installed under `<data>/design-systems` and the content index reloads,
//! so it shows up as `user:<id>` next to the bundled systems.
//!
//! - `POST /api/design-systems/generate` `{name, brief, category?, agentId?,
//!   model?}` → 202 `{job}`; the job's `runId` streams through
//!   `/api/runs/:id/events` like any run.
//! - `GET /api/design-systems/jobs`, `GET /api/design-systems/jobs/:id`.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use od_content::design_system_gen::{self, GenError, GenerateRequest};
use od_contracts::{ErrorCode, RunStatus};
use od_runs::RunMeta;
use od_runtime::env::{RunEnv, agent_env};
use od_runtime::{LaunchSpec, McpLaunch, PermissionMode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// Staging folders live here, one per job, removed when the job ends.
const STAGING_DIR: &str = "design-system-jobs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub name: String,
    pub run_id: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_system_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// In-memory job table (jobs are short; the run itself is persisted).
#[derive(Debug, Default)]
pub struct Jobs(Mutex<HashMap<String, Job>>);

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Job>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.lock().get_mut(id) {
            f(job);
            job.updated_at = od_store::now_ms();
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/design-systems/generate", post(generate))
        .route("/api/design-systems/jobs", get(list))
        .route("/api/design-systems/jobs/{id}", get(get_one))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateBody {
    name: String,
    brief: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

impl From<GenError> for ApiErr {
    fn from(e: GenError) -> Self {
        match e {
            GenError::BadRequest(m) => ApiErr::bad_request(m),
            other => ApiErr::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::ValidationFailed,
                other.to_string(),
            ),
        }
    }
}

pub async fn generate(
    State(state): State<AppState>,
    Extension(jobs): Extension<Arc<Jobs>>,
    ApiJson(body): ApiJson<GenerateBody>,
) -> ApiResult<impl IntoResponse> {
    let req = GenerateRequest::new(&body.name, &body.brief, body.category.as_deref())?;
    let agent = body.agent_id.as_deref().unwrap_or("claude");
    let def = od_runtime::def::find(agent).ok_or_else(|| {
        ApiErr::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::AgentNotFound,
            format!("unknown agent {agent:?}"),
        )
    })?;
    let job_id = uuid::Uuid::new_v4().to_string();
    let run_id = uuid::Uuid::new_v4().to_string();
    let data_root = state.store.data_dir().root().to_path_buf();
    let staging = data_root.join(STAGING_DIR).join(&job_id);
    let staging2 = staging.clone();
    let permission = tokio::task::spawn_blocking({
        let store = Arc::clone(&state.store);
        move || {
            std::fs::create_dir_all(&staging2)?;
            Ok::<_, std::io::Error>(
                store
                    .data_dir()
                    .app_setting("agent_permission_mode")
                    .and_then(|v| v.as_str().map(PermissionMode::from_setting))
                    .unwrap_or_default(),
            )
        }
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(|e| ApiErr::internal(format!("creating the staging folder: {e}")))?;

    let (run, _) = state
        .runs
        .registry
        .create_or_reuse(RunMeta {
            run_id: run_id.clone(),
            agent_id: def.id.to_owned(),
            project_id: None,
            conversation_id: None,
            assistant_message_id: None,
            client_request_id: None,
            request_hash: None,
            created_at: od_runs::now_ms(),
        })
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let now = od_store::now_ms();
    let job = Job {
        id: job_id.clone(),
        status: JobStatus::Running,
        name: req.name.clone(),
        run_id: run_id.clone(),
        agent_id: def.id.to_owned(),
        design_system_id: None,
        error: None,
        created_at: now,
        updated_at: now,
    };
    jobs.lock().insert(job_id.clone(), job.clone());

    let engine = &state.runs.engine;
    let od_bin = std::env::current_exe().ok();
    let env = agent_env(
        def,
        &engine.config().base_env,
        &engine.config().path,
        &RunEnv {
            data_dir: Some(&data_root),
            daemon_url: state.config.daemon_url.as_deref(),
            tool_token: None,
            project_id: None,
            project_dir: Some(&staging),
            od_bin: od_bin.as_deref(),
            run_id: Some(&run_id),
        },
    );
    let handle = engine.launch(LaunchSpec {
        run: Arc::clone(&run),
        def,
        cwd: staging.clone(),
        prompt: design_system_gen::generation_prompt(&req),
        fallback_prompt: None,
        model: body
            .model
            .filter(|m| !m.trim().is_empty() && m != "default"),
        reasoning: None,
        extra_dirs: Vec::new(),
        resume_session_id: None,
        permission,
        env,
        project_id: None,
        byok: None,
        // A design system draft runs without the user's external MCP servers.
        mcp: McpLaunch::default(),
    });
    tokio::spawn(finish(
        state.clone(),
        Arc::clone(&jobs),
        job_id,
        staging,
        req,
        handle,
    ));
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))))
}

/// Waits for the agent, installs its package, reloads the index.
async fn finish(
    state: AppState,
    jobs: Arc<Jobs>,
    job_id: String,
    staging: PathBuf,
    req: GenerateRequest,
    handle: tokio::task::JoinHandle<od_runtime::RunOutcome>,
) {
    let outcome = handle.await;
    let result: Result<String, String> = match outcome {
        Ok(o) if o.status == RunStatus::Succeeded => {
            let user_root = state.store.data_dir().root().join("design-systems");
            let staging2 = staging.clone();
            let content = Arc::clone(&state.content);
            tokio::task::spawn_blocking(move || {
                let installed = design_system_gen::install(&staging2, &user_root, &req)
                    .map_err(|e| e.to_string())?;
                content.reload();
                Ok(installed.id)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        }
        Ok(o) => Err(format!("the agent run {}", o.status.as_str())),
        Err(e) => Err(format!("the agent run failed: {e}")),
    };
    let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(staging)).await;
    match &result {
        Ok(id) => tracing::info!(job = %job_id, design_system = %id, "design system generated"),
        Err(e) => tracing::warn!(job = %job_id, error = %e, "design system generation failed"),
    }
    jobs.update(&job_id, |job| match result {
        Ok(id) => {
            job.status = JobStatus::Succeeded;
            job.design_system_id = Some(id);
        }
        Err(e) => {
            job.status = JobStatus::Failed;
            job.error = Some(e);
        }
    });
}

pub async fn list(Extension(jobs): Extension<Arc<Jobs>>) -> Json<serde_json::Value> {
    let mut all: Vec<Job> = jobs.lock().values().cloned().collect();
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Json(json!({ "jobs": all }))
}

pub async fn get_one(
    Extension(jobs): Extension<Arc<Jobs>>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let job = jobs
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found(format!("unknown design system job {id:?}")))?;
    Ok(Json(json!({ "job": job })))
}
