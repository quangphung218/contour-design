//! Track W3-agent UI (spec 1.1, 1.5): session mode toggle, side chat tab,
//! project memory panel, Design Jury switch and theater, conversation
//! export, and the warning for agents that can hang in "Ask first" mode.

use super::chat_view::{ChatItem, ItemKind, render_block};
use super::export_menu::download;
use super::icons::{Glyph, icon};
use super::sse::Sse;
use super::studio::StudioCtx;
use super::{Shell, api};
use crate::chat::{AssistantTurn, Block, TurnState};
use crate::events::{
    ChatRequest, CreateRunResponse, MessageRole, MessagesResponse, RunFrame, decode_frame,
};
use crate::i18n::{Locale, T, t, tf};
use crate::settings::PermissionMode;
use leptos::prelude::*;
use od_contracts::{
    ConversationResponse, CritiqueReport, CritiqueSettings, CritiqueStatus, MemoryEntry,
    MemoryResponse, MemorySource, PanelRole, SessionMode,
};
use send_wrapper::SendWrapper;
use serde_json::json;

/// Agents whose CLI can block on an approval prompt the daemon never
/// answers when the permission mode is "ask" (decision: keep the runtime,
/// warn in the UI).
const ASK_MODE_HANGS: [&str; 9] = [
    "copilot",
    "qwen",
    "amp",
    "qoder",
    "aider",
    "deepseek",
    "grok-build",
    "atomcode",
    "antigravity",
];

/// A side-chat turn runs in chat mode whatever its conversation was
/// switched to in the main pane: a side question never edits files.
fn side_turn(req: ChatRequest) -> ChatRequest {
    ChatRequest {
        session_mode: Some(SessionMode::Chat),
        ..req
    }
}

fn mode_label(mode: SessionMode) -> (&'static str, &'static str) {
    match mode {
        SessionMode::Design => (t(T::ModeDesign), t(T::ModeDesignHint)),
        SessionMode::Chat => (t(T::ModeChat), t(T::ModeChatHint)),
        SessionMode::Plan => (t(T::ModePlan), t(T::ModePlanHint)),
    }
}

fn mode_id(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Design => "design",
        SessionMode::Chat => "chat",
        SessionMode::Plan => "plan",
    }
}

/// Whether `title` names a side chat conversation, in any UI language.
pub fn is_side_chat_title(title: Option<&str>) -> bool {
    title.is_some_and(|t| Locale::ALL.iter().any(|l| T::SideChat.text(*l) == t))
}

impl StudioCtx {
    /// Switches the open conversation's mode; the next run uses it.
    pub fn set_session_mode(self, mode: SessionMode) {
        self.session_mode.set(mode);
        let Some(cid) = self.conversation_id.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations/{}", self.base(), api::seg(&cid));
            match api::patch::<_, ConversationResponse>(&path, &json!({ "sessionMode": mode }))
                .await
            {
                Ok(r) => self.conversations.update(|list| {
                    if let Some(c) = list.iter_mut().find(|c| c.id == r.conversation.id) {
                        *c = r.conversation;
                    }
                }),
                Err(e) => self.notice.set(Some(e.to_string())),
            }
        });
    }

    /// Loads the project's Design Jury switch and memory.
    pub fn load_w3(self) {
        leptos::task::spawn_local(async move {
            if let Ok(s) = api::get::<CritiqueSettings>(&format!("{}/critique", self.base())).await
            {
                self.jury.set(s.enabled);
            }
        });
        self.reload_memory();
    }

    /// The settled jury report (with the shipped file) is stored when the
    /// run is finalized, just after its `end`: read it back.
    pub fn settle_jury(self, turn: RwSignal<AssistantTurn>) {
        let (Some(cid), Some(id)) = (
            self.conversation_id.get_untracked(),
            self.items.with_untracked(|items| {
                items.iter().find_map(|i| match &i.kind {
                    ItemKind::Assistant(t) if *t == turn => Some(i.id.clone()),
                    _ => None,
                })
            }),
        ) else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations/{}/messages", self.base(), api::seg(&cid));
            for _ in 0..30 {
                let stored = api::get::<MessagesResponse>(&path)
                    .await
                    .ok()
                    .and_then(|r| r.messages.into_iter().find(|m| m.id == id))
                    .filter(|m| m.ended_at.is_some());
                if let Some(m) = stored {
                    let settled = AssistantTurn::from_stored(&m).critique;
                    if settled.is_some() {
                        turn.update(|t| t.critique = settled);
                    }
                    return;
                }
                gloo_timers::future::TimeoutFuture::new(100).await;
            }
        });
    }

    pub fn reload_memory(self) {
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<MemoryResponse>(&format!("{}/memory", self.base())).await {
                self.memory.set(r.memory);
            }
        });
    }
}

/// Mode, jury, memory and export controls under the conversation picker.
#[component]
pub fn ChatTools(ctx: StudioCtx) -> impl IntoView {
    view! {
        <div class="chat-tools">
            <div class="segmented small" role="radiogroup" aria-label=t(T::SessionMode) data-testid="session-mode">
                {[SessionMode::Design, SessionMode::Chat, SessionMode::Plan]
                    .into_iter()
                    .map(|m| {
                        let (label, hint) = mode_label(m);
                        view! {
                            <button
                                class="seg"
                                role="radio"
                                title=hint
                                data-mode=mode_id(m)
                                class:active=move || ctx.session_mode.get() == m
                                aria-checked=move || (ctx.session_mode.get() == m).to_string()
                                disabled=move || ctx.live_run.get().is_some()
                                on:click=move |_| ctx.set_session_mode(m)
                            >
                                {label}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            <JuryToggle ctx=ctx/>
            <MemoryMenu ctx=ctx/>
        </div>
        <AskModeWarning/>
    }
}

#[component]
fn JuryToggle(ctx: StudioCtx) -> impl IntoView {
    let toggle = move |_| {
        let next = !ctx.jury.get_untracked();
        ctx.jury.set(next);
        leptos::task::spawn_local(async move {
            let path = format!("{}/critique", ctx.base());
            if let Err(e) =
                api::put::<_, CritiqueSettings>(&path, &CritiqueSettings { enabled: next }).await
            {
                ctx.jury.set(!next);
                ctx.notice.set(Some(e.to_string()));
            }
        });
    };
    view! {
        <button
            class="btn small ghost"
            class:active=move || ctx.jury.get()
            data-testid="jury-toggle"
            aria-label=t(T::DesignJury)
            title=t(T::JuryHint)
            aria-pressed=move || ctx.jury.get().to_string()
            on:click=toggle
        >
            {icon(Glyph::Jury)}
            <span class="btn-label">{t(T::DesignJury)}</span>
        </button>
    }
}

#[component]
fn MemoryMenu(ctx: StudioCtx) -> impl IntoView {
    let open = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let base = move || format!("{}/memory", ctx.base());
    let apply = move |res: Result<MemoryResponse, api::HttpError>| match res {
        Ok(r) => ctx.memory.set(r.memory),
        Err(e) => ctx.notice.set(Some(e.to_string())),
    };
    let add = move || {
        let text = draft.get_untracked().trim().to_owned();
        if text.is_empty() {
            return;
        }
        draft.set(String::new());
        leptos::task::spawn_local(async move {
            apply(api::post(&base(), &json!({ "text": text })).await);
        });
    };
    let remove = move |id: String| {
        leptos::task::spawn_local(async move {
            apply(api::delete(&format!("{}/{}", base(), api::seg(&id))).await);
        });
    };
    let clear = move |_| {
        leptos::task::spawn_local(async move {
            match api::delete::<serde_json::Value>(&base()).await {
                Ok(_) => ctx.memory.set(Vec::new()),
                Err(e) => ctx.notice.set(Some(e.to_string())),
            }
        });
    };
    view! {
        <div class="menu-wrap">
            <button
                class="btn small ghost"
                class:active=move || open.get()
                data-testid="memory-toggle"
                aria-label=t(T::Memory)
                data-tip=t(T::Memory)
                aria-expanded=move || open.get().to_string()
                on:click=move |_| {
                    if !open.get_untracked() {
                        ctx.reload_memory();
                    }
                    open.update(|o| *o = !*o);
                }
            >
                {icon(Glyph::Memory)}
                <span class="btn-label">{t(T::Memory)}</span>
                <span class="count">{move || ctx.memory.with(Vec::len)}</span>
            </button>
            <Show when=move || open.get()>
                <div class="menu popover left memory-panel" data-testid="memory-panel" role="dialog" aria-label=t(T::Memory)>
                    <div class="popover-head">
                        <strong>{icon(Glyph::Memory)}{t(T::Memory)}</strong>
                        <button class="icon-btn small" aria-label=t(T::Close) on:click=move |_| open.set(false)>{icon(Glyph::Close)}</button>
                    </div>
                    <p class="muted small-text">{t(T::MemoryHint)}</p>
                    <ul class="memory-list">
                        {move || {
                            let list = ctx.memory.get();
                            if list.is_empty() {
                                return view! { <li class="memory-empty subtle">{t(T::MemoryEmpty)}</li> }.into_any();
                            }
                            list.into_iter().map(|m| memory_row(m, remove)).collect_view().into_any()
                        }}
                    </ul>
                    <div class="row">
                        <input
                            class="input"
                            data-testid="memory-input"
                            placeholder=t(T::MemoryPlaceholder)
                            prop:value=move || draft.get()
                            on:input=move |ev| draft.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Enter" && !ev.is_composing() {
                                    ev.prevent_default();
                                    add();
                                }
                            }
                        />
                        <button class="btn small primary" data-testid="memory-add" on:click=move |_| add()>
                            {icon(Glyph::Add)}
                            {t(T::MemoryAdd)}
                        </button>
                    </div>
                    <Show when=move || ctx.memory.with(|m| !m.is_empty())>
                        <button class="btn small ghost danger memory-clear" data-testid="memory-clear" on:click=clear>
                            {icon(Glyph::Delete)}
                            {t(T::MemoryClear)}
                        </button>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

fn memory_row(m: MemoryEntry, remove: impl Fn(String) + Copy + 'static) -> impl IntoView {
    let id = m.id.clone();
    view! {
        <li class="memory-item" data-testid="memory-item">
            <span class="memory-text">{m.text}</span>
            {(m.source == MemorySource::Chat).then(|| view! { <span class="chip small-chip">{t(T::MemoryFromChat)}</span> })}
            <button
                class="icon-btn small danger"
                aria-label=t(T::Delete)
                data-testid="memory-delete"
                on:click=move |_| remove(id.clone())
            >
                {icon(Glyph::Close)}
            </button>
        </li>
    }
}

#[component]
pub(super) fn TranscriptMenu(ctx: StudioCtx) -> impl IntoView {
    let open = RwSignal::new(false);
    let export = move |format: &'static str| {
        open.set(false);
        let Some(cid) = ctx.conversation_id.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!(
                "{}/conversations/{}/transcript?format={format}",
                ctx.base(),
                api::seg(&cid)
            );
            match api::get_bytes(&path).await {
                Ok(bytes) => {
                    let short: String = cid.chars().take(8).collect();
                    let mime = if format == "json" {
                        "application/json"
                    } else {
                        "text/markdown"
                    };
                    download(&bytes, &format!("conversation-{short}.{format}"), mime);
                }
                Err(e) => ctx.notice.set(Some(e.to_string())),
            }
        });
    };
    view! {
        <div class="menu-wrap">
            <button
                class="icon-btn"
                aria-label=t(T::ExportConversation)
                data-tip=t(T::ExportConversation)
                data-tip-pos="left"
                aria-expanded=move || open.get().to_string()
                data-testid="export-conversation"
                disabled=move || ctx.conversation_id.get().is_none()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {icon(Glyph::Export)}
            </button>
            <Show when=move || open.get()>
                <div class="menu" role="menu">
                    <button class="menu-item" role="menuitem" data-testid="export-md" on:click=move |_| export("md")>
                        {icon(Glyph::Markdown)}
                        {t(T::ExportMarkdown)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="export-json" on:click=move |_| export("json")>
                        {icon(Glyph::Braces)}
                        {t(T::ExportJson)}
                    </button>
                </div>
            </Show>
        </div>
    }
}

/// Copilot and Qwen in "Ask first" mode can block on a prompt nobody
/// answers until the idle timeout.
#[component]
pub fn AskModeWarning() -> impl IntoView {
    let shell = expect_context::<Shell>();
    move || {
        let ask = shell.config.with(|c| {
            c.as_ref()
                .is_some_and(|c| c.agent_permission_mode == PermissionMode::Ask)
        });
        // Tracked: the warning follows a settings change.
        shell.agents.track();
        let agent = shell
            .config
            .with(|c| c.as_ref().and_then(|c| c.agent_id.clone()))
            .or_else(|| shell.agent_id())?;
        (ask && ASK_MODE_HANGS.contains(&agent.as_str())).then(|| {
            let name = shell
                .agents
                .with(|a| match a {
                    Some(Ok(list)) => list.iter().find(|x| x.id == agent).map(|x| x.name.clone()),
                    _ => None,
                })
                .unwrap_or(agent);
            view! {
                <p class="warning-text" role="alert" data-testid="ask-mode-warning">
                    {icon(Glyph::Warning)}
                    <span>{tf(T::AskModeWarning, &[&name])}</span>
                </p>
            }
        })
    }
}

fn role_label(role: PanelRole) -> &'static str {
    match role {
        PanelRole::Designer => t(T::RoleDesigner),
        PanelRole::Critic => t(T::RoleCritic),
        PanelRole::Brand => t(T::RoleBrand),
        PanelRole::A11y => t(T::RoleA11y),
        PanelRole::Copy => t(T::RoleCopy),
    }
}

fn score(v: f64) -> String {
    format!("{v:.1}")
}

/// The Design Jury of one turn: a lane per panelist with its score per
/// round, the composite per round against the threshold, and the outcome.
#[component]
pub fn JuryTheater(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    move || {
        let (report, live) = turn.with(|t| Some((t.critique.clone()?, t.state.is_live())))?;
        Some(theater(ctx, report, live))
    }
}

fn theater(ctx: StudioCtx, report: CritiqueReport, live: bool) -> impl IntoView {
    // A stream that ended before the settled report arrived.
    let status = if !live && report.status == CritiqueStatus::Running {
        CritiqueStatus::Interrupted
    } else {
        report.status
    };
    let composite = report.composite.map(score).unwrap_or_else(|| "–".into());
    let ship_round = report
        .ship_round
        .or_else(|| report.rounds.last().map(|r| r.n))
        .unwrap_or(0)
        .to_string();
    let outcome = match status {
        CritiqueStatus::Running => t(T::JuryReviewing).to_owned(),
        CritiqueStatus::Shipped => tf(T::JuryShipped, &[&ship_round, &composite]),
        CritiqueStatus::BelowThreshold => tf(T::JuryBelow, &[&ship_round, &composite]),
        CritiqueStatus::Interrupted => tf(T::JuryInterrupted, &[&composite]),
        CritiqueStatus::Degraded => tf(
            T::JuryDegraded,
            &[report.degraded_reason.as_deref().unwrap_or("?")],
        ),
    };
    let status_class = format!("jury-status {}", serde_status(status));
    let lanes = PanelRole::ALL
        .into_iter()
        .map(|role| {
            let scores: Vec<String> = report
                .rounds
                .iter()
                .filter_map(|r| r.panelists.iter().find(|p| p.role == role))
                .map(|p| p.score.map(score).unwrap_or_else(|| "·".into()))
                .collect();
            let latest = report
                .rounds
                .iter()
                .rev()
                .find_map(|r| r.panelists.iter().find(|p| p.role == role));
            let fixes = latest.map_or(0, |p| p.must_fix.len());
            let notes = latest.map(|p| p.notes.clone()).unwrap_or_default();
            // The designer drafts and does not score: show its intent.
            let scores = if role == PanelRole::Designer {
                vec![notes.chars().take(80).collect()]
            } else {
                scores
            };
            view! {
                <li class="jury-lane" data-testid="jury-lane" data-role=role.as_str() title=notes>
                    <span class="jury-role">{role_label(role)}</span>
                    <span class="jury-scores" class:mono=role != PanelRole::Designer>{scores.join(" → ")}</span>
                    {(fixes > 0).then(|| view! { <span class="jury-fix">{tf(T::JuryMustFix, &[&fixes.to_string()])}</span> })}
                </li>
            }
        })
        .collect_view();
    let ticker = report
        .rounds
        .iter()
        .map(|r| {
            let pass = r.composite.is_some_and(|c| c >= report.threshold);
            view! {
                <span class="jury-round" class:pass=pass data-testid="jury-round">
                    {tf(T::JuryRound, &[&r.n.to_string()])}": "
                    <strong>{r.composite.map(score).unwrap_or_else(|| "…".into())}</strong>
                </span>
            }
        })
        .collect_view();
    let artifact = report.artifact_path.clone();
    view! {
        <section class="jury" data-testid="jury" data-status=serde_status(status)>
            <header class="jury-head">
                <strong>{icon(Glyph::Jury)}{t(T::DesignJury)}</strong>
                <span class="muted">{format!("≥ {}", score(report.threshold))}</span>
            </header>
            <ul class="jury-lanes">{lanes}</ul>
            <div class="jury-ticker">{ticker}</div>
            <p class=status_class data-testid="jury-outcome">
                {(status == CritiqueStatus::Running).then(|| view! { <span class="spinner"></span> })}
                {outcome}
            </p>
            {artifact.map(|path| {
                let open = path.clone();
                view! {
                    <button class="btn ghost small" data-testid="jury-artifact" on:click=move |_| ctx.open_file(&open)>
                        {t(T::JuryOpenArtifact)}" · "<span class="mono">{path}</span>
                    </button>
                }
            })}
        </section>
    }
}

fn serde_status(status: CritiqueStatus) -> &'static str {
    match status {
        CritiqueStatus::Running => "running",
        CritiqueStatus::Shipped => "shipped",
        CritiqueStatus::BelowThreshold => "below_threshold",
        CritiqueStatus::Interrupted => "interrupted",
        CritiqueStatus::Degraded => "degraded",
    }
}

/// The side chat: its own conversation in chat mode, with the same agent,
/// shown as a workspace tab next to the file tabs.
#[derive(Clone, Copy)]
pub struct SideChat {
    /// The side chat tab is the active workspace tab.
    pub open: RwSignal<bool>,
    pub conversation_id: RwSignal<Option<String>>,
    pub items: RwSignal<Vec<ChatItem>>,
    pub live_run: RwSignal<Option<String>>,
    loaded: StoredValue<bool>,
    stream: StoredValue<Option<SendWrapper<Sse>>>,
}

impl SideChat {
    pub fn new() -> Self {
        Self {
            open: RwSignal::new(false),
            conversation_id: RwSignal::new(None),
            items: RwSignal::new(Vec::new()),
            live_run: RwSignal::new(None),
            loaded: StoredValue::new(false),
            stream: StoredValue::new(None),
        }
    }

    pub fn close_stream(self) {
        self.stream.set_value(None);
    }

    /// Shows the side chat tab, loading its history the first time.
    pub fn show(self, ctx: StudioCtx) {
        ctx.active_term.set(None);
        self.open.set(true);
        if self.loaded.get_value() {
            return;
        }
        self.loaded.set_value(true);
        let Some(cid) = self.conversation_id.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations/{}/messages", ctx.base(), api::seg(&cid));
            let Ok(res) = api::get::<MessagesResponse>(&path).await else {
                return;
            };
            let items = res
                .messages
                .iter()
                .map(|m| match m.role {
                    MessageRole::User => ChatItem::user(m.id.clone(), m.content.clone()),
                    MessageRole::Assistant => ChatItem::assistant(
                        m.id.clone(),
                        RwSignal::new(AssistantTurn::from_stored(m)),
                    ),
                })
                .collect();
            self.items.set(items);
        });
    }

    fn send(self, ctx: StudioCtx, text: String) {
        let text = text.trim().to_owned();
        if text.is_empty() || self.live_run.get_untracked().is_some() {
            return;
        }
        let shell = ctx.shell;
        let (user_id, assistant_id) = (api::random_id(), api::random_id());
        let turn = RwSignal::new(AssistantTurn::new());
        self.items.update(|items| {
            items.push(ChatItem::user(user_id.clone(), text.clone()));
            items.push(ChatItem::assistant(assistant_id.clone(), turn));
        });
        let Some(agent_id) = shell.agent_id() else {
            fail(turn, t(T::NoAgentSelected).to_owned());
            return;
        };
        self.live_run.set(Some(String::new()));
        leptos::task::spawn_local(async move {
            let cid = match self.conversation_id.get_untracked() {
                Some(cid) => cid,
                None => {
                    let body = json!({ "title": t(T::SideChat), "sessionMode": "chat" });
                    let path = format!("{}/conversations", ctx.base());
                    match api::post::<_, ConversationResponse>(&path, &body).await {
                        Ok(r) => {
                            let id = r.conversation.id.clone();
                            self.conversation_id.set(Some(id.clone()));
                            ctx.reload_conversations();
                            id
                        }
                        Err(e) => {
                            fail(turn, e.to_string());
                            self.live_run.set(None);
                            return;
                        }
                    }
                }
            };
            let req = ChatRequest {
                agent_id,
                message: text,
                project_id: ctx.project_id.get_value(),
                conversation_id: Some(cid),
                user_message_id: Some(user_id),
                assistant_message_id: Some(assistant_id),
                client_request_id: Some(api::random_id()),
                model: shell
                    .config
                    .with_untracked(|c| c.as_ref().and_then(|c| c.model.clone())),
                locale: Some(shell.locale.get_untracked().tag().to_owned()),
                ..ChatRequest::default()
            };
            match api::post::<_, CreateRunResponse>("/api/runs", &side_turn(req)).await {
                Ok(res) => self.attach(ctx, res.run_id, turn),
                Err(e) => {
                    fail(turn, e.to_string());
                    self.live_run.set(None);
                }
            }
        });
    }

    fn attach(self, ctx: StudioCtx, run_id: String, turn: RwSignal<AssistantTurn>) {
        self.live_run.set(Some(run_id.clone()));
        let url = format!("/api/runs/{}/events", api::seg(&run_id));
        let stream = Sse::open(
            &url,
            &["start", "agent", "error", "end"],
            move |name, data| {
                let frame = decode_frame(name, &data);
                let ended = matches!(frame, RunFrame::End(_));
                turn.update(|t| t.apply(frame));
                if ended {
                    self.finish(ctx);
                }
            },
            move || {
                if turn.with_untracked(|t| t.state.is_live()) {
                    fail(turn, t(T::LostRunStream).to_owned());
                }
                self.finish(ctx);
            },
        );
        match stream {
            Some(s) => self.stream.set_value(Some(SendWrapper::new(s))),
            None => {
                fail(turn, t(T::CannotOpenRunStream).to_owned());
                self.live_run.set(None);
            }
        }
    }

    fn finish(self, ctx: StudioCtx) {
        self.live_run.set(None);
        self.stream.with_value(|s| {
            if let Some(s) = s {
                s.close();
            }
        });
        leptos::task::spawn_local(async move {
            self.stream.set_value(None);
        });
        // A side question may still touch files (or memory).
        ctx.refresh_files();
        ctx.reload_memory();
    }

    fn cancel(self) {
        let Some(run_id) = self.live_run.get_untracked().filter(|r| !r.is_empty()) else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("/api/runs/{}/cancel", api::seg(&run_id));
            let _ = api::post::<_, serde_json::Value>(&path, &json!({})).await;
        });
    }
}

fn fail(turn: RwSignal<AssistantTurn>, message: String) {
    turn.update(|t| {
        t.blocks.push(Block::Error(message));
        t.state = TurnState::Failed;
    });
}

#[component]
pub fn SideChatView(ctx: StudioCtx) -> impl IntoView {
    let side = ctx.side;
    let draft = RwSignal::new(String::new());
    let list_ref = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        side.items.with(|items| {
            if let Some(ItemKind::Assistant(turn)) = items.last().map(|i| &i.kind) {
                turn.track();
            }
        });
        if let Some(el) = list_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });
    let send = move || {
        let text = draft.get_untracked();
        if text.trim().is_empty() || side.live_run.get_untracked().is_some() {
            return;
        }
        draft.set(String::new());
        side.send(ctx, text);
    };
    // Forms belong to the main chat; here they only show.
    let never = Signal::derive(|| false);
    view! {
        <section class="side-chat" data-testid="side-chat" aria-label=t(T::SideChat)>
            <p class="mode-hint">{icon(Glyph::SideChat)}{t(T::SideChatHint)}</p>
            <div class="messages" node_ref=list_ref data-testid="side-messages">
                <For
                    each=move || side.items.get()
                    key=|item| item.id.clone()
                    children=move |item| match item.kind {
                        ItemKind::User(text) => view! { <div class="msg user" data-testid="side-user-msg">{text}</div> }.into_any(),
                        ItemKind::Assistant(turn) => view! {
                            <div class="msg assistant" data-testid="side-assistant-msg"
                                data-state=move || format!("{:?}", turn.with(|t| t.state)).to_lowercase()>
                                {move || turn.with(|tn| tn.blocks.iter().cloned().map(|b| render_block(b, turn, never)).collect_view())}
                                {move || turn.with(|tn| tn.state.is_live().then(|| view! {
                                    <div class="run-status" role="status"><span class="spinner"></span>{t(T::Working)}</div>
                                }))}
                            </div>
                        }.into_any(),
                    }
                />
            </div>
            <div class="composer">
                <textarea
                    class="composer-input"
                    data-testid="side-composer"
                    rows="2"
                    placeholder=t(T::SideChatPlaceholder)
                    prop:value=move || draft.get()
                    on:input=move |ev| draft.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() {
                            ev.prevent_default();
                            send();
                        }
                    }
                ></textarea>
                <div class="composer-actions">
                    <Show when=move || side.live_run.get().is_some()>
                        <button class="btn small ghost danger" on:click=move |_| side.cancel()>{icon(Glyph::Stop)}{t(T::Stop)}</button>
                    </Show>
                    <span class="spacer"></span>
                    <button
                        class="btn small primary send-btn"
                        data-testid="side-send"
                        disabled=move || side.live_run.get().is_some() || draft.with(|d| d.trim().is_empty())
                        on:click=move |_| send()
                    >
                        {icon(Glyph::Send)}
                        {t(T::Send)}
                    </button>
                </div>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_turns_are_pinned_to_chat_mode() {
        let req = ChatRequest {
            session_mode: Some(SessionMode::Design),
            message: "why?".into(),
            ..ChatRequest::default()
        };
        let sent = side_turn(req);
        assert_eq!(sent.session_mode, Some(SessionMode::Chat));
        assert_eq!(sent.message, "why?");
    }
}
