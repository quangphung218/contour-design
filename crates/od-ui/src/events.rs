//! Run stream decoding (spec 2.2) over the `od-contracts` wire types.
//!
//! Decoding is lenient on purpose: unknown event types and malformed frames
//! become [`RunFrame::Ignored`] so a newer daemon never breaks an older UI.

use serde::Deserialize;

pub use od_contracts::{
    AgentEvent, AgentInfo, AgentsResponse, ChatRequest, ConversationsResponse, CreateRunResponse,
    Message, MessageRole, MessagesResponse, RunEndPayload, RunErrorPayload, RunStartPayload,
    RunStatus,
};

/// One decoded SSE frame of `GET /api/runs/:id/events`.
#[derive(Debug, Clone, PartialEq)]
pub enum RunFrame {
    Start(RunStartPayload),
    Agent(AgentEvent),
    Error(String),
    End(RunEndPayload),
    /// `stdout`, `stderr`, `diagnostic`, retries and malformed frames.
    Ignored,
}

/// Decodes one SSE frame from its event name and `data` line.
#[must_use]
pub fn decode_frame(event: &str, data: &str) -> RunFrame {
    let decoded = match event {
        "start" => serde_json::from_str(data).map(RunFrame::Start),
        "agent" => serde_json::from_str(data).map(RunFrame::Agent),
        "error" => {
            serde_json::from_str::<RunErrorPayload>(data).map(|e| RunFrame::Error(e.message))
        }
        "end" => serde_json::from_str(data).map(RunFrame::End),
        _ => return RunFrame::Ignored,
    };
    decoded.unwrap_or(RunFrame::Ignored)
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

    #[test]
    fn decodes_known_frames() {
        assert_eq!(
            decode_frame("agent", r#"{"type":"text_delta","delta":"Hi"}"#),
            RunFrame::Agent(AgentEvent::TextDelta { delta: "Hi".into() })
        );
        let RunFrame::Start(start) = decode_frame(
            "start",
            r#"{"runId":"r1","agentId":"claude","bin":"claude","protocolVersion":1}"#,
        ) else {
            panic!("start frame");
        };
        assert_eq!(start.run_id, "r1");
        assert_eq!(start.agent_id, "claude");
        let RunFrame::End(end) =
            decode_frame("end", r#"{"status":"canceled","code":null,"terminalAt":1}"#)
        else {
            panic!("end frame");
        };
        assert_eq!(end.status, RunStatus::Canceled);
        assert_eq!(end.error, None);
        assert_eq!(
            decode_frame("error", r#"{"message":"boom","code":"x"}"#),
            RunFrame::Error("boom".into())
        );
    }

    #[test]
    fn tolerates_unknown_and_malformed() {
        assert_eq!(
            decode_frame("agent", r#"{"type":"not_a_type","x":3}"#),
            RunFrame::Ignored
        );
        assert_eq!(decode_frame("agent", "not json"), RunFrame::Ignored);
        assert_eq!(
            decode_frame("stdout", r#"{"chunk":"x"}"#),
            RunFrame::Ignored
        );
    }
}
