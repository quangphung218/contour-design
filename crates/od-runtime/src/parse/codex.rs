//! `codex exec --json` (spec 2.3; ports the codex branch of
//! `json-event-stream.ts`).

use super::{StreamParser, normalize_task_status, raw_event, str_field, stringify_content};
use od_contracts::AgentEvent;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

/// Parser state for one process's stdout.
#[derive(Debug, Default)]
pub struct CodexParser {
    /// Tool ids already announced; `item.started` and `item.completed`
    /// both carry the call, and it must appear once.
    tool_uses: HashSet<String>,
    error_emitted: bool,
    reasoning_emitted: HashMap<String, usize>,
    reasoning_any: bool,
    previous_was_agent_message: bool,
    last_agent_message_ended_with_newline: bool,
}

impl StreamParser for CodexParser {
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

fn status(label: &str, session_id: Option<String>, detail: Option<String>) -> AgentEvent {
    AgentEvent::Status {
        label: label.to_owned(),
        model: None,
        session_id,
        ttft_ms: None,
        detail,
    }
}

fn error_message(value: Option<&Value>, fallback: &str) -> String {
    match value {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(v @ Value::Object(_)) => str_field(v, "message")
            .filter(|m| !m.is_empty())
            .map_or_else(|| fallback.to_owned(), str::to_owned),
        _ => fallback.to_owned(),
    }
}

impl CodexParser {
    fn not_agent_message(&mut self) {
        self.previous_was_agent_message = false;
        self.last_agent_message_ended_with_newline = false;
    }

    fn tool_use_once(&mut self, id: &str, name: &str, input: Value, out: &mut Vec<AgentEvent>) {
        if self.tool_uses.insert(id.to_owned()) {
            out.push(AgentEvent::ToolUse {
                id: id.to_owned(),
                name: name.to_owned(),
                input,
            });
        }
    }

    fn handle(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) -> bool {
        let kind = str_field(obj, "type").unwrap_or_default();
        match kind {
            "error" | "turn.failed" => {
                let fallback = if kind == "error" {
                    "Codex error"
                } else {
                    "Codex turn failed"
                };
                let message = if kind == "error" {
                    error_message(obj.get("message").or_else(|| obj.get("error")), fallback)
                } else {
                    error_message(obj.get("error").or_else(|| obj.get("message")), fallback)
                };
                if let Some(progress) = reconnect_progress(&message) {
                    out.push(status("agent_reconnecting", None, Some(progress)));
                } else if !self.error_emitted {
                    self.error_emitted = true;
                    out.push(AgentEvent::Error {
                        message,
                        code: None,
                        terminal: false,
                    });
                }
                return true;
            }
            "thread.started" => {
                let id = str_field(obj, "thread_id")
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                out.push(status("initializing", id, None));
                return true;
            }
            "turn.started" => {
                self.not_agent_message();
                out.push(status("thinking", None, None));
                return true;
            }
            "turn.completed" => {
                let Some(usage) = obj.get("usage").and_then(Value::as_object) else {
                    return false;
                };
                let mut mapped = Map::new();
                for (from, to) in [
                    ("input_tokens", "input_tokens"),
                    ("output_tokens", "output_tokens"),
                    ("reasoning_output_tokens", "thought_tokens"),
                    ("cached_input_tokens", "cached_read_tokens"),
                ] {
                    if let Some(n) = usage.get(from).filter(|v| v.is_number()) {
                        mapped.insert(to.to_owned(), n.clone());
                    }
                }
                out.push(AgentEvent::Usage {
                    usage: Some(Value::Object(mapped)),
                    cost_usd: None,
                    duration_ms: None,
                    stop_reason: None,
                    is_error: false,
                });
                return true;
            }
            "item.started" | "item.updated" | "item.completed" => {}
            _ => return false,
        }
        let Some(item) = obj.get("item").filter(|i| i.is_object()) else {
            return false;
        };
        let item_type = str_field(item, "type").unwrap_or_default();
        let item_id = str_field(item, "id").unwrap_or_default();
        let completed = kind == "item.completed";

        if item_type == "reasoning" {
            let text = str_field(item, "text").unwrap_or_default();
            let emitted = self.reasoning_emitted.get(item_id).copied().unwrap_or(0);
            if text.len() > emitted && text.is_char_boundary(emitted) {
                let suffix = &text[emitted..];
                let delta = if emitted == 0 && self.reasoning_any {
                    format!("\n\n{suffix}")
                } else {
                    suffix.to_owned()
                };
                out.push(AgentEvent::ThinkingDelta { delta });
                self.reasoning_emitted
                    .insert(item_id.to_owned(), text.len());
                self.reasoning_any = true;
            }
            return true;
        }
        if item_type == "todo_list" && !item_id.is_empty() {
            if let Some(items) = item.get("items").and_then(Value::as_array) {
                self.not_agent_message();
                out.push(AgentEvent::ToolUse {
                    id: item_id.to_owned(),
                    name: "TodoWrite".to_owned(),
                    input: todo_input(items),
                });
                return true;
            }
        }
        if kind == "item.updated" {
            return false;
        }
        match item_type {
            "error" if completed => {
                let Some(message) = str_field(item, "message").filter(|m| !m.is_empty()) else {
                    return false;
                };
                out.push(status("warning", None, Some(message.to_owned())));
                true
            }
            "command_execution" if !item_id.is_empty() => {
                self.not_agent_message();
                let command = str_field(item, "command").unwrap_or_default();
                self.tool_use_once(item_id, "Bash", json!({ "command": command }), out);
                if completed {
                    let content =
                        stringify_content(item.get("aggregated_output").unwrap_or(&Value::Null));
                    let is_error = match item.get("exit_code").and_then(Value::as_i64) {
                        Some(code) => code != 0,
                        None => str_field(item, "status") == Some("failed"),
                    };
                    out.push(AgentEvent::ToolResult {
                        tool_use_id: item_id.to_owned(),
                        content,
                        is_error,
                    });
                }
                true
            }
            "file_change" if !item_id.is_empty() => {
                let Some(changes) = file_changes(item, item_id) else {
                    return false;
                };
                self.not_agent_message();
                for (id, name, path) in &changes {
                    self.tool_use_once(id, name, json!({ "file_path": path }), out);
                }
                if completed {
                    let is_error = str_field(item, "status") == Some("failed");
                    for (id, _, _) in changes {
                        out.push(AgentEvent::ToolResult {
                            tool_use_id: id,
                            content: String::new(),
                            is_error,
                        });
                    }
                }
                true
            }
            "mcp_tool_call" if !item_id.is_empty() => {
                let (Some(server), Some(tool)) = (
                    str_field(item, "server").filter(|s| !s.is_empty()),
                    str_field(item, "tool").filter(|s| !s.is_empty()),
                ) else {
                    return false;
                };
                self.not_agent_message();
                let name = format!("mcp__{server}__{tool}");
                let input = item.get("arguments").cloned().unwrap_or_else(|| json!({}));
                self.tool_use_once(item_id, &name, input, out);
                if completed {
                    let failed = str_field(item, "status") == Some("failed")
                        || item.get("error").is_some_and(|e| !e.is_null());
                    let content = if failed {
                        error_message(item.get("error"), "MCP tool call failed")
                    } else {
                        item.get("result")
                            .map(stringify_content)
                            .unwrap_or_default()
                    };
                    out.push(AgentEvent::ToolResult {
                        tool_use_id: item_id.to_owned(),
                        content,
                        is_error: failed,
                    });
                }
                true
            }
            "agent_message" if completed => {
                let Some(text) = str_field(item, "text").filter(|t| !t.is_empty()) else {
                    return false;
                };
                let needs_boundary = self.previous_was_agent_message
                    && !self.last_agent_message_ended_with_newline
                    && !text.starts_with('\n');
                let delta = if needs_boundary {
                    format!("\n{text}")
                } else {
                    text.to_owned()
                };
                out.push(AgentEvent::TextDelta { delta });
                self.previous_was_agent_message = true;
                self.last_agent_message_ended_with_newline = text.ends_with('\n');
                true
            }
            "agent_message" => true,
            _ => false,
        }
    }
}

/// One `(tool id, Write|Edit, path)` per change, or `None` when any change
/// is not fully recognized (the line then stays visible as `raw`).
fn file_changes(item: &Value, item_id: &str) -> Option<Vec<(String, &'static str, String)>> {
    let changes = item.get("changes")?.as_array()?;
    if changes.is_empty() {
        return None;
    }
    changes
        .iter()
        .enumerate()
        .map(|(index, change)| {
            let path = str_field(change, "path").filter(|p| !p.is_empty())?;
            let name = match str_field(change, "kind")? {
                "add" => "Write",
                "update" => "Edit",
                _ => return None,
            };
            Some((format!("{item_id}#{index}"), name, path.to_owned()))
        })
        .collect()
}

fn todo_input(items: &[Value]) -> Value {
    let todos: Vec<Value> = items
        .iter()
        .filter_map(|it| {
            let content = str_field(it, "text").or_else(|| str_field(it, "content"))?;
            let status = match it.get("completed").and_then(Value::as_bool) {
                Some(true) => "completed",
                Some(false) => "pending",
                None => normalize_task_status(str_field(it, "status")),
            };
            Some(json!({ "content": content, "status": status }))
        })
        .collect();
    json!({ "todos": todos })
}

/// `Reconnecting... 2/5` → `"2/5"`: Codex's own upstream retry loop.
fn reconnect_progress(message: &str) -> Option<String> {
    let rest = message.trim().strip_prefix("Reconnecting")?;
    let tail = rest.trim_start_matches(['.', '…', ' ']);
    let token = tail.split_whitespace().next()?;
    let (a, b) = token.split_once('/')?;
    let a: u32 = a.parse().ok()?;
    let b: u32 = b
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    Some(format!("{a}/{b}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_all;

    #[test]
    fn reconnect_is_status_not_error() {
        let line = json!({"type":"error","message":"Reconnecting... 2/5 (stream disconnected)"});
        let events = parse_all(&mut CodexParser::default(), &line.to_string());
        assert!(matches!(
            &events[0],
            AgentEvent::Status { label, detail: Some(d), .. } if label == "agent_reconnecting" && d == "2/5"
        ));
    }

    #[test]
    fn failed_command_is_error_result() {
        let text = [
            json!({"type":"item.completed","item":{"id":"c1","type":"command_execution","command":"false","aggregated_output":"boom","exit_code":1,"status":"failed"}}),
        ]
        .map(|v| v.to_string())
        .join("\n");
        let events = parse_all(&mut CodexParser::default(), &text);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[1],
            AgentEvent::ToolResult { is_error: true, .. }
        ));
    }

    #[test]
    fn unknown_frames_stay_raw() {
        let events = parse_all(&mut CodexParser::default(), r#"{"type":"mystery"}"#);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }
}
