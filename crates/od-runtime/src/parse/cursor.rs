//! `cursor-agent --print --output-format stream-json --stream-partial-output`
//! (spec 2.3; ports `handleCursorEvent` of `json-event-stream.ts`).
//!
//! Timestamped assistant frames are incremental deltas, emitted verbatim.
//! A frame with `model_call_id` (or without a timestamp) replays the whole
//! turn: only the unseen suffix is emitted, and only when what was shown
//! is a prefix of it. `tool_call` frames (`started`/`completed`, one
//! `<kind>ToolCall` object each) become Claude-shaped tool events.

use super::{StreamParser, map_usage, raw_event, str_field};
use od_contracts::AgentEvent;
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct CursorParser {
    text_so_far: String,
    turn_start: usize,
    tool_uses: HashSet<String>,
    thinking_started: bool,
}

impl StreamParser for CursorParser {
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

fn message_text(message: &Value) -> String {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|b| str_field(b, "type") == Some("text"))
        .filter_map(|b| str_field(b, "text"))
        .collect()
}

/// `editToolCall` → `Edit`, `shellToolCall` → `Bash`, unknown kinds keep
/// their own (title-cased) name.
fn tool_name(key: &str) -> String {
    let base = key.strip_suffix("ToolCall").unwrap_or(key);
    match base {
        "edit" | "strReplace" | "multiEdit" => "Edit".to_owned(),
        "write" => "Write".to_owned(),
        "read" => "Read".to_owned(),
        "shell" | "bash" => "Bash".to_owned(),
        "grep" | "glob" | "ls" => "Grep".to_owned(),
        "updateTodos" | "todo" => "TodoWrite".to_owned(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_else(|| "Tool".to_owned())
        }
    }
}

impl CursorParser {
    /// Reconciles a whole-turn replay with the current turn's text.
    fn replay(&mut self, text: &str, out: &mut Vec<AgentEvent>) {
        let shown = &self.text_so_far[self.turn_start..];
        if !text.is_empty() && text != shown {
            if let Some(suffix) = text.strip_prefix(shown) {
                let suffix = suffix.to_owned();
                self.text_so_far.push_str(&suffix);
                out.push(AgentEvent::TextDelta { delta: suffix });
            }
        }
        self.turn_start = self.text_so_far.len();
    }

    fn handle(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) -> bool {
        match str_field(obj, "type").unwrap_or_default() {
            "system" if str_field(obj, "subtype") == Some("init") => {
                out.push(AgentEvent::Status {
                    label: "initializing".to_owned(),
                    model: str_field(obj, "model").map(str::to_owned),
                    session_id: None,
                    ttft_ms: None,
                    detail: None,
                });
                true
            }
            // The prompt echoed back.
            "user" => true,
            "thinking" => {
                if let Some(text) = str_field(obj, "text").filter(|t| !t.is_empty()) {
                    if !self.thinking_started {
                        self.thinking_started = true;
                        out.push(AgentEvent::ThinkingStart);
                    }
                    out.push(AgentEvent::ThinkingDelta {
                        delta: text.to_owned(),
                    });
                }
                true
            }
            "assistant" => {
                let Some(message) = obj.get("message") else {
                    return false;
                };
                let text = message_text(message);
                if obj.get("model_call_id").is_some_and(Value::is_string) {
                    self.replay(&text, out);
                    return true;
                }
                if text.is_empty() {
                    return false;
                }
                if obj.get("timestamp_ms").is_some_and(Value::is_number) {
                    self.text_so_far.push_str(&text);
                    out.push(AgentEvent::TextDelta { delta: text });
                } else {
                    self.replay(&text, out);
                }
                true
            }
            "tool_call" => self.tool_call(obj, out),
            "result" => {
                let usage = obj.get("usage").and_then(|u| {
                    map_usage(
                        u,
                        &[
                            ("inputTokens", "input_tokens"),
                            ("outputTokens", "output_tokens"),
                            ("cacheReadTokens", "cached_read_tokens"),
                            ("cacheWriteTokens", "cached_write_tokens"),
                        ],
                    )
                });
                let is_error = obj.get("is_error") == Some(&Value::Bool(true));
                if is_error {
                    let message = str_field(obj, "result")
                        .filter(|r| !r.is_empty())
                        .unwrap_or("Cursor Agent failed");
                    out.push(AgentEvent::Error {
                        message: message.to_owned(),
                        code: None,
                        terminal: false,
                    });
                }
                out.push(AgentEvent::Usage {
                    usage,
                    cost_usd: None,
                    duration_ms: obj.get("duration_ms").and_then(Value::as_f64),
                    stop_reason: None,
                    is_error,
                });
                true
            }
            _ => false,
        }
    }

    fn tool_call(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) -> bool {
        let Some(call) = obj.get("tool_call").and_then(Value::as_object) else {
            return false;
        };
        let Some((key, body)) = call
            .iter()
            .find(|(k, v)| k.ends_with("ToolCall") && v.is_object())
        else {
            return false;
        };
        let Some(id) =
            str_field(obj, "call_id").or_else(|| call.get("toolCallId").and_then(Value::as_str))
        else {
            return false;
        };
        if self.tool_uses.insert(id.to_owned()) {
            let mut input: Map<String, Value> = body
                .get("args")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(path) = input.get("path").cloned() {
                input.entry("file_path").or_insert(path);
            }
            out.push(AgentEvent::ToolUse {
                id: id.to_owned(),
                name: tool_name(key),
                input: Value::Object(input),
            });
        }
        if str_field(obj, "subtype") == Some("completed") {
            let result = body.get("result").unwrap_or(&Value::Null);
            let (content, is_error) = if let Some(ok) = result.get("success") {
                let text = str_field(ok, "message")
                    .or_else(|| str_field(ok, "output"))
                    .or_else(|| str_field(ok, "stdout"))
                    .or_else(|| str_field(ok, "diffString"))
                    .or_else(|| str_field(ok, "content"))
                    .map_or_else(|| ok.to_string(), str::to_owned);
                (text, false)
            } else if let Some(err) = result.get("error").or_else(|| result.get("failure")) {
                let text = str_field(err, "message")
                    .or_else(|| str_field(err, "error"))
                    .map_or_else(|| err.to_string(), str::to_owned);
                (text, true)
            } else {
                (String::new(), false)
            };
            out.push(AgentEvent::ToolResult {
                tool_use_id: id.to_owned(),
                content,
                is_error,
            });
        }
        true
    }
}
