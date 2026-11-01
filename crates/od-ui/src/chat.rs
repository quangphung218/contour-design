//! Chat transcript state: folds run frames into what an assistant turn shows
//! (text, thinking, tool rows, notices). Pure data, tested natively.

use crate::events::{AgentEvent, Message, RunFrame, RunStatus};
use od_contracts::CritiqueReport;
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
    /// Full file path of a write tool, as the agent sent it.
    pub path: Option<String>,
    /// `Some(is_error)` once the result arrived.
    pub result: Option<bool>,
    /// Files a folder row of "Files from this turn" stands for, or the
    /// count of a `+N more` row (`files` in the input).
    pub files: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(String),
    Thinking(String),
    Tool(ToolCall),
    Error(String),
    /// A message the user pushed into the running turn (steer).
    Steered(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// One step of the agent's plan (Claude's `TodoWrite`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub content: String,
    /// Present-tense label shown while the step runs.
    pub active_form: String,
    pub status: TodoStatus,
}

/// Reads a `TodoWrite` input: `{"todos": [{content, status, activeForm}]}`.
#[must_use]
pub fn parse_todos(input: &Value) -> Option<Vec<Todo>> {
    let list = input.get("todos")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|t| {
                let content = t.get("content")?.as_str()?.trim().to_owned();
                let status = match t.get("status").and_then(Value::as_str) {
                    Some("completed") => TodoStatus::Completed,
                    Some("in_progress") => TodoStatus::InProgress,
                    _ => TodoStatus::Pending,
                };
                let active_form = t
                    .get("activeForm")
                    .and_then(Value::as_str)
                    .map_or_else(|| content.clone(), |s| s.trim().to_owned());
                Some(Todo {
                    content,
                    active_form,
                    status,
                })
            })
            .collect(),
    )
}

/// "Step N/M" for the plan pill: N is the step in progress, else the next
/// one to start (M when all are done). `None` without a plan.
#[must_use]
pub fn plan_progress(todos: &[Todo]) -> Option<(usize, usize, &Todo)> {
    let total = todos.len();
    let last = todos.last()?;
    if let Some(i) = todos
        .iter()
        .position(|t| t.status == TodoStatus::InProgress)
    {
        return Some((i + 1, total, &todos[i]));
    }
    match todos.iter().position(|t| t.status != TodoStatus::Completed) {
        Some(i) => Some((i + 1, total, &todos[i])),
        None => Some((total, total, last)),
    }
}

/// The transcript once a conversation's stored messages arrive: the stored
/// ones, then what was added locally while they loaded (a message sent at
/// once). A local item wins over its stored copy (same id: the client names
/// the messages of a run), as it holds the live turn.
#[must_use]
pub fn merge_loaded<T>(stored: Vec<T>, local: Vec<T>, id: impl Fn(&T) -> &str) -> Vec<T> {
    let mut out: Vec<T> = stored
        .into_iter()
        .filter(|s| !local.iter().any(|l| id(l) == id(s)))
        .collect();
    out.extend(local);
    out
}

/// Assistant prose without the Design Jury protocol, which the theater
/// shows instead (`<CRITIQUE_RUN>` to its end, or to the end of the text
/// while it still streams).
#[must_use]
pub fn without_jury_protocol(text: &str) -> std::borrow::Cow<'_, str> {
    const OPEN: &str = "<CRITIQUE_RUN";
    const CLOSE: &str = "</CRITIQUE_RUN>";
    let Some(start) = text.find(OPEN) else {
        return text.into();
    };
    let end = text[start..]
        .find(CLOSE)
        .map_or(text.len(), |i| start + i + CLOSE.len());
    format!("{}{}", &text[..start], &text[end..]).into()
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
    /// The latest plan the agent wrote with `TodoWrite`.
    pub todos: Vec<Todo>,
    /// The run's project folder (from `start`), to make written paths
    /// project-relative.
    pub cwd: Option<String>,
    /// Latest Design Jury state, when the run had the jury on.
    pub critique: Option<CritiqueReport>,
    /// Media the agent generated during the run (track C2), in start order.
    pub media: Vec<od_contracts::MediaTask>,
    /// Why the run failed, from its `end` (or its log, for a stored turn).
    pub failure: Option<od_contracts::FailureCategory>,
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
            todos: Vec::new(),
            cwd: None,
            critique: None,
            media: Vec::new(),
            failure: None,
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
                self.cwd = start.cwd;
            }
            RunFrame::Agent(ev) => self.apply_agent(ev),
            RunFrame::Error(message) => self.blocks.push(Block::Error(message)),
            RunFrame::End(end) => {
                self.state = end.status.into();
                self.status = None;
                self.failure = end.failure_category;
                if let Some(error) = crate::events::end_error(&end) {
                    self.push_error(error);
                }
            }
            RunFrame::Ignored => {}
        }
    }

    /// Adds an error block unless the same one is already shown.
    fn push_error(&mut self, error: String) {
        if !self
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Error(m) if *m == error))
        {
            self.blocks.push(Block::Error(error));
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
                if name == "TodoWrite" {
                    if let Some(todos) = parse_todos(&input) {
                        self.todos = todos;
                    }
                }
                let summary = summarize_input(&name, &input);
                let path = is_write_tool(&name)
                    .then(|| {
                        ["file_path", "path"]
                            .iter()
                            .find_map(|k| input.get(*k)?.as_str())
                    })
                    .flatten()
                    .map(str::to_owned);
                let files = input.get("files").and_then(Value::as_u64);
                self.blocks.push(Block::Tool(ToolCall {
                    id,
                    name,
                    summary,
                    path,
                    result: None,
                    files,
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
            AgentEvent::Error { code, .. }
                if code.as_deref() == Some(od_contracts::DAEMON_RESTART_CODE) =>
            {
                self.failure = Some(od_contracts::FailureCategory::DaemonRestart);
                self.push_error(crate::events::app_closed());
            }
            AgentEvent::Error { message, .. } => self.blocks.push(Block::Error(message)),
            AgentEvent::ToolLoop {
                tool_name,
                count,
                stopped: true,
                ..
            } => self.blocks.push(Block::Error(crate::i18n::tf(
                crate::i18n::T::ToolLoopStopped,
                &[&tool_name, &count.to_string()],
            ))),
            AgentEvent::Critique { report } => self.critique = Some(report),
            AgentEvent::Media { task } => {
                match self.media.iter_mut().find(|m| m.task_id == task.task_id) {
                    Some(slot) => *slot = task,
                    None => self.media.push(task),
                }
            }
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

    /// Project files (relative paths) the agent wrote or edited in this
    /// turn. Built from the full tool paths and the run's cwd, never from
    /// the truncated row summary; writes outside the project are left out.
    #[must_use]
    pub fn touched_files(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for b in &self.blocks {
            let Block::Tool(c) = b else { continue };
            if !is_write_tool(&c.name) {
                continue;
            }
            let Some(rel) = c
                .path
                .as_deref()
                .and_then(|p| project_relative(p, self.cwd.as_deref()))
            else {
                continue;
            };
            if !out.contains(&rel) {
                out.push(rel);
            }
        }
        out
    }

    /// Files this turn wrote or edited, project-relative, in first-touch
    /// order. A file written and then edited counts as written.
    #[must_use]
    pub fn file_ops(&self) -> Vec<(String, FileOp)> {
        let mut out: Vec<(String, FileOp)> = Vec::new();
        for b in &self.blocks {
            let Block::Tool(c) = b else { continue };
            let op = match c.name.as_str() {
                "Write" | "write_file" => FileOp::Write,
                "Edit" | "MultiEdit" | "edit_file" => FileOp::Edit,
                _ => continue,
            };
            let Some(rel) = c
                .path
                .as_deref()
                .and_then(|p| project_relative(p, self.cwd.as_deref()))
            else {
                continue;
            };
            if !out.iter().any(|(p, _)| *p == rel) {
                out.push((rel, op));
            }
        }
        out
    }

    /// Files the folder row `rel` (`frames/`) of [`Self::file_ops`] stands
    /// for; `None` for a file.
    #[must_use]
    pub fn folder_files(&self, rel: &str) -> Option<u64> {
        self.blocks.iter().find_map(|b| match b {
            Block::Tool(c)
                if c.files.is_some()
                    && c.path
                        .as_deref()
                        .and_then(|p| project_relative(p, self.cwd.as_deref()))
                        .as_deref()
                        == Some(rel) =>
            {
                c.files
            }
            _ => None,
        })
    }

    /// Files changed outside the file tools that the turn did not list
    /// (the `+N more` row the engine adds past its row limit).
    #[must_use]
    pub fn unlisted_files(&self) -> u64 {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Tool(c) if c.name == MORE_FILES_TOOL => c.files,
                _ => None,
            })
            .sum()
    }
}

/// Tool name of the engine's `+N more` row (`od_runtime::turn_files::MORE_TOOL`).
pub const MORE_FILES_TOOL: &str = "TurnFilesMore";

/// How a turn touched a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    Write,
    Edit,
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

/// `path` relative to the project folder `cwd`; `None` when it lies outside.
/// Without a cwd (a stored turn) only the `…/projects/<id>/` layout is known.
fn project_relative(path: &str, cwd: Option<&str>) -> Option<String> {
    // Windows paths (`C:\p\a.mp4`) compare as `C:/p/a.mp4`.
    let path = path.trim().replace('\\', "/");
    let cwd = cwd.map(|c| c.replace('\\', "/"));
    let path = path.as_str();
    let cwd = cwd.as_deref();
    let is_drive = |p: &str| {
        let b = p.as_bytes();
        b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
    };
    let rel = if path.starts_with('/') || is_drive(path) {
        match cwd.map(|c| c.trim_end_matches('/')) {
            Some(cwd) => {
                // macOS: the daemon may name `/var/…` while the agent
                // reports the real `/private/var/…` (or the other way round).
                let other = cwd
                    .strip_prefix("/private")
                    .map_or_else(|| format!("/private{cwd}"), str::to_owned);
                [cwd, other.as_str()]
                    .iter()
                    .find_map(|root| path.strip_prefix(root)?.strip_prefix('/'))?
            }
            None => {
                let rel = strip_cwd(path);
                (rel != path).then_some(rel)?
            }
        }
    } else {
        path
    };
    let rel = rel.trim_start_matches("./");
    (!rel.is_empty()).then(|| rel.to_owned())
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
    fn windows_paths_are_project_relative() {
        assert_eq!(
            project_relative("C:\\p\\renders\\clip.mp4", Some("C:\\p")).as_deref(),
            Some("renders/clip.mp4")
        );
        assert_eq!(project_relative("D:\\other\\x.mp4", Some("C:\\p")), None);
        assert_eq!(
            project_relative("/u/p/a/b.html", Some("/u/p")).as_deref(),
            Some("a/b.html")
        );
    }

    #[test]
    fn a_message_sent_while_the_transcript_loads_is_kept() {
        fn id<'a>(s: &'a (&str, &str)) -> &'a str {
            s.0
        }
        // Nothing sent meanwhile: the stored transcript as is.
        let stored = vec![("u1", "stored"), ("a1", "stored")];
        assert_eq!(merge_loaded(stored.clone(), vec![], id), stored);
        // Sent meanwhile, not stored yet: kept after the stored ones.
        let local = vec![("u2", "local"), ("a2", "local")];
        assert_eq!(
            merge_loaded(stored.clone(), local.clone(), id),
            vec![
                ("u1", "stored"),
                ("a1", "stored"),
                ("u2", "local"),
                ("a2", "local")
            ]
        );
        // Already stored too: once, the local (live) copy.
        let both = vec![("u2", "stored"), ("a2", "stored")];
        assert_eq!(merge_loaded(both, local.clone(), id), local);
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
                    path: Some("/data/projects/p1/index.html".into()),
                    result: Some(false),
                    files: None,
                }),
                Block::Text(" done".into()),
            ]
        );
        assert_eq!(t.text(), "Hello done");
        assert_eq!(t.touched_files(), vec!["index.html".to_owned()]);
    }

    #[test]
    fn folder_rows_and_the_more_row_carry_their_counts() {
        let mut t = AssistantTurn::new();
        t.apply(decode_frame(
            "start",
            r#"{"runId":"r1","agentId":"claude","bin":"claude","protocolVersion":1,"cwd":"/p"}"#,
        ));
        t.apply(agent(
            json!({"type": "tool_use", "id": "a", "name": "Write",
            "input": {"file_path": "/p/frames/", "files": 1000}}),
        ));
        t.apply(agent(
            json!({"type": "tool_use", "id": "b", "name": "Write",
            "input": {"file_path": "/p/clip.mp4"}}),
        ));
        t.apply(agent(
            json!({"type": "tool_use", "id": "c", "name": MORE_FILES_TOOL,
            "input": {"files": 70, "description": "+70 more"}}),
        ));
        assert_eq!(
            t.file_ops(),
            [
                ("frames/".to_owned(), FileOp::Write),
                ("clip.mp4".to_owned(), FileOp::Write)
            ]
        );
        assert_eq!(t.folder_files("frames/"), Some(1000));
        assert_eq!(t.folder_files("clip.mp4"), None);
        assert_eq!(t.unlisted_files(), 70);
    }

    #[test]
    fn touched_files_are_relative_to_the_run_cwd_and_never_truncated() {
        let write = |path: &str| {
            agent(json!({"type": "tool_use", "id": path, "name": "Write",
                "input": {"file_path": path, "content": "x"}}))
        };
        let start = |cwd: &str| {
            decode_frame(
                "start",
                &json!({"runId": "r", "agentId": "claude", "bin": "claude",
                    "protocolVersion": 1, "cwd": cwd})
                .to_string(),
            )
        };
        let long = format!("{}/index.html", "a".repeat(130));
        let mut t = AssistantTurn::new();
        t.apply(start("/Users/me/projects/site"));
        t.apply(write("/Users/me/projects/site/pages/about.html"));
        t.apply(write(&format!("/Users/me/projects/site/{long}")));
        t.apply(write("notes/plan.md"));
        t.apply(write("/Users/me/.zshrc"));
        assert_eq!(
            t.touched_files(),
            vec![
                "pages/about.html".to_owned(),
                long,
                "notes/plan.md".to_owned()
            ]
        );

        let mut t = AssistantTurn::new();
        t.apply(start("/Users/me/site"));
        t.apply(write("/Users/me/site/index.html"));
        assert_eq!(t.touched_files(), vec!["index.html".to_owned()]);

        let mut t = AssistantTurn::new();
        t.apply(start("/var/folders/x/projects/p1"));
        t.apply(write("/private/var/folders/x/projects/p1/index.html"));
        t.apply(write("/var/folders/x/projects/p1/a.html"));
        assert_eq!(
            t.touched_files(),
            vec!["index.html".to_owned(), "a.html".to_owned()]
        );
    }

    #[test]
    fn todo_write_drives_the_plan_pill() {
        let mut t = AssistantTurn::new();
        assert!(plan_progress(&t.todos).is_none());
        let plan = |a: &str, b: &str, c: &str| {
            agent(
                json!({"type": "tool_use", "id": "p", "name": "TodoWrite", "input": {"todos": [
                    {"content": "Sketch layout", "activeForm": "Sketching layout", "status": a},
                    {"content": "Write HTML", "activeForm": "Writing HTML", "status": b},
                    {"content": "Polish", "status": c}
                ]}}),
            )
        };
        t.apply(plan("in_progress", "pending", "pending"));
        let (n, m, step) = plan_progress(&t.todos).unwrap_or_else(|| panic!("plan"));
        assert_eq!(
            (n, m, step.active_form.as_str()),
            (1, 3, "Sketching layout")
        );
        t.apply(plan("completed", "completed", "pending"));
        let (n, _, step) = plan_progress(&t.todos).unwrap_or_else(|| panic!("plan"));
        assert_eq!(
            (n, step.active_form.as_str()),
            (3, "Polish"),
            "next step to start"
        );
        t.apply(plan("completed", "completed", "completed"));
        assert_eq!(plan_progress(&t.todos).map(|p| (p.0, p.1)), Some((3, 3)));
        assert!(parse_todos(&json!({"todos": "x"})).is_none());
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
    fn a_turn_cut_short_by_contour_closing_says_so_in_plain_words() {
        // QA-030: what the restarted daemon replays for a run it found running.
        let mut t = AssistantTurn::new();
        t.apply(decode_frame(
            "error",
            r#"{"message":"the daemon restarted while this run was in progress","code":"DAEMON_RESTART"}"#,
        ));
        t.apply(decode_frame(
            "end",
            r#"{"status":"failed","code":null,"terminalAt":1,"retryable":true,"failureCategory":"daemon_restart","error":"daemon restarted"}"#,
        ));
        assert_eq!(t.state, TurnState::Failed);
        assert_eq!(
            t.failure,
            Some(od_contracts::FailureCategory::DaemonRestart)
        );
        let [Block::Error(message)] = t.blocks.as_slice() else {
            panic!("one error block: {:?}", t.blocks);
        };
        assert!(message.contains("Contour closed"), "{message}");
        assert!(!message.to_lowercase().contains("daemon"), "{message}");
    }

    #[test]
    fn a_stored_turn_cut_short_by_contour_closing_knows_it_at_once() {
        // QA-030: the restarted daemon stores the cause with the message,
        // so the turn is right before its run log loads.
        let mut t = AssistantTurn::new();
        t.apply(RunFrame::Agent(AgentEvent::Error {
            message: "the daemon restarted while this run was in progress".into(),
            code: Some(od_contracts::DAEMON_RESTART_CODE.into()),
            terminal: true,
        }));
        assert_eq!(
            t.failure,
            Some(od_contracts::FailureCategory::DaemonRestart)
        );
        let [Block::Error(message)] = t.blocks.as_slice() else {
            panic!("one error block: {:?}", t.blocks);
        };
        assert!(message.contains("Contour closed"), "{message}");
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

    #[test]
    fn media_tasks_fold_in_by_id_in_start_order() {
        use od_contracts::{MediaTask, MediaTaskStatus};
        let task = |id: &str, status| MediaTask {
            task_id: id.into(),
            surface: "image".into(),
            model: "gpt-image-1".into(),
            status,
            progress: Vec::new(),
            file: None,
            error: None,
            started_at: 0,
            ended_at: None,
        };
        let mut turn = AssistantTurn::new();
        for t in [
            task("a", MediaTaskStatus::Running),
            task("b", MediaTaskStatus::Running),
            task("a", MediaTaskStatus::Done),
        ] {
            turn.apply(RunFrame::Agent(AgentEvent::Media { task: t }));
        }
        let got: Vec<(&str, MediaTaskStatus)> = turn
            .media
            .iter()
            .map(|m| (m.task_id.as_str(), m.status))
            .collect();
        assert_eq!(
            got,
            [
                ("a", MediaTaskStatus::Done),
                ("b", MediaTaskStatus::Running)
            ]
        );
    }

    #[test]
    fn jury_reports_fold_in_and_the_protocol_is_hidden() {
        use od_contracts::{CritiqueReport, CritiqueStatus};
        let mut turn = AssistantTurn::new();
        let report = CritiqueReport {
            status: CritiqueStatus::Shipped,
            composite: Some(8.6),
            ..CritiqueReport::default()
        };
        turn.apply(RunFrame::Agent(AgentEvent::Critique {
            report: report.clone(),
        }));
        assert_eq!(turn.critique, Some(report));
        assert_eq!(
            without_jury_protocol(
                "Intro <CRITIQUE_RUN v=\"1\"><ROUND n=\"1\">x</ROUND></CRITIQUE_RUN> done"
            ),
            "Intro  done"
        );
        assert_eq!(without_jury_protocol("a <CRITIQUE_RUN><ROUND"), "a ");
        assert_eq!(without_jury_protocol("plain"), "plain");
    }
}
