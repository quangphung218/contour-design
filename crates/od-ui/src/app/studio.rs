//! Studio: chat on the left, the live preview in the middle (the main
//! area), the file tree on the right, collapsible.
//! Keyboard: Cmd/Ctrl+P quick switcher, +T tab launcher, +W close tab, +1–9 pick a
//! tab, Ctrl+Tab cycle, Cmd/Ctrl+B files panel, Cmd/Ctrl+= / - / 0 zoom;
//! arrows/Home/End drive a deck, Esc leaves present and comment modes
//! (spec 1.2). Cmd/Ctrl+K (palette) and ? (shortcuts) are app-wide.

use super::agent_w3::{SideChat, SideChatView, is_side_chat_title};
use super::bridge::{self, Source};
use super::chat_view::{ChatItem, ChatPane};
use super::deck::{DeckBar, NotesPanel, ThumbRail};
use super::doc_view::{DocView, MarkdownView, is_document};
use super::export_menu::ExportMenu;
use super::files_panel::{FilesPanel, FilesToast, FilesUi, TabLauncher};
use super::icons::{Glyph, icon};
use super::preview_comments::CommentLayer;
use super::preview_tools::{self, DrawLayer, Tool, ToolBar, ToolHints, ToolPanel, Tools};
use super::sketch::{SketchView, is_sketch};
use super::sse::Sse;
use super::switcher::QuickSwitcher;
use super::terminal::{self, TermTab, TerminalView};
use super::versions::VersionsPanel;
use super::{Shell, api};
use crate::chat::{AssistantTurn, Block, TurnState};
use crate::comment_board::{AnchorState, attached_images};
use crate::events::{
    ChatRequest, ConversationsResponse, CreateRunResponse, FileChanged, MessageRole,
    MessagesResponse, RunFrame, RunStatus, decode_frame,
};
use crate::file_tree::is_markdown;
use crate::highlight::{Lang, highlight};
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

/// The pane a phone-width Studio shows (one at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowPane {
    Chat,
    Preview,
    Files,
}

impl NarrowPane {
    fn key(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Preview => "preview",
            Self::Files => "files",
        }
    }
}

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

    fn glyph(self) -> Glyph {
        match self {
            Self::Desktop => Glyph::Desktop,
            Self::Tablet => Glyph::Tablet,
            Self::Phone => Glyph::Phone,
        }
    }

    fn width(self) -> &'static str {
        match self {
            Self::Desktop => "100%",
            Self::Tablet => "820px",
            Self::Phone => "390px",
        }
    }

    /// The width the page lays out at, px; `None` for the pane's width.
    fn px(self) -> Option<u32> {
        match self {
            Self::Desktop => None,
            Self::Tablet => Some(820),
            Self::Phone => Some(390),
        }
    }
}

/// Zoom levels of the preview, percent.
pub const ZOOM_STEPS: [u32; 7] = [50, 67, 75, 100, 125, 150, 200];

/// The zoom step after `current` (`up`) or before it, clamped.
#[must_use]
pub fn zoom_step(current: u32, up: bool) -> u32 {
    let i = ZOOM_STEPS
        .iter()
        .position(|&z| z >= current)
        .unwrap_or(ZOOM_STEPS.len() - 1);
    let i = if up {
        (i + usize::from(ZOOM_STEPS[i] <= current)).min(ZOOM_STEPS.len() - 1)
    } else {
        i.saturating_sub(1)
    };
    ZOOM_STEPS[i]
}

/// `zoom` lowered to `cap`, the largest zoom a device frame fits at.
fn capped(zoom: u32, cap: Option<u32>) -> u32 {
    cap.map_or(zoom, |c| zoom.min(c))
}

/// The zoom one step up or down from the one shown (`chosen` under `cap`);
/// `None` when that step would not change the size the page is drawn at.
fn next_zoom(chosen: u32, cap: Option<u32>, up: bool) -> Option<u32> {
    let shown = capped(chosen, cap);
    let next = zoom_step(shown, up);
    (capped(next, cap) != shown).then_some(next)
}

/// One zoom step as [`next_zoom`], with `past_fit`: the frame is drawn
/// above its fit, the stage scrolling, and this is the zoom chosen before
/// it went there. Only a frame that fits below the smallest step goes
/// there: + steps past the fit, and − from the smallest step comes back to
/// it and to that zoom (QA-024). Returns the new zoom and `past_fit`.
fn zoom_move(
    chosen: u32,
    cap: Option<u32>,
    past_fit: Option<u32>,
    up: bool,
) -> Option<(u32, Option<u32>)> {
    let cap = cap.filter(|_| past_fit.is_none());
    if let Some(z) = next_zoom(chosen, cap, up) {
        return Some((z, past_fit));
    }
    let below_steps = cap.is_some_and(|c| c < ZOOM_STEPS[0]);
    match (up, past_fit) {
        (true, None) if below_steps => Some((ZOOM_STEPS[0], Some(chosen))),
        (false, Some(before)) => Some((before, None)),
        _ => None,
    }
}

/// The zoom to go back to once the frame fits `cap` at the smallest step
/// or more (a wider pane, another device): the one chosen before `past_fit`.
fn leave_past_fit(cap: Option<u32>, past_fit: Option<u32>) -> Option<u32> {
    past_fit.filter(|_| cap.is_none_or(|c| c >= ZOOM_STEPS[0]))
}

/// The zoom-in button is off because the frame already fills the pane
/// (not at the largest step).
fn fit_stops_zoom_in(chosen: u32, cap: Option<u32>, past_fit: Option<u32>) -> bool {
    let cap = cap.filter(|_| past_fit.is_none());
    let shown = capped(chosen, cap);
    cap.is_some()
        && zoom_move(chosen, cap, past_fit, true).is_none()
        && zoom_step(shown, true) > shown
}

/// Slide states a deck restore waits through before it gives up (the
/// user may have moved on).
const RESTORE_PATIENCE: u8 = 3;

/// `localStorage` key of the files panel state.
const FILES_OPEN_KEY: &str = "od.studio.files-open";
/// Below this window width the file column starts closed (until the user
/// opens it), so the preview keeps room for its toolbar and tool panels.
const FILES_OPEN_MIN_WIDTH: f64 = 1100.0;

fn wide_window() -> bool {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|w| w.as_f64())
        .is_none_or(|w| w >= FILES_OPEN_MIN_WIDTH)
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
    /// The composer's text (next steps and starter prompts fill it).
    pub draft: RwSignal<String>,
    /// Messages typed while a run was going, sent one by one after it.
    pub queue: RwSignal<Vec<Queued>>,
    pub files: RwSignal<Vec<ProjectFile>>,
    /// Open file tabs; the active tab is the previewed file.
    pub tabs: RwSignal<Tabs>,
    /// Terminal tabs, after the file tabs.
    pub terms: RwSignal<Vec<TermTab>>,
    /// The terminal shown instead of the preview, if any.
    pub active_term: RwSignal<Option<String>>,
    /// Recently opened files, newest first (quick switcher).
    pub recent: RwSignal<Vec<String>>,
    /// Bumped to reload the preview iframe.
    pub preview_rev: RwSignal<u32>,
    pub notice: RwSignal<Option<String>>,
    /// The project's event stream dropped and is reconnecting.
    pub link_down: RwSignal<bool>,
    /// The project could not be loaded; shown until a retry succeeds.
    pub load_error: RwSignal<Option<String>>,
    /// The load failed because the project does not exist (404).
    pub load_gone: RwSignal<bool>,
    pub comments: RwSignal<Vec<PreviewComment>>,
    pub comment_mode: RwSignal<bool>,
    /// The element picked in comment mode, waiting for its note.
    pub comment_draft: RwSignal<Option<Picked>>,
    /// Where each shown comment's element is now (`od:located`), so pins
    /// follow scroll and reflow; missing ids fall back to the stored box.
    pub pin_rects: RwSignal<HashMap<String, CommentRect>>,
    /// Anchor state of each located comment (Comments tab).
    pub anchors: RwSignal<HashMap<String, AnchorState>>,
    /// Bumped on every `od:ready`: the new document needs `od:locate`.
    pub frame_ready: RwSignal<u32>,
    /// Set once the previewed file turns out to be a deck.
    pub deck: RwSignal<Option<DeckState>>,
    /// What the exporters accept for the previewed page (`od:export-fit`);
    /// `None` until its bridge says.
    pub export_fit: RwSignal<Option<pb::ExportFit>>,
    pub notes: RwSignal<Vec<String>>,
    pub show_notes: RwSignal<bool>,
    pub presenting: RwSignal<bool>,
    pub switcher_open: RwSignal<bool>,
    /// Under 480 px: the one pane shown. Opening a file shows the preview.
    pub pane: RwSignal<NarrowPane>,
    pub history_open: RwSignal<bool>,
    /// The Export menu is open (its button, or the Export chip in the chat).
    pub export_open: RwSignal<bool>,
    /// Under 480 px: the preview's edit and zoom tools show, on a row of
    /// their own, instead of folded behind "More actions".
    pub preview_tools_open: RwSignal<bool>,
    pub viewing: RwSignal<Option<ViewedVersion>>,
    /// Inspect, text edit, draw, tweaks, palette, lint (spec 1.2).
    pub tools: Tools,
    /// Mode of the open conversation, sent with each run.
    pub session_mode: RwSignal<SessionMode>,
    /// The project's Design Jury switch.
    pub jury: RwSignal<bool>,
    pub memory: RwSignal<Vec<MemoryEntry>>,
    pub side: SideChat,
    /// The file tree on the right; remembered in `localStorage`.
    pub files_open: RwSignal<bool>,
    /// Source of the active file instead of its rendering.
    pub show_code: RwSignal<bool>,
    /// Preview zoom, percent (one of [`ZOOM_STEPS`]).
    pub zoom: RwSignal<u32>,
    /// The largest zoom the device frame fits the preview at (`None` for
    /// Desktop, which follows the pane).
    zoom_cap: RwSignal<Option<u32>>,
    /// Zoomed in past the fit ([`zoom_move`]), from this zoom: the cap
    /// does not apply.
    past_fit: RwSignal<Option<u32>>,
    /// Composer context, uploads and the chat pane's size (track B3-chat).
    pub chat: super::composer::ChatState,
    /// Last slide shown per deck file, restored when the deck reloads.
    last_slide: StoredValue<Option<(String, u32)>>,
    /// A restore in flight: (file, slide, states left to wait for).
    restoring: StoredValue<Option<(String, u32, u8)>>,
    /// Number of the latest file list request; older answers are dropped.
    files_seq: StoredValue<u32>,
    run_stream: StoredValue<Option<SendWrapper<Sse>>>,
    project_stream: StoredValue<Option<SendWrapper<Sse>>>,
}

pub(super) fn is_previewable(f: &ProjectFile) -> bool {
    !f.path.starts_with('.') && !f.path.contains("/.")
}

pub fn is_html_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm")
}

/// The file to preview first: the project's entry file, else `index.html`,
/// else the first HTML file.
pub(super) fn pick_entry(project: Option<&Project>, files: &[ProjectFile]) -> Option<String> {
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
    /// Whether something goes with the next message besides its text:
    /// attached comments, a drawing or uploaded files. Then Send works
    /// with an empty composer.
    pub fn has_attached(self) -> bool {
        self.tools.attachments.with(|a| !a.is_empty())
            || self
                .comments
                .with(|c| c.iter().any(|c| c.status == CommentStatus::Attached))
    }

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
    /// Opens `path` in a tab and, on a phone-width window, shows the preview.
    pub fn open_file(self, path: &str) {
        self.show_tab(path);
        if self.pane.get_untracked() != NarrowPane::Preview {
            self.pane.set(NarrowPane::Preview);
        }
    }

    /// Opens `path` in a tab without leaving the pane shown (the entry file
    /// opened on load must not pull a phone away from the chat).
    pub(crate) fn show_tab(self, path: &str) {
        self.side.open.set(false);
        self.active_term.set(None);
        self.tabs.update(|t| t.open(path));
        self.recent.update(|r| touch_recent(r, path));
        self.viewing.set(None);
    }

    pub fn refresh_files(self) {
        let seq = self.files_seq.get_value().wrapping_add(1);
        self.files_seq.set_value(seq);
        leptos::task::spawn_local(async move {
            let Ok(res) = api::get::<FilesResponse>(&format!("{}/files", self.base())).await else {
                return;
            };
            // A slower, older answer must not undo a newer one (an upload
            // and the watcher's event both refresh).
            if self.files_seq.try_get_value() != Some(seq) {
                return;
            }
            let mut files: Vec<ProjectFile> =
                res.files.into_iter().filter(is_previewable).collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            // Live artifact tabs are not files.
            self.tabs.update(|t| {
                t.retain_existing(|p| {
                    super::live_view::is_live(p) || files.iter().any(|f| f.path == p)
                })
            });
            if self.active_untracked().is_none() {
                let entry = self
                    .project
                    .with_untracked(|p| pick_entry(p.as_ref(), &files));
                if let Some(entry) = entry {
                    self.show_tab(&entry);
                }
            }
            self.files.set(files);
        });
    }

    pub fn toggle_files(self) {
        let next = !self.files_open.get_untracked();
        self.files_open.set(next);
        super::store_flag(FILES_OPEN_KEY, next);
    }

    /// One zoom step from the size shown, when it changes that size.
    pub fn zoom_by(self, up: bool) {
        let (zoom, cap) = (self.zoom.get_untracked(), self.zoom_cap.get_untracked());
        if let Some((z, past)) = zoom_move(zoom, cap, self.past_fit.get_untracked(), up) {
            self.zoom.set(z);
            self.past_fit.set(past);
        }
    }

    fn can_zoom(self, up: bool) -> bool {
        zoom_move(
            self.zoom.get(),
            self.zoom_cap.get(),
            self.past_fit.get(),
            up,
        )
        .is_some()
    }

    pub fn reset_zoom(self) {
        self.zoom.set(100);
        self.past_fit.set(None);
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
        // The stored draft of this conversation goes once the message is out:
        // a queued one is not (the queue lives only in memory, so a reload
        // brings it back to the composer).
        if self.live_run.get_untracked().is_some() {
            self.queue.update(|q| {
                q.push(Queued {
                    id: api::random_id(),
                    text,
                });
            });
            return;
        }
        let draft_key = self.draft_key();
        let user_id = api::random_id();
        let assistant_id = api::random_id();
        let turn = RwSignal::new(AssistantTurn::new());
        let files = self.tools.attachments.get_untracked();
        self.items.update(|items| {
            items.push(ChatItem::user(user_id.clone(), text.clone()).with_attachments(files));
            items.push(ChatItem::assistant(assistant_id.clone(), turn));
        });
        let Some(agent_id) = shell.agent_id() else {
            fail(turn, t(T::NoAgentSelected).to_owned());
            return;
        };
        let project = self.project.get_untracked();
        let selection = shell.config.with_untracked(|c| {
            c.as_ref()
                .map(|c| crate::models::selection(c, &agent_id))
                .unwrap_or_default()
        });
        // Mentioned skills, plugin and MCP servers still in the text.
        let staged = self.chat.context.get_untracked();
        let extras = crate::mention::extras(&staged, &text);
        let attached: Vec<PreviewComment> = self.comments.with_untracked(|c| {
            c.iter()
                .filter(|c| c.status == CommentStatus::Attached)
                .cloned()
                .collect()
        });
        let req = ChatRequest {
            agent_id,
            message: extras.message,
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
            model: selection.model,
            reasoning: selection.reasoning,
            skill_ids: extras.skill_ids,
            plugin_id: extras.plugin_id,
            plugin_inputs: extras.plugin_inputs,
            locale: Some(shell.locale.get_untracked().tag().to_owned()),
            session_mode: Some(self.session_mode.get_untracked()),
            attachments: {
                // Images on the attached comments go along too.
                let mut files = self.tools.attachments.get_untracked();
                for path in attached_images(&attached) {
                    if !files.contains(&path) {
                        files.push(path);
                    }
                }
                files
            },
        };
        self.tools.deliverable.set(None);
        self.live_run.set(Some(String::new()));
        let req_attachments = req.attachments.clone();
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateRunResponse>("/api/runs", &req).await {
                Ok(res) => {
                    if let Some(k) = &draft_key {
                        super::composer::clear_draft_if(k, &text);
                    }
                    self.tools.attachments.update(|a| {
                        a.retain(|p| !req_attachments.contains(p));
                    });
                    self.chat
                        .context
                        .update(|c| c.retain(|x| !staged.contains(x)));
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

    /// `localStorage` key of the open conversation's composer draft.
    fn draft_key(self) -> Option<String> {
        self.conversation_id
            .get_untracked()
            .map(|c| super::composer::draft_key(&self.project_id.get_value(), &c))
    }

    /// Takes a queued message off the queue once it is steered or deleted:
    /// its stored draft goes too, as after a send (QA-019).
    pub fn drop_queued(self, id: &str) {
        let text = self.queue.try_update(|q| {
            let i = q.iter().position(|x| x.id == id)?;
            Some(q.remove(i).text)
        });
        if let (Some(Some(text)), Some(key)) = (text, self.draft_key()) {
            super::composer::clear_draft_if(&key, &text);
        }
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
                    self.drop_queued(&id);
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
                        self.record_versions(files.clone(), prompt.get_value());
                        self.check_deliverable(run.get_value(), files);
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
            &["file-changed", "reload-all", "conversation-created"],
            move |name, data| {
                if name == "conversation-created" {
                    self.reload_conversations();
                } else if name == "reload-all" {
                    // The watcher lost events: any file may have changed.
                    self.refresh_files();
                    self.reload_preview();
                } else if let Ok(changed) = serde_json::from_str::<FileChanged>(&data) {
                    self.on_file_changed(changed);
                }
            },
            || {},
        )
        .map(|s| s.on_connection(move |up| self.on_project_link(up)));
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
        if self.conversation_id.get_untracked().as_deref() != Some(cid.as_str()) {
            return; // Another conversation was opened meanwhile.
        }
        let mut resume = None;
        let items = res
            .messages
            .iter()
            .map(|m| match m.role {
                MessageRole::User => ChatItem::user(m.id.clone(), m.content.clone())
                    .with_attachments(m.attachments.clone()),
                MessageRole::Assistant => {
                    let turn = RwSignal::new(AssistantTurn::from_stored(m));
                    if turn.with_untracked(|t| t.state.is_live()) {
                        resume = m.run_id.clone().map(|r| (r, turn, m.id.clone()));
                    }
                    ChatItem::assistant(m.id.clone(), turn)
                }
            })
            .collect();
        // A message sent while this loaded is in `items` already, its turn
        // attached to the run: merge, do not replace (QA-002).
        let local = self.items.get_untracked();
        let resume = resume.filter(|(_, _, id)| !local.iter().any(|l| &l.id == id));
        self.items
            .set(crate::chat::merge_loaded(items, local, |i| i.id.as_str()));
        if let Some((run_id, turn, _)) = resume {
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
            // Comments are placed on the rendered page.
            self.show_code.set(false);
        }
        self.comment_mode.set(on);
        if !on {
            self.comment_draft.set(None);
        }
        bridge::post_main(&pb::select_mode(on, false));
    }

    pub fn slide(self, cmd: Slide) {
        bridge::post_main(&cmd.message());
    }

    fn on_bridge(self, source: Source, msg: BridgeMsg) {
        match (source, msg) {
            (Source::Main, BridgeMsg::Ready) => {
                if self.comment_mode.get_untracked() {
                    bridge::post_main(&pb::select_mode(true, false));
                }
                self.pin_rects.set(HashMap::new());
                self.frame_ready.update(|n| *n = n.wrapping_add(1));
                self.tools_ready();
            }
            (Source::Main, BridgeMsg::Located { items }) => {
                self.on_located(&items);
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
                    Some(i) => {
                        self.restoring
                            .set_value(path.map(|p| (p, i, RESTORE_PATIENCE)));
                        self.slide(Slide::Go(i));
                    }
                    None => bridge::post_main(&pb::slide_state_request()),
                }
            }
            (Source::Main, BridgeMsg::SlideState { active, count }) => {
                self.deck.set(Some(DeckState { active, count }));
                let path = self.active_untracked();
                // The bridge reports slide 0 right after `deck-ready`, before
                // the restore lands; a reload in between must not take that
                // for where the user was.
                let pending = self.restoring.get_value();
                if let Some((p, target, left)) = pending.filter(|(p, ..)| Some(p) == path.as_ref())
                {
                    if active != target && left > 0 {
                        self.restoring.set_value(Some((p, target, left - 1)));
                        return;
                    }
                    self.restoring.set_value(None);
                }
                if let Some(path) = path {
                    self.last_slide.set_value(Some((path, active)));
                }
            }
            (Source::Main, BridgeMsg::ExportFit(fit)) => self.export_fit.set(Some(fit)),
            (Source::Main, BridgeMsg::Esc) => self.presenting.set(false),
            (Source::Main, BridgeMsg::Select(picked)) => {
                if self.comment_mode.get_untracked() {
                    self.comment_draft.set(Some(picked));
                } else if self.tools.tool.get_untracked() == Some(Tool::Inspect) {
                    self.inspect(&picked.selector);
                } else if self.tools.tool.get_untracked() == Some(Tool::Manual) {
                    self.manual_select(&picked.selector);
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
                "p" => {
                    self.switcher_open.set(true);
                    true
                }
                "b" => {
                    self.toggle_files();
                    true
                }
                "=" | "+" => {
                    self.zoom_by(true);
                    true
                }
                "-" => {
                    self.zoom_by(false);
                    true
                }
                "0" => {
                    self.zoom.set(100);
                    true
                }
                "w" => {
                    if let Some(term) = self.active_term.get_untracked() {
                        terminal::close(self, term);
                    } else if let Some(active) = self.active_untracked() {
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
    pub async fn load(self, shell: Shell) {
        let base = self.base();
        let project = match api::get::<ProjectResponse>(&base).await {
            Ok(r) => r.project,
            Err(e) => {
                // Not a toast: without its project the Studio is unusable,
                // so the error stays until a retry loads it.
                self.load_gone.set(e.status == 404);
                self.load_error.set(Some(e.to_string()));
                return;
            }
        };
        self.load_error.set(None);
        let pending = project.pending_prompt.clone();
        self.project.set(Some(project));
        self.refresh_files();
        self.watch_project();
        self.load_w3();
        terminal::restore(self);

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
            // Files and mentions staged on Home ride with the brief.
            let mut meta = self
                .project
                .with_untracked(|p| p.as_ref().and_then(|p| p.metadata.clone()));
            let staged = meta
                .as_mut()
                .map(super::composer::take_pending)
                .unwrap_or_default();
            // Clear first so a reload never sends the brief twice.
            let mut patch = json!({ "pendingPrompt": null });
            if staged.is_some() {
                patch["metadata"] = json!(meta);
            }
            let _ = api::patch::<_, ProjectResponse>(&base, &patch).await;
            if let Some((files, context)) = staged {
                self.tools
                    .attachments
                    .update(|a| crate::workspace::add_missing(a, files));
                self.chat.context.set(context);
            }
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

/// Icon of a file, from its extension.
pub fn file_glyph(path: &str) -> Glyph {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" | "css" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "svelte" | "vue" => {
            Glyph::FileCode
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "ico" => Glyph::FileImage,
        "md" | "markdown" => Glyph::Markdown,
        "json" | "toml" | "yaml" | "yml" => Glyph::Braces,
        "txt" | "csv" => Glyph::FileText,
        _ => Glyph::File,
    }
}

/// Whether a key press belongs to a text field rather than the workspace.
pub(super) fn is_editing(ev: &web_sys::KeyboardEvent) -> bool {
    is_editing_target(ev.target())
}

/// Whether `target` is a text field (input, textarea, editable, terminal).
pub(super) fn is_editing_target(target: Option<web_sys::EventTarget>) -> bool {
    use wasm_bindgen::JsCast;
    let Some(el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
        return false;
    };
    matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        || el.get_attribute("contenteditable").is_some()
        || el.get_attribute("data-terminal").is_some()
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
        draft: RwSignal::new(String::new()),
        queue: RwSignal::new(Vec::new()),
        files: RwSignal::new(Vec::new()),
        tabs: RwSignal::new(Tabs::default()),
        terms: RwSignal::new(Vec::new()),
        active_term: RwSignal::new(None),
        recent: RwSignal::new(Vec::new()),
        preview_rev: RwSignal::new(0),
        notice: RwSignal::new(None),
        link_down: RwSignal::new(false),
        load_error: RwSignal::new(None),
        load_gone: RwSignal::new(false),
        comments: RwSignal::new(Vec::new()),
        comment_mode: RwSignal::new(false),
        comment_draft: RwSignal::new(None),
        pin_rects: RwSignal::new(HashMap::new()),
        anchors: RwSignal::new(HashMap::new()),
        frame_ready: RwSignal::new(0),
        deck: RwSignal::new(None),
        export_fit: RwSignal::new(None),
        notes: RwSignal::new(Vec::new()),
        show_notes: RwSignal::new(false),
        presenting: RwSignal::new(false),
        switcher_open: RwSignal::new(false),
        pane: RwSignal::new(NarrowPane::Chat),
        history_open: RwSignal::new(false),
        export_open: RwSignal::new(false),
        preview_tools_open: RwSignal::new(false),
        viewing: RwSignal::new(None),
        session_mode: RwSignal::new(SessionMode::default()),
        jury: RwSignal::new(false),
        memory: RwSignal::new(Vec::new()),
        side: SideChat::new(),
        files_open: RwSignal::new(super::stored_flag(FILES_OPEN_KEY).unwrap_or_else(wide_window)),
        show_code: RwSignal::new(false),
        zoom: RwSignal::new(100),
        zoom_cap: RwSignal::new(None),
        past_fit: RwSignal::new(None),
        chat: super::composer::ChatState::new(),
        tools: Tools::new(),
        last_slide: StoredValue::new(None),
        restoring: StoredValue::new(None),
        files_seq: StoredValue::new(0),
        run_stream: StoredValue::new(None),
        project_stream: StoredValue::new(None),
    };
    provide_context(ctx);
    let files_ui = FilesUi::new();
    provide_context(files_ui);
    preview_tools::watch_lint(ctx);
    super::deck::sync_deck(ctx);
    super::composer::keep_draft(ctx.draft, ctx.chat, ctx.project_id, ctx.conversation_id);
    // The command palette offers this Studio's commands while it is open.
    shell.studio.set(Some(ctx));

    // Wait for settings so the first turn uses the configured agent.
    let started = StoredValue::new(false);
    Effect::new(move |_| {
        if shell.config.with(Option::is_some) && !started.get_value() {
            started.set_value(true);
            leptos::task::spawn_local(ctx.load(shell));
        }
    });
    let keys = window_event_listener(ev::keydown, move |ev| ctx.on_key(&ev));
    let more_keys = window_event_listener(ev::keydown, move |ev| {
        super::files_panel::on_studio_key(ctx, files_ui, &ev);
    });
    let messages = window_event_listener(ev::message, move |ev| {
        if let Some((source, msg)) = bridge::decode(&ev) {
            ctx.on_bridge(source, msg);
        }
    });
    on_cleanup(move || {
        keys.remove();
        more_keys.remove();
        messages.remove();
        ctx.run_stream.set_value(None);
        ctx.project_stream.set_value(None);
        ctx.side.close_stream();
        shell.studio.set(None);
    });

    // Under 480 px (a phone browser) one pane shows at a time, picked here;
    // the CSS hides the switch on wider windows.
    let pane = ctx.pane;
    Effect::new(move |_| {
        if !ctx.files_open.get() && pane.get_untracked() == NarrowPane::Files {
            pane.set(NarrowPane::Preview);
        }
    });
    let pick = move |p: NarrowPane| {
        match p {
            NarrowPane::Chat => ctx.chat.collapsed.set(false),
            NarrowPane::Files => ctx.files_open.set(true),
            // The side chat covers the preview on a phone: Preview brings
            // the page back.
            NarrowPane::Preview => ctx.side.open.set(false),
        }
        pane.set(p);
    };
    let pane_seg = move |p: NarrowPane, label: T| {
        view! {
            <button class="seg" role="radio" data-testid=format!("pane-{}", p.key())
                aria-checked=move || (pane.get() == p).to_string() tabindex=move || super::focus::roving(pane.get() == p)
                class:active=move || pane.get() == p
                on:click=move |_| pick(p)>
                {t(label)}
            </button>
        }
    };
    view! {
        <div
            class="studio"
            class:chat-collapsed=move || ctx.chat.collapsed.get()
            class:load-failed=move || ctx.load_error.with(Option::is_some)
            class:focus=move || files_ui.focus.get()
            data-pane=move || pane.get().key()
            style=move || ctx.chat.width.get().map(|w| format!("--chat-w:{w:.0}px"))
        >
            <div class="segmented small studio-switch" role="radiogroup" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::ViewMode) data-testid="studio-switch">
                {pane_seg(NarrowPane::Chat, T::Chat)}
                {pane_seg(NarrowPane::Preview, T::Preview)}
                {pane_seg(NarrowPane::Files, T::Files)}
            </div>
            <ChatPane ctx=ctx/>
            <Show when=move || ctx.chat.collapsed.get()>
                <button class="btn small show-chat" data-testid="show-chat" on:click=move |_| ctx.chat.collapsed.set(false)>
                    {icon(Glyph::SidebarUnfold)}{t(T::ShowChat)}
                </button>
            </Show>
            <section class="workspace" class:files-closed=move || !ctx.files_open.get()>
                <PreviewPane ctx=ctx/>
                <Show when=move || ctx.files_open.get()>
                    <FilesPanel ctx=ctx/>
                </Show>
            </section>
            <Show when=move || ctx.switcher_open.get()>
                <QuickSwitcher ctx=ctx/>
            </Show>
            <NoticeToast ctx=ctx/>
            <super::recovery_tip::WorkspaceRecoveryTip ctx=ctx/>
            <FilesToast/>
            <super::project_actions::BrandReady ctx=ctx/>
        </div>
    }
}

#[component]
fn TabsBar(ctx: StudioCtx) -> impl IntoView {
    let files_ui = FilesUi::get();
    // Left/Right move between tabs, as in the WAI-ARIA tabs pattern; a
    // tab opens on Enter or a click, not on focus (it loads a file).
    let tab_keys = move |ev: ev::KeyboardEvent| super::focus::group_keys(&ev, false);
    view! {
        <div class="tabs" data-testid="tabs">
            // Only the tabs sit in the tablist: the "+" launcher and the focus
            // toggle are siblings, and each close "×" is a mouse shortcut
            // hidden from assistive tech (Delete on a focused tab closes it).
            <div class="tab-list" role="tablist" on:keydown=tab_keys>
            {move || {
                let tabs = ctx.tabs.get();
                let side_open = ctx.side.open.get();
                let side_tab = view! {
                    <div class="tab side-tab" class:active=side_open role="presentation" data-testid="side-chat-tab" title=t(T::SideChatHint)>
                        <button class="tab-name" role="tab" aria-selected=side_open.to_string() on:click=move |_| ctx.side.show(ctx)>{icon(Glyph::SideChat)}{t(T::SideChat)}</button>
                    </div>
                };
                let term_active = ctx.active_term.with(Option::is_some);
                let files = tabs.open
                    .iter()
                    .map(|path| {
                        let active = !side_open && !term_active && tabs.active.as_deref() == Some(path.as_str());
                        let live = super::live_view::is_live(path);
                        let name = if live {
                            super::live_view::tab_label(files_ui, path)
                        } else {
                            path.rsplit('/').next().unwrap_or(path).to_owned()
                        };
                        let open = path.clone();
                        let close = path.clone();
                        let close_key = path.clone();
                        view! {
                            <div class="tab" class:active=active role="presentation" data-testid="tab" title=path.clone()>
                                <button class="tab-name" role="tab" aria-selected=active.to_string() aria-keyshortcuts="Delete"
                                    on:click=move |_| ctx.open_file(&open)
                                    on:keydown=move |ev| if ev.key() == "Delete" { ev.prevent_default(); ctx.tabs.update(|t| t.close(&close_key)); }
                                >{icon(if live { Glyph::Sparkle } else { file_glyph(&name) })}{name.clone()}</button>
                                <button
                                    class="tab-close"
                                    tabindex="-1"
                                    aria-hidden="true"
                                    title=t(T::CloseTab)
                                    on:click=move |_| ctx.tabs.update(|t| t.close(&close))
                                >
                                    {icon(Glyph::Close)}
                                </button>
                            </div>
                        }
                    })
                    .collect_view();
                view! { {files}{side_tab} }
            }}
            {move || {
                ctx.terms
                    .get()
                    .into_iter()
                    .map(|term| {
                        let active = !ctx.side.open.get() && ctx.active_term.with(|a| a.as_deref() == Some(term.id.as_str()));
                        let (show, close, close_key) = (term.id.clone(), term.id.clone(), term.id.clone());
                        view! {
                            <div class="tab term-tab" class:active=active role="presentation" data-testid="terminal-tab">
                                <button class="tab-name" role="tab" aria-selected=active.to_string() aria-keyshortcuts="Delete"
                                    on:click=move |_| { ctx.side.open.set(false); ctx.active_term.set(Some(show.clone())); }
                                    on:keydown=move |ev| if ev.key() == "Delete" { ev.prevent_default(); terminal::close(ctx, close_key.clone()); }
                                >
                                    {icon(Glyph::Terminal)}
                                    {tf(T::TerminalTab, &[&term.number.to_string()])}
                                </button>
                                <button
                                    class="tab-close"
                                    tabindex="-1"
                                    aria-hidden="true"
                                    title=t(T::CloseTerminal)
                                    data-testid="terminal-close"
                                    on:click=move |_| terminal::close(ctx, close.clone())
                                >
                                    {icon(Glyph::Close)}
                                </button>
                            </div>
                        }
                    })
                    .collect_view()
            }}
            </div>
            <TabLauncher ctx=ctx/>
            <span class="spacer"></span>
            <super::files_panel::FocusToggle/>
        </div>
    }
}

#[component]
fn PreviewPane(ctx: StudioCtx) -> impl IntoView {
    let device = RwSignal::new(Device::Desktop);
    let stage = NodeRef::<leptos::html::Div>::new();
    let stage_size = observe_size(stage);
    let stage_width = Memo::new(move |_| stage_size.get().0);
    // A device wider than the stage is scaled down to fit, as the original.
    let zoom = Memo::new(move |_| match ctx.past_fit.get() {
        Some(_) => ctx.zoom.get(),
        None => fit_zoom(device.get(), ctx.zoom.get(), stage_width.get()),
    });
    Effect::new(move |_| {
        let cap = fit_cap(device.get(), stage_width.get());
        ctx.zoom_cap.set(cap);
        // Past the fit only while the frame fits below the smallest step;
        // then back to the zoom chosen before.
        if let Some(before) = leave_past_fit(cap, ctx.past_fit.get_untracked()) {
            ctx.zoom.set(before);
            ctx.past_fit.set(None);
        }
    });
    // A deck lays out at its design size and is scaled to the stage, so a
    // narrow pane shows the same slide, only smaller. Remembered per file:
    // a reload (an auto-save) keeps the slide size instead of flashing the
    // reflowed page until the bridge reports the deck again.
    let deck_file = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        if ctx.deck.with(Option::is_some) {
            deck_file.set(ctx.active_untracked());
        }
    });
    let slide_scale = Memo::new(move |_| {
        let deck =
            ctx.deck.with(Option::is_some) || deck_file.with(|f| f.is_some() && *f == ctx.active());
        if !deck || device.get() != Device::Desktop {
            return None;
        }
        slide_fit(stage_size.get(), zoom.get())
    });
    let frame_box = move || match slide_scale.get() {
        Some(scale) => slide_box(scale),
        None => zoom_box(device.get(), zoom.get()),
    };
    let frame_inner = move || match slide_scale.get() {
        Some(scale) => slide_frame(scale),
        None => zoomed(device.get(), zoom.get()),
    };
    let src = Memo::new(move |_| {
        let path = ctx.active()?;
        let rev = ctx.preview_rev.get();
        let raw = format!(
            "/api/projects/{}/raw/{}",
            api::seg(&ctx.project_id.get_value()),
            api::file_path(&path)
        );
        // HTML goes through the bridge: deck control, comments, guards.
        // A React component renders through its own page.
        Some(if is_html_path(&path) {
            format!("{raw}?bridge=1&v={rev}")
        } else if crate::jsx_export::is_component(&path) {
            format!(
                "/api/projects/{}/react/{}?v={rev}",
                api::seg(&ctx.project_id.get_value()),
                api::file_path(&path)
            )
        } else {
            format!("{raw}?v={rev}")
        })
    });
    // A load that reports no deck (the file is a page now) forgets it. The
    // bridge posts `od:deck-ready` on the frame's own load, so its message
    // can land just after the frame's `load` event: give it a moment.
    let forget_deck = move |_| {
        let loaded = src.get_untracked();
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(DECK_READY_WITHIN_MS).await;
            // `try_`: the studio may be gone by then.
            if src.try_get_untracked() == Some(loaded)
                && ctx.deck.try_with_untracked(Option::is_none) == Some(true)
            {
                deck_file.try_set(None);
            }
        });
    };
    // A new document: deck state comes back with its `od:deck-ready`.
    Effect::new(move |_| {
        src.track();
        ctx.deck.set(None);
        ctx.export_fit.set(None);
        ctx.comment_draft.set(None);
    });
    // Speaker notes follow the document and its slide count.
    let notes_key = Memo::new(move |_| (src.get(), ctx.deck.with(|d| d.map(|d| d.count))));
    Effect::new(move |_| match notes_key.get() {
        (Some(_), Some(count)) => super::deck::load_notes(ctx, count),
        _ => ctx.notes.set(Vec::new()),
    });
    // A memo: a files refresh (any upload) must not rebuild the frame and
    // what sits on it (comment draft, drawing) unless this flips.
    let image_shown = Memo::new(move |_| {
        let path = ctx.active();
        ctx.files.with(|files| {
            files
                .iter()
                .find(|f| Some(&f.path) == path.as_ref())
                .is_some_and(|f| matches!(f.kind, FileKind::Image))
        })
    });
    let is_image = move || image_shown.get();
    let is_html = move || ctx.active().is_some_and(|p| is_html_path(&p));
    // Markdown, sketches and documents have their own views, kept across
    // preview reloads (an auto-save reloads the preview).
    let special = Memo::new(move |_| {
        let path = ctx.active()?;
        if super::live_view::is_live(&path) {
            return Some(Special::Live(path));
        }
        if ctx.show_code.get() || ctx.viewing.with(Option::is_some) {
            return None;
        }
        if is_markdown(&path) {
            return Some(Special::Markdown(path));
        }
        if is_sketch(&path) {
            return Some(Special::Sketch(path));
        }
        let doc = ctx
            .files
            .with(|f| f.iter().any(|f| f.path == path && is_document(f)));
        doc.then_some(Special::Doc(path))
    });
    // Rendered pages, images and versions; reloads with the preview.
    let page = move || match (src.get(), ctx.viewing.get()) {
        (None, _) => view! { <PreviewEmpty ctx=ctx/> }.into_any(),
        // A version on view shows its own source, not the live file's.
        (Some(_), Some(v)) if ctx.show_code.get() => {
            view! { <CodeView ctx=ctx version=Some(v.content)/> }.into_any()
        }
        (Some(_), None) if ctx.show_code.get() && !is_image() => {
            view! { <CodeView ctx=ctx version=None/> }.into_any()
        }
        (Some(_), Some(v)) => view! {
            <div class="frame-zoom" style=move || zoom_box(device.get(), zoom.get())>
            <iframe
                class="preview-frame"
                data-testid="version-preview"
                title=tf(T::ViewingVersion, &[&v.version.to_string()])
                sandbox="allow-scripts"
                srcdoc=v.content
                style=move || zoomed(device.get(), zoom.get())
            ></iframe>
            </div>
        }
        .into_any(),
        (Some(url), None) if is_image() => {
            view! { <img class="preview-image" src=url alt=t(T::Preview)/> }.into_any()
        }
        (Some(url), None) => {
            // Each load of a bridged frame gets a fresh
            // edit-key id (od-preview `edit_key`).
            let url = if url.contains("bridge=1") {
                format!("{url}&key={}", ctx.tools.new_frame_kid())
            } else {
                url
            };
            view! {
                <div class="frame-zoom" style=frame_box>
                <div class="frame-wrap" style=frame_inner>
                    // No allow-same-origin: artifact scripts run in an
                    // opaque origin and cannot call the daemon API.
                    <iframe
                        class="preview-frame"
                        data-testid="preview"
                        title=t(T::Preview)
                        sandbox="allow-scripts allow-forms allow-popups allow-modals"
                        src=url
                        on:load=forget_deck
                    ></iframe>
                    <CommentLayer ctx=ctx/>
                    {move || (ctx.tools.tool.get() == Some(Tool::Draw)).then(|| view! { <DrawLayer ctx=ctx/> })}
                </div>
                </div>
            }.into_any()
        }
    };
    view! {
        <section class="preview" class:presenting=move || ctx.presenting.get() data-testid="preview-pane">
            <div class="preview-top">
                <TabsBar ctx=ctx/>
                <div class="preview-actions">
                    <Show when=move || is_html() && !ctx.side.open.get() && ctx.active_term.with(Option::is_none)>
                        <button
                            class="icon-btn"
                            class:active=move || ctx.history_open.get()
                            aria-pressed=move || ctx.history_open.get().to_string()
                            aria-label=t(T::History)
                            data-tip=t(T::History)
                            data-testid="history"
                            on:click=move |_| ctx.history_open.update(|o| *o = !*o)
                        >
                            {icon(Glyph::History)}
                        </button>
                    </Show>
                    <ExportMenu ctx=ctx/>
                </div>
            </div>
            <Show when=move || !ctx.side.open.get() fallback=move || view! { <SideChatView ctx=ctx/> }>
            {move || ctx.active_term.get().map(|id| view! { <TerminalView id=id/> })}
            <div class="preview-main" style=move || if ctx.active_term.with(Option::is_some) { "display:none" } else { "display:contents" }>
            <div class="pane-head preview-head" class:tools-open=move || ctx.preview_tools_open.get() data-testid="preview-toolbar">
                <button class="icon-btn reload-preview" aria-label=t(T::ReloadPreview) data-tip=t(T::ReloadPreview) on:click=move |_| ctx.reload_preview()>
                    {icon(Glyph::Refresh)}
                </button>
                <div class="segmented small" role="radiogroup" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::ViewAs)>
                    <button
                        class="seg"
                        role="radio"
                        data-testid="view-preview"
                        aria-label=t(T::Preview)
                        aria-checked=move || (!ctx.show_code.get()).to_string() tabindex=move || super::focus::roving(!ctx.show_code.get())
                        class:active=move || !ctx.show_code.get()
                        on:click=move |_| ctx.show_code.set(false)
                    >
                        {icon(Glyph::Eye)}<span class="btn-label">{t(T::Preview)}</span>
                    </button>
                    <button
                        class="seg"
                        role="radio"
                        data-testid="view-code"
                        aria-label=t(T::Code)
                        aria-checked=move || ctx.show_code.get().to_string() tabindex=move || super::focus::roving(ctx.show_code.get())
                        class:active=move || ctx.show_code.get()
                        disabled=move || is_image() || ctx.active().is_none()
                        on:click=move |_| ctx.show_code.set(true)
                    >
                        {icon(Glyph::Code)}<span class="btn-label">{t(T::Code)}</span>
                    </button>
                </div>
                <div class="segmented small icons" role="radiogroup" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::Device)>
                    {Device::ALL
                        .into_iter()
                        .map(|d| {
                            view! {
                                <button
                                    class="seg"
                                    role="radio"
                                    aria-checked=move || (device.get() == d).to_string() tabindex=move || super::focus::roving(device.get() == d)
                                    aria-label=d.label()
                                    data-tip=d.label()
                                    class:active=move || device.get() == d
                                    disabled=move || ctx.show_code.get()
                                    on:click=move |_| device.set(d)
                                >
                                    {icon(d.glyph())}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                // Phone width: the tools after this fold behind it, so the
                // head stays one row and the page gets the height.
                <button
                    class="icon-btn preview-more"
                    class:active=move || ctx.preview_tools_open.get()
                    aria-label=t(T::MoreActions)
                    data-tip=t(T::MoreActions)
                    data-tip-pos="left"
                    aria-expanded=move || ctx.preview_tools_open.get().to_string()
                    data-testid="preview-more"
                    on:click=move |_| ctx.preview_tools_open.update(|o| *o = !*o)
                >
                    {icon(Glyph::More)}
                </button>
                <span class="pane-title path-title mono" title=move || ctx.active().unwrap_or_default()>
                    {move || ctx.active().unwrap_or_default()}
                </span>
                <Show when=move || is_html() && ctx.viewing.with(Option::is_none) && !ctx.show_code.get()>
                    <span class="head-divider" aria-hidden="true"></span>
                    <ToolBar ctx=ctx/>
                    <span class="head-divider" aria-hidden="true"></span>
                </Show>
                <div class="zoom" role="group" aria-label=t(T::Zoom)>
                    <button class="icon-btn small" aria-label=t(T::ZoomOut) data-tip=t(T::ZoomOut) data-testid="zoom-out"
                        disabled=move || ctx.show_code.get() || !ctx.can_zoom(false)
                        on:click=move |_| ctx.zoom_by(false)>{icon(Glyph::ZoomOut)}</button>
                    <button class="zoom-value" data-tip=t(T::ZoomReset) aria-label=t(T::ZoomReset) data-testid="zoom-reset"
                        disabled=move || ctx.show_code.get()
                        on:click=move |_| ctx.reset_zoom()>{move || format!("{}%", zoom.get())}</button>
                    <button class="icon-btn small" aria-label=t(T::ZoomIn) data-testid="zoom-in"
                        data-tip=move || {
                            let fits = fit_stops_zoom_in(ctx.zoom.get(), ctx.zoom_cap.get(), ctx.past_fit.get());
                            t(if fits { T::ZoomFitsPane } else { T::ZoomIn })
                        }
                        disabled=move || ctx.show_code.get() || !ctx.can_zoom(true)
                        on:click=move |_| ctx.zoom_by(true)>{icon(Glyph::ZoomIn)}</button>
                </div>
                <Show when=move || !ctx.files_open.get()>
                    <button class="icon-btn" aria-label=t(T::ShowFiles) data-tip=t(T::ShowFiles) data-tip-pos="left" data-testid="show-files" on:click=move |_| ctx.toggle_files()>
                        {icon(Glyph::FilesPanel)}
                    </button>
                </Show>
            </div>
            <DeckBar ctx=ctx/>
            <Show when=move || is_html() && ctx.viewing.with(Option::is_none) && !ctx.show_code.get()>
                <ToolHints ctx=ctx/>
            </Show>
            {move || ctx.comment_mode.get().then(|| view! { <p class="mode-hint">{icon(Glyph::Comment)}{t(T::CommentModeOn)}</p> })}
            <div class="preview-body">
                <ThumbRail ctx=ctx src=src/>
                <div class="preview-stage" node_ref=stage>
                    <super::first_hint::FirstArtifactHint ctx=ctx/>
                    {move || match special.get() {
                        Some(Special::Markdown(path)) => view! { <MarkdownView ctx=ctx path=path/> }.into_any(),
                        Some(Special::Sketch(path)) => view! { <SketchView ctx=ctx path=path/> }.into_any(),
                        Some(Special::Live(path)) => view! { <super::live_view::LiveView ctx=ctx path=path/> }.into_any(),
                        Some(Special::Doc(path)) => match ctx.files.with_untracked(|f| f.iter().find(|f| f.path == path).cloned()) {
                            Some(file) => view! { <DocView ctx=ctx file=file/> }.into_any(),
                            None => ().into_any(),
                        },
                        None => page().into_any(),
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
            </div>
            </Show>
        </section>
    }
}

/// A file shown by its own view instead of the preview frame.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Special {
    Live(String),
    Markdown(String),
    Sketch(String),
    Doc(String),
}

/// The zoom a device frame is shown at: the chosen one, lowered so the
/// frame fits `stage` px (its content width; 0 while not measured).
/// Desktop follows the pane, so it keeps the chosen zoom.
fn fit_zoom(device: Device, zoom: u32, stage: f64) -> u32 {
    capped(zoom, fit_cap(device, stage))
}

/// The largest zoom (at most 100) a device frame fits `stage` px at;
/// `None` for Desktop and while the stage is not measured.
fn fit_cap(device: Device, stage: f64) -> Option<u32> {
    let px = device.px().filter(|_| stage >= 1.0)?;
    // Whole percent, rounded down so the frame never overflows.
    Some((stage * 100.0 / f64::from(px)).floor().clamp(1.0, 100.0) as u32)
}

/// Follows the content size of `node`, (width, height) (a
/// `ResizeObserver`); 0 until it has been measured or where the observer
/// is missing.
fn observe_size(node: NodeRef<leptos::html::Div>) -> RwSignal<(f64, f64)> {
    use wasm_bindgen::JsCast;
    type SizeCallback = wasm_bindgen::closure::Closure<dyn FnMut(js_sys::Array)>;
    let size = RwSignal::new((0.0, 0.0));
    let observer = StoredValue::new(None::<SendWrapper<(web_sys::ResizeObserver, SizeCallback)>>);
    Effect::new(move |_| {
        let Some(el) = node.get() else {
            return;
        };
        let on_size = SizeCallback::new(move |entries: js_sys::Array| {
            let last = entries
                .iter()
                .filter_map(|e| e.dyn_into::<web_sys::ResizeObserverEntry>().ok())
                .next_back();
            if let Some(e) = last {
                let rect = e.content_rect();
                size.set((rect.width(), rect.height()));
            }
        });
        if let Ok(obs) = web_sys::ResizeObserver::new(on_size.as_ref().unchecked_ref()) {
            obs.observe(&el);
            observer.set_value(Some(SendWrapper::new((obs, on_size))));
        }
    });
    on_cleanup(move || {
        observer.with_value(|o| {
            if let Some(o) = o {
                o.0.disconnect();
            }
        });
    });
    size
}

/// The box a zoomed preview takes in the stage: its on-screen size.
fn zoom_box(device: Device, zoom: u32) -> String {
    match device {
        Device::Desktop => "width:100%".to_owned(),
        d => format!("width:calc({} * {zoom} / 100);max-width:none", d.width()),
    }
}

/// The frame inside [`zoom_box`]: laid out at the device size, scaled to
/// fit. A transform, not CSS `zoom`, so WebKit and Chromium agree and the
/// comment pins and drawing (local coordinates) follow it. A device frame
/// is never narrowed by the pane (`max-width:none`): its page must see the
/// device width.
fn zoomed(device: Device, zoom: u32) -> String {
    if zoom == 100 {
        return match device {
            Device::Desktop => format!("width:{}", device.width()),
            d => format!("width:{};max-width:none", d.width()),
        };
    }
    let width = match device {
        Device::Desktop => format!("calc(100% * 100 / {zoom})"),
        d => d.width().to_owned(),
    };
    format!(
        "width:{width};max-width:none;height:calc(100% * 100 / {zoom});transform:scale({});transform-origin:0 0",
        f64::from(zoom) / 100.0
    )
}

/// How long after its frame loads a deck has to say `od:deck-ready`, ms.
const DECK_READY_WITHIN_MS: u32 = 1_000;

/// The size a deck lays out at, px: the deck framework's canvas
/// (`DECK_FRAMEWORK_DIRECTIVE.md`) and html-ppt's design size.
pub const SLIDE_SIZE: (f64, f64) = (1920.0, 1080.0);

/// The scale a slide is shown at in a `stage` of (width, height) px: the
/// whole slide fits, times the chosen zoom. `None` while the stage is not
/// measured.
fn slide_fit(stage: (f64, f64), zoom: u32) -> Option<f64> {
    let (w, h) = stage;
    if w < 1.0 || h < 1.0 {
        return None;
    }
    let fit = (w / SLIDE_SIZE.0).min(h / SLIDE_SIZE.1);
    Some(fit * f64::from(zoom) / 100.0)
}

/// The box a slide at `scale` takes in the stage, centred.
fn slide_box(scale: f64) -> String {
    format!(
        "width:{:.2}px;height:{:.2}px;max-width:none;align-self:var(--slide-align, safe center)",
        SLIDE_SIZE.0 * scale,
        SLIDE_SIZE.1 * scale
    )
}

/// The frame inside [`slide_box`]: the slide's design size, scaled.
fn slide_frame(scale: f64) -> String {
    format!(
        "width:{}px;height:{}px;max-width:none;transform:scale({scale:.4});transform-origin:0 0",
        SLIDE_SIZE.0, SLIDE_SIZE.1
    )
}

/// No file to show yet: what happens next and how to get there.
#[component]
fn PreviewEmpty(ctx: StudioCtx) -> impl IntoView {
    let running = move || ctx.live_run.with(Option::is_some);
    view! {
        <div class="empty-state preview-empty" data-testid="preview-empty">
            <span class="empty-icon">{move || if running() { view! { <span class="spinner"></span> }.into_any() } else { icon(Glyph::Desktop).into_any() }}</span>
            <strong>{move || if running() { t(T::PreviewWaitingTitle) } else { t(T::PreviewEmptyTitle) }}</strong>
            // No project: no steps to follow in it.
            <Show when=move || ctx.load_error.with(Option::is_none)>
                <p>{move || if running() { t(T::PreviewWaiting) } else { t(T::PreviewEmpty) }}</p>
                <ol class="empty-steps">
                    <li>{t(T::PreviewStep1)}</li>
                    <li>{t(T::PreviewStep2)}</li>
                    <li>{t(T::PreviewStep3)}</li>
                </ol>
            </Show>
        </div>
    }
}

/// How long an error notice stays, ms.
const NOTICE_MS: u32 = 8000;

/// Studio errors (stop, steer, load…) as a toast the user can close; it
/// leaves by itself after [`NOTICE_MS`] unless a newer one replaced it.
#[component]
fn NoticeToast(ctx: StudioCtx) -> impl IntoView {
    let shown = StoredValue::new(0u32);
    Effect::new(move |_| {
        if ctx.notice.with(Option::is_some) {
            let n = shown.get_value().wrapping_add(1);
            shown.set_value(n);
            leptos::task::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(NOTICE_MS).await;
                if shown.try_get_value() == Some(n) {
                    ctx.notice.set(None);
                }
            });
        }
    });
    view! {
        <div class="toast-region notice" aria-live="assertive">
            {move || ctx.notice.get().map(|text| view! {
                <div class="toast error" role="alert" data-testid="notice">
                    <span class="toast-ico">{icon(Glyph::Warning)}</span>
                    <span class="toast-text">{text}</span>
                    <button class="chip-x" aria-label=t(T::Close) on:click=move |_| ctx.notice.set(None)>{icon(Glyph::Close)}</button>
                </div>
            })}
        </div>
    }
}

/// Longest source shown in the code view; the rest is cut with a note.
const CODE_LINES: usize = 4000;

/// Read-only source of the active file, with line numbers and syntax
/// colors (`crate::highlight`); `version` is
/// the content of the version on view, shown instead of the live file.
#[component]
fn CodeView(ctx: StudioCtx, version: Option<String>) -> impl IntoView {
    let version = StoredValue::new(version);
    let text = LocalResource::new(move || {
        let path = ctx.active();
        let _rev = ctx.preview_rev.get();
        let shown = version.get_value();
        async move {
            if let Some(src) = shown {
                return Some(Ok(src));
            }
            let path = path?;
            let raw = format!("{}/raw/{}", ctx.base(), api::file_path(&path));
            Some(api::get_text(&raw).await.map_err(|e| e.to_string()))
        }
    });
    view! {
        <div class="code-view" data-testid="code-view">
            {move || match text.get() {
                None => view! { <div class="skeleton code-skeleton"></div> }.into_any(),
                Some(None) => ().into_any(),
                Some(Some(Err(e))) => view! { <p class="error-text" role="alert">{e}</p> }.into_any(),
                Some(Some(Ok(src))) => {
                    let lang = Lang::for_path(&ctx.active_untracked().unwrap_or_default());
                    let colored = highlight(&src, lang);
                    let total = colored.len();
                    let lines = colored
                        .into_iter()
                        .take(CODE_LINES)
                        .map(|line| {
                            let parts = line
                                .into_iter()
                                .map(|(tok, part)| view! { <span class=tok.class()>{part.to_owned()}</span> })
                                .collect_view();
                            view! { <span class="code-line">{parts}"\n"</span> }
                        })
                        .collect_view();
                    view! {
                        <pre class="code-src mono"><code>{lines}</code></pre>
                        {(total > CODE_LINES).then(|| view! { <p class="subtle code-cut">{tf(T::CodeCut, &[&CODE_LINES.to_string(), &total.to_string()])}</p> })}
                    }.into_any()
                }
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_moves_one_step_and_stops_at_the_ends() {
        assert_eq!(zoom_step(100, true), 125);
        assert_eq!(zoom_step(100, false), 75);
        assert_eq!(zoom_step(60, true), 67);
        assert_eq!(zoom_step(60, false), 50);
        assert_eq!(zoom_step(200, true), 200);
        assert_eq!(zoom_step(50, false), 50);
    }

    #[test]
    fn a_zoomed_frame_lays_out_at_device_size_and_scales() {
        assert_eq!(zoomed(Device::Desktop, 100), "width:100%");
        assert_eq!(
            zoomed(Device::Phone, 50),
            "width:390px;max-width:none;height:calc(100% * 100 / 50);transform:scale(0.5);transform-origin:0 0"
        );
        assert_eq!(
            zoom_box(Device::Phone, 50),
            "width:calc(390px * 50 / 100);max-width:none"
        );
        assert_eq!(zoomed(Device::Tablet, 100), "width:820px;max-width:none");
    }

    #[test]
    fn a_slide_keeps_its_design_size_and_scales_to_the_stage() {
        // A phone-wide stage, 384 × 520: the slide is 384 px wide.
        let phone = slide_fit((384.0, 520.0), 100).unwrap();
        assert!((phone - 0.2).abs() < 1e-9);
        assert_eq!(
            slide_box(phone),
            "width:384.00px;height:216.00px;max-width:none;align-self:var(--slide-align, safe center)"
        );
        // A wide, short stage: the height decides.
        assert!((slide_fit((1600.0, 540.0), 100).unwrap() - 0.5).abs() < 1e-9);
        // Zoom scales the fit; an unmeasured stage has none.
        assert!((slide_fit((960.0, 1080.0), 200).unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(slide_fit((0.0, 0.0), 100), None);
        assert_eq!(
            slide_frame(0.5),
            "width:1920px;height:1080px;max-width:none;transform:scale(0.5000);transform-origin:0 0"
        );
    }

    #[test]
    fn a_device_wider_than_the_stage_is_scaled_to_fit() {
        // 1360 px window: a 650 px stage shows Tablet at 79 %.
        assert_eq!(fit_zoom(Device::Tablet, 100, 650.0), 79);
        assert_eq!(fit_zoom(Device::Tablet, 125, 650.0), 79);
        assert_eq!(fit_zoom(Device::Tablet, 50, 650.0), 50);
        assert_eq!(fit_zoom(Device::Tablet, 100, 1200.0), 100);
        assert_eq!(fit_zoom(Device::Phone, 100, 650.0), 100);
        // Desktop follows the pane; an unmeasured stage keeps the zoom.
        assert_eq!(fit_zoom(Device::Desktop, 125, 650.0), 125);
        assert_eq!(fit_zoom(Device::Tablet, 100, 0.0), 100);
    }

    #[test]
    fn zoom_steps_from_the_size_shown_and_stops_at_the_fit() {
        // Tablet shown at 79 %: + would not grow it, - goes to 75 %.
        assert_eq!(next_zoom(100, Some(79), true), None);
        assert_eq!(next_zoom(125, Some(79), false), Some(75));
        assert_eq!(next_zoom(75, Some(79), true), Some(100));
        // Desktop: the plain steps, clamped at both ends.
        assert_eq!(next_zoom(100, None, true), Some(125));
        assert_eq!(next_zoom(200, None, true), None);
        assert_eq!(next_zoom(50, None, false), None);
    }

    #[test]
    fn a_frame_fit_below_the_steps_zooms_past_the_fit_and_back() {
        // Tablet on a phone: fit at 35 %. + goes past it to 50 %, then on.
        // It keeps the zoom chosen before (100 %).
        assert_eq!(zoom_move(100, Some(35), None, true), Some((50, Some(100))));
        assert_eq!(
            zoom_move(50, Some(35), Some(100), true),
            Some((67, Some(100)))
        );
        assert_eq!(
            zoom_move(67, Some(35), Some(100), false),
            Some((50, Some(100)))
        );
        // − from the smallest step comes back to the fit and that zoom.
        assert_eq!(zoom_move(50, Some(35), Some(100), false), Some((100, None)));
        assert_eq!(zoom_move(100, Some(35), None, false), None);
        // A wider pane (or Desktop) goes back to it too; a narrow one stays.
        assert_eq!(leave_past_fit(Some(79), Some(100)), Some(100));
        assert_eq!(leave_past_fit(None, Some(75)), Some(75));
        assert_eq!(leave_past_fit(Some(40), Some(100)), None);
        assert_eq!(leave_past_fit(Some(79), None), None);
        // A fit within the steps still stops + (the tooltip says why).
        assert_eq!(zoom_move(100, Some(79), None, true), None);
        assert!(fit_stops_zoom_in(100, Some(79), None));
        assert!(!fit_stops_zoom_in(200, None, None));
        assert!(!fit_stops_zoom_in(100, Some(35), None));
    }
}
