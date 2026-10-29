//! Automations (spec 1.5): routines that run a prompt in a project on a
//! local cron schedule, backed by `od-automation`.
//!
//! - `GET/POST /api/routines`
//! - `GET/PATCH/DELETE /api/routines/:rid`
//! - `POST /api/routines/:rid/run` runs it now → 202, 409 while running.
//! - `GET /api/routines/:rid/runs?limit=` history, newest first.
//!
//! Each routine run is a normal agent run (`POST /api/runs` path) in a new
//! conversation titled after the routine, so it shows up in the Studio.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::runs;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use od_automation::{
    AutomationError, Launched, Launcher, NewRoutine, Routine, RoutinePatch, RoutineRun,
    RoutineStore, RunEnd, RunState, Scheduler, TargetMode,
};
use od_contracts::{
    ChatRequest, CreateConversationRequest, CreateRunResponse, ErrorCode, RunStatus,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// How often due routines are checked.
const TICK: Duration = Duration::from_secs(15);
/// Largest history page.
const MAX_RUNS_PAGE: u32 = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/routines", get(list).post(create))
        .route(
            "/api/routines/{rid}",
            get(get_one).patch(update).delete(remove),
        )
        .route("/api/routines/{rid}/run", post(run_now))
        .route("/api/routines/{rid}/runs", get(history))
}

/// Opens `<data>/automations.sqlite` and starts the schedule loop (when a
/// tokio runtime is around). `None` when the database cannot be opened.
pub fn scheduler(state: &AppState) -> Option<Arc<Scheduler>> {
    let path = state.store.data_dir().root().join("automations.sqlite");
    let store = match RoutineStore::open(&path) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            tracing::error!(error = %e, path = %path.display(), "automations disabled");
            return None;
        }
    };
    let launcher = Arc::new(RunLauncher {
        state: state.clone(),
    });
    let scheduler = Arc::new(Scheduler::new(store, launcher));
    if let Err(e) = scheduler.resync(od_runs::now_ms()) {
        tracing::error!(error = %e, "rescheduling routines");
    }
    if tokio::runtime::Handle::try_current().is_ok() {
        Arc::clone(&scheduler).spawn_loop(TICK);
    }
    Some(scheduler)
}

/// Starts routine runs through the regular run path.
struct RunLauncher {
    state: AppState,
}

impl Launcher for RunLauncher {
    fn launch(&self, routine: Routine) -> BoxFuture<'static, Result<Launched, String>> {
        let state = self.state.clone();
        async move { launch(state, routine).await.map_err(|e| e.message) }.boxed()
    }
}

async fn launch(state: AppState, routine: Routine) -> ApiResult<Launched> {
    let title = format!(
        "{} · {}",
        routine.name,
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );
    let mode = routine.target_mode;
    let reuse_project = routine.project_id.clone();
    let (project_id, conversation_id) = state
        .blocking(move |s| {
            let request = CreateConversationRequest {
                title: Some(title.clone()),
                ..CreateConversationRequest::default()
            };
            match mode {
                TargetMode::Reuse => {
                    let c = s.create_conversation(&reuse_project, request)?;
                    Ok((reuse_project, c.id))
                }
                // A fresh, isolated project per fire, in its first conversation.
                TargetMode::CreateEachRun => {
                    // Your usage counts projects people make, not these.
                    let (project, cid) =
                        s.create_automation_project(od_contracts::CreateProjectRequest {
                            name: title.clone(),
                            ..od_contracts::CreateProjectRequest::default()
                        })?;
                    s.update_conversation(
                        &project.id,
                        &cid,
                        od_contracts::UpdateConversationRequest {
                            title: Some(title),
                            session_mode: None,
                        },
                    )?;
                    Ok((project.id, cid))
                }
            }
        })
        .await?;
    let req: ChatRequest = serde_json::from_value(serde_json::json!({
        "agentId": routine.agent_id,
        "message": routine.prompt,
        "projectId": project_id,
        "conversationId": conversation_id,
    }))
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    let res = runs::create(State(state.clone()), ApiJson(req)).await?;
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let created: CreateRunResponse =
        serde_json::from_slice(&body).map_err(|e| ApiErr::internal(e.to_string()))?;
    let run = state
        .runs
        .registry
        .get(&created.run_id)
        .ok_or_else(|| ApiErr::internal("the run disappeared"))?;
    let done = async move {
        let mut sub = run.subscribe(0);
        while sub.next().await.is_some() {}
        let end = run.end_payload();
        let status = match run.status() {
            RunStatus::Succeeded => RunState::Succeeded,
            RunStatus::Canceled => RunState::Canceled,
            RunStatus::Failed | RunStatus::Running => RunState::Failed,
        };
        RunEnd {
            status,
            error: end.and_then(|e| e.error),
        }
    }
    .boxed();
    Ok(Launched {
        run_id: created.run_id,
        conversation_id: created.conversation_id,
        project_id,
        done,
    })
}

fn api_err(e: AutomationError) -> ApiErr {
    match e {
        AutomationError::Invalid(m) => ApiErr::bad_request(m),
        AutomationError::NotFound => ApiErr::not_found("routine not found"),
        AutomationError::Busy => {
            ApiErr::new(StatusCode::CONFLICT, ErrorCode::Conflict, e.to_string())
        }
        AutomationError::Db(_) => {
            tracing::error!(error = %e, "automations database");
            ApiErr::internal("automations database error")
        }
    }
}

/// Runs blocking routine-store work.
async fn with_store<T, F>(sched: &Arc<Scheduler>, f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&RoutineStore) -> Result<T, AutomationError> + Send + 'static,
{
    let store = Arc::clone(sched.store());
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
        .map_err(api_err)
}

fn next_fire(schedule: &od_automation::Schedule) -> Option<i64> {
    od_automation::next_fire_ms(schedule, chrono::Local::now())
}

#[derive(Debug, Serialize)]
pub struct RoutinesResponse {
    pub routines: Vec<Routine>,
}

#[derive(Debug, Serialize)]
pub struct RoutineResponse {
    pub routine: Routine,
}

#[derive(Debug, Serialize)]
pub struct RoutineRunsResponse {
    pub runs: Vec<RoutineRun>,
}

#[derive(Debug, Serialize)]
pub struct RoutineRunResponse {
    pub run: RoutineRun,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<u32>,
}

fn known_agent(id: &str) -> ApiResult<()> {
    od_runtime::def::find(id).map(|_| ()).ok_or_else(|| {
        ApiErr::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::AgentNotFound,
            format!("unknown agent {id:?}"),
        )
    })
}

async fn list(Extension(sched): Extension<Arc<Scheduler>>) -> ApiResult<ApiJson<RoutinesResponse>> {
    let routines = with_store(&sched, |s| s.list()).await?;
    Ok(ApiJson(RoutinesResponse { routines }))
}

async fn create(
    State(state): State<AppState>,
    Extension(sched): Extension<Arc<Scheduler>>,
    ApiJson(new): ApiJson<NewRoutine>,
) -> ApiResult<Response> {
    known_agent(&new.agent_id)?;
    if new.target_mode == TargetMode::Reuse {
        let project_id = new.project_id.clone();
        state.blocking(move |s| s.get_project(&project_id)).await?;
    }
    let routine = with_store(&sched, move |s| s.create(&new, next_fire)).await?;
    tracing::info!(routine = %routine.id, schedule = %routine.schedule, "routine created");
    Ok((StatusCode::CREATED, ApiJson(RoutineResponse { routine })).into_response())
}

async fn get_one(
    Extension(sched): Extension<Arc<Scheduler>>,
    Path(rid): Path<String>,
) -> ApiResult<ApiJson<RoutineResponse>> {
    let routine = with_store(&sched, move |s| s.get(&rid)).await?;
    Ok(ApiJson(RoutineResponse { routine }))
}

async fn update(
    State(state): State<AppState>,
    Extension(sched): Extension<Arc<Scheduler>>,
    Path(rid): Path<String>,
    ApiJson(patch): ApiJson<RoutinePatch>,
) -> ApiResult<ApiJson<RoutineResponse>> {
    if let Some(agent) = &patch.agent_id {
        known_agent(agent)?;
    }
    if let Some(project_id) = patch.project_id.clone().filter(|p| !p.is_empty()) {
        state.blocking(move |s| s.get_project(&project_id)).await?;
    }
    let routine = with_store(&sched, move |s| s.update(&rid, &patch, next_fire)).await?;
    Ok(ApiJson(RoutineResponse { routine }))
}

async fn remove(
    Extension(sched): Extension<Arc<Scheduler>>,
    Path(rid): Path<String>,
) -> ApiResult<ApiJson<od_contracts::OkResponse>> {
    if with_store(&sched, move |s| s.delete(&rid)).await? {
        Ok(ApiJson(od_contracts::OkResponse { ok: true }))
    } else {
        Err(ApiErr::not_found("routine not found"))
    }
}

async fn run_now(
    Extension(sched): Extension<Arc<Scheduler>>,
    Path(rid): Path<String>,
) -> ApiResult<Response> {
    let run = sched.run_now(&rid).await.map_err(api_err)?;
    Ok((StatusCode::ACCEPTED, ApiJson(RoutineRunResponse { run })).into_response())
}

async fn history(
    Extension(sched): Extension<Arc<Scheduler>>,
    Path(rid): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<ApiJson<RoutineRunsResponse>> {
    let limit = q.limit.unwrap_or(50).clamp(1, MAX_RUNS_PAGE);
    let runs = with_store(&sched, move |s| s.runs(&rid, limit)).await?;
    Ok(ApiJson(RoutineRunsResponse { runs }))
}
