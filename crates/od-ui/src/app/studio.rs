//! Studio: chat on the left; files, tabs and live preview on the right.
//! Keyboard: Cmd/Ctrl+P or +T quick switcher, +W close tab, +1–9 pick a
//! tab, Ctrl+Tab cycle; arrows/Home/End drive a deck, Esc leaves present
//! and comment modes (spec 1.2).

use super::agent_w3::{SideChat, SideChatView, is_side_chat_title};
use super::bridge::{self, Source};
use super::chat_view::{ChatItem, ChatPane};
use super::deck::{DeckBar, NotesPanel, ThumbRail};
use super::export_menu::ExportMenu;
use super::preview_comments::CommentLayer;
use super::preview_tools::{self, DrawLayer, Tool, ToolBar, ToolPanel, Tools};
use super::sse::Sse;
use super::switcher::QuickSwitcher;
use super::versions::VersionsPanel;
use super::{Shell, api};
use crate::chat::{AssistantTurn, Block, TurnState};
use crate::events::{
    ChatRequest, ConversationsResponse, CreateRunResponse, FileChanged, MessageRole,
    MessagesResponse, RunFrame, RunStatus, decode_frame,
};
use crate::i18n::{T, t, tf};
use crate::preview_bridge::{self as pb, BridgeMsg, DeckState, Picked, Slide};
use crate::workspace::{Queued, Tabs, touch_recent};
use leptos::ev;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use od_contracts::{
    CommentAttachment, CommentRect, CommentStatus, CommentsResponse, Conversation,
    ConversationResponse, CreateVersionRequest, FileKind, FilesResponse, MemoryEntry,
    PreviewComment, Project, ProjectFile, ProjectResponse, SessionMode, UpdateCommentRequest,
    VersionSource,
};
use send_wrapper::SendWrapper;
use serde_json::json;
use std::collections::HashMap;

/// Preview frame widths (spec 1.2 device frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Desktop,
    Tablet,
    Phone,
}

impl Device {
    const ALL: [Self; 3] = [Self::Desktop, Self::Tablet, Self::Phone];

    fn label(self) -> &'static str {
        match self {
            Self::Desktop => t(T::DeviceDesktop),
            Self::Tablet => t(T::DeviceTablet),
            Self::Phone => t(T::DevicePhone),
        }
    }

    fn width(self) -> &'static str {
        match self {
            Self::Desktop => "100%",
            Self::Tablet => "820px",
            Self::Phone => "390px",
        }
    }
}

/// A file version shown in place of the live preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewedVersion {
    pub id: String,
    pub version: u32,
    pub content: String,
}

/// Everything the Studio page shares between its panes.
#[derive(Clone, Copy)]
pub struct StudioCtx {
    /// App context, kept here because async tasks run without an owner.
    pub shell: Shell,
    pub project_id: StoredValue<String>,
    pub project: RwSignal<Option<Project>>,
    pub conversation_id: RwSignal<Option<String>>,
    pub conversations: RwSignal<Vec<Conversation>>,
    pub items: RwSignal<Vec<ChatItem>>,
    /// The run currently streaming, if any (`Some("")` while starting).
    pub live_run: RwSignal<Option<String>>,
    /// Messages typed while a run was going, sent one by one after it.
    pub queue: RwSignal<Vec<Queued>>,
    pub files: RwSignal<Vec<ProjectFile>>,
    /// Open file tabs; the active tab is the previewed file.
    pub tabs: RwSignal<Tabs>,
    /// Recently opened files, newest first (quick switcher).
    pub recent: RwSignal<Vec<String>>,
    /// Bumped to reload the preview iframe.
    pub preview_rev: RwSignal<u32>,
    pub notice: RwSignal<Option<String>>,
    pub comments: RwSignal<Vec<PreviewComment>>,
    pub comment_mode: RwSignal<bool>,
    /// The element picked in comment mode, waiting for its note.
    pub comment_draft: RwSignal<Option<Picked>>,
    /// Where each shown comment's element is now (`od:located`), so pins
    /// follow scroll and reflow; missing ids fall back to the stored box.
    pub pin_rects: RwSignal<HashMap<String, CommentRect>>,
    /// Bumped on every `od:ready`: the new document needs `od:locate`.
    pub frame_ready: RwSignal<u32>,
    /// Set once the previewed file turns out to be a deck.
    pub deck: RwSignal<Option<DeckState>>,
    pub notes: RwSignal<Vec<String>>,
    pub show_notes: RwSignal<bool>,
    pub presenting: RwSignal<bool>,
    pub switcher_open: RwSignal<bool>,
    pub history_open: RwSignal<bool>,
    pub viewing: RwSignal<Option<ViewedVersion>>,
    /// Inspect, text edit, draw, tweaks, palette, lint (spec 1.2).
    pub tools: Tools,
    /// Mode of the open conversation, sent with each run.
    pub session_mode: RwSignal<SessionMode>,
    /// The project's Design Jury switch.
    pub jury: RwSignal<bool>,
    pub memory: RwSignal<Vec<MemoryEntry>>,
    pub side: SideChat,
    /// Last slide shown per deck file, restored when the deck reloads.
    last_slide: StoredValue<Option<(String, u32)>>,
    run_stream: StoredValue<Option<SendWrapper<Sse>>>,
    project_stream: StoredValue<Option<SendWrapper<Sse>>>,
}

fn is_previewable(f: &ProjectFile) -> bool {
    !f.path.starts_with('.') && !f.path.contains("/.")
}

pub fn is_html_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm")
}

/// The file to preview first: the project's entry file, else `index.html`,
/// else the first HTML file.
fn pick_entry(project: Option<&Project>, files: &[ProjectFile]) -> Option<String> {
    let entry = project
        .and_then(|p| p.metadata.as_ref())
        .and_then(|m| m.entry_file.clone());
    let has = |p: &str| files.iter().any(|f| f.path == p);
    entry
        .filter(|e| has(e))
        .or_else(|| has("index.html").then(|| "index.html".to_owned()))
        .or_else(|| {
            files
                .iter()
                .find(|f| f.kind == FileKind::Html && is_previewable(f))
                .map(|f| f.path.clone())
        })
}

impl StudioCtx {
    pub fn base(self) -> String {
        format!("/api/projects/{}", api::seg(&self.project_id.get_value()))
    }

    fn conversation_base(self, cid: &str) -> String {
        format!("{}/conversations/{}", self.base(), api::seg(cid))
    }

    /// The previewed file (tracked).
    pub fn active(self) -> Option<String> {
        self.tabs.with(|t| t.active.clone())
    }

    pub fn active_untracked(self) -> Option<String> {
        self.tabs.with_untracked(|t| t.active.clone())
    }

    /// Opens `path` in a tab (or switches to its tab).
    pub fn open_file(self, path: &str) {
        self.side.open.set(false);
        self.tabs.update(|t| t.open(path));
        self.recent.update(|r| touch_recent(r, path));
        self.viewing.set(None);
    }

    pub fn refresh_files(self) {
        leptos::task::spawn_local(async move {
            let Ok(res) = api::get::<FilesResponse>(&format!("{}/files", self.base())).await else {
                return;
            };
            let mut files: Vec<ProjectFile> =
                res.files.into_iter().filter(is_previewable).collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            self.tabs
                .update(|t| t.retain_existing(|p| files.iter().any(|f| f.path == p)));
            if self.active_untracked().is_none() {
                let entry = self
                    .project
                    .with_untracked(|p| pick_entry(p.as_ref(), &files));
                if let Some(entry) = entry {
                    self.open_file(&entry);
                }
            }
            self.files.set(files);
        });
    }

    pub fn reload_preview(self) {
        self.preview_rev.update(|r| *r = r.wrapping_add(1));
    }

    fn on_file_changed(self, changed: FileChanged) {
        self.refresh_files();
        let shown = self.active_untracked();
        if shown.as_deref() == Some(changed.path.as_str()) || shown.is_none() {
            self.reload_preview();
        }
    }

    /// The agent the next run uses, and whether it can be steered.
    fn agent_supports_steer(shell: Shell) -> bool {
        let Some(id) = shell.agent_id() else {
            return false;
        };
        shell.agents.with_untracked(|a| match a {
            Some(Ok(list)) => list.iter().any(|a| a.id == id && a.supports_steer),
            _ => false,
        })
    }

    pub fn can_steer(self, shell: Shell) -> bool {
        self.live_run
            .with(|r| r.as_ref().is_some_and(|r| !r.is_empty()))
            && Self::agent_supports_steer(shell)
    }

    /// Sends a user turn, or queues it while a run is going.
    pub fn send(self, text: String, shell: Shell) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if self.live_run.get_untracked().is_some() {
            self.queue.update(|q| {
                q.push(Queued {
                    id: api::random_id(),
                    text,
                });
            });
            return;
        }
        let user_id = api::random_id();
        let assistant_id = api::random_id();
        let turn = RwSignal::new(AssistantTurn::new());
        self.items.update(|items| {
            items.push(ChatItem::user(user_id.clone(), text.clone()));
            items.push(ChatItem::assistant(assistant_id.clone(), turn));
        });
        let Some(agent_id) = shell.agent_id() else {
            fail(turn, t(T::NoAgentSelected).to_owned());
            return;
        };
        let project = self.project.get_untracked();
        let model = shell
            .config
            .with_untracked(|c| c.as_ref().and_then(|c| c.model.clone()));
        let attached: Vec<PreviewComment> = self.comments.with_untracked(|c| {
            c.iter()
                .filter(|c| c.status == CommentStatus::Attached)
                .cloned()
                .collect()
        });
        let req = ChatRequest {
            agent_id,
            message: text.clone(),
            project_id: self.project_id.get_value(),
            conversation_id: self.conversation_id.get_untracked(),
            user_message_id: Some(user_id),
            assistant_message_id: Some(assistant_id),
            client_request_id: Some(api::random_id()),
            skill_id: project.as_ref().and_then(|p| p.skill_id.clone()),
            design_system_id: project.as_ref().and_then(|p| p.design_system_id.clone()),
            comment_attachments: attached
                .iter()
                .zip(1u32..)
                .map(|(c, i)| CommentAttachment::from_comment(c, i))
                .collect(),
            model,
            locale: Some(shell.locale.get_untracked().tag().to_owned()),
            session_mode: Some(self.session_mode.get_untracked()),
            attachments: self.tools.attachments.get_untracked(),
            ..ChatRequest::default()
        };
        self.tools.deliverable.set(None);
        self.live_run.set(Some(String::new()));
        let req_attachments = req.attachments.clone();
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateRunResponse>("/api/runs", &req).await {
                Ok(res) => {
                    self.tools.attachments.update(|a| {
                        a.retain(|p| !req_attachments.contains(p));
                    });
                    self.conversation_id.set(Some(res.conversation_id));
                    turn.update(|t| t.run_id = Some(res.run_id.clone()));
                    self.attach(res.run_id, turn, Some(text));
                    // The daemon moves them to applying, then resolved or
                    // back to attached when the run ends; mirror it here.
                    self.comments.update(|list| {
                        for c in list
                            .iter_mut()
                            .filter(|c| attached.iter().any(|a| a.id == c.id))
                        {
                            c.status = CommentStatus::Applying;
                        }
                    });
                }
                Err(e) => {
                    fail(turn, e.to_string());
                    self.live_run.set(None);
                    self.send_next(shell);
                }
            }
        });
    }

    /// Sends the first queued message, if any.
    fn send_next(self, shell: Shell) {
        let next = self
            .queue
            .try_update(|q| (!q.is_empty()).then(|| q.remove(0)));
        if let Some(Some(next)) = next {
            self.send(next.text, shell);
        }
    }

    /// Pushes a queued message into the running turn (Claude only).
    pub fn steer(self, id: String) {
        let Some(run_id) = self.live_run.get_untracked().filter(|r| !r.is_empty()) else {
            return;
        };
        let Some(item) = self
            .queue
            .with_untracked(|q| q.iter().find(|q| q.id == id).cloned())
        else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("/api/runs/{}/steer", api::seg(&run_id));
            match api::post::<_, serde_json::Value>(&path, &json!({ "message": item.text })).await {
                Ok(_) => {
                    self.queue.update(|q| q.retain(|q| q.id != id));
                    let turn = self.items.with_untracked(|items| {
                        items.iter().rev().find_map(|i| match &i.kind {
                            super::chat_view::ItemKind::Assistant(turn) => Some(*turn),
                            super::chat_view::ItemKind::User(_) => None,
                        })
                    });
                    if let Some(turn) = turn {
                        turn.update(|t| t.blocks.push(Block::Steered(item.text)));
                    }
                }
                Err(e) => self.notice.set(Some(tf(T::SteerFailed, &[&e.to_string()]))),
            }
        });
    }

    /// Streams `run_id` into `turn` until its `end` frame. `prompt` is the
    /// user message, recorded on the AI versions of the files the turn wrote.
    pub fn attach(self, run_id: String, turn: RwSignal<AssistantTurn>, prompt: Option<String>) {
        let shell = self.shell;
        self.live_run.set(Some(run_id.clone()));
        let url = format!("/api/runs/{}/events", api::seg(&run_id));
        let prompt = StoredValue::new(prompt);
        let run = StoredValue::new(run_id);
        let stream = Sse::open(
            &url,
            &["start", "agent", "error", "end"],
            move |name, data| {
                let frame = decode_frame(name, &data);
                let ended = match &frame {
                    RunFrame::End(end) => Some(end.status),
                    _ => None,
                };
                let wrote = matches!(
                    &frame,
                    RunFrame::Agent(crate::events::AgentEvent::ToolResult { .. })
                );
                turn.update(|t| t.apply(frame));
                if wrote {
                    // Cheap fallback when project events are unavailable.
                    self.refresh_files();
                }
                if let Some(status) = ended {
                    if turn.with_untracked(|t| t.critique.is_some()) {
                        self.settle_jury(turn);
                    }
                    if status == RunStatus::Succeeded {
                        let files = turn.with_untracked(AssistantTurn::touched_files);
                        self.record_versions(files, prompt.get_value());
                        self.check_deliverable(run.get_value());
                    }
                    self.finish_run(shell);
                }
            },
            move || {
                if turn.with_untracked(|t| t.state.is_live()) {
                    fail(turn, t(T::LostRunStream).to_owned());
                }
                self.finish_run(shell);
            },
        );
        match stream {
            Some(s) => self.run_stream.set_value(Some(SendWrapper::new(s))),
            None => {
                fail(turn, t(T::CannotOpenRunStream).to_owned());
                self.live_run.set(None);
            }
        }
    }

    /// Snapshots the HTML files a turn wrote as AI versions (spec 1.2).
    fn record_versions(self, files: Vec<String>, prompt: Option<String>) {
        let files: Vec<String> = files.into_iter().filter(|f| is_html_path(f)).collect();
        if files.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            for f in files {
                let path = format!("{}/files/{}/versions", self.base(), api::file_path(&f));
                let req = CreateVersionRequest {
                    prompt: prompt.clone(),
                    source: Some(VersionSource::Ai),
                    label: None,
                };
                let _ = api::post::<_, serde_json::Value>(&path, &req).await;
            }
        });
    }

    /// Closes the run stream after the current event handler returns, then
    /// sends the next queued message.
    fn finish_run(self, shell: Shell) {
        self.live_run.set(None);
        self.run_stream.with_value(|s| {
            if let Some(s) = s {
                s.close();
            }
        });
        leptos::task::spawn_local(async move {
            self.run_stream.set_value(None);
            self.send_next(shell);
        });
        self.refresh_files();
        self.reload_preview();
        self.reload_comments();
    }

    pub fn cancel(self) {
        let Some(run_id) = self.live_run.get_untracked().filter(|r| !r.is_empty()) else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("/api/runs/{}/cancel", api::seg(&run_id));
            if let Err(e) = api::post::<_, serde_json::Value>(&path, &json!({})).await {
                self.notice.set(Some(tf(T::StopFailed, &[&e.to_string()])));
            }
        });
    }

    fn watch_project(self) {
        let url = format!("{}/events", self.base());
        let stream = Sse::open(
            &url,
            &["file-changed", "conversation-created"],
            move |name, data| {
                if name == "conversation-created" {
                    self.reload_conversations();
                } else if let Ok(changed) = serde_json::from_str::<FileChanged>(&data) {
                    self.on_file_changed(changed);
                }
            },
            || {},
        );
        self.project_stream.set_value(stream.map(SendWrapper::new));
    }

    /// Re-reads the open conversation's comments (statuses change when a
    /// run that applies them ends).
    fn reload_comments(self) {
        let Some(cid) = self.conversation_id.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("{}/comments", self.conversation_base(&cid));
            if let Ok(res) = api::get::<CommentsResponse>(&path).await {
                if self.conversation_id.get_untracked().as_deref() == Some(cid.as_str()) {
                    self.comments.set(res.comments);
                }
            }
        });
    }

    pub fn reload_conversations(self) {
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations", self.base());
            if let Ok(r) = api::get::<ConversationsResponse>(&path).await {
                self.conversations.set(r.conversations);
            }
        });
    }

    /// Shows `cid`: its messages, its comments, and its live run if any.
    pub async fn open_conversation(self, cid: String) {
        self.conversation_id.set(Some(cid.clone()));
        let mode = self
            .conversations
            .with_untracked(|list| list.iter().find(|c| c.id == cid).map(|c| c.session_mode));
        self.session_mode.set(mode.unwrap_or_default());
        self.items.set(Vec::new());
        self.comments.set(Vec::new());
        let base = self.conversation_base(&cid);
        if let Ok(res) = api::get::<CommentsResponse>(&format!("{base}/comments")).await {
            self.comments.set(res.comments);
        }
        let Ok(res) = api::get::<MessagesResponse>(&format!("{base}/messages")).await else {
            return;
        };
        let mut resume = None;
        let items = res
            .messages
            .iter()
            .map(|m| match m.role {
                MessageRole::User => ChatItem::user(m.id.clone(), m.content.clone()),
                MessageRole::Assistant => {
                    let turn = RwSignal::new(AssistantTurn::from_stored(m));
                    if turn.with_untracked(|t| t.state.is_live()) {
                        resume = m.run_id.clone().map(|r| (r, turn));
                    }
                    ChatItem::assistant(m.id.clone(), turn)
                }
            })
            .collect();
        self.items.set(items);
        if let Some((run_id, turn)) = resume {
            // Replay from the start: the turn is rebuilt from frames.
            turn.update(|t| *t = AssistantTurn::new());
            self.attach(run_id, turn, None);
        }
    }

    /// Starts an empty conversation, or a fork of the current one that
    /// keeps the messages up to `after` (inclusive).
    pub fn new_conversation(self, fork_after: Option<String>) {
        if self.live_run.get_untracked().is_some() {
            return;
        }
        let current = self.conversation_id.get_untracked();
        let mut body = json!({});
        if let (Some(after), Some(cid)) = (fork_after, current.clone()) {
            let title = self.conversations.with_untracked(|list| {
                list.iter()
                    .position(|c| c.id == cid)
                    .map(|i| conversation_title(&list[i], list.len() - i))
                    .unwrap_or_default()
            });
            body = json!({
                "title": tf(T::ForkTitle, &[&title]),
                "seedFromConversationId": cid,
                "forkAfterMessageId": after,
            });
        }
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations", self.base());
            match api::post::<_, ConversationResponse>(&path, &body).await {
                Ok(r) => {
                    let id = r.conversation.id.clone();
                    self.conversations.update(|list| {
                        if !list.iter().any(|c| c.id == id) {
                            list.insert(0, r.conversation);
                        }
                    });
                    self.open_conversation(id).await;
                }
                Err(e) => self.notice.set(Some(e.to_string())),
            }
        });
    }

    pub fn set_comment_status(self, id: &str, status: CommentStatus) {
        let Some(cid) = self.conversation_id.get_untracked() else {
            return;
        };
        let id = id.to_owned();
        self.comments.update(|list| {
            if let Some(c) = list.iter_mut().find(|c| c.id == id) {
                c.status = status;
            }
        });
        leptos::task::spawn_local(async move {
            let path = format!(
                "{}/comments/{}",
                self.conversation_base(&cid),
                api::seg(&id)
            );
            let req = UpdateCommentRequest {
                status: Some(status),
                note: None,
            };
            let _ = api::patch::<_, serde_json::Value>(&path, &req).await;
        });
    }

    pub fn remove_comment(self, id: &str) {
        let Some(cid) = self.conversation_id.get_untracked() else {
            return;
        };
        let id = id.to_owned();
        self.comments.update(|list| list.retain(|c| c.id != id));
        leptos::task::spawn_local(async move {
            let path = format!(
                "{}/comments/{}",
                self.conversation_base(&cid),
                api::seg(&id)
            );
            let _ = api::delete::<serde_json::Value>(&path).await;
        });
    }

    pub fn set_comment_mode(self, on: bool) {
        if on {
            self.leave_tool();
        }
        self.comment_mode.set(on);
        if !on {
            self.comment_draft.set(None);
        }
        bridge::post_main(&pb::select_mode(on));
    }

    pub fn slide(self, cmd: Slide) {
        bridge::post_main(&cmd.message());
    }

    fn on_bridge(self, source: Source, msg: BridgeMsg) {
        match (source, msg) {
            (Source::Main, BridgeMsg::Ready) => {
                if self.comment_mode.get_untracked() {
                    bridge::post_main(&pb::select_mode(true));
                }
                self.pin_rects.set(HashMap::new());
                self.frame_ready.update(|n| *n = n.wrapping_add(1));
                self.tools_ready();
            }
            (Source::Main, BridgeMsg::Located { items }) => {
                self.pin_rects.set(
                    items
                        .into_iter()
                        .filter_map(|l| Some((l.id, l.rect?)))
                        .collect(),
                );
            }
            (Source::Main, BridgeMsg::DeckReady { count }) => {
                self.deck.set(Some(DeckState { active: 0, count }));
                // Live reload keeps the slide the user was on.
                let path = self.active_untracked();
                let keep = self.last_slide.with_value(|l| match (l, &path) {
                    (Some((p, i)), Some(cur)) if p == cur && *i > 0 && *i < count => Some(*i),
                    _ => None,
                });
                match keep {
                    Some(i) => self.slide(Slide::Go(i)),
                    None => bridge::post_main(&pb::slide_state_request()),
                }
            }
            (Source::Main, BridgeMsg::SlideState { active, count }) => {
                self.deck.set(Some(DeckState { active, count }));
                if let Some(path) = self.active_untracked() {
                    self.last_slide.set_value(Some((path, active)));
                }
            }
            (Source::Main, BridgeMsg::Esc) => self.presenting.set(false),
            (Source::Main, BridgeMsg::Select(picked)) => {
                if self.comment_mode.get_untracked() {
                    self.comment_draft.set(Some(picked));
                } else if self.tools.tool.get_untracked() == Some(Tool::Inspect) {
                    self.inspect(&picked.selector);
                }
            }
            (Source::Thumb(i), BridgeMsg::DeckReady { .. }) => {
                bridge::post_thumb(i, &Slide::Go(i).message());
            }
            (Source::Main, msg) => {
                self.on_tool_msg(&msg);
            }
            _ => {}
        }
    }

    fn on_key(self, ev: &web_sys::KeyboardEvent) {
        let key = ev.key();
        let modifier = ev.ctrl_key() || ev.meta_key();
        if modifier && !ev.alt_key() {
            let handled = match key.to_ascii_lowercase().as_str() {
                "p" | "t" => {
                    self.switcher_open.set(true);
                    true
                }
                "w" => {
                    if let Some(active) = self.active_untracked() {
                        self.tabs.update(|t| t.close(&active));
                    }
                    true
                }
                d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && d != "0" => {
                    let n = usize::from(d.as_bytes()[0] - b'0');
                    self.tabs.update(|t| t.select_nth(n));
                    true
                }
                _ => false,
            };
            if handled {
                ev.prevent_default();
                return;
            }
        }
        if ev.ctrl_key() && key == "Tab" {
            ev.prevent_default();
            self.tabs.update(|t| t.cycle(!ev.shift_key()));
            return;
        }
        if self.switcher_open.get_untracked() || is_editing(ev) {
            return;
        }
        if key == "Escape" {
            if self.presenting.get_untracked() {
                self.presenting.set(false);
            } else if self.comment_mode.get_untracked() {
                self.set_comment_mode(false);
            } else if self.tools.tool.get_untracked().is_some() {
                self.set_tool(None);
            }
            return;
        }
        if self.deck.get_untracked().is_none() || modifier {
            return;
        }
        let cmd = match key.as_str() {
            "ArrowRight" | "PageDown" | " " => Slide::Next,
            "ArrowLeft" | "PageUp" => Slide::Prev,
            "Home" => Slide::First,
            "End" => Slide::Last,
            _ => return,
        };
        ev.prevent_default();
        self.slide(cmd);
    }

    /// Loads history, resumes a live run, then sends a pending prompt.
    async fn load(self, shell: Shell) {
        let base = self.base();
        let project = match api::get::<ProjectResponse>(&base).await {
            Ok(r) => r.project,
            Err(e) => {
                self.notice.set(Some(e.to_string()));
                return;
            }
        };
        let pending = project.pending_prompt.clone();
        self.project.set(Some(project));
        self.refresh_files();
        self.watch_project();
        self.load_w3();

        let conversations = api::get::<ConversationsResponse>(&format!("{base}/conversations"))
            .await
            .map(|r| r.conversations)
            .unwrap_or_default();
        // The side chat keeps its own conversation out of the main pane.
        let side = conversations
            .iter()
            .find(|c| is_side_chat_title(c.title.as_deref()))
            .map(|c| c.id.clone());
        self.side.conversation_id.set(side.clone());
        let first = conversations
            .iter()
            .find(|c| Some(&c.id) != side.as_ref())
            .map(|c| c.id.clone());
        self.conversations.set(conversations);
        if let Some(cid) = first {
            self.open_conversation(cid).await;
        }
        if self.live_run.get_untracked().is_some() {
            return;
        }
        if let Some(prompt) = pending.filter(|p| !p.trim().is_empty()) {
            // Clear first so a reload never sends the brief twice.
            let _ =
                api::patch::<_, ProjectResponse>(&base, &json!({ "pendingPrompt": null })).await;
            self.send(prompt, shell);
        }
    }
}

/// A conversation's name: its title, else "Conversation N" (1 = oldest).
pub fn conversation_title(c: &Conversation, number: usize) -> String {
    c.title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| tf(T::ConversationN, &[&number.to_string()]))
}

/// Whether a key press belongs to a text field rather than the workspace.
fn is_editing(ev: &web_sys::KeyboardEvent) -> bool {
    use wasm_bindgen::JsCast;
    let Some(el) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return false;
    };
    matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        || el.get_attribute("contenteditable").is_some()
}

fn fail(turn: RwSignal<AssistantTurn>, message: String) {
    turn.update(|t| {
        t.blocks.push(Block::Error(message));
        t.state = TurnState::Failed;
    });
}

#[component]
pub fn Studio() -> impl IntoView {
    let shell = Shell::get();
    let params = use_params_map();
    let project_id = params.with_untracked(|p| p.get("id").unwrap_or_default());
    let ctx = StudioCtx {
        shell,
        project_id: StoredValue::new(project_id),
        project: RwSignal::new(None),
        conversation_id: RwSignal::new(None),
        conversations: RwSignal::new(Vec::new()),
        items: RwSignal::new(Vec::new()),
        live_run: RwSignal::new(None),
        queue: RwSignal::new(Vec::new()),
        files: RwSignal::new(Vec::new()),
        tabs: RwSignal::new(Tabs::default()),
        recent: RwSignal::new(Vec::new()),
        preview_rev: RwSignal::new(0),
        notice: RwSignal::new(None),
        comments: RwSignal::new(Vec::new()),
        comment_mode: RwSignal::new(false),
        comment_draft: RwSignal::new(None),
        pin_rects: RwSignal::new(HashMap::new()),
        frame_ready: RwSignal::new(0),
        deck: RwSignal::new(None),
        notes: RwSignal::new(Vec::new()),
        show_notes: RwSignal::new(false),
        presenting: RwSignal::new(false),
        switcher_open: RwSignal::new(false),
        history_open: RwSignal::new(false),
        viewing: RwSignal::new(None),
        session_mode: RwSignal::new(SessionMode::default()),
        jury: RwSignal::new(false),
        memory: RwSignal::new(Vec::new()),
        side: SideChat::new(),
        tools: Tools::new(),
        last_slide: StoredValue::new(None),
        run_stream: StoredValue::new(None),
        project_stream: StoredValue::new(None),
    };
    provide_context(ctx);
    preview_tools::watch_lint(ctx);

    // Wait for settings so the first turn uses the configured agent.
    let started = StoredValue::new(false);
    Effect::new(move |_| {
        if shell.config.with(Option::is_some) && !started.get_value() {
            started.set_value(true);
            leptos::task::spawn_local(ctx.load(shell));
        }
    });
    let keys = window_event_listener(ev::keydown, move |ev| ctx.on_key(&ev));
    let messages = window_event_listener(ev::message, move |ev| {
        if let Some((source, msg)) = bridge::decode(&ev) {
            ctx.on_bridge(source, msg);
        }
    });
    on_cleanup(move || {
        keys.remove();
        messages.remove();
        ctx.run_stream.set_value(None);
        ctx.project_stream.set_value(None);
        ctx.side.close_stream();
    });

    view! {
        <div class="studio">
            <ChatPane ctx=ctx/>
            <section class="workspace">
                <FilesPanel ctx=ctx/>
                <PreviewPane ctx=ctx/>
            </section>
            <Show when=move || ctx.switcher_open.get()>
                <QuickSwitcher ctx=ctx/>
            </Show>
        </div>
    }
}

#[component]
fn FilesPanel(ctx: StudioCtx) -> impl IntoView {
    view! {
        <aside class="files" aria-label=t(T::ProjectFiles)>
            <div class="pane-head">
                <span class="pane-title">
                    {move || ctx.project.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default())}
                </span>
                <button class="icon-btn" title=t(T::QuickOpen) data-testid="open-switcher" on:click=move |_| ctx.switcher_open.set(true)>
                    "⌘P"
                </button>
                <button class="icon-btn" title=t(T::RefreshFiles) on:click=move |_| ctx.refresh_files()>
                    "↻"
                </button>
            </div>
            <ul class="file-tree" data-testid="file-tree">
                {move || {
                    let files = ctx.files.get();
                    if files.is_empty() {
                        return view! { <li class="empty small">{t(T::NoFilesYet)}</li> }.into_any();
                    }
                    files
                        .into_iter()
                        .map(|f| {
                            let depth = f.path.matches('/').count();
                            let path = f.path.clone();
                            let path_active = f.path.clone();
                            view! {
                                <li>
                                    <button
                                        class="file"
                                        class:active=move || ctx.active().as_deref() == Some(path_active.as_str())
                                        style=format!("padding-left:{}px", 10 + depth * 14)
                                        title=f.path.clone()
                                        on:click=move |_| ctx.open_file(&path)
                                    >
                                        <span class=format!("dot kind-{:?}", f.kind).to_lowercase()></span>
                                        {f.name.clone()}
                                    </button>
                                </li>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </ul>
        </aside>
    }
}

#[component]
fn TabsBar(ctx: StudioCtx) -> impl IntoView {
    view! {
        <div class="tabs" role="tablist" data-testid="tabs">
            {move || {
                let tabs = ctx.tabs.get();
                let side_open = ctx.side.open.get();
                let side_tab = view! {
                    <div class="tab side-tab" class:active=side_open role="tab" aria-selected=side_open.to_string() data-testid="side-chat-tab" title=t(T::SideChatHint)>
                        <button class="tab-name" on:click=move |_| ctx.side.show(ctx)>{format!("💬 {}", t(T::SideChat))}</button>
                    </div>
                };
                let files = tabs.open
                    .iter()
                    .map(|path| {
                        let active = !side_open && tabs.active.as_deref() == Some(path.as_str());
                        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
                        let open = path.clone();
                        let close = path.clone();
                        view! {
                            <div class="tab" class:active=active role="tab" aria-selected=active.to_string() data-testid="tab" title=path.clone()>
                                <button class="tab-name" on:click=move |_| ctx.open_file(&open)>{name}</button>
                                <button
                                    class="tab-close"
                                    title=t(T::CloseTab)
                                    aria-label=t(T::CloseTab)
                                    on:click=move |_| ctx.tabs.update(|t| t.close(&close))
                                >
                                    "×"
                                </button>
                            </div>
                        }
                    })
                    .collect_view();
                view! { {files}{side_tab} }
            }}
        </div>
    }
}

#[component]
fn PreviewPane(ctx: StudioCtx) -> impl IntoView {
    let device = RwSignal::new(Device::Desktop);
    let src = Memo::new(move |_| {
        let path = ctx.active()?;
        let rev = ctx.preview_rev.get();
        let raw = format!(
            "/api/projects/{}/raw/{}",
            api::seg(&ctx.project_id.get_value()),
            api::file_path(&path)
        );
        // HTML goes through the bridge: deck control, comments, guards.
        Some(if is_html_path(&path) {
            format!("{raw}?bridge=1&v={rev}")
        } else {
            format!("{raw}?v={rev}")
        })
    });
    // A new document: deck state comes back with its `od:deck-ready`.
    Effect::new(move |_| {
        src.track();
        ctx.deck.set(None);
        ctx.comment_draft.set(None);
    });
    // Speaker notes follow the document and its slide count.
    let notes_key = Memo::new(move |_| (src.get(), ctx.deck.with(|d| d.map(|d| d.count))));
    Effect::new(move |_| match notes_key.get() {
        (Some(_), Some(count)) => super::deck::load_notes(ctx, count),
        _ => ctx.notes.set(Vec::new()),
    });
    let is_image = move || {
        let path = ctx.active();
        ctx.files.with(|files| {
            files
                .iter()
                .find(|f| Some(&f.path) == path.as_ref())
                .is_some_and(|f| matches!(f.kind, FileKind::Image))
        })
    };
    let is_html = move || ctx.active().is_some_and(|p| is_html_path(&p));
    view! {
        <section class="preview" class:presenting=move || ctx.presenting.get() data-testid="preview-pane">
            <TabsBar ctx=ctx/>
            <Show when=move || !ctx.side.open.get() fallback=move || view! { <SideChatView ctx=ctx/> }>
            <div class="pane-head">
                <span class="pane-title mono">{move || ctx.active().unwrap_or_default()}</span>
                <div class="segmented small">
                    {Device::ALL
                        .into_iter()
                        .map(|d| {
                            view! {
                                <button class="seg" class:active=move || device.get() == d on:click=move |_| device.set(d)>
                                    {d.label()}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                <Show when=is_html>
                    <button
                        class="btn small"
                        class:active=move || ctx.comment_mode.get()
                        data-testid="comment-mode"
                        aria-pressed=move || ctx.comment_mode.get().to_string()
                        on:click=move |_| ctx.set_comment_mode(!ctx.comment_mode.get_untracked())
                    >
                        {t(T::CommentMode)}
                    </button>
                    <button
                        class="btn small"
                        class:active=move || ctx.history_open.get()
                        data-testid="history"
                        on:click=move |_| ctx.history_open.update(|o| *o = !*o)
                    >
                        {t(T::History)}
                    </button>
                </Show>
                <ExportMenu ctx=ctx/>
                <button class="icon-btn" title=t(T::ReloadPreview) on:click=move |_| ctx.reload_preview()>"↻"</button>
            </div>
            <DeckBar ctx=ctx/>
            <Show when=move || is_html() && ctx.viewing.with(Option::is_none)>
                <ToolBar ctx=ctx/>
            </Show>
            {move || ctx.comment_mode.get().then(|| view! { <p class="mode-hint">{t(T::CommentModeOn)}</p> })}
            <div class="preview-body">
                <ThumbRail ctx=ctx src=src/>
                <div class="preview-stage">
                    {move || match (src.get(), ctx.viewing.get()) {
                        (None, _) => view! { <p class="empty">{t(T::PreviewEmpty)}</p> }.into_any(),
                        (Some(_), Some(v)) => view! {
                            <iframe
                                class="preview-frame"
                                data-testid="version-preview"
                                title=tf(T::ViewingVersion, &[&v.version.to_string()])
                                sandbox="allow-scripts"
                                srcdoc=v.content
                                style=move || format!("width:{}", device.get().width())
                            ></iframe>
                        }.into_any(),
                        (Some(url), None) if is_image() => view! { <img class="preview-image" src=url alt=t(T::Preview)/> }.into_any(),
                        (Some(url), None) => view! {
                            <div class="frame-wrap" style=move || format!("width:{}", device.get().width())>
                                // No allow-same-origin: artifact scripts run in an
                                // opaque origin and cannot call the daemon API.
                                <iframe
                                    class="preview-frame"
                                    data-testid="preview"
                                    title=t(T::Preview)
                                    sandbox="allow-scripts allow-forms allow-popups allow-modals"
                                    src=url
                                ></iframe>
                                <CommentLayer ctx=ctx/>
                                {move || (ctx.tools.tool.get() == Some(Tool::Draw)).then(|| view! { <DrawLayer ctx=ctx/> })}
                            </div>
                        }.into_any(),
                    }}
                </div>
                <Show when=move || ctx.history_open.get() && !ctx.presenting.get()>
                    <VersionsPanel ctx=ctx/>
                </Show>
                <Show when=move || !ctx.presenting.get()>
                    <ToolPanel ctx=ctx/>
                </Show>
            </div>
            <NotesPanel ctx=ctx/>
            </Show>
        </section>
    }
}
