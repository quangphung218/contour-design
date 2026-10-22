//! Stream parsers: one stdout line in, zero or more [`AgentEvent`]s out.
//!
//! Protocol runtimes (ACP, dsh) are parsers too: they also queue the lines
//! the engine writes back to the agent's stdin, say how to ask the agent to
//! stop, and say when the prompt is complete.

mod acp;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod dsh;
mod opencode;
mod plain;

pub use acp::AcpSession;
pub use claude::ClaudeParser;
pub use codex::CodexParser;
pub use copilot::CopilotParser;
pub use cursor::CursorParser;
pub use dsh::DshSession;
pub use opencode::OpenCodeParser;
pub use plain::PlainParser;

use crate::def::{PermissionMode, StreamFormat};
use od_contracts::AgentEvent;
use serde_json::Value;
use std::path::PathBuf;

/// `raw` events carry at most this many bytes of the line.
pub const RAW_LINE_LIMIT: usize = 4096;

/// Turns one agent's stdout lines into normalized events.
pub trait StreamParser: Send {
    /// Handles one complete line (without the newline).
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>);

    /// Called once when stdout closes.
    fn finish(&mut self, _out: &mut Vec<AgentEvent>) {}

    /// Lines to write to the agent's stdin, drained by the engine after
    /// every [`Self::line`] call and once right after spawn.
    fn take_writes(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// The line that asks the agent to stop (ACP `session/cancel`, dsh
    /// `cancel`), sent before any signal. `None` when the protocol has no
    /// such message or the session is not established yet.
    fn abort_line(&mut self) -> Option<String> {
        None
    }

    /// The protocol says the prompt is over (answered, failed or
    /// canceled). The engine then closes stdin and stops the process,
    /// since ACP agents wait for the next prompt instead of exiting.
    fn completed(&self) -> bool {
        false
    }
}

/// What a protocol session needs to drive one prompt.
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub prompt: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    /// Continue this stored session (`session/load`, dsh `resume_session_id`).
    pub resume_session_id: Option<String>,
    pub permission: PermissionMode,
    /// Correlates dsh commands and frames.
    pub request_id: String,
    /// See [`crate::RuntimeDef::acp_turn_end_completes`].
    pub acp_turn_end_completes: bool,
}

#[must_use]
pub fn parser_for(format: StreamFormat, ctx: &SessionContext) -> Box<dyn StreamParser> {
    match format {
        StreamFormat::ClaudeStreamJson => Box::new(ClaudeParser::default()),
        StreamFormat::CodexExecJson => Box::new(CodexParser::default()),
        StreamFormat::Acp => Box::new(AcpSession::new(ctx.clone())),
        StreamFormat::OpenCodeJson => Box::new(OpenCodeParser::default()),
        StreamFormat::CursorStreamJson => Box::new(CursorParser::default()),
        StreamFormat::CopilotJson => Box::new(CopilotParser),
        StreamFormat::DshJsonl => Box::new(DshSession::new(ctx.clone())),
        StreamFormat::Plain | StreamFormat::Byok => Box::new(PlainParser),
    }
}

/// Parses a whole recording, for tests and tools.
#[must_use]
pub fn parse_all(parser: &mut dyn StreamParser, text: &str) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    for line in text.lines() {
        parser.line(line, &mut out);
    }
    parser.finish(&mut out);
    out
}

/// A `raw` event for an unparseable line, cut at a char boundary.
#[must_use]
pub fn raw_event(line: &str) -> AgentEvent {
    if line.len() <= RAW_LINE_LIMIT {
        return AgentEvent::Raw {
            line: line.to_owned(),
            truncated: false,
        };
    }
    let mut end = RAW_LINE_LIMIT;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    AgentEvent::Raw {
        line: line[..end].to_owned(),
        truncated: true,
    }
}

/// A `status` event with only a label (and optionally more).
pub(crate) fn status(label: &str) -> AgentEvent {
    AgentEvent::Status {
        label: label.to_owned(),
        model: None,
        session_id: None,
        ttft_ms: None,
        detail: None,
    }
}

/// Copies the numeric fields `from → to` of `src` into a usage object;
/// `None` when none is present.
pub(crate) fn map_usage(src: &Value, keys: &[(&str, &str)]) -> Option<Value> {
    let mut out = serde_json::Map::new();
    for (from, to) in keys {
        if out.contains_key(*to) {
            continue;
        }
        if let Some(n) = src.get(*from).filter(|v| v.is_number()) {
            out.insert((*to).to_owned(), n.clone());
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

pub(crate) fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Tool-result content as text: strings verbatim, text blocks joined.
pub(crate) fn stringify_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(
                |c| match (c.get("type").and_then(Value::as_str), c.get("text")) {
                    (Some("text"), Some(Value::String(t))) => t.clone(),
                    (Some("text"), Some(other)) => other.to_string(),
                    _ => c.to_string(),
                },
            )
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A `TodoWrite` row status from the many spellings CLIs use.
pub(crate) fn normalize_task_status(value: Option<&str>) -> &'static str {
    match value {
        Some("completed" | "complete" | "done") => "completed",
        Some("in_progress" | "doing" | "active") => "in_progress",
        Some("stopped" | "failed" | "canceled" | "cancelled") => "stopped",
        _ => "pending",
    }
}
