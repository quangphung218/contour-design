//! `pi --mode rpc` (ports `agent-protocol/pi-rpc/session.ts` and
//! `events.ts`).
//!
//! JSON commands on stdin (`{"id", "type", …}`), typed events on stdout.
//! A resumed turn first loads the stored session with `switch_session` and
//! sends the prompt only once Pi accepts it. After the prompt is accepted
//! the session file is asked for with `get_state`; that path is the
//! resumable session id. Pi stays alive after `agent_end`, so the engine
//! closes stdin and stops it. Extension UI requests are answered at once so
//! Pi never waits on a dialog nobody sees.

use super::{SessionContext, StreamParser, map_usage, status, str_field};
use od_contracts::AgentEvent;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// Extension UI methods that expect no answer.
const FIRE_AND_FORGET: [&str; 5] = [
    "setStatus",
    "setWidget",
    "notify",
    "setTitle",
    "set_editor_text",
];

/// Error code of a stored session Pi would not load (also a resume
/// failure marker, so the engine starts over with the full transcript).
pub const PI_PARENT_SESSION_FAILED: &str = "PI_PARENT_SESSION_FAILED";

#[derive(Debug)]
pub struct PiSession {
    ctx: SessionContext,
    writes: Vec<String>,
    next_id: u64,
    started: Instant,
    announced: bool,
    first_token: bool,
    parent_rpc: Option<u64>,
    prompt_rpc: Option<u64>,
    state_rpc: Option<u64>,
    reported_session: Option<String>,
    sessions_before: HashMap<PathBuf, (SystemTime, u64)>,
    /// `agent_end` seen or a fatal failure: the prompt is over.
    finished: bool,
    agent_ended: bool,
    aborted: bool,
}

impl PiSession {
    #[must_use]
    pub fn new(ctx: SessionContext) -> Self {
        let sessions_before = session_files(&ctx.cwd);
        let mut session = Self {
            ctx,
            writes: Vec::new(),
            next_id: 1,
            started: Instant::now(),
            announced: false,
            first_token: false,
            parent_rpc: None,
            prompt_rpc: None,
            state_rpc: None,
            reported_session: None,
            sessions_before,
            finished: false,
            agent_ended: false,
            aborted: false,
        };
        match session.ctx.resume_session_id.clone() {
            Some(path) => {
                session.parent_rpc =
                    Some(session.command("switch_session", &json!({"sessionPath": path})));
            }
            None => session.send_prompt(),
        }
        session
    }

    fn command(&mut self, kind: &str, params: &Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut obj = Map::new();
        obj.insert("id".to_owned(), json!(id));
        obj.insert("type".to_owned(), json!(kind));
        if let Some(extra) = params.as_object() {
            obj.extend(extra.clone());
        }
        let mut line = Value::Object(obj).to_string();
        line.push('\n');
        self.writes.push(line);
        id
    }

    fn send_prompt(&mut self) {
        let message = self.ctx.prompt.clone();
        self.prompt_rpc = Some(self.command("prompt", &json!({"message": message})));
    }

    fn fail(&mut self, message: String, code: Option<&str>, out: &mut Vec<AgentEvent>) {
        if self.finished {
            return;
        }
        self.finished = true;
        out.push(AgentEvent::Error {
            message,
            code: code.map(str::to_owned),
            terminal: true,
        });
    }

    fn session_status(id: String) -> AgentEvent {
        AgentEvent::Status {
            label: "session".to_owned(),
            model: None,
            session_id: Some(id),
            ttft_ms: None,
            detail: None,
        }
    }

    fn response(&mut self, raw: &Value, out: &mut Vec<AgentEvent>) {
        let id = raw.get("id").and_then(Value::as_u64);
        let ok = raw.get("success") == Some(&Value::Bool(true));
        let error = || {
            raw.get("error")
                .map_or_else(|| "unknown".to_owned(), |e| e.as_str().map_or_else(|| e.to_string(), str::to_owned))
        };
        if id.is_some() && id == self.state_rpc {
            self.state_rpc = None;
            let file = raw
                .pointer("/data/sessionFile")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|f| !f.is_empty());
            if let (true, Some(file)) = (ok, file) {
                self.reported_session = Some(file.to_owned());
                if self.agent_ended {
                    out.push(Self::session_status(file.to_owned()));
                }
            }
            return;
        }
        if self.finished {
            return;
        }
        if id.is_some() && id == self.parent_rpc {
            self.parent_rpc = None;
            if raw.pointer("/data/cancelled") == Some(&Value::Bool(true)) {
                self.fail(
                    "Parent session loading was canceled by a Pi extension.".to_owned(),
                    Some(PI_PARENT_SESSION_FAILED),
                    out,
                );
            } else if raw.get("success") == Some(&Value::Bool(false)) {
                self.fail(
                    format!("parent session rejected: {}", error()),
                    Some(PI_PARENT_SESSION_FAILED),
                    out,
                );
            } else if !self.aborted {
                self.send_prompt();
            }
            return;
        }
        if id.is_some() && id == self.prompt_rpc {
            if raw.get("success") == Some(&Value::Bool(false)) {
                self.fail(format!("prompt rejected: {}", error()), None, out);
            } else if ok {
                self.prompt_rpc = None;
                self.state_rpc = Some(self.command("get_state", &json!({})));
            }
        }
    }

    fn extension_ui(&mut self, raw: &Value) {
        let Some(id) = raw.get("id").filter(|v| !v.is_null()).cloned() else {
            return;
        };
        let method = str_field(raw, "method").unwrap_or_default();
        if FIRE_AND_FORGET.contains(&method) {
            return;
        }
        let mut reply = json!({"type": "extension_ui_response", "id": id});
        if method == "confirm" {
            reply["confirmed"] = json!(true);
        } else {
            let options = raw
                .pointer("/params/options")
                .or_else(|| raw.get("options"))
                .and_then(Value::as_array)
                .and_then(|o| o.first());
            match options {
                Some(Value::String(first)) => reply["value"] = json!(first),
                Some(first) => {
                    reply["value"] = first
                        .get("label")
                        .or_else(|| first.get("value"))
                        .cloned()
                        .unwrap_or_else(|| json!(""));
                }
                None => reply["cancelled"] = json!(true),
            }
        }
        let mut line = reply.to_string();
        line.push('\n');
        self.writes.push(line);
    }

    fn agent_end(&mut self, out: &mut Vec<AgentEvent>) {
        self.finished = true;
        self.agent_ended = true;
        // Pi owns where its sessions live; older builds wrote them under
        // the project, where exactly one changed file is safe to take.
        let path = self
            .reported_session
            .clone()
            .or_else(|| changed_session(&self.ctx.cwd, &self.sessions_before));
        if let Some(path) = path {
            out.push(Self::session_status(path));
        }
    }

    fn event(&mut self, raw: &Value, out: &mut Vec<AgentEvent>) {
        let error = |message: String| AgentEvent::Error {
            message,
            code: None,
            terminal: false,
        };
        let non_empty = |v: &Value, key: &str| {
            str_field(v, key)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        match str_field(raw, "type").unwrap_or_default() {
            "agent_start" => out.push(status("working")),
            "agent_end" => self.agent_end(out),
            "turn_start" => out.push(status("thinking")),
            "turn_end" => {
                let message = raw.get("message").unwrap_or(&Value::Null);
                if let Some(u) = message.get("usage").filter(|u| u.is_object()) {
                    let usage = map_usage(
                        u,
                        &[
                            ("input", "input_tokens"),
                            ("output", "output_tokens"),
                            ("cacheRead", "cached_read_tokens"),
                            ("cacheWrite", "cached_write_tokens"),
                            ("totalTokens", "total_tokens"),
                        ],
                    );
                    if usage.is_some() {
                        let cost = u.get("cost").and_then(|c| {
                            c.get("total")
                                .or_else(|| c.get("totalCost"))
                                .and_then(Value::as_f64)
                        });
                        out.push(AgentEvent::Usage {
                            usage,
                            cost_usd: cost,
                            duration_ms: Some(self.elapsed_ms()),
                            stop_reason: None,
                            is_error: false,
                        });
                    }
                }
                if str_field(message, "stopReason") == Some("error") {
                    out.push(error(
                        non_empty(message, "errorMessage")
                            .unwrap_or_else(|| "Pi agent error".to_owned()),
                    ));
                }
            }
            "message_update" => {
                let Some(ev) = raw.get("assistantMessageEvent").filter(|e| e.is_object()) else {
                    return;
                };
                let delta = str_field(ev, "delta");
                match (str_field(ev, "type").unwrap_or_default(), delta) {
                    ("text_delta", Some(delta)) => {
                        if !self.first_token {
                            self.first_token = true;
                            out.push(AgentEvent::Status {
                                label: "streaming".to_owned(),
                                model: None,
                                session_id: None,
                                ttft_ms: Some(self.elapsed_ms()),
                                detail: None,
                            });
                        }
                        out.push(AgentEvent::TextDelta {
                            delta: delta.to_owned(),
                        });
                    }
                    ("thinking_delta", Some(delta)) => out.push(AgentEvent::ThinkingDelta {
                        delta: delta.to_owned(),
                    }),
                    ("thinking_start", _) => out.push(AgentEvent::ThinkingStart),
                    ("error", _) => out.push(error(
                        non_empty(ev, "reason")
                            .or_else(|| non_empty(ev, "delta"))
                            .unwrap_or_else(|| "Agent error".to_owned()),
                    )),
                    _ => {}
                }
            }
            "tool_execution_start" => out.push(AgentEvent::ToolUse {
                id: str_field(raw, "toolCallId").unwrap_or_default().to_owned(),
                name: str_field(raw, "toolName").unwrap_or_default().to_owned(),
                input: raw.get("args").cloned().unwrap_or(Value::Null),
            }),
            "tool_execution_end" => {
                let content = raw.pointer("/result/content");
                let text = match content {
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|c| {
                            if str_field(c, "type") == Some("text") {
                                str_field(c, "text").unwrap_or_default().to_owned()
                            } else {
                                c.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                };
                out.push(AgentEvent::ToolResult {
                    tool_use_id: str_field(raw, "toolCallId").unwrap_or_default().to_owned(),
                    content: text,
                    is_error: raw.get("isError") == Some(&Value::Bool(true)),
                });
            }
            "extension_error" => out.push(error(
                non_empty(raw, "error").unwrap_or_else(|| "Extension error".to_owned()),
            )),
            "compaction_start" => out.push(status("compacting")),
            "auto_retry_start" => out.push(status("retrying")),
            "auto_retry_end" if raw.get("success") == Some(&Value::Bool(false)) => {
                out.push(error(
                    non_empty(raw, "finalError")
                        .unwrap_or_else(|| "Auto-retry exhausted".to_owned()),
                ));
            }
            _ => {}
        }
    }

    fn elapsed_ms(&self) -> f64 {
        u32::try_from(self.started.elapsed().as_millis()).map_or(f64::MAX, f64::from)
    }
}

/// `<cwd>/.pi/sessions/*.jsonl` with mtime and size.
fn session_files(cwd: &Path) -> HashMap<PathBuf, (SystemTime, u64)> {
    let Ok(entries) = std::fs::read_dir(cwd.join(".pi").join("sessions")) else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some((e.path(), (meta.modified().ok()?, meta.len())))
        })
        .collect()
}

/// The one session file that changed since `before`; `None` when zero or
/// several did (another Pi in the same folder).
fn changed_session(cwd: &Path, before: &HashMap<PathBuf, (SystemTime, u64)>) -> Option<String> {
    let changed: Vec<PathBuf> = session_files(cwd)
        .into_iter()
        .filter(|(path, (mtime, size))| {
            before
                .get(path)
                .is_none_or(|(m, s)| mtime > m || size != s)
        })
        .map(|(path, _)| path)
        .collect();
    match changed.as_slice() {
        [one] => Some(one.to_string_lossy().into_owned()),
        _ => None,
    }
}

impl StreamParser for PiSession {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        if !self.announced {
            self.announced = true;
            out.push(AgentEvent::Status {
                label: "initializing".to_owned(),
                model: self.ctx.model.clone().filter(|m| !m.is_empty()),
                session_id: None,
                ttft_ms: None,
                detail: None,
            });
        }
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if !raw.is_object() {
            return;
        }
        if str_field(&raw, "type") == Some("response") {
            self.response(&raw, out);
            return;
        }
        if self.finished {
            return;
        }
        if str_field(&raw, "type") == Some("extension_ui_request") {
            self.extension_ui(&raw);
            return;
        }
        self.event(&raw, out);
    }

    fn finish(&mut self, out: &mut Vec<AgentEvent>) {
        if !self.finished && !self.aborted {
            self.fail("Pi exited before the run completed.".to_owned(), None, out);
        }
    }

    fn take_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.writes)
    }

    fn abort_line(&mut self) -> Option<String> {
        if self.finished || self.aborted {
            return None;
        }
        self.aborted = true;
        self.command("abort", &json!({}));
        self.writes.pop()
    }

    fn completed(&self) -> bool {
        self.finished
    }
}
