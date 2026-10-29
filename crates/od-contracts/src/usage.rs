//! Settings ▸ About ▸ Your usage: numbers worked out from what is already
//! on this machine, for beta users to paste into a discussion. Nothing here
//! leaves the machine on its own (no telemetry, `docs/decisions.md`).

use serde::{Deserialize, Serialize};

/// Fewest projects (or finished runs) a number is shown for; below it the
/// screen says there is not enough yet instead of a misleading figure.
pub const USAGE_MIN_SAMPLE: u32 = 3;

/// `GET /api/usage`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageResponse {
    /// When this data folder started recording previews and exports
    /// (first launch of 0.2.0-beta.7 or later). Only projects created
    /// since then count towards `firstPreview` and `engaged`.
    pub since: i64,
    pub first_preview: FirstPreviewUsage,
    pub engaged: EngagedUsage,
    pub runs: RunUsage,
}

/// Time from creating a project to its first preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstPreviewUsage {
    /// Projects (created since `since`) that reached a preview.
    pub projects: u32,
    /// Median over those projects; `None` when there are none.
    pub median_ms: Option<i64>,
}

/// Projects with at least one export or [`ENGAGED_TURNS`] chat turns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngagedUsage {
    /// Projects created since `since`.
    pub projects: u32,
    pub engaged: u32,
}

/// Chat turns that make a project count as used without an export.
pub const ENGAGED_TURNS: u32 = 3;

/// Runs that ended on their own, and the failed ones by cause. A run the
/// user stopped is no failure: it is counted apart and left out of both.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunUsage {
    /// Runs that succeeded or failed.
    pub total: u32,
    pub failed: u32,
    /// Failure causes, most frequent first.
    pub causes: Vec<RunCause>,
    /// Runs the user stopped.
    pub canceled: u32,
}

/// One cause: a `failureCategory` (`spawn_failed`, `agent_error`,
/// `timeout`, …), or `other` for a failure without one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCause {
    pub cause: String,
    pub count: u32,
}
