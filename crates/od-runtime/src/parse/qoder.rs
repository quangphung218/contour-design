//! `qodercli -p --output-format stream-json` (ports `qoder-stream.ts`).
//!
//! Qoder's records are wrappers (`system`, `assistant`, `result`) with
//! fields of their own, so they get their own parser rather than Claude's.

use super::{StreamParser, raw_event, str_field};
use od_contracts::AgentEvent;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct QoderParser {
    thinking_started: bool,
}

impl StreamParser for QoderParser {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(obj) if obj.is_object() => self.handle(&obj, line, out),
            _ => out.push(raw_event(line)),
        }
    }
}

fn non_empty<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    str_field(v, key).filter(|s| !s.is_empty())
}

fn error_message(error: &Value) -> String {
    match error {
        Value::String(s) if !s.is_empty() => s.clone(),
        Value::Object(_) => str_field(error, "message").map_or_else(
            || "Unknown Qoder error".to_owned(),
            str::to_owned,
        ),
        _ => "Unknown Qoder error".to_owned(),
    }
}

fn result_message(obj: &Value) -> String {
    non_empty(obj, "error")
        .or_else(|| obj.get("error").and_then(|e| non_empty(e, "message")))
        .or_else(|| non_empty(obj, "message"))
        .map(str::to_owned)
        .or_else(|| non_empty(obj, "stop_reason").map(|r| format!("Qoder run failed: {r}")))
        .unwrap_or_else(|| "Qoder run failed".to_owned())
}

impl QoderParser {
    fn handle(&mut self, obj: &Value, line: &str, out: &mut Vec<AgentEvent>) {
        match str_field(obj, "type").unwrap_or_default() {
            "system" if str_field(obj, "subtype") == Some("init") => {
                out.push(AgentEvent::Status {
                    label: "initializing".to_owned(),
                    model: str_field(obj, "model").map(str::to_owned),
                    session_id: str_field(obj, "session_id").map(str::to_owned),
                    ttft_ms: None,
                    detail: None,
                });
            }
            "assistant" if obj.get("message").is_some_and(Value::is_object) => {
                self.assistant(obj, out);
            }
            "result" => {
                let is_error = obj.get("is_error").is_some_and(|v| match v {
                    Value::Bool(b) => *b,
                    Value::Null => false,
                    _ => true,
                });
                out.push(AgentEvent::Usage {
                    usage: obj.get("usage").filter(|u| !u.is_null()).cloned(),
                    cost_usd: obj.get("total_cost_usd").and_then(Value::as_f64),
                    duration_ms: obj.get("duration_ms").and_then(Value::as_f64),
                    stop_reason: str_field(obj, "stop_reason").map(str::to_owned),
                    is_error,
                });
                if is_error {
                    out.push(AgentEvent::Error {
                        message: result_message(obj),
                        code: None,
                        terminal: true,
                    });
                }
            }
            // A `user` echo of tool results and anything else.
            _ => out.push(raw_event(line)),
        }
    }

    fn assistant(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) {
        let message = &obj["message"];
        let mut emitted_text = false;
        for block in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(text) = str_field(block, "text").filter(|t| !t.is_empty()) {
                emitted_text = true;
                out.push(AgentEvent::TextDelta {
                    delta: text.to_owned(),
                });
                continue;
            }
            if str_field(block, "type") == Some("thinking") {
                if let Some(thinking) = non_empty(block, "thinking") {
                    if !self.thinking_started {
                        self.thinking_started = true;
                        out.push(AgentEvent::ThinkingStart);
                    }
                    out.push(AgentEvent::ThinkingDelta {
                        delta: thinking.to_owned(),
                    });
                }
            }
        }
        if !emitted_text {
            if let Some(text) = str_field(message, "content") {
                emitted_text = true;
                out.push(AgentEvent::TextDelta {
                    delta: text.to_owned(),
                });
            }
        }
        if let Some(error) = obj.get("error").filter(|e| !e.is_null()) {
            if !emitted_text && error != &Value::Bool(false) && error.as_str() != Some("") {
                out.push(AgentEvent::Error {
                    message: error_message(error),
                    code: None,
                    terminal: false,
                });
            }
        }
    }
}
