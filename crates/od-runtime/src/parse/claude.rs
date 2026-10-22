//! Claude Code `--output-format stream-json --verbose`, with or without
//! `--include-partial-messages` (spec 2.3; ports `claude-stream.ts`).
//!
//! With partial messages, text and tool input arrive as `stream_event`
//! deltas and the turn's `stop_reason` only on `message_delta`. Without
//! them, content arrives only on the `assistant` wrapper (one per content
//! block on 2.1.259) and the only turn boundary is the `result` frame.

use super::{StreamParser, normalize_task_status, raw_event, str_field, stringify_content};
use od_contracts::AgentEvent;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Debug, Default)]
struct Block {
    kind: Option<String>,
    name: Option<String>,
    id: Option<String>,
    /// Assembled `input_json_delta` text.
    input: String,
    /// `content_block_start.content_block.input`, if present.
    input_value: Option<Value>,
}

#[derive(Debug, Clone)]
struct Task {
    id: String,
    content: String,
    status: &'static str,
    active_form: Option<String>,
}

/// Parser state for one process's stdout.
#[derive(Debug, Default)]
pub struct ClaudeParser {
    /// Open content blocks keyed by `<message id>:<index>`.
    blocks: HashMap<String, Block>,
    /// Tool-use ids already emitted by either path (delta stop or wrapper):
    /// one call yields exactly one `tool_use`, whichever frame came first.
    emitted_tool_ids: HashSet<String>,
    current_message_id: Option<String>,
    text_streamed: HashSet<String>,
    thinking_streamed: HashSet<String>,
    current_streamed_text: bool,
    current_streamed_thinking: bool,
    /// Message ids whose `turn_end` was already announced.
    turn_end_emitted: HashSet<String>,
    /// Task rows in declaration order, keyed by a slot handle this parser
    /// owns (the runtime may rebind a task's id later).
    tasks: Vec<(String, Task)>,
    task_slot_by_id: HashMap<String, String>,
    pending_task_creates: HashMap<String, String>,
    pending_task_lists: HashSet<String>,
    canonical_task_tool_ids: HashSet<String>,
    next_task_id: u64,
}

static TASK_CREATED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bTask\s+#([A-Za-z0-9_-]+)\s+created successfully").expect("static regex")
});
static TASK_LIST_ROW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#([A-Za-z0-9_-]+)\s+\[([^\]]*)\]\s*(.*)$").expect("static regex")
});
static PROMPT_TOO_LONG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:API Error:\s*)?Prompt is too long\.?$").expect("static regex")
});

impl StreamParser for ClaudeParser {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(obj) if obj.is_object() => self.handle(&obj, out),
            Ok(_) => {}
            Err(_) => out.push(raw_event(line)),
        }
    }
}

impl ClaudeParser {
    fn handle(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) {
        let kind = str_field(obj, "type").unwrap_or_default();
        let subtype = str_field(obj, "subtype").unwrap_or_default();
        let parent = obj.get("parent_tool_use_id").filter(|v| !v.is_null());
        match (kind, subtype) {
            ("system", "init") => out.push(AgentEvent::Status {
                label: "initializing".to_owned(),
                model: str_field(obj, "model").map(str::to_owned),
                session_id: str_field(obj, "session_id").map(str::to_owned),
                ttft_ms: None,
                detail: None,
            }),
            ("system", "status") => out.push(AgentEvent::Status {
                label: str_field(obj, "status").unwrap_or("working").to_owned(),
                model: None,
                session_id: None,
                ttft_ms: None,
                detail: None,
            }),
            ("system", "thinking_tokens") => {
                if let Some(tokens) = obj
                    .get("estimated_tokens")
                    .and_then(Value::as_f64)
                    .filter(|t| t.is_finite() && *t > 0.0)
                {
                    out.push(AgentEvent::ThinkingTokens { tokens });
                }
            }
            ("stream_event", _) => {
                if let Some(ev) = obj.get("event").filter(|e| e.is_object()) {
                    // `parent_tool_use_id` rides on the outer envelope.
                    self.stream_event(ev, parent, out);
                }
            }
            ("assistant", _) => self.assistant(obj, parent, out),
            ("user", _) => self.user(obj, out),
            ("result", _) => Self::result(obj, out),
            _ => {}
        }
    }

    /// Announces a main-stream turn boundary once per message. Sub-agent
    /// frames (non-null `parent_tool_use_id`) never close the parent turn.
    fn turn_end_once(
        &mut self,
        message_id: Option<String>,
        stop_reason: &str,
        parent: Option<&Value>,
        out: &mut Vec<AgentEvent>,
    ) {
        if parent.is_some() {
            return;
        }
        if let Some(id) = message_id {
            if !self.turn_end_emitted.insert(id) {
                return;
            }
        }
        out.push(AgentEvent::TurnEnd {
            stop_reason: stop_reason.to_owned(),
        });
    }

    fn block_key(&self, index: Option<&Value>) -> String {
        let index = index.map_or_else(|| "null".to_owned(), Value::to_string);
        format!(
            "{}:{index}",
            self.current_message_id.as_deref().unwrap_or("anon")
        )
    }

    fn stream_event(&mut self, ev: &Value, parent: Option<&Value>, out: &mut Vec<AgentEvent>) {
        match str_field(ev, "type").unwrap_or_default() {
            "message_delta" => {
                if let Some(stop) = ev.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.turn_end_once(self.current_message_id.clone(), stop, parent, out);
                }
            }
            "message_start" => {
                self.current_message_id = ev
                    .pointer("/message/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.current_streamed_text = false;
                self.current_streamed_thinking = false;
                if let Some(ttft) = ev.get("ttft_ms").and_then(Value::as_f64) {
                    out.push(AgentEvent::Status {
                        label: "streaming".to_owned(),
                        model: None,
                        session_id: None,
                        ttft_ms: Some(ttft),
                        detail: None,
                    });
                }
            }
            "content_block_start" => {
                let Some(block) = ev.get("content_block").filter(|b| b.is_object()) else {
                    return;
                };
                let key = self.block_key(ev.get("index"));
                let kind = str_field(block, "type").map(str::to_owned);
                if kind.as_deref() == Some("thinking") {
                    out.push(AgentEvent::ThinkingStart);
                }
                self.blocks.insert(
                    key,
                    Block {
                        kind,
                        name: str_field(block, "name").map(str::to_owned),
                        id: str_field(block, "id").map(str::to_owned),
                        input: String::new(),
                        input_value: block.get("input").cloned(),
                    },
                );
            }
            "content_block_delta" => self.content_delta(ev, out),
            "content_block_stop" => {
                let key = self.block_key(ev.get("index"));
                let Some(state) = self.blocks.remove(&key) else {
                    return;
                };
                if state.kind.as_deref() != Some("tool_use") {
                    return;
                }
                let Some(id) = state.id else { return };
                if self.emitted_tool_ids.contains(&id) {
                    return;
                }
                let input = if state.input.trim().is_empty() {
                    state.input_value
                } else {
                    // Malformed streamed JSON: leave it to the wrapper.
                    serde_json::from_str::<Value>(&state.input).ok()
                };
                if let Some(input) = input {
                    self.emitted_tool_ids.insert(id.clone());
                    self.emit_tool_use(id, state.name, input, out);
                }
            }
            _ => {}
        }
    }

    fn content_delta(&mut self, ev: &Value, out: &mut Vec<AgentEvent>) {
        let Some(delta) = ev.get("delta") else { return };
        match str_field(delta, "type").unwrap_or_default() {
            "text_delta" => {
                if let Some(text) = str_field(delta, "text") {
                    if let Some(id) = &self.current_message_id {
                        self.text_streamed.insert(id.clone());
                    }
                    self.current_streamed_text = true;
                    if !text.is_empty() {
                        out.push(AgentEvent::TextDelta {
                            delta: text.to_owned(),
                        });
                    }
                }
            }
            "thinking_delta" => {
                if let Some(text) = str_field(delta, "thinking") {
                    // Only a delta with characters retires the wrapper
                    // fallback: Claude sends many empty thinking deltas.
                    if !text.is_empty() {
                        if let Some(id) = &self.current_message_id {
                            self.thinking_streamed.insert(id.clone());
                        }
                        self.current_streamed_thinking = true;
                        out.push(AgentEvent::ThinkingDelta {
                            delta: text.to_owned(),
                        });
                    }
                }
            }
            "input_json_delta" => {
                let key = self.block_key(ev.get("index"));
                let Some(partial) = str_field(delta, "partial_json") else {
                    return;
                };
                let Some(state) = self.blocks.get_mut(&key) else {
                    return;
                };
                if state.kind.as_deref() != Some("tool_use") {
                    return;
                }
                state.input.push_str(partial);
                if let (Some(id), Some(name)) = (&state.id, &state.name) {
                    out.push(AgentEvent::ToolInputDelta {
                        id: id.clone(),
                        name: name.clone(),
                        delta: partial.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }

    /// Input for a `tool_use` seen on the wrapper: the delta-assembled input
    /// wins when it parses (the wrapper often replays `{}`).
    fn wrapper_tool_input(&self, block: &Value) -> Value {
        if let Some(id) = str_field(block, "id") {
            let open = self
                .blocks
                .values()
                .find(|b| b.kind.as_deref() == Some("tool_use") && b.id.as_deref() == Some(id));
            if let Some(parsed) = open
                .filter(|b| !b.input.trim().is_empty())
                .and_then(|b| serde_json::from_str::<Value>(&b.input).ok())
            {
                return parsed;
            }
        }
        block.get("input").cloned().unwrap_or(Value::Null)
    }

    fn assistant(&mut self, obj: &Value, parent: Option<&Value>, out: &mut Vec<AgentEvent>) {
        let Some(message) = obj.get("message") else {
            return;
        };
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        let explicit_id = str_field(message, "id").map(str::to_owned);
        let text_msg_id = explicit_id.clone().or_else(|| {
            self.current_streamed_text
                .then(|| self.current_message_id.clone())
                .flatten()
        });
        let thinking_msg_id = explicit_id.clone().or_else(|| {
            self.current_streamed_thinking
                .then(|| self.current_message_id.clone())
                .flatten()
        });
        if let Some(id) = &explicit_id {
            self.current_message_id = Some(id.clone());
            if let Some(usage) = per_request_usage(message.get("usage")) {
                out.push(AgentEvent::RequestUsage {
                    request_id: id.clone(),
                    usage,
                });
            }
        }
        let text_done = text_msg_id
            .as_ref()
            .is_some_and(|id| self.text_streamed.contains(id));
        let thinking_done = thinking_msg_id
            .as_ref()
            .is_some_and(|id| self.thinking_streamed.contains(id));
        // Legacy boundary; null on 2.1.259. Emitted after the blocks so a
        // stdin-close decision never precedes this message's tool calls.
        let stop_reason = str_field(message, "stop_reason").map(str::to_owned);
        for block in content {
            match str_field(block, "type").unwrap_or_default() {
                "tool_use" => {
                    if let Some(id) = str_field(block, "id") {
                        if !self.emitted_tool_ids.insert(id.to_owned()) {
                            continue;
                        }
                    }
                    let input = self.wrapper_tool_input(block);
                    let id = str_field(block, "id").unwrap_or_default().to_owned();
                    let name = str_field(block, "name").map(str::to_owned);
                    self.emit_tool_use(id, name, input, out);
                }
                "text" if !text_done => {
                    if let Some(text) = str_field(block, "text").filter(|t| !t.is_empty()) {
                        out.push(AgentEvent::TextDelta {
                            delta: text.to_owned(),
                        });
                    }
                }
                "thinking" if !thinking_done => {
                    if let Some(text) = str_field(block, "thinking").filter(|t| !t.is_empty()) {
                        out.push(AgentEvent::ThinkingDelta {
                            delta: text.to_owned(),
                        });
                    }
                }
                _ => {}
            }
        }
        if let Some(stop) = stop_reason {
            let id = explicit_id.or_else(|| self.current_message_id.clone());
            self.turn_end_once(id, &stop, parent, out);
        }
        // A sub-agent's in-stream error must not fail the main turn.
        if let Some(code) = str_field(obj, "error").filter(|e| !e.trim().is_empty()) {
            if parent.is_none() {
                let text = assistant_text(content);
                out.push(AgentEvent::Error {
                    message: if text.is_empty() {
                        code.to_owned()
                    } else {
                        text
                    },
                    code: Some(code.to_owned()),
                    terminal: false,
                });
            }
        }
        self.current_streamed_text = false;
        self.current_streamed_thinking = false;
    }

    fn user(&mut self, obj: &Value, out: &mut Vec<AgentEvent>) {
        let Some(content) = obj.pointer("/message/content").and_then(Value::as_array) else {
            return;
        };
        for block in content {
            if str_field(block, "type") != Some("tool_result") {
                continue;
            }
            let tool_use_id = str_field(block, "tool_use_id")
                .unwrap_or_default()
                .to_owned();
            let content = stringify_content(block.get("content").unwrap_or(&Value::Null));
            let is_error = block
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            self.absorb_task_result(&tool_use_id, &content, is_error, out);
            out.push(AgentEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            });
        }
    }

    fn result(obj: &Value, out: &mut Vec<AgentEvent>) {
        let is_error = obj.get("is_error").and_then(Value::as_bool) == Some(true);
        let stop_reason = str_field(obj, "stop_reason")
            .filter(|s| !s.is_empty())
            .or_else(|| str_field(obj, "terminal_reason").filter(|s| !s.is_empty()))
            .map(str::to_owned);
        out.push(AgentEvent::Usage {
            usage: obj.get("usage").cloned(),
            cost_usd: obj.get("total_cost_usd").and_then(Value::as_f64),
            duration_ms: obj.get("duration_ms").and_then(Value::as_f64),
            stop_reason,
            is_error,
        });
        if is_error {
            let message = error_result_message(obj);
            let code = if PROMPT_TOO_LONG_RE.is_match(message.trim()) {
                "AGENT_PROMPT_TOO_LARGE".to_owned()
            } else {
                str_field(obj, "subtype")
                    .filter(|s| !s.is_empty())
                    .unwrap_or("result_error")
                    .to_owned()
            };
            out.push(AgentEvent::Error {
                message,
                code: Some(code),
                terminal: true,
            });
        }
    }

    fn emit_tool_use(
        &mut self,
        id: String,
        name: Option<String>,
        input: Value,
        out: &mut Vec<AgentEvent>,
    ) {
        if self.task_snapshot(&id, name.as_deref(), &input, out) {
            return;
        }
        out.push(AgentEvent::ToolUse {
            id,
            name: name.unwrap_or_default(),
            input,
        });
    }

    // ---- TaskCreate / TaskUpdate / TaskList → one TodoWrite snapshot ----

    fn next_generated_task_id(&mut self) -> String {
        loop {
            self.next_task_id += 1;
            let id = self.next_task_id.to_string();
            if !self.task_slot_by_id.contains_key(&id) {
                return id;
            }
        }
    }

    fn note_task_id(&mut self, id: &str) {
        if let Ok(n) = id.parse::<u64>() {
            self.next_task_id = self.next_task_id.max(n);
        }
    }

    fn task_mut(&mut self, slot: &str) -> Option<&mut Task> {
        self.tasks
            .iter_mut()
            .find(|(s, _)| s == slot)
            .map(|(_, t)| t)
    }

    fn upsert_task(&mut self, slot: String, task: Task) {
        match self.tasks.iter_mut().find(|(s, _)| *s == slot) {
            // Same slot keeps its position, so the plan keeps its order.
            Some((_, existing)) => *existing = task,
            None => self.tasks.push((slot, task)),
        }
    }

    fn emit_task_snapshot(&self, tool_use_id: &str, out: &mut Vec<AgentEvent>) {
        let todos: Vec<Value> = self
            .tasks
            .iter()
            .map(|(_, t)| {
                let mut row = Map::new();
                row.insert("content".to_owned(), json!(t.content));
                row.insert("status".to_owned(), json!(t.status));
                if let Some(af) = &t.active_form {
                    row.insert("activeForm".to_owned(), json!(af));
                }
                Value::Object(row)
            })
            .collect();
        out.push(AgentEvent::ToolUse {
            id: format!("{tool_use_id}:todo-task"),
            name: "TodoWrite".to_owned(),
            input: json!({ "todos": todos }),
        });
    }

    /// Folds `TaskCreate`/`TaskUpdate` into a `TodoWrite` snapshot. Returns
    /// true when the call was consumed.
    fn task_snapshot(
        &mut self,
        tool_use_id: &str,
        name: Option<&str>,
        input: &Value,
        out: &mut Vec<AgentEvent>,
    ) -> bool {
        let Some(name) = name else { return false };
        if name == "TaskList" {
            self.pending_task_lists.insert(tool_use_id.to_owned());
            return false;
        }
        if !input.is_object() || !matches!(name, "TaskCreate" | "TaskUpdate") {
            return false;
        }
        if self.canonical_task_tool_ids.contains(tool_use_id) {
            return true;
        }
        let subject = str_field(input, "subject")
            .or_else(|| str_field(input, "description"))
            .map(str::to_owned);
        let active_form = str_field(input, "activeForm").map(str::to_owned);
        let status = normalize_task_status(str_field(input, "status"));
        if name == "TaskCreate" {
            let Some(content) = subject.filter(|s| !s.is_empty()) else {
                return false;
            };
            let id = match str_field(input, "taskId").filter(|s| !s.is_empty()) {
                Some(id) => {
                    self.note_task_id(id);
                    id.to_owned()
                }
                None => self.next_generated_task_id(),
            };
            let slot = format!("create:{tool_use_id}");
            self.upsert_task(
                slot.clone(),
                Task {
                    id: id.clone(),
                    content,
                    status,
                    active_form,
                },
            );
            self.task_slot_by_id.insert(id, slot.clone());
            self.pending_task_creates
                .insert(tool_use_id.to_owned(), slot);
        } else {
            // An id this stream never saw belongs to an earlier run of the
            // resumed session; never guess which row it meant.
            let Some(slot) = str_field(input, "taskId")
                .and_then(|id| self.task_slot_by_id.get(id))
                .cloned()
            else {
                return false;
            };
            let Some(task) = self.task_mut(&slot) else {
                return false;
            };
            if let Some(content) = subject {
                task.content = content;
            }
            task.status = status;
            if active_form.is_some() {
                task.active_form = active_form;
            }
        }
        self.canonical_task_tool_ids.insert(tool_use_id.to_owned());
        if self.tasks.is_empty() {
            return false;
        }
        self.emit_task_snapshot(tool_use_id, out);
        true
    }

    fn absorb_task_result(
        &mut self,
        tool_use_id: &str,
        content: &str,
        is_error: bool,
        out: &mut Vec<AgentEvent>,
    ) {
        if let Some(slot) = self.pending_task_creates.remove(tool_use_id) {
            if is_error {
                return;
            }
            let Some(created) = TASK_CREATED_RE
                .captures(content)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_owned())
            else {
                return;
            };
            self.note_task_id(&created);
            if let Some(task) = self.task_mut(&slot) {
                let old = std::mem::replace(&mut task.id, created.clone());
                if self.task_slot_by_id.get(&old) == Some(&slot) {
                    self.task_slot_by_id.remove(&old);
                }
            }
            self.task_slot_by_id.insert(created, slot);
            return;
        }
        if !self.pending_task_lists.remove(tool_use_id) || is_error {
            return;
        }
        let mut changed = false;
        for line in content.lines() {
            let Some(row) = TASK_LIST_ROW_RE.captures(line.trim()) else {
                continue;
            };
            let (id, status, subject) = (&row[1], &row[2], row[3].trim());
            let slot = self
                .task_slot_by_id
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("task:{id}"));
            let existing = self.tasks.iter().find(|(s, _)| *s == slot).map(|(_, t)| t);
            let content = if subject.is_empty() {
                existing.map(|t| t.content.clone()).unwrap_or_default()
            } else {
                subject.to_owned()
            };
            if content.is_empty() {
                continue;
            }
            let status = normalize_task_status(Some(status.trim()));
            if existing.is_some_and(|t| t.content == content && t.status == status) {
                continue;
            }
            let active_form = existing.and_then(|t| t.active_form.clone());
            self.upsert_task(
                slot.clone(),
                Task {
                    id: id.to_owned(),
                    content,
                    status,
                    active_form,
                },
            );
            self.task_slot_by_id.insert(id.to_owned(), slot);
            self.note_task_id(id);
            changed = true;
        }
        if changed {
            self.emit_task_snapshot(tool_use_id, out);
        }
    }
}

fn per_request_usage(usage: Option<&Value>) -> Option<Value> {
    let usage = usage?.as_object()?;
    let mut out = Map::new();
    for key in [
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ] {
        if let Some(n) = usage.get(key).filter(|v| v.is_number()) {
            out.insert(key.to_owned(), n.clone());
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn assistant_text(content: &[Value]) -> String {
    content
        .iter()
        .filter(|b| str_field(b, "type") == Some("text"))
        .filter_map(|b| str_field(b, "text"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn error_result_message(obj: &Value) -> String {
    if let Some(errors) = obj.get("errors").and_then(Value::as_array) {
        let parts: Vec<&str> = errors
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty())
            .collect();
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    if let Some(result) = str_field(obj, "result").filter(|r| !r.trim().is_empty()) {
        return result.to_owned();
    }
    match str_field(obj, "subtype").filter(|s| !s.is_empty()) {
        Some(subtype) => format!("Claude run failed: {subtype}"),
        None => "Claude run failed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_all;

    fn run(lines: &[Value]) -> Vec<AgentEvent> {
        let text = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        parse_all(&mut ClaudeParser::default(), &text)
    }

    #[test]
    fn task_tools_fold_into_todo_snapshots() {
        let events = run(&[
            json!({"type":"assistant","message":{"id":"m1","content":[
                {"type":"tool_use","id":"t1","name":"TaskCreate","input":{"subject":"Draft copy"}},
                {"type":"tool_use","id":"t2","name":"TaskCreate","input":{"subject":"Ship"}}
            ]}}),
            json!({"type":"user","message":{"content":[
                {"type":"tool_result","tool_use_id":"t1","content":"Task #7 created successfully: Draft copy"}
            ]}}),
            json!({"type":"assistant","message":{"id":"m2","content":[
                {"type":"tool_use","id":"t3","name":"TaskUpdate","input":{"taskId":"7","status":"done"}},
                {"type":"tool_use","id":"t4","name":"TaskUpdate","input":{"taskId":"99","status":"done"}}
            ]}}),
        ]);
        let todos: Vec<&Value> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolUse { name, input, .. } if name == "TodoWrite" => Some(input),
                _ => None,
            })
            .collect();
        assert_eq!(todos.len(), 3);
        assert_eq!(
            todos[2],
            &json!({"todos":[
                {"content":"Draft copy","status":"completed"},
                {"content":"Ship","status":"pending"}
            ]})
        );
        // The unknown task id passes through as an ordinary tool call.
        assert!(events.iter().any(
            |e| matches!(e, AgentEvent::ToolUse { id, name, .. } if id == "t4" && name == "TaskUpdate")
        ));
    }

    #[test]
    fn error_result_is_usage_plus_terminal_error() {
        let events = run(&[json!({
            "type":"result","subtype":"error_during_execution","is_error":true,
            "errors":["No conversation found with session ID: abc"]
        })]);
        assert!(matches!(
            &events[0],
            AgentEvent::Usage { is_error: true, .. }
        ));
        assert!(matches!(
            &events[1],
            AgentEvent::Error { terminal: true, message, .. } if message.contains("No conversation found")
        ));
    }

    #[test]
    fn tool_use_seen_by_block_stop_first_is_not_repeated_by_the_wrapper() {
        // Recorded 2.1.259 streams send the wrapper first; older builds sent
        // `content_block_stop` first. Either order yields one `tool_use`.
        let events = run(&[
            json!({"type":"stream_event","event":{"type":"message_start","message":{"id":"m1"}}}),
            json!({"type":"stream_event","event":{"type":"content_block_start","index":0,
                   "content_block":{"type":"tool_use","id":"t1","name":"Write","input":{}}}}),
            json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,
                   "delta":{"type":"input_json_delta","partial_json":"{\"file_path\":\"a.html\"}"}}}),
            json!({"type":"stream_event","event":{"type":"content_block_stop","index":0}}),
            json!({"type":"assistant","message":{"id":"m1","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{}}]}}),
        ]);
        let uses: Vec<&AgentEvent> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolUse { .. }))
            .collect();
        assert_eq!(uses.len(), 1);
        assert!(
            matches!(uses[0], AgentEvent::ToolUse { input, .. } if input["file_path"] == "a.html")
        );
    }

    #[test]
    fn sub_agent_error_does_not_fail_the_main_turn() {
        let frame = |parent: Value| {
            json!({"type":"assistant","error":"overloaded","parent_tool_use_id":parent,
                   "message":{"id":"m","content":[{"type":"text","text":"Overloaded"}]}})
        };
        let child = run(&[frame(json!("toolu_1"))]);
        assert!(!child.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
        let main = run(&[frame(Value::Null)]);
        assert!(
            main.iter()
                .any(|e| matches!(e, AgentEvent::Error { message, .. } if message == "Overloaded"))
        );
    }

    #[test]
    fn unparseable_line_is_bounded_raw() {
        let long = "x".repeat(10_000);
        let events = parse_all(&mut ClaudeParser::default(), &long);
        assert!(
            matches!(&events[0], AgentEvent::Raw { line, truncated: true } if line.len() == 4096)
        );
    }
}
