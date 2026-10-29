//! Settings ▸ About ▸ Your usage (no telemetry: computed on request, from
//! what is already on this machine, and never sent anywhere):
//!
//! - `GET /api/usage` time to first preview, projects put to use, and
//!   finished runs by failure cause ([`UsageResponse`]).
//! - `POST /api/projects/:id/exported` records the project's first export.
//!   Exports run in several routes and one is read through `raw`, so the UI
//!   reports a finished download here. The first preview is recorded by the
//!   preview route itself.

use crate::error::{ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use od_contracts::{
    ENGAGED_TURNS, EngagedUsage, FailureCategory, FirstPreviewUsage, RunCause, RunStatus, RunUsage,
    UsageResponse,
};
use od_store::usage::{ProjectUsage, UsageMark};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub async fn usage(State(state): State<AppState>) -> ApiResult<ApiJson<UsageResponse>> {
    let (since, projects) = state.blocking(|s| s.project_usage()).await?;
    let runs = state
        .runs
        .registry
        .list()
        .iter()
        .map(|run| {
            let category = run.end_payload().and_then(|e| e.failure_category);
            (run.status(), category)
        })
        .collect::<Vec<_>>();
    Ok(ApiJson(summarize(since, &projects, &runs)))
}

/// `{"recorded": bool}`: false when the project already had an export or
/// does not exist.
pub async fn exported(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<Value>> {
    let recorded = state
        .blocking(move |s| s.mark_usage(&id, UsageMark::Export))
        .await?;
    Ok(ApiJson(json!({ "recorded": recorded })))
}

/// The three numbers from the stored projects and the runs' end states.
#[must_use]
pub fn summarize(
    since: i64,
    projects: &[ProjectUsage],
    runs: &[(RunStatus, Option<FailureCategory>)],
) -> UsageResponse {
    let mut waits: Vec<i64> = projects
        .iter()
        .filter_map(|p| p.first_preview_at.map(|at| (at - p.created_at).max(0)))
        .collect();
    waits.sort_unstable();
    let engaged = projects
        .iter()
        .filter(|p| p.exported || p.turns >= ENGAGED_TURNS)
        .count();
    let mut causes = BTreeMap::<&'static str, u32>::new();
    let mut total = 0;
    for (status, category) in runs {
        let cause = match status {
            RunStatus::Running => continue,
            RunStatus::Succeeded => None,
            RunStatus::Canceled => Some("canceled"),
            RunStatus::Failed => Some(category.map_or("other", cause_name)),
        };
        total += 1;
        if let Some(cause) = cause {
            *causes.entry(cause).or_default() += 1;
        }
    }
    let mut causes: Vec<RunCause> = causes
        .into_iter()
        .map(|(cause, count)| RunCause {
            cause: cause.to_owned(),
            count,
        })
        .collect();
    // Stable: the map already ordered equal counts by name.
    causes.sort_by(|a, b| b.count.cmp(&a.count));
    UsageResponse {
        since,
        first_preview: FirstPreviewUsage {
            projects: count(waits.len()),
            median_ms: median(&waits),
        },
        engaged: EngagedUsage {
            projects: count(projects.len()),
            engaged: count(engaged),
        },
        runs: RunUsage {
            total,
            failed: causes.iter().map(|c| c.count).sum(),
            causes,
        },
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Median of sorted values; the mean of the middle two for an even count.
fn median(sorted: &[i64]) -> Option<i64> {
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[mid]),
        _ => Some(sorted[mid - 1] + (sorted[mid] - sorted[mid - 1]) / 2),
    }
}

/// The wire name of a failure category (`end.failureCategory`).
fn cause_name(category: FailureCategory) -> &'static str {
    match category {
        FailureCategory::AgentError => "agent_error",
        FailureCategory::RateLimit => "rate_limit",
        FailureCategory::Transient => "transient",
        FailureCategory::Timeout => "timeout",
        FailureCategory::ToolLoop => "tool_loop",
        FailureCategory::SpawnFailed => "spawn_failed",
        FailureCategory::DaemonRestart => "daemon_restart",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(created_at: i64, preview: Option<i64>, exported: bool, turns: u32) -> ProjectUsage {
        ProjectUsage {
            created_at,
            first_preview_at: preview,
            exported,
            turns,
        }
    }

    #[test]
    fn no_data_gives_no_median_and_zero_counts() {
        let u = summarize(5, &[], &[]);
        assert_eq!(u.since, 5);
        assert_eq!(u.first_preview, FirstPreviewUsage::default());
        assert_eq!(u.engaged, EngagedUsage::default());
        assert_eq!(u.runs, RunUsage::default());
    }

    #[test]
    fn the_median_wait_covers_only_previewed_projects() {
        let projects = [
            project(100, Some(400), false, 0),
            project(100, Some(200), false, 0),
            project(100, None, false, 0),
            project(100, Some(1100), false, 0),
        ];
        let u = summarize(0, &projects, &[]);
        assert_eq!(u.first_preview.projects, 3);
        assert_eq!(u.first_preview.median_ms, Some(300));
        assert_eq!(median(&[10, 20]), Some(15));
        assert_eq!(median(&[7]), Some(7));
        // A clock step backwards is no negative wait.
        let back = summarize(0, &[project(100, Some(60), false, 0)], &[]);
        assert_eq!(back.first_preview.median_ms, Some(0));
    }

    #[test]
    fn a_project_counts_as_used_with_an_export_or_enough_turns() {
        let projects = [
            project(0, None, true, 0),
            project(0, None, false, ENGAGED_TURNS),
            project(0, None, false, ENGAGED_TURNS - 1),
        ];
        let u = summarize(0, &projects, &[]);
        assert_eq!((u.engaged.engaged, u.engaged.projects), (2, 3));
    }

    #[test]
    fn runs_split_by_cause_most_frequent_first_and_skip_live_ones() {
        let runs = [
            (RunStatus::Running, None),
            (RunStatus::Succeeded, None),
            (RunStatus::Succeeded, None),
            (RunStatus::Canceled, None),
            (RunStatus::Failed, Some(FailureCategory::Timeout)),
            (RunStatus::Failed, Some(FailureCategory::SpawnFailed)),
            (RunStatus::Failed, Some(FailureCategory::SpawnFailed)),
            (RunStatus::Failed, None),
        ];
        let u = summarize(0, &[], &runs);
        assert_eq!(u.runs.total, 7);
        assert_eq!(u.runs.failed, 5);
        let causes: Vec<(&str, u32)> = u
            .runs
            .causes
            .iter()
            .map(|c| (c.cause.as_str(), c.count))
            .collect();
        assert_eq!(
            causes,
            [
                ("spawn_failed", 2),
                ("canceled", 1),
                ("other", 1),
                ("timeout", 1)
            ]
        );
    }

    #[test]
    fn cause_names_match_the_wire_names() {
        for category in [
            FailureCategory::AgentError,
            FailureCategory::RateLimit,
            FailureCategory::Transient,
            FailureCategory::Timeout,
            FailureCategory::ToolLoop,
            FailureCategory::SpawnFailed,
            FailureCategory::DaemonRestart,
        ] {
            assert_eq!(
                serde_json::to_value(category).unwrap(),
                serde_json::json!(cause_name(category))
            );
        }
    }
}
