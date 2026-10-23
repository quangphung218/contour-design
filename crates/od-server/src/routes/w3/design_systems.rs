//! Creating a design system from a brief (spec 1.4): a generation job runs
//! a coding agent in a staging folder; when it succeeds, the package is
//! installed under `<data>/design-systems` and the content index reloads,
//! so it shows up as `user:<id>` next to the bundled systems.
//!
//! - `POST /api/design-systems/generate` `{name, brief, category?, agentId?,
//!   model?, urls?, github?, figma?, files?: [{name, dataBase64}]}` → 202
//!   `{job}`; the job's `runId` streams through `/api/runs/:id/events` like
//!   any run. Sources (a website, a repository, Figma, attached files) are
//!   studied by the agent; the brief may then be empty.
//! - `GET /api/design-systems/jobs`, `GET /api/design-systems/jobs/:id`:
//!   a running job reports its `phase` (`sources`, `design`, `tokens`,
//!   `checking`) from what the agent has written so far.

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
    /// Progress of a running job, filled in when read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<&'static str>,
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
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    github: Option<String>,
    #[serde(default)]
    figma: Option<String>,
    #[serde(default)]
    files: Vec<SourceFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    name: String,
    data_base64: String,
}

/// Attached files the agent may read, at most this many bytes in all.
const MAX_SOURCE_BYTES: usize = 20 * 1024 * 1024;

fn clean_url(u: &str) -> Option<String> {
    let u = u.trim();
    (u.starts_with("https://") || u.starts_with("http://"))
        .then(|| u.to_owned())
        .filter(|u| u.len() <= 2048 && !u.chars().any(char::is_whitespace))
}

/// A safe file name for `source/`.
fn source_name(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next()?.trim();
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    (!clean.is_empty() && !clean.starts_with('.')).then_some(clean)
}

/// What a running job is doing, from the files in its staging folder.
fn phase_of(staging: &std::path::Path) -> &'static str {
    let has =
        |f: &str| std::fs::metadata(staging.join(f)).is_ok_and(|m| m.is_file() && m.len() > 0);
    match (has("DESIGN.md"), has("tokens.css")) {
        (false, false) => "sources",
        (true, false) => "tokens",
        (false, true) => "design",
        (true, true) => "checking",
    }
}

fn with_phase(mut job: Job, data_root: &std::path::Path) -> Job {
    if job.status == JobStatus::Running {
        job.phase = Some(phase_of(&data_root.join(STAGING_DIR).join(&job.id)));
    }
    job
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
    let mut sources = design_system_gen::Sources {
        urls: body
            .urls
            .iter()
            .filter_map(|u| clean_url(u))
            .take(8)
            .collect(),
        github: body
            .github
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty() && g.len() <= 512)
            .map(str::to_owned),
        figma: body.figma.as_deref().and_then(clean_url),
        files: Vec::new(),
    };
    let mut attached: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total = 0usize;
    for f in &body.files {
        use base64::Engine as _;
        let name = source_name(&f.name)
            .ok_or_else(|| ApiErr::bad_request(format!("invalid file name {:?}", f.name)))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(f.data_base64.trim())
            .map_err(|_| ApiErr::bad_request(format!("{name} is not base64")))?;
        total += bytes.len();
        if total > MAX_SOURCE_BYTES {
            return Err(ApiErr::bad_request("attached files are larger than 20 MB"));
        }
        sources.files.push(name.clone());
        attached.push((name, bytes));
    }
    let brief = if body.brief.trim().is_empty() && !sources.is_empty() {
        design_system_gen::SOURCES_ONLY_BRIEF
    } else {
        body.brief.as_str()
    };
    let req = GenerateRequest::new(&body.name, brief, body.category.as_deref())?;
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
            if !attached.is_empty() {
                let dir = staging2.join("source");
                std::fs::create_dir_all(&dir)?;
                for (name, bytes) in &attached {
                    std::fs::write(dir.join(name), bytes)?;
                }
            }
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
        phase: None,
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
        prompt: design_system_gen::generation_prompt_with_sources(&req, &sources),
        fallback_prompt: None,
        model: body
            .model
            .filter(|m| !m.trim().is_empty() && m != "default"),
        reasoning: None,
        service_tier: None,
        bin: None,
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

pub async fn list(
    State(state): State<AppState>,
    Extension(jobs): Extension<Arc<Jobs>>,
) -> Json<serde_json::Value> {
    let root = state.store.data_dir().root().to_path_buf();
    let mut all: Vec<Job> = jobs
        .lock()
        .values()
        .cloned()
        .map(|j| with_phase(j, &root))
        .collect();
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Json(json!({ "jobs": all }))
}

pub async fn get_one(
    State(state): State<AppState>,
    Extension(jobs): Extension<Arc<Jobs>>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let job = jobs
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found(format!("unknown design system job {id:?}")))?;
    let job = with_phase(job, state.store.data_dir().root());
    Ok(Json(json!({ "job": job })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_follows_the_files_written() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert_eq!(phase_of(d), "sources");
        std::fs::write(d.join("DESIGN.md"), "# x").unwrap();
        assert_eq!(phase_of(d), "tokens");
        std::fs::write(d.join("tokens.css"), ":root{}").unwrap();
        assert_eq!(phase_of(d), "checking");
    }

    #[test]
    fn sources_are_cleaned() {
        assert_eq!(
            clean_url(" https://a.example/x "),
            Some("https://a.example/x".into())
        );
        assert_eq!(clean_url("file:///etc/passwd"), None);
        assert_eq!(clean_url("https://a b"), None);
        assert_eq!(source_name("../../logo.png").as_deref(), Some("logo.png"));
        assert_eq!(source_name("my brand.css").as_deref(), Some("my-brand.css"));
        assert_eq!(source_name(".env"), None);
    }
}
