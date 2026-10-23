//! Projects page support (spec 1.1):
//!
//! - `GET /api/search?q=` projects by name, conversations by title and
//!   messages by content.
//! - `GET /api/project-status` every project's run state, as the original's
//!   `project.status`: the latest persisted run, overridden by a live run in
//!   the registry, `awaiting_input` when the last turn asks an unanswered
//!   question form, `not_started` when absent.
//! - `POST /api/projects/:id/viewed`, `GET /api/recently-viewed`.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use od_contracts::RunStatus;
use od_store::manage::SearchHit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How many projects "Recently viewed" keeps.
const RECENT_VIEWS: usize = 50;
/// Longest accepted query.
const MAX_QUERY_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
}

pub async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<ApiJson<SearchResponse>> {
    if q.q.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiErr::bad_request(format!(
            "q must be at most {MAX_QUERY_CHARS} characters"
        )));
    }
    let hits = state.blocking(move |s| s.search(&q.q)).await?;
    Ok(ApiJson(SearchResponse { hits }))
}

/// One project's run state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatus {
    /// `not_started`, `running`, `awaiting_input`, `incomplete`,
    /// `succeeded`, `failed` or `canceled`.
    pub value: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProjectStatusResponse {
    /// Keyed by project id; every project is present.
    pub statuses: BTreeMap<String, ProjectStatus>,
}

fn stored_value(status: &str) -> &'static str {
    match status {
        "running" => "running",
        "succeeded" => "succeeded",
        "canceled" => "canceled",
        _ => "failed",
    }
}

pub async fn project_status(
    State(state): State<AppState>,
) -> ApiResult<ApiJson<ProjectStatusResponse>> {
    let (ids, stored) = state
        .blocking(|s| {
            let ids: Vec<String> = s.list_projects()?.into_iter().map(|p| p.id).collect();
            Ok((ids, s.stored_run_statuses()?))
        })
        .await?;
    let mut statuses: BTreeMap<String, ProjectStatus> = ids
        .into_iter()
        .map(|id| {
            (
                id,
                ProjectStatus {
                    value: "not_started",
                    updated_at: None,
                    run_id: None,
                },
            )
        })
        .collect();
    for s in stored {
        if let Some(slot) = statuses.get_mut(&s.project_id) {
            *slot = ProjectStatus {
                value: if s.awaiting_input {
                    "awaiting_input"
                } else if s.incomplete {
                    "incomplete"
                } else {
                    stored_value(&s.status)
                },
                updated_at: Some(s.at),
                run_id: s.run_id,
            };
        }
    }
    // Live runs: an active one always wins, a finished one when newer.
    for run in state.runs.registry.list() {
        let info = run.info();
        let Some(slot) = info.project_id.as_ref().and_then(|p| statuses.get_mut(p)) else {
            continue;
        };
        let at = info.ended_at.unwrap_or(info.created_at);
        let active = info.status == RunStatus::Running;
        let newer = slot.updated_at.is_none_or(|t| at > t);
        // The stored row of this same run already knows about its question.
        let same_run = slot.run_id.as_deref() == Some(info.id.as_str());
        if !active && (!newer || same_run) {
            continue;
        }
        *slot = ProjectStatus {
            value: info.status.as_str(),
            updated_at: Some(at),
            run_id: Some(info.id),
        };
    }
    Ok(ApiJson(ProjectStatusResponse { statuses }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewedResponse {
    pub viewed_at: i64,
}

pub async fn mark_viewed(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<ViewedResponse>> {
    let viewed_at = state.blocking(move |s| s.mark_viewed(&id)).await?;
    Ok(ApiJson(ViewedResponse { viewed_at }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentView {
    pub project_id: String,
    pub viewed_at: i64,
}

#[derive(Debug, Serialize)]
pub struct RecentViewsResponse {
    pub views: Vec<RecentView>,
}

pub async fn recently_viewed(
    State(state): State<AppState>,
) -> ApiResult<ApiJson<RecentViewsResponse>> {
    let views = state
        .blocking(|s| s.recently_viewed(RECENT_VIEWS))
        .await?
        .into_iter()
        .map(|(project_id, viewed_at)| RecentView {
            project_id,
            viewed_at,
        })
        .collect();
    Ok(ApiJson(RecentViewsResponse { views }))
}
