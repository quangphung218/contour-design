//! Agent Client Protocol client, shared by every ACP agent (spec 2.3; ports
//! `agent-protocol/acp/session.ts`, `rpc.ts`, `updates.ts`).
//!
//! JSON-RPC 2.0 over stdio: `initialize` → `session/new` (or
//! `session/load` to resume) → optional `session/set_model` →
//! `session/prompt`. `session/update` notifications become events,
//! `session/request_permission` is answered by [`PermissionMode`], and
//! cancel is the `session/cancel` notification.

use super::{
    SessionContext, StreamParser, map_usage, normalize_task_status, raw_event, status, str_field,
};
use crate::def::PermissionMode;
use od_contracts::AgentEvent;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::Path;

/// `ACP_PROTOCOL_VERSION` of the original client.
pub const ACP_PROTOCOL_VERSION: u64 = 1;

const INITIALIZE_ID: u64 = 1;
const SESSION_ID: u64 = 2;

/// JSON-RPC "method not found": answer to agent→client requests this
/// client does not implement (we advertise no fs or terminal capability).
const METHOD_NOT_FOUND: i64 = -32601;

/// One prompt over one ACP connection.
#[derive(Debug)]
pub struct AcpSession {
    ctx: SessionContext,
    writes: Vec<String>,
    next_id: u64,
    session_id: Option<String>,
    set_model_id: Option<u64>,
    prompt_id: Option<u64>,
    done: bool,
    aborted: bool,
    thinking_started: bool,
    /// All text emitted so far; agents that resend the whole message as a
    /// cumulative snapshot only contribute the unseen suffix.
    text_so_far: String,
    tools: HashMap<String, ToolState>,
    /// Tool ids in first-seen order, for a stable flush.
    tool_order: Vec<String>,
    plans: u32,
}

#[derive(Debug, Default)]
struct ToolState {
    name: String,
    name_from_kind: bool,
    input: Map<String, Value>,
    result: String,
    failed: bool,
    emitted: bool,
}

impl AcpSession {
    #[must_use]
    pub fn new(ctx: SessionContext) -> Self {
        let mut session = Self {
            ctx,
            writes: Vec::new(),
            next_id: SESSION_ID + 1,
            session_id: None,
            set_model_id: None,
            prompt_id: None,
            done: false,
            aborted: false,
            thinking_started: false,
            text_so_far: String::new(),
            tools: HashMap::new(),
            tool_order: Vec::new(),
            plans: 0,
        };
        session.request(
            INITIALIZE_ID,
            "initialize",
            json!({
                "protocolVersion": ACP_PROTOCOL_VERSION,
                "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false},
                    "terminal": false,
                },
                "clientInfo": {"name": "open-design", "version": "runtime-adapter"},
            }),
        );
        session
    }

    fn send(&mut self, frame: &Value) {
        let mut line = frame.to_string();
        line.push('\n');
        self.writes.push(line);
    }

    fn request(&mut self, id: u64, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn cwd(&self) -> String {
        self.ctx.cwd.to_string_lossy().into_owned()
    }

    fn open_session(&mut self) {
        let cwd = self.cwd();
        match self.ctx.resume_session_id.clone() {
            Some(id) => self.request(
                SESSION_ID,
                "session/load",
                json!({"sessionId": id, "cwd": cwd, "mcpServers": []}),
            ),
            None => self.request(
                SESSION_ID,
                "session/new",
                json!({"cwd": cwd, "mcpServers": []}),
            ),
        }
    }

    fn send_prompt(&mut self) {
        let id = self.take_id();
        self.prompt_id = Some(id);
        let session = self.session_id.clone().unwrap_or_default();
        let prompt = self.ctx.prompt.clone();
        self.request(
            id,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": prompt}]}),
        );
    }

    fn on_result(&mut self, id: u64, result: &Value, out: &mut Vec<AgentEvent>) {
        if id == INITIALIZE_ID {
            self.open_session();
            return;
        }
        if id == SESSION_ID {
            // `session/load` may answer `null`: the session is the one asked for.
            let session = str_field(result, "sessionId")
                .map(str::to_owned)
                .or_else(|| self.ctx.resume_session_id.clone());
            let Some(session) = session else {
                self.fail(
                    &format!("invalid session/new response: {result}"),
                    None,
                    out,
                );
                return;
            };
            self.session_id = Some(session.clone());
            let model = result
                .pointer("/models/currentModelId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            out.push(AgentEvent::Status {
                label: "initializing".to_owned(),
                model,
                session_id: Some(session.clone()),
                ttft_ms: None,
                detail: None,
            });
            let wanted = self
                .ctx
                .model
                .clone()
                .filter(|m| !m.is_empty() && m != "default");
            if let Some(model) = wanted {
                let id = self.take_id();
                self.set_model_id = Some(id);
                self.request(
                    id,
                    "session/set_model",
                    json!({"sessionId": session, "modelId": model}),
                );
            } else {
                self.send_prompt();
            }
            return;
        }
        if Some(id) == self.set_model_id {
            self.set_model_id = None;
            self.send_prompt();
            return;
        }
        if Some(id) == self.prompt_id {
            self.flush_tools(out);
            let stop = str_field(result, "stopReason")
                .unwrap_or("end_turn")
                .to_owned();
            out.push(AgentEvent::Usage {
                usage: result.get("usage").and_then(format_usage),
                cost_usd: None,
                duration_ms: None,
                stop_reason: Some(stop),
                is_error: false,
            });
            self.done = true;
        }
    }

    fn on_error(&mut self, id: Option<u64>, error: &Value, out: &mut Vec<AgentEvent>) {
        if self.done {
            return;
        }
        // A rejected model switch is recoverable: prompt on the default.
        if id.is_some() && id == self.set_model_id {
            self.set_model_id = None;
            out.push(AgentEvent::Status {
                label: "model".to_owned(),
                model: None,
                session_id: None,
                ttft_ms: None,
                detail: str_field(error, "message").map(str::to_owned),
            });
            self.send_prompt();
            return;
        }
        let message = str_field(error, "message")
            .filter(|m| !m.is_empty())
            .unwrap_or("ACP request failed");
        let message = match error.get("data") {
            Some(Value::String(d)) if !d.is_empty() => format!("{message}: {d}"),
            Some(d @ Value::Object(_)) => match str_field(d, "message").or(str_field(d, "details"))
            {
                Some(detail) => format!("{message}: {detail}"),
                None => message.to_owned(),
            },
            _ => message.to_owned(),
        };
        let code = error.get("code").map(|c| match c {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        self.fail(&message, code, out);
    }

    fn fail(&mut self, message: &str, code: Option<String>, out: &mut Vec<AgentEvent>) {
        self.flush_tools(out);
        out.push(AgentEvent::Error {
            message: message.to_owned(),
            code,
            terminal: true,
        });
        self.done = true;
    }

    fn on_request(&mut self, id: &Value, method: &str, params: &Value, out: &mut Vec<AgentEvent>) {
        if method == "session/request_permission" {
            let tool = params.get("toolCall").unwrap_or(&Value::Null);
            let (outcome, approved) =
                permission_outcome(self.ctx.permission, params.get("options"), tool);
            let title = str_field(tool, "title").unwrap_or("tool call");
            out.push(AgentEvent::Status {
                label: "permission".to_owned(),
                model: None,
                session_id: None,
                ttft_ms: None,
                detail: Some(format!(
                    "{} {title}",
                    if approved { "approved" } else { "denied" }
                )),
            });
            self.send(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": outcome}}));
            return;
        }
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": METHOD_NOT_FOUND, "message": format!("{method} is not supported")},
        }));
    }

    fn on_update(&mut self, update: &Value, out: &mut Vec<AgentEvent>) {
        let kind = str_field(update, "sessionUpdate").unwrap_or_default();
        match kind {
            "agent_message_chunk" => {
                let Some(text) = update_text(update) else {
                    return;
                };
                let delta = match text.strip_prefix(self.text_so_far.as_str()) {
                    Some(rest) if !self.text_so_far.is_empty() => rest.to_owned(),
                    _ => text,
                };
                if !delta.is_empty() {
                    self.text_so_far.push_str(&delta);
                    out.push(AgentEvent::TextDelta { delta });
                }
            }
            "agent_thought_chunk" => {
                if let Some(text) = update_text(update) {
                    if !self.thinking_started {
                        self.thinking_started = true;
                        out.push(AgentEvent::ThinkingStart);
                    }
                    out.push(AgentEvent::ThinkingDelta { delta: text });
                }
            }
            "tool_call" | "tool_call_update" => self.on_tool(update, out),
            "plan" => {
                let todos: Vec<Value> = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| {
                        let content = str_field(e, "content")?;
                        Some(json!({
                            "content": content,
                            "status": normalize_task_status(str_field(e, "status")),
                        }))
                    })
                    .collect();
                if !todos.is_empty() {
                    self.plans += 1;
                    out.push(AgentEvent::ToolUse {
                        id: format!("acp-plan-{}", self.plans),
                        name: "TodoWrite".to_owned(),
                        input: json!({ "todos": todos }),
                    });
                }
            }
            "turn_end" if self.ctx.acp_turn_end_completes && self.prompt_id.is_some() => {
                self.flush_tools(out);
                out.push(AgentEvent::Usage {
                    usage: update.get("usage").and_then(format_usage),
                    cost_usd: None,
                    duration_ms: None,
                    stop_reason: Some("end_turn".to_owned()),
                    is_error: false,
                });
                self.done = true;
            }
            // Bookkeeping the chat has nothing to show for.
            "available_commands_update" | "current_mode_update" | "user_message_chunk" => {}
            other => {
                let mut ev = status(if other.is_empty() {
                    "session_update"
                } else {
                    other
                });
                if let AgentEvent::Status { detail, .. } = &mut ev {
                    *detail = str_field(update, "message")
                        .or_else(|| str_field(update, "detail"))
                        .map(str::to_owned);
                }
                out.push(ev);
            }
        }
    }

    fn on_tool(&mut self, update: &Value, out: &mut Vec<AgentEvent>) {
        let Some(id) = str_field(update, "toolCallId")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return;
        };
        let id = id.to_owned();
        let name = tool_name(update);
        let from_kind = str_field(update, "kind").is_some_and(|k| kind_name(k).is_some());
        let input = tool_input(update, &self.ctx.cwd);
        let result = tool_result(update);
        let failed = tool_failed(update);
        if !self.tools.contains_key(&id) {
            self.tool_order.push(id.clone());
        }
        let st = self.tools.entry(id.clone()).or_default();
        if st.emitted {
            return;
        }
        // A name from `kind` is authoritative; later title-only frames
        // must not rename the call.
        if st.name.is_empty() || from_kind || (!st.name_from_kind && name != "Tool") {
            st.name = name;
            st.name_from_kind |= from_kind;
        }
        st.input.extend(input);
        if !result.is_empty() {
            st.result = result;
        }
        st.failed |= failed;
        let status = acp_status(update);
        if matches!(
            status.as_str(),
            "completed" | "complete" | "succeeded" | "success"
        ) || is_failure_status(&status)
        {
            emit_tool(&id, st, out);
        }
    }

    /// Emits every call that never reached a terminal status.
    fn flush_tools(&mut self, out: &mut Vec<AgentEvent>) {
        for id in std::mem::take(&mut self.tool_order) {
            if let Some(st) = self.tools.get_mut(&id) {
                if !st.emitted {
                    if st.result.is_empty() {
                        st.result = "tool call did not finish".to_owned();
                    }
                    st.failed = true;
                    emit_tool(&id, st, out);
                }
            }
        }
    }
}

fn emit_tool(id: &str, st: &mut ToolState, out: &mut Vec<AgentEvent>) {
    st.emitted = true;
    out.push(AgentEvent::ToolUse {
        id: id.to_owned(),
        name: st.name.clone(),
        input: Value::Object(st.input.clone()),
    });
    out.push(AgentEvent::ToolResult {
        tool_use_id: id.to_owned(),
        content: std::mem::take(&mut st.result),
        is_error: st.failed,
    });
}

impl StreamParser for AcpSession {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            out.push(raw_event(line));
            return;
        };
        let id = obj.get("id").filter(|v| !v.is_null());
        let method = str_field(&obj, "method");
        match (method, id) {
            (Some("session/update"), _) => {
                if let Some(update) = obj.pointer("/params/update") {
                    if !self.done {
                        self.on_update(update, out);
                    }
                }
            }
            (Some(method), Some(id)) => {
                let params = obj.get("params").cloned().unwrap_or(Value::Null);
                self.on_request(&id.clone(), method, &params, out);
            }
            (Some(_), None) => {}
            (None, id) => {
                let id = id.and_then(Value::as_u64);
                if let Some(error) = obj.get("error").filter(|e| !e.is_null()) {
                    self.on_error(id, error, out);
                } else if let Some(id) = id {
                    let result = obj.get("result").cloned().unwrap_or(Value::Null);
                    self.on_result(id, &result, out);
                } else {
                    out.push(raw_event(line));
                }
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<AgentEvent>) {
        if !self.done && !self.aborted {
            self.fail("ACP session exited before completion", None, out);
        }
    }

    fn take_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.writes)
    }

    fn abort_line(&mut self) -> Option<String> {
        self.aborted = true;
        if self.done {
            return None;
        }
        let session = self.session_id.clone()?;
        let mut line =
            json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}})
                .to_string();
        line.push('\n');
        Some(line)
    }

    fn completed(&self) -> bool {
        self.done
    }
}

/// The permission answer and whether it approves. `Auto` approves like the
/// original (`approve_for_session`, then `allow_always`, then
/// `allow_once`). `Ask` mirrors Claude's `acceptEdits`: reads and edits are
/// allowed once, anything else (commands, fetches, custom tools) is
/// rejected, since there is nobody to ask mid-run.
#[must_use]
pub fn permission_outcome(
    mode: PermissionMode,
    options: Option<&Value>,
    tool: &Value,
) -> (Value, bool) {
    let options: &[Value] = options.and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let by_kind = |kind: &str| {
        options
            .iter()
            .find(|o| str_field(o, "kind") == Some(kind))
            .and_then(|o| str_field(o, "optionId"))
    };
    let selected = |id: &str| json!({"outcome": "selected", "optionId": id});
    let allow = match mode {
        PermissionMode::Auto => options
            .iter()
            .find_map(|o| str_field(o, "optionId").filter(|id| *id == "approve_for_session"))
            .or_else(|| by_kind("allow_always"))
            .or_else(|| by_kind("allow_once")),
        PermissionMode::Ask => {
            let kind = str_field(tool, "kind").unwrap_or_default().to_lowercase();
            let editing = matches!(
                kind.as_str(),
                "read" | "edit" | "delete" | "move" | "search" | "think"
            );
            if editing { by_kind("allow_once") } else { None }
        }
    };
    if let Some(id) = allow {
        return (selected(id), true);
    }
    match by_kind("reject_once").or_else(|| by_kind("reject_always")) {
        Some(id) => (selected(id), false),
        None => (json!({"outcome": "cancelled"}), false),
    }
}

fn update_text(update: &Value) -> Option<String> {
    let text = text_of(update.get("content")?)?;
    (!text.is_empty()).then_some(text)
}

/// Text of an ACP content block, a string, or a list of either.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| o.get("content").and_then(text_of)),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(text_of).collect();
            (!parts.is_empty()).then(|| parts.concat())
        }
        _ => None,
    }
}

fn acp_status(update: &Value) -> String {
    str_field(update, "status")
        .unwrap_or_default()
        .to_lowercase()
        .replace([' ', '_', '-'], "")
}

fn is_failure_status(status: &str) -> bool {
    matches!(
        status,
        "failed" | "failure" | "error" | "cancelled" | "canceled"
    )
}

fn tool_failed(update: &Value) -> bool {
    if is_failure_status(&acp_status(update)) || update.get("isError") == Some(&Value::Bool(true)) {
        return true;
    }
    update
        .get("exitCode")
        .or_else(|| update.get("exit_code"))
        .and_then(Value::as_i64)
        .is_some_and(|c| c != 0)
}

/// Claude-shaped tool name from an ACP `kind` (`acpToolNameFromKind`).
fn kind_name(kind: &str) -> Option<&'static str> {
    Some(match kind.trim().to_lowercase().as_str() {
        "edit" | "patch" | "replace" | "update" => "Edit",
        "write" | "create" | "save" => "Write",
        "read" => "Read",
        "execute" | "bash" | "shell" | "terminal" => "Bash",
        "search" | "grep" | "glob" => "Grep",
        "think" | "thought" | "reason" | "reasoning" => "Think",
        "fetch" | "web" => "Fetch",
        _ => return None,
    })
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

/// `acpToolName`: a recognised kind wins, then an identifier-like `name`,
/// then words in the `title`. Shell-family names are only believed from
/// `kind`.
fn tool_name(update: &Value) -> String {
    let kind = str_field(update, "kind").unwrap_or_default();
    let name = str_field(update, "name").map(str::trim).unwrap_or_default();
    if [kind, name]
        .iter()
        .any(|n| matches!(n.to_lowercase().as_str(), "todowrite" | "todo_write"))
    {
        return "TodoWrite".to_owned();
    }
    if let Some(n) = kind_name(kind) {
        return n.to_owned();
    }
    let not_shell = |n: String| {
        if matches!(
            n.to_lowercase().as_str(),
            "bash" | "shell" | "execute" | "terminal"
        ) {
            "Other".to_owned()
        } else {
            n
        }
    };
    if !name.is_empty() {
        return not_shell(if is_identifier(name) {
            name.to_owned()
        } else {
            "Other".to_owned()
        });
    }
    if kind.eq_ignore_ascii_case("other") {
        return "Other".to_owned();
    }
    if let Some(title) = str_field(update, "title").filter(|t| !t.trim().is_empty()) {
        let lower = title.to_lowercase();
        let has = |words: &[&str]| {
            lower
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| words.contains(&w))
        };
        let derived = if has(&["grep", "search", "glob"]) {
            "Grep".to_owned()
        } else if has(&["read"]) {
            "Read".to_owned()
        } else if has(&["edit", "patch", "replace"]) {
            "Edit".to_owned()
        } else if has(&["write", "create", "save", "update"]) {
            "Write".to_owned()
        } else {
            let first = title.split_whitespace().next().unwrap_or("Tool");
            let mut chars = first.chars();
            let cap: String = chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default();
            if is_identifier(&cap) {
                cap
            } else {
                "Other".to_owned()
            }
        };
        return not_shell(derived);
    }
    "Tool".to_owned()
}

/// `rawInput` plus a real `file_path` when the update names one
/// (`locations[].path` or a path-like `rawInput` key). Never the tool id.
fn tool_input(update: &Value, cwd: &Path) -> Map<String, Value> {
    let mut input = update
        .get("rawInput")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let path = update
        .get("locations")
        .and_then(Value::as_array)
        .and_then(|l| l.iter().find_map(|loc| str_field(loc, "path")))
        .or_else(|| {
            ["file_path", "filePath", "path", "abs_path"]
                .iter()
                .find_map(|k| input.get(*k).and_then(Value::as_str))
        })
        .or_else(|| {
            update
                .get("content")
                .and_then(Value::as_array)
                .and_then(|c| c.iter().find_map(|e| str_field(e, "path")))
        })
        .filter(|p| !p.is_empty())
        .map(|p| {
            let p = Path::new(p);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        });
    if let Some(path) = path {
        input.insert(
            "file_path".to_owned(),
            Value::String(path.to_string_lossy().into_owned()),
        );
    }
    if input.is_empty() {
        if let Some(title) = str_field(update, "title").filter(|t| !t.trim().is_empty()) {
            input.insert("title".to_owned(), Value::String(title.trim().to_owned()));
        }
    }
    input
}

const MAX_RESULT_CHARS: usize = 8000;

/// `acpToolResultContent`: `rawOutput`, else text and diff entries.
fn tool_result(update: &Value) -> String {
    let text = match update.get("rawOutput") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => update
            .get("content")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| {
                        text_of(e).or_else(|| {
                            let new = str_field(e, "newText")?;
                            Some(new.to_owned())
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        Some(other) => other.to_string(),
    };
    if text.chars().count() > MAX_RESULT_CHARS {
        text.chars().take(MAX_RESULT_CHARS).collect()
    } else {
        text
    }
}

/// `formatUsage`: camelCase or snake_case token counts → snake_case.
fn format_usage(usage: &Value) -> Option<Value> {
    map_usage(
        usage,
        &[
            ("inputTokens", "input_tokens"),
            ("input_tokens", "input_tokens"),
            ("outputTokens", "output_tokens"),
            ("output_tokens", "output_tokens"),
            ("thoughtTokens", "thought_tokens"),
            ("thought_tokens", "thought_tokens"),
            ("cachedReadTokens", "cached_read_tokens"),
            ("cached_read_tokens", "cached_read_tokens"),
            ("cache_read_input_tokens", "cached_read_tokens"),
            ("cachedWriteTokens", "cached_write_tokens"),
            ("cached_write_tokens", "cached_write_tokens"),
            ("cache_creation_input_tokens", "cached_write_tokens"),
            ("totalTokens", "total_tokens"),
            ("total_tokens", "total_tokens"),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Value {
        json!([
            {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
            {"optionId": "always", "name": "Always", "kind": "allow_always"},
            {"optionId": "deny", "name": "Deny", "kind": "reject_once"},
        ])
    }

    #[test]
    fn auto_prefers_session_wide_approval() {
        let tool = json!({"kind": "execute"});
        let (o, ok) = permission_outcome(PermissionMode::Auto, Some(&options()), &tool);
        assert!(ok);
        assert_eq!(o["optionId"], "always");
        let with_session = json!([{"optionId": "approve_for_session", "kind": "x"}]);
        let (o, _) = permission_outcome(PermissionMode::Auto, Some(&with_session), &tool);
        assert_eq!(o["optionId"], "approve_for_session");
    }

    #[test]
    fn ask_allows_edits_once_and_rejects_commands() {
        let (o, ok) = permission_outcome(
            PermissionMode::Ask,
            Some(&options()),
            &json!({"kind": "edit"}),
        );
        assert!(ok);
        assert_eq!(o["optionId"], "allow");
        let (o, ok) = permission_outcome(
            PermissionMode::Ask,
            Some(&options()),
            &json!({"kind": "execute"}),
        );
        assert!(!ok);
        assert_eq!(o["optionId"], "deny");
        let (o, ok) = permission_outcome(PermissionMode::Ask, None, &json!({"kind": "execute"}));
        assert!(!ok);
        assert_eq!(o["outcome"], "cancelled");
    }

    #[test]
    fn tool_names() {
        assert_eq!(
            tool_name(&json!({"kind": "execute", "title": "ls"})),
            "Bash"
        );
        assert_eq!(tool_name(&json!({"name": "bash"})), "Other");
        assert_eq!(tool_name(&json!({"title": "Write index.html"})), "Write");
        assert_eq!(tool_name(&json!({"kind": "todowrite"})), "TodoWrite");
        assert_eq!(tool_name(&json!({})), "Tool");
    }
}
