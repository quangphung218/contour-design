//! Run stream wire types (spec 2.2) and SSE frame decoding.
//!
//! TODO(M2 merge): these mirror the run types M2 adds to `od-contracts`
//! (`AgentEvent`, `RunStartPayload`, `RunEndPayload`, `RunErrorPayload`,
//! `ChatRequest`, `CreateRunResponse`, `AgentInfo`, `Message`). Once M2 is on
//! `main`, replace this module with re-exports from `od_contracts` and keep
//! only [`RunFrame`] / [`decode_frame`].
//!
//! Deserialisation is lenient on purpose: unknown event types and fields are
//! ignored so a newer daemon never breaks an older UI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The normalized payload of an `agent` SSE event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    #[serde(rename_all = "camelCase")]
    Status {
        #[serde(default)]
        label: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
    },
    TextDelta {
        delta: String,
    },
    ThinkingStart,
    ThinkingDelta {
        delta: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_use_id: String,
        /// A string from most CLIs, an array of content blocks from some.
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(rename_all = "camelCase")]
    TurnEnd {
        #[serde(default)]
        stop_reason: String,
    },
    #[serde(rename_all = "camelCase")]
    Usage {
        #[serde(default)]
        cost_usd: Option<f64>,
        #[serde(default)]
        duration_ms: Option<f64>,
    },
    Error {
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    ToolLoop {
        #[serde(default)]
        tool_name: String,
        #[serde(default)]
        count: u32,
        #[serde(default)]
        stopped: bool,
    },
    Raw {
        line: String,
    },
    /// Any type this UI does not render (`thinking_tokens`, `request_usage`…).
    #[serde(other)]
    Unknown,
}

/// Terminal status of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Canceled,
}

/// `start` payload (fields the UI shows).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStart {
    pub run_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub resumed: bool,
}

/// `end` payload (fields the UI shows).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEnd {
    pub status: RunStatus,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
}

/// One decoded SSE frame of `GET /api/runs/:id/events`.
#[derive(Debug, Clone, PartialEq)]
pub enum RunFrame {
    Start(RunStart),
    Agent(AgentEvent),
    Error(String),
    End(RunEnd),
    /// `stdout`, `stderr`, `diagnostic`, retries and malformed frames.
    Ignored,
}

#[derive(Deserialize)]
struct ErrorPayload {
    message: String,
}

/// Decodes one SSE frame from its event name and `data` line.
#[must_use]
pub fn decode_frame(event: &str, data: &str) -> RunFrame {
    let decoded = match event {
        "start" => serde_json::from_str(data).map(RunFrame::Start),
        "agent" => serde_json::from_str(data).map(RunFrame::Agent),
        "error" => serde_json::from_str::<ErrorPayload>(data).map(|e| RunFrame::Error(e.message)),
        "end" => serde_json::from_str(data).map(RunFrame::End),
        _ => return RunFrame::Ignored,
    };
    decoded.unwrap_or(RunFrame::Ignored)
}

/// `POST /api/runs` body (`ChatRequest`, the fields the UI sends).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub agent_id: String,
    pub message: String,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_system_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// `POST /api/runs` → 202.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunResponse {
    pub run_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// One coding agent from `GET /api/agents`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentsResponse {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConversationRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConversationsResponse {
    pub conversations: Vec<ConversationRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

/// A stored message (`GET .../conversations/:cid/messages`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: String,
    pub role: MessageRole,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_status: Option<RunStatus>,
    #[serde(default)]
    pub last_run_event_id: Option<u64>,
    /// Persisted `agent` payloads of the run, in order.
    #[serde(default)]
    pub events: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MessagesResponse {
    pub messages: Vec<StoredMessage>,
}

/// `file-changed` payload of `GET /api/projects/:id/events`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileChanged {
    pub path: String,
    #[serde(default)]
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_known_frames() {
        assert_eq!(
            decode_frame("agent", r#"{"type":"text_delta","delta":"Hi"}"#),
            RunFrame::Agent(AgentEvent::TextDelta { delta: "Hi".into() })
        );
        assert_eq!(
            decode_frame(
                "start",
                r#"{"runId":"r1","agentId":"claude","bin":"claude","protocolVersion":1}"#
            ),
            RunFrame::Start(RunStart {
                run_id: "r1".into(),
                agent_id: "claude".into(),
                model: None,
                resumed: false,
            })
        );
        assert_eq!(
            decode_frame("end", r#"{"status":"canceled","code":null,"terminalAt":1}"#),
            RunFrame::End(RunEnd {
                status: RunStatus::Canceled,
                error: None,
                artifact_paths: vec![],
            })
        );
        assert_eq!(
            decode_frame("error", r#"{"message":"boom","code":"x"}"#),
            RunFrame::Error("boom".into())
        );
    }

    #[test]
    fn tolerates_unknown_and_malformed() {
        assert_eq!(
            decode_frame("agent", r#"{"type":"thinking_tokens","tokens":3}"#),
            RunFrame::Agent(AgentEvent::Unknown)
        );
        assert_eq!(decode_frame("agent", "not json"), RunFrame::Ignored);
        assert_eq!(
            decode_frame("stdout", r#"{"chunk":"x"}"#),
            RunFrame::Ignored
        );
    }

    #[test]
    fn tool_result_content_may_be_blocks() {
        let ev: AgentEvent = serde_json::from_value(json!({
            "type": "tool_result", "toolUseId": "t1",
            "content": [{"type": "text", "text": "ok"}], "isError": true
        }))
        .unwrap_or(AgentEvent::Unknown);
        assert!(matches!(ev, AgentEvent::ToolResult { is_error: true, .. }));
    }

    #[test]
    fn chat_request_wire_shape() {
        let req = ChatRequest {
            agent_id: "claude".into(),
            message: "hi".into(),
            project_id: "p1".into(),
            client_request_id: Some("c1".into()),
            ..ChatRequest::default()
        };
        assert_eq!(
            serde_json::to_value(&req).unwrap_or_default(),
            json!({"agentId": "claude", "message": "hi", "projectId": "p1", "clientRequestId": "c1"})
        );
    }
}
