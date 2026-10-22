//! Runs, agents, conversations and the normalized agent event union
//! (spec 2.2, 2.3).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `protocolVersion` carried on every `start` event.
pub const RUN_PROTOCOL_VERSION: u32 = 1;

/// SSE event names of a run stream (spec 2.2).
pub mod sse {
    pub const START: &str = "start";
    pub const RUN_RETRY_ATTEMPTED: &str = "run_retry_attempted";
    pub const AGENT: &str = "agent";
    pub const STDOUT: &str = "stdout";
    pub const STDERR: &str = "stderr";
    pub const DIAGNOSTIC: &str = "diagnostic";
    pub const ERROR: &str = "error";
    pub const END: &str = "end";
}

/// The normalized payload of an `agent` SSE event, the same for every CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentEvent {
    #[serde(rename_all = "camelCase")]
    Status {
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ttft_ms: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    TextDelta {
        delta: String,
    },
    ThinkingStart,
    ThinkingDelta {
        delta: String,
    },
    ThinkingTokens {
        tokens: f64,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    ToolInputDelta {
        id: String,
        name: String,
        delta: String,
    },
    /// One per main-stream assistant message; sub-agent frames never fire it.
    #[serde(rename_all = "camelCase")]
    TurnEnd {
        stop_reason: String,
    },
    #[serde(rename_all = "camelCase")]
    Usage {
        #[serde(default)]
        usage: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
    #[serde(rename_all = "camelCase")]
    RequestUsage {
        request_id: String,
        usage: Value,
    },
    /// An error the agent reported in-stream. The engine turns it into the
    /// run's `error` SSE event, not an `agent` event.
    Error {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        terminal: bool,
    },
    /// Tool-loop guard warning or stop (spec 2.3).
    #[serde(rename_all = "camelCase")]
    ToolLoop {
        kind: ToolLoopKind,
        count: u32,
        tool_name: String,
        stopped: bool,
    },
    /// A stdout line the parser did not recognise, bounded in length.
    Raw {
        line: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        truncated: bool,
    },
}

impl AgentEvent {
    /// Whether the user saw output: text, thinking or a tool call. Retry is
    /// only allowed while this has never been true (spec 2.3).
    #[must_use]
    pub fn is_visible_output(&self) -> bool {
        match self {
            Self::TextDelta { delta } | Self::ThinkingDelta { delta } => !delta.is_empty(),
            Self::ToolUse { .. } | Self::ToolResult { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLoopKind {
    /// N tool errors in a row.
    ConsecutiveErrors,
    /// The same failing action repeated N times.
    RepeatedAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl RunStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// Why a run failed, as surfaced on `end.failureCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    AgentError,
    RateLimit,
    Transient,
    Timeout,
    ToolLoop,
    SpawnFailed,
    /// The daemon restarted while the run was in flight.
    DaemonRestart,
}

/// `start` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartPayload {
    pub run_id: String,
    pub agent_id: String,
    pub bin: String,
    pub protocol_version: u32,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Whether this run continues a stored agent session.
    #[serde(default)]
    pub resumed: bool,
}

/// `end` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEndPayload {
    pub status: RunStatus,
    pub code: Option<i32>,
    #[serde(default)]
    pub signal: Option<String>,
    pub terminal_at: i64,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub resumable: bool,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub failure_category: Option<FailureCategory>,
    #[serde(default)]
    pub error: Option<String>,
}

/// `error` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunErrorPayload {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `stdout` / `stderr` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunChunkPayload {
    pub chunk: String,
}

/// `run_retry_attempted` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRetryPayload {
    pub retry_attempt_index: u32,
    pub retry_max_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<FailureCategory>,
    pub delay_ms: u64,
}

/// How a session is used (stored on conversations).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    #[default]
    Design,
    Plan,
    Chat,
}

/// `POST /api/runs` body (`ChatRequest`, main fields).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub agent_id: String,
    pub message: String,
    pub project_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub user_message_id: Option<String>,
    #[serde(default)]
    pub assistant_message_id: Option<String>,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub skill_id: Option<String>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub design_system_id: Option<String>,
    /// Project-relative paths the user attached.
    #[serde(default)]
    pub attachments: Vec<String>,
    /// Preview comments sent along as scoped context for this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comment_attachments: Vec<crate::CommentAttachment>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub session_mode: Option<SessionMode>,
    #[serde(default)]
    pub locale: Option<String>,
}

/// `POST /api/runs` → 202.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunResponse {
    pub run_id: String,
    pub conversation_id: String,
    pub assistant_message_id: String,
    pub client_request_id: Option<String>,
    pub reused: bool,
    pub resumed: bool,
}

/// One run as listed by `GET /api/runs` and `GET /api/runs/:id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: String,
    pub status: RunStatus,
    pub agent_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub assistant_message_id: Option<String>,
    #[serde(default)]
    pub client_request_id: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub ended_at: Option<i64>,
    pub last_event_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunsResponse {
    pub runs: Vec<RunInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResponse {
    pub run: RunInfo,
}

/// `POST /api/runs/:id/steer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteerRequest {
    pub message: String,
}

/// `POST /api/runs/:id/cancel` and `/steer` → 200/202.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunActionResponse {
    pub ok: bool,
    pub status: RunStatus,
}

/// One coding agent as reported by `GET /api/agents`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub bin: String,
    pub available: bool,
    /// Resolved executable, when found.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Flags found in `--help` that change how the agent is launched.
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub supports_resume: bool,
    pub supports_steer: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentsResponse {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub project_id: String,
    pub title: Option<String>,
    pub session_mode: SessionMode,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationsResponse {
    pub conversations: Vec<Conversation>,
}

/// `POST /api/projects/:id/conversations`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConversationRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub session_mode: Option<SessionMode>,
    /// Fork: copy the messages of this conversation (same project)…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_from_conversation_id: Option<String>,
    /// …up to and including this message. Absent: copy all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_after_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationResponse {
    pub conversation: Conversation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_status: Option<RunStatus>,
    #[serde(default)]
    pub last_run_event_id: Option<u64>,
    pub position: i64,
    /// Persisted `agent` payloads of the run, in order.
    #[serde(default)]
    pub events: Vec<Value>,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub ended_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesResponse {
    pub messages: Vec<Message>,
}

/// `PUT /api/projects/:id/conversations/:cid/messages/:mid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertMessageRequest {
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: Message,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn agent_event_wire_shape() {
        let ev = AgentEvent::ToolResult {
            tool_use_id: "t1".into(),
            content: "ok".into(),
            is_error: false,
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap_or_default(),
            json!({"type": "tool_result", "toolUseId": "t1", "content": "ok", "isError": false})
        );
        let back: AgentEvent =
            serde_json::from_value(json!({"type": "turn_end", "stopReason": "end_turn"}))
                .unwrap_or(AgentEvent::ThinkingStart);
        assert_eq!(
            back,
            AgentEvent::TurnEnd {
                stop_reason: "end_turn".into()
            }
        );
    }
}
