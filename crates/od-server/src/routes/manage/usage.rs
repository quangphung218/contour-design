//! Settings ▸ About ▸ Your usage (no telemetry: computed on request, from
//! what is already on this machine, and never sent anywhere):
//!
//! - `GET /api/usage` time to first preview, projects put to use,
//!   finished runs by failure cause, Stops apart, and how far first-run
//!   setup got ([`UsageResponse`]).
//! - `POST /api/projects/:id/exported` records the project's first export.
//!   Exports run in several routes and one is read through `raw`, so the UI
//!   reports a finished download here. The first preview is recorded by the
//!   preview route itself.
//!
//! Setup steps ([`SetupStep`]) are recorded where they happen: the first
//! open by the store, an agent found by `GET /api/agents`, a passed test by
//! `POST /api/test/connection`, the first project by the store and the
//! first preview by the preview route.

use crate::error::{ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use od_contracts::{
    ENGAGED_TURNS, EngagedUsage, FailureCategory, FirstPreviewUsage, RunCause, RunStatus, RunUsage,
    SetupMark, SetupStep, SetupUsage, UsageResponse,
};
use od_store::usage::{ProjectUsage, UsageMark};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub async fn usage(State(state): State<AppState>) -> ApiResult<ApiJson<UsageResponse>> {
    let (since, projects, setup) = state
        .blocking(|s| {
            let (since, projects) = s.project_usage()?;
            Ok((since, projects, s.setup_marks()?))
        })
        .await?;
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
    let mut usage = summarize(since, &projects, &runs);
    usage.setup = setup.map(setup_usage);
    Ok(ApiJson(usage))
}

/// Records a setup step the first time it happens. A failure is logged,
/// never passed on: the request it rides on matters more.
pub(crate) async fn record_setup(state: &AppState, step: SetupStep) {
    if let Err(err) = state.blocking(move |s| s.mark_setup(step)).await {
        tracing::warn!(error = %err.message, ?step, "recording a setup step failed");
    }
}

/// How far setup got from its marks (in step order): the furthest step,
/// the one after it until the first preview, and the time from the first
/// open to the first preview.
#[must_use]
pub fn setup_usage(steps: Vec<SetupMark>) -> SetupUsage {
    let furthest = steps.iter().map(|m| m.step).max();
    let at = |step| steps.iter().find(|m| m.step == step).map(|m| m.at);
    let took_ms = at(SetupStep::Opened)
        .zip(at(SetupStep::Preview))
        .map(|(open, preview)| (preview - open).max(0));
    SetupUsage {
        reached: furthest.map_or(0, SetupStep::number),
        stopped_at: match furthest {
            None => Some(SetupStep::Opened),
            Some(step) => SetupStep::ALL.into_iter().find(|s| *s > step),
        },
        took_ms,
        steps,
    }
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
    let (mut total, mut canceled) = (0, 0);
    for (status, category) in runs {
        match status {
            RunStatus::Running => {}
            RunStatus::Canceled => canceled += 1,
            RunStatus::Succeeded => total += 1,
            RunStatus::Failed => {
                total += 1;
                *causes
                    .entry(category.map_or("other", cause_name))
                    .or_default() += 1;
            }
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
    causes.sort_by_key(|a| std::cmp::Reverse(a.count));
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
            canceled,
        },
        setup: None,
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
    fn runs_split_by_cause_most_frequent_first_and_skip_live_and_stopped_ones() {
        let runs = [
            (RunStatus::Running, None),
            (RunStatus::Succeeded, None),
            (RunStatus::Succeeded, None),
            (RunStatus::Canceled, None),
            (RunStatus::Canceled, None),
            (RunStatus::Failed, Some(FailureCategory::Timeout)),
            (RunStatus::Failed, Some(FailureCategory::SpawnFailed)),
            (RunStatus::Failed, Some(FailureCategory::SpawnFailed)),
            (RunStatus::Failed, None),
        ];
        let u = summarize(0, &[], &runs);
        assert_eq!(u.runs.total, 6);
        assert_eq!(u.runs.failed, 4);
        assert_eq!(u.runs.canceled, 2, "a Stop is no failure");
        let causes: Vec<(&str, u32)> = u
            .runs
            .causes
            .iter()
            .map(|c| (c.cause.as_str(), c.count))
            .collect();
        assert_eq!(causes, [("spawn_failed", 2), ("other", 1), ("timeout", 1)]);
    }

    fn marks(steps: &[(SetupStep, i64)]) -> Vec<SetupMark> {
        steps
            .iter()
            .map(|&(step, at)| SetupMark { step, at })
            .collect()
    }

    #[test]
    fn setup_stopped_at_the_agent_when_none_was_found() {
        let u = setup_usage(marks(&[(SetupStep::Opened, 1_000)]));
        assert_eq!(u.reached, 1);
        assert_eq!(u.stopped_at, Some(SetupStep::AgentFound));
        assert_eq!(u.took_ms, None);
    }

    #[test]
    fn setup_through_the_first_preview_says_how_long_it_took() {
        let u = setup_usage(marks(&[
            (SetupStep::Opened, 1_000),
            (SetupStep::AgentFound, 2_000),
            (SetupStep::AgentTested, 5_000),
            (SetupStep::Project, 9_000),
            (SetupStep::Preview, 61_000),
        ]));
        assert_eq!((u.reached, u.stopped_at), (5, None));
        assert_eq!(u.took_ms, Some(60_000));
        assert_eq!(u.steps.len(), 5);
    }

    #[test]
    fn setup_counts_the_furthest_step_even_with_one_skipped() {
        // An API key passes its test without an agent ever found.
        let u = setup_usage(marks(&[
            (SetupStep::Opened, 1_000),
            (SetupStep::AgentTested, 3_000),
        ]));
        assert_eq!(u.reached, 3);
        assert_eq!(u.stopped_at, Some(SetupStep::Project));
        // A clock step backwards is no negative time.
        let back = setup_usage(marks(&[
            (SetupStep::Opened, 5_000),
            (SetupStep::Preview, 4_000),
        ]));
        assert_eq!(back.took_ms, Some(0));
        let none = setup_usage(Vec::new());
        assert_eq!(
            (none.reached, none.stopped_at),
            (0, Some(SetupStep::Opened))
        );
    }

    #[test]
    fn setup_step_names_match_the_wire_names() {
        for step in SetupStep::ALL {
            assert_eq!(serde_json::to_value(step).unwrap(), json!(step.as_str()));
            assert_eq!(SetupStep::parse(step.as_str()), Some(step));
        }
        assert_eq!(SetupStep::ALL.map(SetupStep::number), [1, 2, 3, 4, 5]);
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
