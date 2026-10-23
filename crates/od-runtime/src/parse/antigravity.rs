//! Antigravity `agy --output-format stream-json` (ports
//! `handleAntigravityEvent` of `json-event-stream.ts`).
//!
//! Every frame has an `event`: `init` once, `step_update` per step
//! (`user_input`, `agent_response`, `tool`, `checkpoint`, `subagent`,
//! `error_message`), then one `result`. Frames without `event` are the
//! Gemini CLI JSONL some agy-compatible builds print.

use super::{StreamParser, map_usage, raw_event, str_field, stringify_content};
use od_contracts::AgentEvent;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct AntigravityParser {
    tool_uses: HashSet<String>,
    text_emitted: bool,
}

impl StreamParser for AntigravityParser {
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

fn id_part(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

impl AntigravityParser {
    fn handle(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) -> bool {
        let Some(event) = str_field(obj, "event") else {
            return gemini(obj, out);
        };
        match event {
            "init" => {
                out.push(AgentEvent::Status {
                    label: "initializing".to_owned(),
                    model: None,
                    session_id: str_field(obj, "conversation_id").map(str::to_owned),
                    ttft_ms: None,
                    detail: None,
                });
                true
            }
            "step_update" => {
                let Some(step) = obj.get("step_update").filter(|s| s.is_object()) else {
                    return false;
                };
                match str_field(step, "step_type").unwrap_or_default() {
                    "agent_response" => {
                        if let Some(delta) = str_field(step, "text_delta").filter(|d| !d.is_empty())
                        {
                            self.text_emitted = true;
                            out.push(AgentEvent::TextDelta {
                                delta: delta.to_owned(),
                            });
                        }
                    }
                    "tool" => self.tool(obj, step, out),
                    // user_input echoes the prompt; the rest carry nothing
                    // visible (an error's message is on the result).
                    _ => {}
                }
                true
            }
            "result" => {
                let Some(result) = obj.get("result").filter(|r| r.is_object()) else {
                    return false;
                };
                if str_field(result, "status") != Some("SUCCESS") {
                    let message = str_field(result, "error")
                        .filter(|e| !e.is_empty())
                        .map_or_else(
                            || {
                                format!(
                                    "Antigravity run ended with status {}",
                                    str_field(result, "status").unwrap_or("unknown")
                                )
                            },
                            str::to_owned,
                        );
                    out.push(AgentEvent::Error {
                        message,
                        code: None,
                        terminal: true,
                    });
                    return true;
                }
                if !self.text_emitted {
                    if let Some(text) = str_field(result, "response").filter(|r| !r.is_empty()) {
                        self.text_emitted = true;
                        out.push(AgentEvent::TextDelta {
                            delta: text.to_owned(),
                        });
                    }
                }
                let usage = result.get("usage").and_then(|u| {
                    map_usage(
                        u,
                        &[
                            ("input_tokens", "input_tokens"),
                            ("output_tokens", "output_tokens"),
                            ("thinking_tokens", "thought_tokens"),
                            ("cache_read_tokens", "cached_read_tokens"),
                        ],
                    )
                });
                out.push(AgentEvent::Usage {
                    usage: Some(usage.unwrap_or_else(|| Value::Object(serde_json::Map::new()))),
                    cost_usd: None,
                    duration_ms: result
                        .get("duration_seconds")
                        .and_then(Value::as_f64)
                        .map(|s| (s * 1000.0).round()),
                    stop_reason: None,
                    is_error: false,
                });
                true
            }
            _ => false,
        }
    }

    fn tool(&mut self, obj: &Value, step: &Value, out: &mut Vec<AgentEvent>) {
        let info = step.get("tool_info").filter(|i| i.is_object());
        let name = info
            .and_then(|i| str_field(i, "name"))
            .or_else(|| str_field(step, "tool_name"))
            .unwrap_or("tool")
            .to_owned();
        let conversation = step
            .get("conversation_id")
            .filter(|c| !c.is_null())
            .or_else(|| obj.get("conversation_id"));
        let id = format!(
            "{}:{}",
            id_part(conversation),
            id_part(step.get("step_index"))
        );
        if self.tool_uses.insert(id.clone()) {
            out.push(AgentEvent::ToolUse {
                id: id.clone(),
                name,
                input: info
                    .and_then(|i| i.get("parameters"))
                    .cloned()
                    .unwrap_or(Value::Null),
            });
        }
        let state = str_field(step, "state");
        if matches!(state, Some("DONE" | "ERROR")) {
            let content = match info.and_then(|i| i.get("output")) {
                Some(Value::String(s)) => s.clone(),
                Some(other) => stringify_content(other),
                None => String::new(),
            };
            out.push(AgentEvent::ToolResult {
                tool_use_id: id,
                content,
                is_error: state == Some("ERROR"),
            });
        }
    }
}

fn error_text(v: Option<&Value>, fallback: &str) -> String {
    match v {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(obj @ Value::Object(_)) => str_field(obj, "message")
            .filter(|m| !m.is_empty())
            .unwrap_or(fallback)
            .to_owned(),
        _ => fallback.to_owned(),
    }
}

/// The Gemini CLI JSONL branch (`handleGeminiEvent`).
fn gemini(obj: &Value, out: &mut Vec<AgentEvent>) -> bool {
    match str_field(obj, "type").unwrap_or_default() {
        "init" => {
            out.push(AgentEvent::Status {
                label: "initializing".to_owned(),
                model: str_field(obj, "model").map(str::to_owned),
                session_id: None,
                ttft_ms: None,
                detail: None,
            });
            true
        }
        "message" => {
            if str_field(obj, "role") == Some("assistant") {
                if let Some(text) = str_field(obj, "content").filter(|c| !c.is_empty()) {
                    out.push(AgentEvent::TextDelta {
                        delta: text.to_owned(),
                    });
                    return true;
                }
                return false;
            }
            str_field(obj, "role") == Some("user")
        }
        "tool_use" => {
            let (Some(id), Some(name)) = (str_field(obj, "tool_id"), str_field(obj, "tool_name"))
            else {
                return false;
            };
            let input = match obj.get("parameters") {
                Some(Value::String(s)) => {
                    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                }
                Some(other) => other.clone(),
                None => Value::Null,
            };
            out.push(AgentEvent::ToolUse {
                id: id.to_owned(),
                name: name.to_owned(),
                input,
            });
            true
        }
        "tool_result" => {
            let Some(id) = str_field(obj, "tool_id") else {
                return false;
            };
            let error = obj.get("error").filter(|e| e.is_object());
            let content = match obj.get("output") {
                Some(Value::String(s)) => s.clone(),
                other => match error {
                    Some(e) => error_text(Some(e), ""),
                    None => stringify_content(other.unwrap_or(&Value::Null)),
                },
            };
            out.push(AgentEvent::ToolResult {
                tool_use_id: id.to_owned(),
                content,
                is_error: str_field(obj, "status") == Some("error") || error.is_some(),
            });
            true
        }
        "error" => {
            let warning =
                str_field(obj, "severity").is_some_and(|s| s.eq_ignore_ascii_case("warning"));
            let source = obj.get("message").or_else(|| obj.get("error"));
            if warning {
                out.push(AgentEvent::Status {
                    label: "warning".to_owned(),
                    model: None,
                    session_id: None,
                    ttft_ms: None,
                    detail: Some(error_text(source, "Gemini CLI warning")),
                });
            } else {
                out.push(AgentEvent::Error {
                    message: error_text(source, "Gemini CLI error"),
                    code: None,
                    terminal: false,
                });
            }
            true
        }
        "result" => {
            if str_field(obj, "status") == Some("error")
                || obj.get("error").is_some_and(Value::is_object)
            {
                out.push(AgentEvent::Error {
                    message: error_text(obj.get("error"), "Gemini CLI error"),
                    code: None,
                    terminal: true,
                });
                return true;
            }
            if let Some(stats) = obj.get("stats").filter(|s| s.is_object()) {
                out.push(AgentEvent::Usage {
                    usage: map_usage(
                        stats,
                        &[
                            ("input_tokens", "input_tokens"),
                            ("output_tokens", "output_tokens"),
                            ("cached", "cached_read_tokens"),
                        ],
                    ),
                    cost_usd: None,
                    duration_ms: stats.get("duration_ms").and_then(Value::as_f64),
                    stop_reason: None,
                    is_error: false,
                });
            }
            true
        }
        _ => false,
    }
}
