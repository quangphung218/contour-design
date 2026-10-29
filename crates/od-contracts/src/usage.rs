//! Settings ▸ About ▸ Your usage: numbers worked out from what is already
//! on this machine, for beta users to paste into the "Share your usage"
//! discussion on GitHub. Nothing here leaves the machine on its own (no
//! telemetry, `docs/decisions.md`).

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
    /// How far first-run setup got; `None` when the data folder is older
    /// than 0.2.0-beta.8 and its setup was never recorded.
    #[serde(default)]
    pub setup: Option<SetupUsage>,
}

/// A first-run setup step, in order. Each is recorded once, the first
/// time it happens in a data folder made by 0.2.0-beta.8 or later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    /// The app started on a new data folder.
    Opened,
    /// Agent detection found an installed coding agent.
    AgentFound,
    /// An agent (or an API key) passed its connection test.
    AgentTested,
    /// The first project was created.
    Project,
    /// A project was first shown in the preview.
    Preview,
}

impl SetupStep {
    pub const ALL: [Self; 5] = [
        Self::Opened,
        Self::AgentFound,
        Self::AgentTested,
        Self::Project,
        Self::Preview,
    ];

    /// The wire and database name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::AgentFound => "agent_found",
            Self::AgentTested => "agent_tested",
            Self::Project => "project",
            Self::Preview => "preview",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }

    /// 1 for [`Self::Opened`] … 5 for [`Self::Preview`].
    #[must_use]
    pub fn number(self) -> u32 {
        match self {
            Self::Opened => 1,
            Self::AgentFound => 2,
            Self::AgentTested => 3,
            Self::Project => 4,
            Self::Preview => 5,
        }
    }
}

/// When one setup step was first reached (Unix ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupMark {
    pub step: SetupStep,
    pub at: i64,
}

/// First-run setup: the steps reached and where it stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupUsage {
    /// Steps reached, in step order.
    pub steps: Vec<SetupMark>,
    /// The furthest step reached, 1 to 5 (0 with none).
    pub reached: u32,
    /// The step after the furthest one, until the first preview.
    pub stopped_at: Option<SetupStep>,
    /// From the first open to the first preview.
    pub took_ms: Option<i64>,
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
