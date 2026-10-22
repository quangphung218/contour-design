//! `opencode run --format json` (spec 2.3; ports `handleOpenCodeEvent` of
//! `json-event-stream.ts`).
//!
//! OpenCode names its file tools in lower case with a camelCase
//! `filePath`; they are mapped to Claude-shaped `Write`/`Edit`/`Read` with
//! `file_path` so the chat and `artifactPaths` treat them like every other
//! agent's writes.

use super::{StreamParser, map_usage, raw_event, str_field, stringify_content};
use od_contracts::AgentEvent;
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct OpenCodeParser {
    tool_uses: HashSet<String>,
    tool_results: HashSet<String>,
}

impl StreamParser for OpenCodeParser {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let handled = match serde_json::from_str::<Value>(line) {
            Ok(obj) if obj.is_object() => self.handle(&obj, out),
            _ => false,
        };
        if !handled {
            out.push(raw_event(line));
        }
    }
}

fn tool_name(tool: &str) -> String {
    match tool {
        "write" => "Write".to_owned(),
        "edit" | "patch" | "multiedit" => "Edit".to_owned(),
        "read" => "Read".to_owned(),
        "bash" => "Bash".to_owned(),
        "todowrite" => "TodoWrite".to_owned(),
        other => other.to_owned(),
    }
}

fn tool_input(state: Option<&Value>) -> Value {
    let raw = state
        .and_then(|s| s.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut input = match raw {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        other => other,
    };
    if let Some(obj) = input.as_object_mut() {
        if !obj.contains_key("file_path") {
            if let Some(path) = obj.get("filePath").cloned() {
                obj.insert("file_path".to_owned(), path);
            }
        }
    }
    input
}

/// `openCodeToolResult`: only for a terminal state.
fn tool_result(state: &Value) -> Option<(String, bool)> {
    let status = str_field(state, "status")
        .unwrap_or_default()
        .to_lowercase();
    if !matches!(status.as_str(), "completed" | "error" | "failed") {
        return None;
    }
    let nonzero_exit = [
        state.get("exit"),
        state.get("exitCode"),
        state.pointer("/metadata/exit"),
    ]
    .into_iter()
    .flatten()
    .any(|c| c.as_f64().is_some_and(|c| c != 0.0));
    let error = state.get("error").filter(|e| match e {
        Value::String(s) => !s.trim().is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => false,
    });
    let is_error = status != "completed" || error.is_some() || nonzero_exit;
    let content = stringify_content(
        error
            .or_else(|| state.get("output"))
            .unwrap_or(&Value::Null),
    );
    Some((content, is_error))
}

impl OpenCodeParser {
    fn handle(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) -> bool {
        let part = obj.get("part").unwrap_or(&Value::Null);
        match str_field(obj, "type").unwrap_or_default() {
            "step_start" => {
                out.push(AgentEvent::Status {
                    label: "running".to_owned(),
                    model: None,
                    session_id: str_field(obj, "sessionID")
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    ttft_ms: None,
                    detail: None,
                });
                true
            }
            "text" => match str_field(part, "text").filter(|t| !t.is_empty()) {
                Some(text) => {
                    out.push(AgentEvent::TextDelta {
                        delta: text.to_owned(),
                    });
                    true
                }
                None => false,
            },
            "tool_use" => {
                let (Some(tool), Some(call)) = (str_field(part, "tool"), str_field(part, "callID"))
                else {
                    return false;
                };
                let state = part.get("state").filter(|s| s.is_object());
                let key = format!(
                    "{}:{call}",
                    str_field(obj, "sessionID").unwrap_or("session")
                );
                if self.tool_uses.insert(key.clone()) {
                    out.push(AgentEvent::ToolUse {
                        id: call.to_owned(),
                        name: tool_name(tool),
                        input: tool_input(state),
                    });
                }
                if let Some((content, is_error)) = state.and_then(tool_result) {
                    if self.tool_results.insert(key) {
                        out.push(AgentEvent::ToolResult {
                            tool_use_id: call.to_owned(),
                            content,
                            is_error,
                        });
                    }
                }
                true
            }
            "step_finish" => {
                let usage = part.get("tokens").and_then(|t| {
                    let mut usage = map_usage(
                        t,
                        &[
                            ("input", "input_tokens"),
                            ("output", "output_tokens"),
                            ("reasoning", "thought_tokens"),
                        ],
                    );
                    if let (Some(Value::Object(u)), Some(cache)) = (&mut usage, t.get("cache")) {
                        for (from, to) in [
                            ("read", "cached_read_tokens"),
                            ("write", "cached_write_tokens"),
                        ] {
                            if let Some(n) = cache.get(from).filter(|v| v.is_number()) {
                                u.insert(to.to_owned(), n.clone());
                            }
                        }
                    }
                    usage
                });
                if usage.is_some() {
                    out.push(AgentEvent::Usage {
                        usage,
                        cost_usd: part.get("cost").and_then(Value::as_f64),
                        duration_ms: None,
                        stop_reason: str_field(part, "reason").map(str::to_owned),
                        is_error: false,
                    });
                }
                true
            }
            "error" => {
                // OpenCode prints provider errors on stdout and exits 0.
                let err = obj.get("error").or_else(|| obj.get("message"));
                let message = match err {
                    Some(Value::String(s)) if !s.is_empty() => s.clone(),
                    Some(v) => str_field(v, "message")
                        .or_else(|| v.pointer("/data/message").and_then(Value::as_str))
                        .map_or_else(|| json!(v).to_string(), str::to_owned),
                    None => "OpenCode error".to_owned(),
                };
                out.push(AgentEvent::Error {
                    message,
                    code: None,
                    terminal: false,
                });
                true
            }
            _ => false,
        }
    }
}
