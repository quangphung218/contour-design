//! Chat transcript state: folds run frames into what an assistant turn shows
//! (text, thinking, tool rows, notices). Pure data, tested natively.

use crate::events::{AgentEvent, Message, RunFrame, RunStatus};
use serde_json::Value;

/// Longest tool input summary shown on a tool row.
const SUMMARY_CHARS: usize = 120;

/// Where an assistant turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    /// Request sent, no `start` yet.
    Pending,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl TurnState {
    #[must_use]
    pub fn is_live(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }
}

impl From<RunStatus> for TurnState {
    fn from(s: RunStatus) -> Self {
        match s {
            RunStatus::Running => Self::Running,
            RunStatus::Succeeded => Self::Succeeded,
            RunStatus::Failed => Self::Failed,
            RunStatus::Canceled => Self::Canceled,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// One-line description of the input (file path, command…).
    pub summary: String,
    /// `Some(is_error)` once the result arrived.
    pub result: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(String),
    Thinking(String),
    Tool(ToolCall),
    Error(String),
}

/// One assistant turn, built from its run's frames.
#[derive(Debug, Clone, PartialEq)]
pub struct AssistantTurn {
    pub blocks: Vec<Block>,
    pub state: TurnState,
    pub run_id: Option<String>,
    pub model: Option<String>,
    /// Latest `status` label while running ("thinking", "writing"…).
    pub status: Option<String>,
}

impl Default for AssistantTurn {
    fn default() -> Self {
        Self::new()
    }
}

impl AssistantTurn {
    #[must_use]
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            state: TurnState::Pending,
            run_id: None,
            model: None,
            status: None,
        }
    }

    /// Rebuilds a finished (or still running) turn from a stored message.
    #[must_use]
    pub fn from_stored(msg: &Message) -> Self {
        let mut turn = Self::new();
        turn.run_id.clone_from(&msg.run_id);
        for raw in &msg.events {
            if let Ok(ev) = serde_json::from_value::<AgentEvent>(raw.clone()) {
                turn.apply(RunFrame::Agent(ev));
            }
        }
        if turn.blocks.is_empty() && !msg.content.is_empty() {
            turn.blocks.push(Block::Text(msg.content.clone()));
        }
        turn.state = msg.run_status.map_or(TurnState::Succeeded, TurnState::from);
        turn
    }

    /// Folds one frame into the turn.
    pub fn apply(&mut self, frame: RunFrame) {
        match frame {
            RunFrame::Start(start) => {
                self.state = TurnState::Running;
                self.run_id = Some(start.run_id);
                self.model = start.model;
            }
            RunFrame::Agent(ev) => self.apply_agent(ev),
            RunFrame::Error(message) => self.blocks.push(Block::Error(message)),
            RunFrame::End(end) => {
                self.state = end.status.into();
                self.status = None;
                if let Some(error) = end.error.filter(|e| !e.is_empty()) {
                    let already = self
                        .blocks
                        .iter()
                        .any(|b| matches!(b, Block::Error(m) if *m == error));
                    if !already {
                        self.blocks.push(Block::Error(error));
                    }
                }
            }
            RunFrame::Ignored => {}
        }
    }

    fn apply_agent(&mut self, ev: AgentEvent) {
        if self.state == TurnState::Pending {
            self.state = TurnState::Running;
        }
        match ev {
            AgentEvent::Status { label, model, .. } => {
                if !label.is_empty() {
                    self.status = Some(label);
                }
                if model.is_some() {
                    self.model = model;
                }
            }
            AgentEvent::TextDelta { delta } => match self.blocks.last_mut() {
                Some(Block::Text(text)) => text.push_str(&delta),
                _ => self.blocks.push(Block::Text(delta)),
            },
            AgentEvent::ThinkingStart => self.blocks.push(Block::Thinking(String::new())),
            AgentEvent::ThinkingDelta { delta } => match self.blocks.last_mut() {
                Some(Block::Thinking(text)) => text.push_str(&delta),
                _ => self.blocks.push(Block::Thinking(delta)),
            },
            AgentEvent::ToolUse { id, name, input } => {
                let summary = summarize_input(&name, &input);
                self.blocks.push(Block::Tool(ToolCall {
                    id,
                    name,
                    summary,
                    result: None,
                }));
            }
            AgentEvent::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                let call = self.blocks.iter_mut().rev().find_map(|b| match b {
                    Block::Tool(c) if c.id == tool_use_id => Some(c),
                    _ => None,
                });
                if let Some(call) = call {
                    call.result = Some(is_error);
                }
            }
            AgentEvent::Error { message, .. } => self.blocks.push(Block::Error(message)),
            AgentEvent::ToolLoop {
                tool_name,
                count,
                stopped: true,
                ..
            } => self.blocks.push(Block::Error(format!(
                "Stopped: {tool_name} failed {count} times in a row"
            ))),
            // Usage, turn ends, raw lines, loop warnings: nothing to show.
            _ => {}
        }
    }

    /// All assistant prose, concatenated (for question forms and copy).
    #[must_use]
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Project files the agent wrote or edited in this turn.
    #[must_use]
    pub fn touched_files(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for b in &self.blocks {
            if let Block::Tool(c) = b {
                if is_write_tool(&c.name) && !c.summary.is_empty() && !out.contains(&c.summary) {
                    out.push(c.summary.clone());
                }
            }
        }
        out
    }
}

fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "Write" | "Edit" | "MultiEdit" | "write_file" | "edit_file"
    )
}

/// Picks the most telling field of a tool input for a one-line row.
#[must_use]
pub fn summarize_input(name: &str, input: &Value) -> String {
    const KEYS: [&str; 7] = [
        "file_path",
        "path",
        "command",
        "pattern",
        "url",
        "query",
        "description",
    ];
    let pick = KEYS
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .map(str::to_owned)
        .or_else(|| match input {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let line = pick.lines().next().unwrap_or_default().trim();
    let line = if is_write_tool(name) {
        strip_cwd(line)
    } else {
        line
    };
    truncate(line, SUMMARY_CHARS)
}

/// Agents report absolute paths; the project-relative tail is what matters.
fn strip_cwd(path: &str) -> &str {
    path.rsplit_once("/projects/")
        .and_then(|(_, rest)| rest.split_once('/').map(|(_, rel)| rel))
        .unwrap_or(path)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::decode_frame;
    use serde_json::json;

    fn agent(v: Value) -> RunFrame {
        decode_frame("agent", &v.to_string())
    }

    #[test]
    fn text_deltas_merge_and_tools_split_them() {
        let mut t = AssistantTurn::new();
        t.apply(decode_frame(
            "start",
            r#"{"runId":"r1","agentId":"claude","bin":"claude","protocolVersion":1,"model":"m"}"#,
        ));
        assert_eq!(t.state, TurnState::Running);
        t.apply(agent(json!({"type": "text_delta", "delta": "Hel"})));
        t.apply(agent(json!({"type": "text_delta", "delta": "lo"})));
        t.apply(agent(
            json!({"type": "tool_use", "id": "t1", "name": "Write",
            "input": {"file_path": "/data/projects/p1/index.html", "content": "<h1>"}}),
        ));
        t.apply(agent(
            json!({"type": "tool_result", "toolUseId": "t1", "content": "ok", "isError": false}),
        ));
        t.apply(agent(json!({"type": "text_delta", "delta": " done"})));
        assert_eq!(
            t.blocks,
            vec![
                Block::Text("Hello".into()),
                Block::Tool(ToolCall {
                    id: "t1".into(),
                    name: "Write".into(),
                    summary: "index.html".into(),
                    result: Some(false),
                }),
                Block::Text(" done".into()),
            ]
        );
        assert_eq!(t.text(), "Hello done");
        assert_eq!(t.touched_files(), vec!["index.html".to_owned()]);
    }

    #[test]
    fn thinking_blocks_collect_deltas() {
        let mut t = AssistantTurn::new();
        t.apply(agent(json!({"type": "thinking_start"})));
        t.apply(agent(json!({"type": "thinking_delta", "delta": "a"})));
        t.apply(agent(json!({"type": "thinking_delta", "delta": "b"})));
        assert_eq!(t.blocks, vec![Block::Thinking("ab".into())]);
        assert_eq!(
            t.state,
            TurnState::Running,
            "first agent event implies running"
        );
    }

    #[test]
    fn end_sets_state_and_reports_error_once() {
        let mut t = AssistantTurn::new();
        t.apply(RunFrame::Error("agent crashed".into()));
        t.apply(decode_frame(
            "end",
            r#"{"status":"failed","code":1,"terminalAt":1,"error":"agent crashed"}"#,
        ));
        assert_eq!(t.state, TurnState::Failed);
        assert_eq!(t.blocks, vec![Block::Error("agent crashed".into())]);
        assert!(!t.state.is_live());
    }

    #[test]
    fn rebuilds_from_stored_message() {
        let msg: Message = serde_json::from_value(json!({
            "id": "m2", "conversationId": "c1", "role": "assistant", "content": "",
            "position": 1, "createdAt": 1,
            "runId": "r9", "runStatus": "canceled",
            "events": [
                {"type": "text_delta", "delta": "partial"},
                {"type": "not_a_type"}
            ]
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let t = AssistantTurn::from_stored(&msg);
        assert_eq!(t.blocks, vec![Block::Text("partial".into())]);
        assert_eq!(t.state, TurnState::Canceled);
        assert_eq!(t.run_id.as_deref(), Some("r9"));
    }

    #[test]
    fn summaries() {
        assert_eq!(
            summarize_input("Bash", &json!({"command": "ls -la\nrm x"})),
            "ls -la"
        );
        assert_eq!(summarize_input("Grep", &json!({"pattern": "foo"})), "foo");
        assert_eq!(summarize_input("X", &json!({"n": 1})), "");
        let long = "a".repeat(300);
        assert_eq!(
            summarize_input("Bash", &json!({ "command": long }))
                .chars()
                .count(),
            SUMMARY_CHARS
        );
    }
}
