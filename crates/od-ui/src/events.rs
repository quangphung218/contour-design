//! Run stream decoding (spec 2.2) over the `od-contracts` wire types.
//!
//! Decoding is lenient on purpose: unknown event types and malformed frames
//! become [`RunFrame::Ignored`] so a newer daemon never breaks an older UI.

use serde::Deserialize;

pub use od_contracts::{
    AgentEvent, AgentInfo, AgentsResponse, ChatRequest, ConversationsResponse, CreateRunResponse,
    FailureCategory, Message, MessageRole, MessagesResponse, RunEndPayload, RunErrorPayload,
    RunStartPayload, RunStatus,
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
        "error" => serde_json::from_str::<RunErrorPayload>(data).map(|e| {
            RunFrame::Error(
                if e.code.as_deref() == Some(od_contracts::DAEMON_RESTART_CODE) {
                    app_closed()
                } else {
                    e.message
                },
            )
        }),
        "end" => serde_json::from_str(data).map(RunFrame::End),
        _ => return RunFrame::Ignored,
    };
    decoded.unwrap_or(RunFrame::Ignored)
}

/// A run cut short because Contour closed or crashed (the daemon's
/// `daemon_restart`), told in the user's words: the agent was fine.
pub(crate) fn app_closed() -> String {
    crate::i18n::t(crate::i18n::T::RunAppClosed).to_owned()
}

/// The error a run ended with, as the chat shows it.
#[must_use]
pub fn end_error(end: &RunEndPayload) -> Option<String> {
    if end.failure_category == Some(FailureCategory::DaemonRestart) {
        return Some(app_closed());
    }
    end.error.clone().filter(|e| !e.is_empty())
}

/// What a finished run's whole SSE log (`GET /api/runs/:id/events`) says
/// about how it went: the agent's `stderr` in order, and the error it
/// ended with. A stored turn keeps neither, only the agent's events.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunRecord {
    pub stderr: String,
    pub error: Option<String>,
    /// Why it failed, when the daemon said.
    pub failure: Option<FailureCategory>,
}

#[must_use]
pub fn run_record(sse: &str) -> RunRecord {
    let mut record = RunRecord::default();
    for frame in sse.replace("\r\n", "\n").split("\n\n") {
        let mut event = "";
        let mut data = Vec::new();
        for line in frame.lines() {
            if let Some(name) = line.strip_prefix("event:") {
                event = name.trim();
            } else if let Some(d) = line.strip_prefix("data:") {
                data.push(d.strip_prefix(' ').unwrap_or(d));
            }
        }
        let data = data.join("\n");
        if event == "stderr" {
            if let Ok(p) = serde_json::from_str::<od_contracts::RunChunkPayload>(&data) {
                record.stderr.push_str(&p.chunk);
            }
            continue;
        }
        match decode_frame(event, &data) {
            RunFrame::Error(message) => record.error = Some(message),
            RunFrame::End(end) => {
                if let Some(error) = end_error(&end) {
                    record.error = Some(error);
                }
                record.failure = end.failure_category;
            }
            _ => {}
        }
    }
    record
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
    fn a_run_log_cut_short_by_contour_closing_keeps_the_cause() {
        let sse = "id: 1\nevent: start\ndata: {\"runId\":\"r\"}\n\n\
                   id: 2\nevent: error\ndata: {\"message\":\"the daemon restarted while this run was in progress\",\"code\":\"DAEMON_RESTART\"}\n\n\
                   id: 3\nevent: end\ndata: {\"status\":\"failed\",\"code\":null,\"terminalAt\":1,\"failureCategory\":\"daemon_restart\",\"error\":\"daemon restarted\"}\n\n";
        let record = run_record(sse);
        assert_eq!(
            record.failure,
            Some(od_contracts::FailureCategory::DaemonRestart)
        );
        let error = record.error.unwrap();
        assert!(error.contains("Contour closed"), "{error}");
        assert!(!error.to_lowercase().contains("daemon"), "{error}");
    }

    #[test]
    fn a_run_log_gives_back_the_agents_stderr_and_its_error() {
        let sse = "id: 1\nevent: start\ndata: {\"runId\":\"r\"}\n\n\
                   id: 2\nevent: stderr\ndata: {\"chunk\":\"Error: ENOSPC\\n\"}\n\n\
                   id: 3\nevent: stdout\ndata: {\"chunk\":\"not this\\n\"}\n\n\
                   id: 4\nevent: stderr\ndata: {\"chunk\":\"    at w (node:fs:1:2)\\n\"}\n\n\
                   id: 5\nevent: end\ndata: {\"status\":\"failed\",\"code\":1,\"terminalAt\":1,\"error\":\"Error: ENOSPC\"}\n\n";
        assert_eq!(
            run_record(sse),
            RunRecord {
                stderr: "Error: ENOSPC\n    at w (node:fs:1:2)\n".into(),
                error: Some("Error: ENOSPC".into()),
                failure: None,
            }
        );
        let ok = "event: end\ndata: {\"status\":\"succeeded\",\"code\":0,\"terminalAt\":1}\n\n";
        assert_eq!(run_record(ok), RunRecord::default());
    }

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
