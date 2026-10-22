//! `copilot --output-format json` (spec 2.3; ports `copilot-stream.ts`).

use super::{StreamParser, raw_event, status, str_field};
use od_contracts::AgentEvent;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct CopilotParser;

impl StreamParser for CopilotParser {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(obj) if obj.is_object() => handle(&obj, out),
            _ => out.push(raw_event(line)),
        }
    }
}

fn stringify_result(r: &Value) -> String {
    match r {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Object(_) => str_field(r, "content")
            .or_else(|| str_field(r, "detailedContent"))
            .map_or_else(|| r.to_string(), str::to_owned),
        other => other.to_string(),
    }
}

/// Unknown frame types are dropped, like the original.
fn handle(obj: &Value, out: &mut Vec<AgentEvent>) {
    let data = obj
        .get("data")
        .filter(|d| d.is_object())
        .unwrap_or(&Value::Null);
    let delta = || {
        str_field(data, "deltaContent")
            .map(str::to_owned)
            .unwrap_or_default()
    };
    match str_field(obj, "type").unwrap_or_default() {
        "session.tools_updated" => {
            if let Some(model) = str_field(data, "model") {
                out.push(AgentEvent::Status {
                    label: "initializing".to_owned(),
                    model: Some(model.to_owned()),
                    session_id: None,
                    ttft_ms: None,
                    detail: None,
                });
            }
        }
        "assistant.turn_start" => out.push(status("streaming")),
        "assistant.reasoning_delta" if data.get("deltaContent").is_some_and(Value::is_string) => {
            out.push(AgentEvent::ThinkingDelta { delta: delta() });
        }
        "assistant.message_delta" if data.get("deltaContent").is_some_and(Value::is_string) => {
            out.push(AgentEvent::TextDelta { delta: delta() });
        }
        "tool.execution_start" => out.push(AgentEvent::ToolUse {
            id: str_field(data, "toolCallId").unwrap_or_default().to_owned(),
            name: str_field(data, "toolName").unwrap_or_default().to_owned(),
            input: data.get("arguments").cloned().unwrap_or(Value::Null),
        }),
        "tool.execution_complete" => out.push(AgentEvent::ToolResult {
            tool_use_id: str_field(data, "toolCallId").unwrap_or_default().to_owned(),
            content: stringify_result(data.get("result").unwrap_or(&Value::Null)),
            is_error: data.get("success") == Some(&Value::Bool(false)),
        }),
        "result" => {
            let ok = obj.get("success") == Some(&Value::Bool(true))
                || obj.get("exitCode").and_then(Value::as_i64) == Some(0);
            let usage = obj.get("usage").filter(|u| !u.is_null()).cloned();
            let duration = usage
                .as_ref()
                .and_then(|u| u.get("sessionDurationMs"))
                .and_then(Value::as_f64);
            out.push(AgentEvent::Usage {
                usage,
                cost_usd: None,
                duration_ms: duration,
                stop_reason: Some(if ok { "completed" } else { "error" }.to_owned()),
                is_error: !ok,
            });
        }
        _ => {}
    }
}
