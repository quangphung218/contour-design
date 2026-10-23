//! Wire types of track C2-chat-home.

use serde::{Deserialize, Serialize};

/// Where one media generation of a run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaTaskStatus {
    Queued,
    Running,
    Done,
    Failed,
}

impl MediaTaskStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }
}

/// A media generation the agent asked for during a run (the original's
/// `ProjectMediaTask`). The daemon emits it as an `agent` event of type
/// `media` at each change; the latest per `taskId` wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTask {
    pub task_id: String,
    /// `image`, `video` or `audio`.
    pub surface: String,
    pub model: String,
    pub status: MediaTaskStatus,
    /// Progress lines, oldest first.
    #[serde(default)]
    pub progress: Vec<String>,
    /// Project path of the written file, once done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
}
