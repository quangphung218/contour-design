//! Crystallize and evolution proposals (track C4-depth), the original
//! `routes/routine.ts` crystallize route and `routes/automation.ts`:
//!
//! - `POST /api/routines/:rid/runs/:runId/crystallize` → `{packet,
//!   compressionReport, proposals, routineId, runId}`: a succeeded run
//!   becomes a skill proposal and a memory proposal (400 for other runs).
//! - `GET /api/automation-proposals?status=pending-review|applied|rejected|all`
//! - `GET /api/automation-proposals/:id`
//! - `POST /api/automation-proposals/:id/apply` → `{proposal, result}`:
//!   writes the user skill (`<data>/skills/<slug>/SKILL.md`, a free slug)
//!   or the global memory entry.
//! - `POST /api/automation-proposals/:id/reject` `{reason?}` → `{proposal}`
//! - `GET /api/automation-source-packets`
//!
//! A proposal is reviewed once; a second apply or reject is a 409.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Router};
use od_automation::evolution::{
    self, Crystallized, EvolutionError, EvolutionStore, Packet, Proposal, ProposalStatus, Review,
    TargetKind,
};
use od_automation::{AutomationError, Scheduler};
use od_content::user_content::{self, SkillDraft};
use od_contracts::{ErrorCode, MessageRole};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

/// Distinct names tried for a skill whose slug is taken.
const MAX_SKILL_NAME_TRIES: usize = 50;

/// The routes, over the data folder's evolution store.
pub fn routes(state: &AppState) -> Router<AppState> {
    let store = Arc::new(EvolutionStore::new(state.store.data_dir().root()));
    Router::new()
        .route(
            "/api/routines/{rid}/runs/{run_id}/crystallize",
            post(crystallize),
        )
        .route("/api/automation-proposals", get(list))
        .route("/api/automation-proposals/{id}", get(get_one))
        .route("/api/automation-proposals/{id}/apply", post(apply))
        .route("/api/automation-proposals/{id}/reject", post(reject))
        .route("/api/automation-source-packets", get(packets))
        .layer(Extension(store))
}

impl From<EvolutionError> for ApiErr {
    fn from(e: EvolutionError) -> Self {
        match e {
            EvolutionError::Invalid(m) => ApiErr::bad_request(m),
            EvolutionError::NotFound => ApiErr::not_found(e.to_string()),
            EvolutionError::NotReviewable { .. } => {
                ApiErr::new(StatusCode::CONFLICT, ErrorCode::Conflict, e.to_string())
            }
            EvolutionError::Io(_) | EvolutionError::Json(_) => {
                tracing::error!(error = %e, "evolution store");
                ApiErr::internal("evolution store error")
            }
        }
    }
}

async fn blocking<T, F>(f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ApiResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrystallizeResponse {
    #[serde(flatten)]
    pub crystallized: Crystallized,
    pub routine_id: String,
    pub run_id: String,
}

/// The run's final assistant text: the message the run wrote, else the
/// conversation's last assistant message.
fn run_summary(state: &AppState, conversation_id: &str, run_id: Option<&str>) -> String {
    let Ok(messages) = state.store.list_messages(conversation_id) else {
        return String::new();
    };
    let assistant = || messages.iter().filter(|m| m.role == MessageRole::Assistant);
    assistant()
        .rev()
        .find(|m| run_id.is_some() && m.run_id.as_deref() == run_id)
        .or_else(|| assistant().next_back())
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

async fn crystallize(
    State(state): State<AppState>,
    Extension(sched): Extension<Arc<Scheduler>>,
    Extension(store): Extension<Arc<EvolutionStore>>,
    Path((rid, run_id)): Path<(String, String)>,
) -> ApiResult<ApiJson<CrystallizeResponse>> {
    let routines = Arc::clone(sched.store());
    blocking(move || {
        let routine = routines.get(&rid).map_err(|e| match e {
            AutomationError::NotFound => ApiErr::not_found("routine not found"),
            other => ApiErr::internal(other.to_string()),
        })?;
        let run = routines
            .run(&run_id)
            .ok()
            .filter(|r| r.routine_id == routine.id)
            .ok_or_else(|| ApiErr::not_found("routine run not found"))?;
        let summary = run
            .conversation_id
            .as_deref()
            .map(|c| run_summary(&state, c, run.run_id.as_deref()))
            .unwrap_or_default();
        let crystallized = evolution::crystallize(&routine, &run, &summary, od_runs::now_ms())?;
        store.save(&crystallized)?;
        tracing::info!(routine = %routine.id, run = %run.id, "crystallized");
        Ok(ApiJson(CrystallizeResponse {
            crystallized,
            routine_id: routine.id,
            run_id: run.id,
        }))
    })
    .await
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProposalsResponse {
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Serialize)]
pub struct ProposalResponse {
    pub proposal: Proposal,
}

async fn list(
    Extension(store): Extension<Arc<EvolutionStore>>,
    Query(q): Query<ListQuery>,
) -> ApiResult<ApiJson<ProposalsResponse>> {
    let status = match q.status.as_deref().unwrap_or("all") {
        "all" => None,
        "pending-review" => Some(ProposalStatus::PendingReview),
        "applied" => Some(ProposalStatus::Applied),
        "rejected" => Some(ProposalStatus::Rejected),
        other => return Err(ApiErr::bad_request(format!("unknown status {other:?}"))),
    };
    let proposals = blocking(move || Ok(store.proposals(status)?)).await?;
    Ok(ApiJson(ProposalsResponse { proposals }))
}

async fn get_one(
    Extension(store): Extension<Arc<EvolutionStore>>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<ProposalResponse>> {
    let proposal = blocking(move || Ok(store.proposal(&id)?)).await?;
    Ok(ApiJson(ProposalResponse { proposal }))
}

fn meta_str<'a>(p: &'a Proposal, key: &str) -> &'a str {
    p.metadata
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Writes a skill proposal as a new user skill under the first free name.
fn apply_skill(state: &AppState, p: &Proposal) -> ApiResult<Value> {
    let base = meta_str(p, "name").trim();
    let base = if base.is_empty() {
        p.title.as_str()
    } else {
        base
    };
    let triggers = p
        .metadata
        .get("triggers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let root = state.store.data_dir().user_skills_dir();
    for n in 1..=MAX_SKILL_NAME_TRIES {
        let name = if n == 1 {
            base.to_owned()
        } else {
            format!("{base} {n}")
        };
        let content = state.content.snapshot();
        let draft = SkillDraft {
            name,
            description: meta_str(p, "description").to_owned(),
            triggers: Vec::clone(&triggers),
            body: meta_str(p, "body").to_owned(),
        };
        match user_content::create_skill(&root, &draft, |id| content.find_skill(id).is_some()) {
            Ok(slug) => {
                state.content.reload();
                return Ok(json!({
                    "skillId": slug,
                    "action": p.action,
                    "path": format!("skills/{slug}/SKILL.md"),
                }));
            }
            Err(user_content::WriteError::Exists(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiErr::new(
        StatusCode::CONFLICT,
        ErrorCode::Conflict,
        "no free skill name",
    ))
}

#[derive(Debug, Serialize)]
pub struct ApplyResponse {
    pub proposal: Proposal,
    pub result: Value,
}

async fn apply(
    State(state): State<AppState>,
    Extension(store): Extension<Arc<EvolutionStore>>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<ApplyResponse>> {
    blocking(move || {
        let proposal = store.review(&id, &Review::Apply, od_runs::now_ms(), |p| {
            match p.target_kind {
                TargetKind::Skill => apply_skill(&state, p),
                TargetKind::MemoryNode => {
                    let kind = meta_str(p, "memoryType");
                    let kind = if kind.is_empty() { "project" } else { kind };
                    let memory = state.store.add_global_memory(kind, &p.patch.after)?;
                    Ok(json!({ "memoryId": memory.id, "action": p.action }))
                }
            }
        })?;
        let result = proposal.metadata["appliedResult"].clone();
        tracing::info!(proposal = %proposal.id, "proposal applied");
        Ok(ApiJson(ApplyResponse { proposal, result }))
    })
    .await
}

#[derive(Debug, Default, Deserialize)]
pub struct RejectBody {
    pub reason: Option<String>,
}

async fn reject(
    Extension(store): Extension<Arc<EvolutionStore>>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<ApiJson<ProposalResponse>> {
    let body: RejectBody = crate::routes::library::optional_json(&body)?;
    let proposal = blocking(move || {
        let review = Review::Reject {
            reason: body.reason,
        };
        store.review(&id, &review, od_runs::now_ms(), |_| -> ApiResult<Value> {
            Err(ApiErr::internal("a rejection applies nothing"))
        })
    })
    .await?;
    Ok(ApiJson(ProposalResponse { proposal }))
}

#[derive(Debug, Serialize)]
pub struct PacketsResponse {
    pub packets: Vec<Packet>,
}

async fn packets(
    Extension(store): Extension<Arc<EvolutionStore>>,
) -> ApiResult<ApiJson<PacketsResponse>> {
    let packets = blocking(move || Ok(store.packets()?)).await?;
    Ok(ApiJson(PacketsResponse { packets }))
}
