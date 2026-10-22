//! DeepSeek Harness `dsh --profile open-design --stdio` (spec 2.3; ports
//! `agent-protocol/dsh-profile/session.ts`).
//!
//! JSONL both ways, every frame versioned `"v": 1`. The harness opens with
//! `ready`; the host answers with one `execute` command; the run streams
//! `session`, `thinking`, `text`, `tool_call`, `tool_result`, `usage` and
//! ends with `result`. Cancel is the `cancel` command.

use super::{SessionContext, StreamParser, raw_event, str_field};
use od_contracts::AgentEvent;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct DshSession {
    ctx: SessionContext,
    writes: Vec<String>,
    ready: bool,
    executed: bool,
    session_id: Option<String>,
    thinking_started: bool,
    done: bool,
    aborted: bool,
}

impl DshSession {
    #[must_use]
    pub fn new(ctx: SessionContext) -> Self {
        Self {
            ctx,
            writes: Vec::new(),
            ready: false,
            executed: false,
            session_id: None,
            thinking_started: false,
            done: false,
            aborted: false,
        }
    }

    fn send(&mut self, command: &Value) {
        let mut line = command.to_string();
        line.push('\n');
        self.writes.push(line);
    }

    fn fail(&mut self, message: &str, code: &str, out: &mut Vec<AgentEvent>) {
        out.push(AgentEvent::Error {
            message: message.to_owned(),
            code: Some(code.to_owned()),
            terminal: true,
        });
        self.done = true;
    }

    fn execute(&mut self, out: &mut Vec<AgentEvent>) {
        let mut command = json!({
            "v": 1,
            "type": "execute",
            "request_id": self.ctx.request_id,
            "cwd": self.ctx.cwd.to_string_lossy(),
            "prompt": self.ctx.prompt,
            "mcp_servers": [],
        });
        if let Some(id) = &self.ctx.resume_session_id {
            command["resume_session_id"] = json!(id);
        }
        if let Some(model) = self
            .ctx
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty() && *m != "default")
        {
            match model.split_once('/') {
                Some((provider, id)) if !provider.is_empty() && !id.is_empty() => {
                    command["model"] = json!({"provider": provider, "id": id});
                }
                _ => {
                    self.fail(
                        "DeepSeek Harness model must use provider/model format.",
                        "DSH_PROFILE_INVALID_MODEL",
                        out,
                    );
                    return;
                }
            }
        }
        if let Some(effort) = self
            .ctx
            .reasoning
            .as_deref()
            .filter(|r| !r.is_empty() && *r != "default")
        {
            command["reasoning_effort"] = json!(effort);
        }
        self.executed = true;
        self.send(&command);
    }

    /// Run content before a `session` frame is a protocol violation.
    fn require_session(&mut self, out: &mut Vec<AgentEvent>) -> bool {
        if self.session_id.is_some() {
            return true;
        }
        self.fail(
            "DeepSeek Harness profile emitted run content before establishing a session.",
            "DSH_PROFILE_PROTOCOL_ERROR",
            out,
        );
        false
    }

    fn frame(&mut self, frame: &Value, out: &mut Vec<AgentEvent>) {
        let kind = str_field(frame, "type").unwrap_or_default();
        if !self.ready {
            if kind == "ready" {
                self.ready = true;
                if !self.aborted {
                    self.execute(out);
                }
            } else {
                self.fail(
                    "DeepSeek Harness profile did not begin with a ready frame.",
                    "DSH_PROFILE_PROTOCOL_ERROR",
                    out,
                );
            }
            return;
        }
        if str_field(frame, "request_id").is_some_and(|id| id != self.ctx.request_id) {
            return;
        }
        let content = || str_field(frame, "content").unwrap_or_default().to_owned();
        match kind {
            "session" => {
                let Some(id) = str_field(frame, "session_id").map(str::to_owned) else {
                    return;
                };
                let resumed = frame.get("resumed") == Some(&Value::Bool(true));
                if let Some(wanted) = &self.ctx.resume_session_id {
                    if !resumed || &id != wanted {
                        self.fail(
                            "DeepSeek Harness profile resumed a different session.",
                            "DSH_PROFILE_RESUME_MISMATCH",
                            out,
                        );
                        return;
                    }
                }
                self.session_id = Some(id.clone());
                out.push(AgentEvent::Status {
                    label: "working".to_owned(),
                    model: None,
                    session_id: Some(id),
                    ttft_ms: None,
                    detail: None,
                });
            }
            "thinking" => {
                if self.require_session(out) {
                    if !self.thinking_started {
                        self.thinking_started = true;
                        out.push(AgentEvent::ThinkingStart);
                    }
                    let delta = content();
                    if !delta.is_empty() {
                        out.push(AgentEvent::ThinkingDelta { delta });
                    }
                }
            }
            "text" => {
                let delta = content();
                if self.require_session(out) && !delta.is_empty() {
                    out.push(AgentEvent::TextDelta { delta });
                }
            }
            "tool_call" => {
                if self.require_session(out) {
                    let args = str_field(frame, "arguments").unwrap_or("{}");
                    let input = serde_json::from_str::<Value>(args)
                        .ok()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({ "raw": args }));
                    out.push(AgentEvent::ToolUse {
                        id: str_field(frame, "call_id").unwrap_or_default().to_owned(),
                        name: str_field(frame, "name").unwrap_or_default().to_owned(),
                        input,
                    });
                }
            }
            "tool_result" => {
                if self.require_session(out) {
                    out.push(AgentEvent::ToolResult {
                        tool_use_id: str_field(frame, "call_id").unwrap_or_default().to_owned(),
                        content: str_field(frame, "output").unwrap_or_default().to_owned(),
                        is_error: frame.get("is_error") == Some(&Value::Bool(true)),
                    });
                }
            }
            "usage" => {
                if self.require_session(out) {
                    let n = |k: &str| frame.get(k).and_then(Value::as_u64).unwrap_or(0);
                    out.push(AgentEvent::Usage {
                        usage: Some(json!({
                            "input_tokens": n("input_tokens"),
                            "output_tokens": n("output_tokens"),
                            "cached_read_tokens": n("cache_read_tokens"),
                            "cached_write_tokens": n("cache_write_tokens"),
                            "total_tokens": n("input_tokens") + n("output_tokens"),
                        })),
                        cost_usd: None,
                        duration_ms: None,
                        stop_reason: None,
                        is_error: false,
                    });
                }
            }
            "protocol_error" => {
                let code = str_field(frame, "code").unwrap_or("DSH_PROFILE_PROTOCOL_ERROR");
                let message = str_field(frame, "message")
                    .unwrap_or("DeepSeek Harness profile reported a protocol error.")
                    .to_owned();
                self.fail(&message, code, out);
            }
            "result" => self.result(frame, out),
            _ => out.push(raw_event(&frame.to_string())),
        }
    }

    fn result(&mut self, frame: &Value, out: &mut Vec<AgentEvent>) {
        let status = str_field(frame, "status").unwrap_or_default();
        let error = frame.get("error");
        let error_message = |fallback: &str| {
            error
                .and_then(|e| str_field(e, "message"))
                .unwrap_or(fallback)
                .to_owned()
        };
        let error_code = |fallback: &str| {
            error
                .and_then(|e| str_field(e, "code"))
                .unwrap_or(fallback)
                .to_owned()
        };
        if self.ctx.resume_session_id.is_some()
            && frame.get("resume_rejected") == Some(&Value::Bool(true))
        {
            let code = error_code("DSH_PROFILE_RESUME_REJECTED");
            self.fail(
                "DeepSeek Harness could not resume the saved session.",
                &code,
                out,
            );
            return;
        }
        if status == "failed" {
            let message = error_message("DeepSeek Harness profile run failed.");
            let code = error_code("DSH_PROFILE_RUN_FAILED");
            self.fail(&message, &code, out);
            return;
        }
        if status == "cancelled" && !self.aborted {
            self.fail(
                "DeepSeek Harness profile cancelled a run that OpenDesign did not cancel.",
                "DSH_PROFILE_PROTOCOL_ERROR",
                out,
            );
            return;
        }
        if !self.require_session(out) {
            return;
        }
        if str_field(frame, "session_id") != self.session_id.as_deref() {
            self.fail(
                "DeepSeek Harness profile terminal result changed the session id.",
                "DSH_PROFILE_SESSION_MISMATCH",
                out,
            );
            return;
        }
        out.push(AgentEvent::Usage {
            usage: None,
            cost_usd: None,
            duration_ms: None,
            stop_reason: Some(
                str_field(frame, "stop_reason")
                    .unwrap_or(if status == "cancelled" {
                        "cancelled"
                    } else {
                        "end_turn"
                    })
                    .to_owned(),
            ),
            is_error: false,
        });
        self.done = true;
    }
}

impl StreamParser for DshSession {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        let line = line.trim();
        if line.is_empty() || self.done {
            return;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(frame) if frame.is_object() && frame.get("v") == Some(&json!(1)) => {
                self.frame(&frame, out);
            }
            _ => self.fail(
                "DeepSeek Harness profile emitted a malformed frame.",
                "DSH_PROFILE_MALFORMED_FRAME",
                out,
            ),
        }
    }

    fn finish(&mut self, out: &mut Vec<AgentEvent>) {
        if !self.done && !self.aborted {
            self.fail(
                "DeepSeek Harness exited before the run completed.",
                "DSH_PROFILE_TRUNCATED_FRAME",
                out,
            );
        }
    }

    fn take_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.writes)
    }

    fn abort_line(&mut self) -> Option<String> {
        self.aborted = true;
        if self.done || !self.executed {
            return None;
        }
        let mut line =
            json!({"v": 1, "type": "cancel", "request_id": self.ctx.request_id}).to_string();
        line.push('\n');
        Some(line)
    }

    fn completed(&self) -> bool {
        self.done
    }
}
