//! Stream parsers: one stdout line in, zero or more [`AgentEvent`]s out.

mod claude;
mod codex;

pub use claude::ClaudeParser;
pub use codex::CodexParser;

use crate::def::StreamFormat;
use od_contracts::AgentEvent;
use serde_json::Value;

/// `raw` events carry at most this many bytes of the line.
pub const RAW_LINE_LIMIT: usize = 4096;

/// Turns one agent's stdout lines into normalized events.
pub trait StreamParser: Send {
    /// Handles one complete line (without the newline).
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>);

    /// Called once when stdout closes.
    fn finish(&mut self, _out: &mut Vec<AgentEvent>) {}
}

#[must_use]
pub fn parser_for(format: StreamFormat) -> Box<dyn StreamParser> {
    match format {
        StreamFormat::ClaudeStreamJson => Box::new(ClaudeParser::default()),
        StreamFormat::CodexExecJson => Box::new(CodexParser::default()),
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
