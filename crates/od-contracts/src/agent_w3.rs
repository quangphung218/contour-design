//! Track W3-agent wire types: session mode changes, project memory,
//! Design Jury (critique theater) reports and transcript export
//! (spec 1.1, 1.5).

use crate::SessionMode;
use serde::{Deserialize, Serialize};

/// `PATCH /api/projects/:id/conversations/:cid`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<SessionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Who wrote a memory entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    /// Typed in the memory panel.
    #[default]
    Manual,
    /// Picked up from a chat message ("remember that…").
    Chat,
}

impl MemorySource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Chat => "chat",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Self {
        if raw == "chat" {
            Self::Chat
        } else {
            Self::Manual
        }
    }
}

/// One remembered fact of a project, read by the agent on every turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub project_id: String,
    pub text: String,
    pub source: MemorySource,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryResponse {
    pub memory: Vec<MemoryEntry>,
}

/// `POST /api/projects/:id/memory`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateMemoryRequest {
    pub text: String,
}

/// `GET`/`PUT /api/projects/:id/critique`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CritiqueSettings {
    pub enabled: bool,
}

/// The five Design Jury panelists. Only the designer does not score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelRole {
    Designer,
    Critic,
    Brand,
    A11y,
    Copy,
}

impl PanelRole {
    pub const ALL: [Self; 5] = [
        Self::Designer,
        Self::Critic,
        Self::Brand,
        Self::A11y,
        Self::Copy,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Designer => "designer",
            Self::Critic => "critic",
            Self::Brand => "brand",
            Self::A11y => "a11y",
            Self::Copy => "copy",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|r| r.as_str().eq_ignore_ascii_case(raw.trim()))
    }

    /// Share of the composite (v1 weights of the original; designer 0).
    #[must_use]
    pub fn weight(self) -> f64 {
        match self {
            Self::Designer => 0.0,
            Self::Critic => 0.4,
            Self::Brand | Self::A11y | Self::Copy => 0.2,
        }
    }
}

/// One panelist's verdict in a round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelistVerdict {
    pub role: PanelRole,
    /// `None` for the designer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default)]
    pub must_fix: Vec<String>,
    /// Designer notes, or the first dimension notes of a scorer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// One round: verdicts so far and, once closed, its composite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CritiqueRound {
    pub n: u32,
    pub panelists: Vec<PanelistVerdict>,
    /// Recomputed from the scores with [`PanelRole::weight`], never taken
    /// from the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite: Option<f64>,
    #[serde(default)]
    pub closed: bool,
}

/// Where a Design Jury run stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CritiqueStatus {
    #[default]
    Running,
    /// Shipped with a composite at or above the threshold.
    Shipped,
    /// Shipped after the last round, under the threshold.
    BelowThreshold,
    /// The run ended without a `<SHIP>`.
    Interrupted,
    /// The protocol could not be read (unbalanced or oversize block).
    Degraded,
}

/// The live state of a Design Jury run, sent as an `agent` event of type
/// `critique` every time a panelist, round or ship block closes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CritiqueReport {
    pub status: CritiqueStatus,
    pub threshold: f64,
    pub max_rounds: u32,
    pub rounds: Vec<CritiqueRound>,
    /// Round the `<SHIP>` named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ship_round: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Project-relative file the shipped artifact was written to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// `GET /api/projects/:id/conversations/:cid/transcript?format=md|json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptFormat {
    #[default]
    Md,
    Json,
}
