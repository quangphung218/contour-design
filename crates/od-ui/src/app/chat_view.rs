//! Chat pane: conversations, message list, assistant blocks, question
//! forms, and the composer with its queue, plan pill and comment chips.

use super::Shell;
use super::composer::{
    ContextChips, FilePicker, MentionArea, ModelPicker, PlusMenu, Sources, UploadState, UploadTray,
    admit, upload_all,
};
use super::icons::{Glyph, icon};
use super::preview_comments::comment_label;
use super::preview_tools::DeliverableWarning;
use super::studio::{StudioCtx, conversation_title};
use crate::chat::{AssistantTurn, Block, TodoStatus, ToolCall, plan_progress};
use crate::guide::{Followup, NextStep, RunPhase, next_steps, run_phase};
use crate::i18n::{T, t, tf};
use crate::markdown;
use crate::mention::{self, Outgoing};
use crate::question_form::{
    self, Answers, Question, QuestionForm, QuestionKind, Segment, format_answers, missing_required,
};
use crate::workspace::move_item;
use leptos::prelude::*;
use od_contracts::CommentStatus;

/// One row of the transcript.
#[derive(Clone)]
pub struct ChatItem {
    pub id: String,
    pub kind: ItemKind,
    /// Files sent with a user message (`attachments`), for a retry.
    pub attachments: Vec<String>,
}

#[derive(Clone)]
pub enum ItemKind {
    User(String),
    Assistant(RwSignal<AssistantTurn>),
}

impl ChatItem {
    pub fn user(id: String, text: String) -> Self {
        Self {
            id,
            kind: ItemKind::User(text),
            attachments: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_attachments(mut self, attachments: Vec<String>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn assistant(id: String, turn: RwSignal<AssistantTurn>) -> Self {
        Self {
            id,
            kind: ItemKind::Assistant(turn),
            attachments: Vec::new(),
        }
    }
}

#[component]
pub fn ChatPane(ctx: StudioCtx) -> impl IntoView {
    let shell = Shell::get();
    let draft = ctx.draft;
    let list_ref = NodeRef::<leptos::html::Div>::new();

    // Keep the newest output in view while streaming, and once the run
    // ends: the next steps under the last turn show only then, after this
    // effect has run, so scroll again on the next frame.
    Effect::new(move |_| {
        ctx.items.with(|items| {
            if let Some(ItemKind::Assistant(turn)) = items.last().map(|i| &i.kind) {
                turn.track();
            }
        });
        ctx.live_run.track();
        if let Some(el) = list_ref.get() {
            el.set_scroll_top(el.scroll_height());
            request_animation_frame(move || el.set_scroll_top(el.scroll_height()));
        }
    });

    let navigate = super::leave_guard::use_leave();
    let to_mcp = Callback::new(move |()| navigate("/settings#settings-mcp"));
    let send = move || {
        if ctx
            .chat
            .uploads
            .with_untracked(|u| u.iter().any(|x| x.state == UploadState::Uploading))
        {
            return;
        }
        let text = draft.get_untracked();
        if text.trim().is_empty() {
            if untrack(|| ctx.has_attached()) {
                ctx.send(t(T::SendAttachedOnly).to_owned(), shell);
            }
            return;
        }
        match mention::outgoing(&text) {
            Outgoing::OpenMcpSettings => {
                draft.set(String::new());
                to_mcp.run(());
            }
            Outgoing::Send(text) => {
                ctx.chat.sent.set_value(true);
                draft.set(String::new());
                // While a run is going this queues the message.
                ctx.send(text, shell);
            }
        }
    };
    let send_key = send;
    let sources = Sources::load();
    let pick_files = RwSignal::new(0u32);
    let stage_files = Callback::new(move |files: Vec<web_sys::File>| {
        let (items, notice) = admit(files);
        if let Some(n) = notice {
            ctx.notice.set(Some(n));
        }
        if items.is_empty() {
            return;
        }
        ctx.chat.uploads.update(|u| u.extend(items));
        leptos::task::spawn_local(upload_all(
            ctx.base(),
            ctx.chat.uploads,
            ctx.tools.attachments,
        ));
    });
    let retry_upload = Callback::new(move |_id: String| {
        leptos::task::spawn_local(upload_all(
            ctx.base(),
            ctx.chat.uploads,
            ctx.tools.attachments,
        ));
    });
    let mention_files = Signal::derive(move || {
        ctx.files
            .with(|f| f.iter().map(|f| f.path.clone()).collect::<Vec<_>>())
    });
    let uploading = move || {
        ctx.chat
            .uploads
            .with(|u| u.iter().any(|x| x.state == UploadState::Uploading))
    };
    provide_context(sources);
    // "Quote": text selected in the transcript goes into the composer.
    let quote = RwSignal::new(None::<(f64, f64, String)>);
    let read_selection = move || {
        let sel = web_sys::window().and_then(|w| w.get_selection().ok().flatten());
        let picked = sel.and_then(|sel| {
            let text: String = sel.to_string().into();
            let text = text.trim().to_owned();
            if text.is_empty() || sel.range_count() == 0 {
                return None;
            }
            let inside = sel
                .anchor_node()
                .and_then(|n| n.parent_element())
                .and_then(|e| e.closest(".msg").ok().flatten())
                .is_some();
            let rect = sel.get_range_at(0).ok()?.get_bounding_client_rect();
            inside.then(|| (rect.left(), rect.top(), text))
        });
        quote.set(picked);
    };

    // Track C2: library assets are copied into the project and attached.
    let import_library = Callback::new(move |assets: Vec<super::library_picker::LibraryAsset>| {
        let project = ctx.project_id.get_value();
        leptos::task::spawn_local(async move {
            match super::library_picker::apply_to_project(&project, &assets).await {
                Ok(paths) => ctx
                    .tools
                    .attachments
                    .update(|a| crate::workspace::add_missing(a, paths)),
                Err(e) => ctx.notice.set(Some(e)),
            }
        });
    });
    view! {
        <section class="chat" aria-label=t(T::Chat)>
            <ConversationBar ctx=ctx/>
            <super::agent_w3::ChatTools ctx=ctx/>
            <div
                class="messages"
                node_ref=list_ref
                data-testid="messages"
                on:mouseup=move |_| read_selection()
                on:keyup=move |_| read_selection()
                on:scroll=move |_| quote.set(None)
            >
                {move || ctx.load_error.get().map(|e| view! {
                    <div class="msg-error load-error" role="alert" data-testid="load-error">
                        {icon(Glyph::Warning)}
                        <div class="load-error-body">
                            <strong>{t(T::ProjectLoadFailed)}</strong>
                            // A missing project in words, not "Not found (HTTP 404)".
                            <span>{if ctx.load_gone.get() { t(T::ProjectGone).to_owned() } else { e }}</span>
                            <div class="row">
                                // A project that does not exist will not load on a retry.
                                {(!ctx.load_gone.get()).then(|| view! {
                                    <button class="btn small" data-testid="load-retry" on:click=move |_| {
                                        leptos::task::spawn_local(ctx.load(shell));
                                    }>{icon(Glyph::Refresh)}{t(T::NextRetry)}</button>
                                })}
                                <a class="btn small ghost" href="/projects">{icon(Glyph::Projects)}{t(T::AllProjects)}</a>
                            </div>
                        </div>
                    </div>
                })}
                <For
                    each=move || ctx.items.get()
                    key=|item| item.id.clone()
                    children=move |item| match item.kind {
                        ItemKind::User(text) => view! { <UserBubble text=text/> }.into_any(),
                        ItemKind::Assistant(turn) => {
                            let id = item.id.clone();
                            let id_last = item.id.clone();
                            // A form can be answered only on the latest turn.
                            let is_last = Signal::derive(move || {
                                ctx.items.with(|items| items.last().is_some_and(|i| i.id == id_last))
                            });
                            view! { <AssistantView turn=turn is_last=is_last item_id=id/> }.into_any()
                        }
                    }
                />
                <Show when=move || ctx.items.with(Vec::is_empty) && ctx.load_error.with(Option::is_none)>
                    <div class="empty-state compact chat-empty" data-testid="chat-empty">
                        <span class="empty-icon">{icon(Glyph::Chat)}</span>
                        <strong>{t(T::DescribeChangeTitle)}</strong>
                        <p>{t(T::DescribeChange)}</p>
                        <div class="starter-prompts">
                            {[T::StarterLanding, T::StarterDashboard, T::StarterDeck]
                                .into_iter()
                                .map(|key| view! {
                                    <button class="chip" data-testid="starter-prompt" on:click=move |_| {
                                        draft.set(t(key).to_owned());
                                        focus_composer();
                                    }>{icon(Glyph::Sparkle)}{t(key)}</button>
                                })
                                .collect_view()}
                        </div>
                    </div>
                </Show>
            </div>
            {move || quote.get().map(|(x, y, text)| view! {
                <button
                    class="btn small quote-btn"
                    data-testid="quote-selection"
                    style=format!("left:{x:.0}px;top:{:.0}px", (y - 36.0).max(4.0))
                    on:mousedown=|ev| ev.prevent_default()
                    on:click=move |_| {
                        let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
                        draft.update(|d| {
                            if !d.trim().is_empty() {
                                d.push_str("\n\n");
                            }
                            d.push_str(&quoted);
                            d.push('\n');
                        });
                        if let Some(sel) = web_sys::window().and_then(|w| w.get_selection().ok().flatten()) {
                            let _ = sel.remove_all_ranges();
                        }
                        quote.set(None);
                        focus_composer();
                    }
                >
                    {icon(Glyph::Comment)}{t(T::QuoteSelection)}
                </button>
            })}
            <super::genui_view::GenuiSurfaces ctx=ctx/>
            <PlanPill ctx=ctx/>
            <QueueStack ctx=ctx/>
            <CommentChips ctx=ctx/>
            <ContextChips context=ctx.chat.context draft=draft/>
            <UploadTray
                uploads=ctx.chat.uploads
                ready=ctx.tools.attachments
                base=Signal::derive(move || Some(ctx.base()))
                on_retry=retry_upload
            />
            <DeliverableWarning ctx=ctx/>
            <div class="composer">
                <MentionArea
                    draft=draft
                    class="composer-input"
                    testid="composer"
                    aria_label=t(T::Chat)
                    placeholder=Signal::derive(|| t(T::ComposerPlaceholder).to_owned())
                    rows=3
                    sources=sources
                    files=mention_files
                    context=ctx.chat.context
                    on_file=Callback::new(move |path: String| {
                        ctx.tools.attachments.update(|a| crate::workspace::add_missing(a, vec![path]));
                    })
                    on_submit=Callback::new(move |()| send_key())
                    on_files=stage_files
                    on_mcp_settings=to_mcp
                    on_notice=Callback::new(move |m: String| ctx.notice.set(Some(m)))
                    on_library=import_library
                    disabled=Signal::derive(move || ctx.load_error.with(Option::is_some))
                />
                <FilePicker trigger=pick_files on_files=stage_files/>
                <div class="composer-actions">
                    <PlusMenu
                        sources=sources
                        context=ctx.chat.context
                        draft=draft
                        on_attach=Callback::new(move |()| pick_files.update(|n| *n += 1))
                        up=true
                        toolbox=true
                        on_library=import_library
                    >
                        <super::chat_ops::ContextFolders ctx=ctx/>
                    </PlusMenu>
                    <ModelPicker up=true/>
                    <super::chat_ops::DesignSystemSwitch ctx=ctx/>
                    <Show when=move || ctx.live_run.get().is_some()>
                        <button class="btn small ghost danger" data-testid="stop" on:click=move |_| ctx.cancel()>
                            {icon(Glyph::Stop)}
                            {t(T::Stop)}
                        </button>
                    </Show>
                    <span class="spacer"></span>
                    {move || {
                        let live = ctx.live_run.get().is_some();
                        let label = if live { t(T::Queue) } else { t(T::Send) };
                        view! {
                            <button
                                class=if live { "btn small send-btn queue" } else { "btn small primary send-btn" }
                                data-testid="send"
                                aria-label=label
                                disabled=move || (draft.with(|d| d.trim().is_empty()) && !ctx.has_attached()) || uploading() || ctx.load_error.with(Option::is_some)
                                on:click=move |_| send()
                            >
                                {icon(if live { Glyph::Queue } else { Glyph::Send })}
                                {label}
                            </button>
                        }
                    }}
                </div>
            </div>
            <super::chat_ops::ChatResizer ctx=ctx/>
        </section>
    }
}

#[component]
fn ConversationBar(ctx: StudioCtx) -> impl IntoView {
    view! {
        <div class="pane-head conversation-bar">
            <span class="conversation-ico">{icon(Glyph::Chat)}</span>
            // Which project this is first; the conversation under it.
            <div class="conversation-names">
                // Only once the project is there: an empty heading says nothing.
                {move || ctx.project.with(|p| p.as_ref().map(|p| p.name.clone())).map(|name| view! {
                    <h1 class="studio-project" data-testid="studio-project-name" title=name.clone()>{name.clone()}</h1>
                })}
                <select
                    class="conversation-select"
                    data-testid="conversation"
                    aria-label=t(T::Conversation)
                    disabled=move || ctx.live_run.get().is_some()
                    on:change=move |ev| {
                        let id = event_target_value(&ev);
                        leptos::task::spawn_local(ctx.open_conversation(id));
                    }
                >
                    {move || {
                        let current = ctx.conversation_id.get();
                        let list = ctx.conversations.get();
                        let n = list.len();
                        list.iter()
                            .enumerate()
                            .map(|(i, c)| {
                                let selected = current.as_deref() == Some(c.id.as_str());
                                view! {
                                    <option value=c.id.clone() selected=selected>{conversation_title(c, n - i)}</option>
                                }
                            })
                            .collect_view()
                    }}
                </select>
            </div>
            <button
                class="icon-btn"
                aria-label=t(T::NewConversation)
                data-tip=t(T::NewConversation)
                data-tip-pos="left"
                data-testid="new-conversation"
                disabled=move || ctx.live_run.get().is_some()
                on:click=move |_| ctx.new_conversation(None)
            >
                {icon(Glyph::Add)}
            </button>
            <super::chat_ops::ConversationMenu ctx=ctx/>
            <super::agent_w3::TranscriptMenu ctx=ctx/>
            <button
                class="icon-btn"
                aria-label=t(T::CollapseChat)
                data-tip=t(T::CollapseChat)
                data-tip-pos="left"
                data-testid="collapse-chat"
                on:click=move |_| ctx.chat.collapsed.set(true)
            >
                {icon(Glyph::SidebarFold)}
            </button>
        </div>
    }
}

/// "Step N/M" over the composer while the agent works through a plan.
#[component]
fn PlanPill(ctx: StudioCtx) -> impl IntoView {
    move || {
        ctx.live_run.get()?;
        let turn = ctx.items.with(|items| {
            items.iter().rev().find_map(|i| match &i.kind {
                ItemKind::Assistant(t) => Some(*t),
                ItemKind::User(_) => None,
            })
        })?;
        turn.with(|tn| {
            let (n, m, step) = plan_progress(&tn.todos)?;
            Some(view! {
                <div class="plan-pill" data-testid="plan-pill" role="status">
                    <span class="spinner"></span>
                    <strong>{tf(T::StepOf, &[&n.to_string(), &m.to_string()])}</strong>
                    <span class="muted">{step.active_form.clone()}</span>
                </div>
            })
        })
    }
}

#[component]
fn QueueStack(ctx: StudioCtx) -> impl IntoView {
    let shell = Shell::get();
    let editing = RwSignal::new(None::<String>);
    let dragging = RwSignal::new(None::<usize>);
    move || {
        let queue = ctx.queue.get();
        if queue.is_empty() {
            return None;
        }
        let n = queue.len();
        let can_steer = ctx.can_steer(shell);
        Some(view! {
            <div class="queue" data-testid="queue" aria-label=t(T::Queued)>
                <span class="queue-title">{icon(Glyph::Queue)}<strong>{t(T::Queued)}</strong><span class="subtle">{t(T::QueuedHint)}</span></span>
                {queue
                    .into_iter()
                    .enumerate()
                    .map(|(i, q)| {
                        let id = q.id.clone();
                        let (id_edit, id_save, id_del, id_steer) = (id.clone(), id.clone(), id.clone(), id.clone());
                        let is_editing = editing.get_untracked().as_deref() == Some(id.as_str());
                        view! {
                            <div
                                class="queued"
                                data-testid="queued"
                                draggable="true"
                                on:dragstart=move |_| dragging.set(Some(i))
                                on:dragover=|ev| ev.prevent_default()
                                on:drop=move |ev| {
                                    ev.prevent_default();
                                    if let Some(from) = dragging.get_untracked() {
                                        ctx.queue.update(|q| move_item(q, from, i));
                                    }
                                    dragging.set(None);
                                }
                            >
                                {if is_editing {
                                    view! {
                                        <textarea
                                            class="input"
                                            rows="2"
                                            prop:value=q.text.clone()
                                            on:change=move |ev| {
                                                let text = event_target_value(&ev);
                                                ctx.queue.update(|q| {
                                                    if let Some(item) = q.iter_mut().find(|x| x.id == id_save) {
                                                        item.text = text;
                                                    }
                                                });
                                                editing.set(None);
                                            }
                                        ></textarea>
                                    }.into_any()
                                } else {
                                    view! { <span class="queued-text">{q.text.clone()}</span> }.into_any()
                                }}
                                <div class="queued-actions">
                                    <button class="icon-btn small" data-tip=t(T::MoveUp) aria-label=t(T::MoveUp) disabled=i == 0
                                        on:click=move |_| ctx.queue.update(|q| move_item(q, i, i.saturating_sub(1)))>{icon(Glyph::ChevronUp)}</button>
                                    <button class="icon-btn small" data-tip=t(T::MoveDown) aria-label=t(T::MoveDown) disabled=i + 1 == n
                                        on:click=move |_| ctx.queue.update(|q| move_item(q, i, i + 1))>{icon(Glyph::ChevronDown)}</button>
                                    <button class="icon-btn small" data-tip=t(T::Edit) aria-label=t(T::Edit)
                                        on:click=move |_| editing.set(Some(id_edit.clone()))>{icon(Glyph::Rename)}</button>
                                    {can_steer.then(|| view! {
                                        <button class="btn small" title=t(T::SteerHint) data-testid="steer"
                                            on:click=move |_| ctx.steer(id_steer.clone())>{icon(Glyph::ArrowRight)}{t(T::Steer)}</button>
                                    })}
                                    <button class="icon-btn small danger" data-tip=t(T::Delete) data-tip-pos="left" aria-label=t(T::Delete)
                                        on:click=move |_| ctx.drop_queued(&id_del)>{icon(Glyph::Close)}</button>
                                </div>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        })
    }
}

/// Comments that go with the next message.
#[component]
fn CommentChips(ctx: StudioCtx) -> impl IntoView {
    move || {
        let attached: Vec<_> = ctx.comments.with(|c| {
            c.iter()
                .filter(|c| c.status == CommentStatus::Attached)
                .cloned()
                .collect()
        });
        if attached.is_empty() {
            return None;
        }
        Some(view! {
            <div class="comment-chips" aria-label=t(T::CommentsAttached)>
                {attached
                    .into_iter()
                    .map(|c| {
                        let id = c.id.clone();
                        view! {
                            <span class="chip comment-chip" data-testid="comment-chip" title=c.file_path.clone()>
                                {icon(Glyph::Comment)}
                                // The colon stays with the label: the chip's gap
                                // would put a space before it ("h1 :").
                                <strong>{format!("{}:", comment_label(&c))}</strong>
                                {c.note.clone()}
                                <button
                                    class="chip-x"
                                    title=t(T::Detach)
                                    aria-label=t(T::Detach)
                                    on:click=move |_| ctx.set_comment_status(&id, CommentStatus::Open)
                                >
                                    {icon(Glyph::Close)}
                                </button>
                            </span>
                        }
                    })
                    .collect_view()}
            </div>
        })
    }
}

#[component]
fn UserBubble(text: String) -> impl IntoView {
    if question_form::is_answers_message(&text) {
        let body: String = text.lines().skip(1).collect::<Vec<_>>().join("\n");
        return view! {
            <div class="msg user answers">
                <span class="answers-label">{icon(Glyph::Check)}{t(T::Answers)}</span>
                <pre>{body}</pre>
            </div>
        }
        .into_any();
    }
    view! { <div class="msg user" data-testid="user-msg">{text}</div> }.into_any()
}

#[component]
fn AssistantView(
    turn: RwSignal<AssistantTurn>,
    is_last: Signal<bool>,
    item_id: String,
) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let state = move || turn.with(|t| t.state);
    let item_id = StoredValue::new(item_id);
    view! {
        <div class="msg assistant" data-testid="assistant-msg" data-state=move || format!("{:?}", state()).to_lowercase()>
            {move || turn.with(|tn| (!tn.todos.is_empty()).then(|| view! { <PlanList todos=tn.todos.clone()/> }))}
            <super::agent_w3::JuryTheater turn=turn/>
            <super::media_rows::MediaRows turn=turn/>
            {move || {
                turn.with(|t| {
                    // The Plan list at the top already shows each plan update.
                    let has_plan = !t.todos.is_empty();
                    t.blocks
                        .iter()
                        .filter(|b| !matches!(b, Block::Tool(call) if folded_into_plan(&call.name, has_plan)))
                        .cloned()
                        .map(|b| render_block(b, turn, is_last))
                        .collect_view()
                })
            }}
            <super::chat_ops::TurnFiles turn=turn/>
            <RunLine turn=turn/>
            <Show when=move || turn.with(|tn| matches!(run_phase(tn), RunPhase::Failed))>
                <RunLog turn=turn/>
            </Show>
            <Show when=move || is_last.get() && ctx.live_run.get().is_none()>
                <super::chat_ops::ContinueTasks turn=turn/>
                <NextSteps turn=turn item_id=item_id.get_value()/>
            </Show>
            <Show when=move || !turn.with(|t| t.state.is_live()) && ctx.live_run.get().is_none()>
                <super::chat_ops::ReconnectTurn turn=turn/>
                <div class="msg-actions">
                    <super::chat_ops::CopyTurn turn=turn/>
                    <button
                        class="btn ghost small"
                        data-testid="fork"
                        on:click=move |_| ctx.new_conversation(Some(item_id.get_value()))
                    >
                        {icon(Glyph::Fork)}
                        {t(T::Fork)}
                    </button>
                </div>
            </Show>
        </div>
    }
}

/// One status line per turn: starting, working (step N of M), done (files
/// written), failed or stopped. `data-phase` mirrors it for tests. The line
/// stays one element while the run goes on, so screen readers announce each
/// change of its live region.
#[component]
fn RunLine(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let line = Memo::new(move |_| {
        turn.with(|tn| match run_phase(tn) {
            RunPhase::Starting => ("starting", None, t(T::RunStarting).to_owned(), None),
            RunPhase::Working { step } => (
                "working",
                None,
                match step {
                    Some((n, m)) => tf(T::RunStep, &[&n.to_string(), &m.to_string()]),
                    None => t(T::RunWorking).to_owned(),
                },
                tn.status.clone(),
            ),
            RunPhase::Done { files } => (
                "done",
                Some(Glyph::CheckCircle),
                t(T::RunDone).to_owned(),
                match files {
                    0 => None,
                    1 => Some(t(T::RunFileWritten).to_owned()),
                    n => Some(tf(T::RunFilesWritten, &[&n.to_string()])),
                },
            ),
            RunPhase::Failed => (
                "failed",
                Some(Glyph::Warning),
                t(T::RunFailed).to_owned(),
                Some(t(T::RunFailedHint).to_owned()),
            ),
            RunPhase::Stopped => ("stopped", Some(Glyph::Stop), t(T::Stopped).to_owned(), None),
        })
    });
    let phase = move || line.with(|l| l.0);
    view! {
        <div class=move || format!("run-status {}", phase()) role="status" aria-live="polite" data-testid="run-state" data-phase=phase>
            {move || match line.with(|l| l.1) {
                Some(g) => icon(g).into_any(),
                None => view! { <span class="spinner"></span> }.into_any(),
            }}
            <strong>{move || line.with(|l| l.2.clone())}</strong>
            {move || line.with(|l| l.3.clone()).map(|d| view! { <span class="muted">{d}</span> })}
        </div>
    }
}

/// A failed turn's agent output (what it wrote to stderr), read from the
/// run's log: the whole story behind the one line above. A turn reopened
/// from history gets its error message back from the same log.
#[component]
fn RunLog(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    // `Err`: the log could not be read, which is not an agent that said
    // nothing.
    let log = RwSignal::new(None::<Result<String, ()>>);
    match turn.with_untracked(|t| t.run_id.clone()) {
        None => log.set(Some(Ok(String::new()))),
        Some(id) => leptos::task::spawn_local(async move {
            let url = format!("/api/runs/{}/events", super::api::seg(&id));
            let record = super::api::get_text(&url)
                .await
                .map(|sse| crate::events::run_record(&sse));
            if let Ok(crate::events::RunRecord {
                error: Some(error), ..
            }) = &record
            {
                turn.try_update(|t| {
                    if !t.blocks.iter().any(|b| matches!(b, Block::Error(_))) {
                        t.blocks.push(Block::Error(error.clone()));
                    }
                });
            }
            log.try_set(Some(record.map(|r| r.stderr).map_err(drop)));
        }),
    }
    view! {
        <details class="run-log" data-testid="run-log">
            <summary>{icon(Glyph::Terminal)}{t(T::AgentOutput)}{icon(Glyph::ChevronDown)}</summary>
            {move || match log.get() {
                None => view! { <p class="muted">{t(T::Loading)}</p> }.into_any(),
                Some(Err(())) => {
                    view! { <p class="muted" data-testid="run-log-failed">{t(T::AgentOutputFailed)}</p> }.into_any()
                }
                Some(Ok(text)) if text.trim().is_empty() => {
                    view! { <p class="muted">{t(T::AgentOutputEmpty)}</p> }.into_any()
                }
                Some(Ok(text)) => view! { <pre class="run-log-text" data-testid="run-log-text">{text}</pre> }.into_any(),
            }}
        </details>
    }
}

/// Suggested next steps under the latest finished turn.
#[component]
fn NextSteps(turn: RwSignal<AssistantTurn>, item_id: String) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let sources = use_context::<Sources>();
    let shell = Shell::get();
    let navigate = super::leave_guard::use_leave();
    let item_id = StoredValue::new(item_id);
    // The user message this turn answered, and its files, for a retry.
    let asked = move || {
        ctx.items.with_untracked(|items| {
            let i = items.iter().position(|i| i.id == item_id.get_value())?;
            items[..i].iter().rev().find_map(|i| match &i.kind {
                ItemKind::User(text) => Some((text.clone(), i.attachments.clone())),
                ItemKind::Assistant(_) => None,
            })
        })
    };
    move || {
        let steps = turn.with(next_steps);
        let steps_wrote = steps.iter().any(|s| matches!(s, NextStep::Open(_)));
        let wrote_page = steps_wrote && sources.is_some();
        (!steps.is_empty()).then(|| {
            let navigate = navigate.clone();
            view! {
                <div class="next-steps" data-testid="next-steps" aria-label=t(T::NextSteps)>
                    <span class="next-label">{t(T::NextSteps)}</span>
                    {steps
                        .into_iter()
                        .map(|step| {
                            let navigate = navigate.clone();
                            let (glyph, label, testid) = match &step {
                                NextStep::Open(path) => (Glyph::Eye, tf(T::NextOpen, &[path.rsplit('/').next().unwrap_or(path)]), "next-open"),
                                NextStep::Comment => (Glyph::Comment, t(T::NextComment).to_owned(), "next-comment"),
                                NextStep::Prompt(Followup::Mobile) => (Glyph::Phone, t(T::NextMobile).to_owned(), "next-mobile"),
                                NextStep::Prompt(Followup::Variations) => (Glyph::Sparkle, t(T::NextVariations).to_owned(), "next-variations"),
                                NextStep::Retry => (Glyph::Refresh, t(T::NextRetry).to_owned(), "next-retry"),
                                NextStep::Settings => (Glyph::Settings, t(T::NextSettings).to_owned(), "next-settings"),
                            };
                            let run = move |_| match &step {
                                NextStep::Open(path) => {
                                    ctx.open_file(path);
                                    ctx.show_code.set(false);
                                }
                                NextStep::Comment => ctx.set_comment_mode(true),
                                NextStep::Prompt(f) => {
                                    let text = match f {
                                        Followup::Mobile => t(T::FollowupMobile),
                                        Followup::Variations => t(T::FollowupVariations),
                                    };
                                    ctx.draft.set(text.to_owned());
                                    focus_composer();
                                }
                                NextStep::Retry => {
                                    if let Some((text, files)) = asked() {
                                        ctx.tools.attachments.update(|a| crate::workspace::add_missing(a, files));
                                        ctx.send(text, shell);
                                    }
                                }
                                NextStep::Settings => navigate("/settings"),
                            };
                            view! {
                                <button class="chip next-step" data-testid=testid on:click=run>
                                    {icon(glyph)}{label}
                                </button>
                            }
                        })
                        .collect_view()}
                    // The way out: on a phone the Export menu sits on the
                    // Preview pane, so this opens it there.
                    {steps_wrote.then(|| view! {
                        <button class="chip next-step" data-testid="next-export"
                            on:click=move |_| {
                                ctx.pane.set(super::studio::NarrowPane::Preview);
                                ctx.export_open.set(true);
                            }>
                            {icon(Glyph::Export)}{t(T::Export)}
                        </button>
                    })}
                    {wrote_page.then(|| crate::toolbox::FEATURED.iter().filter_map(|id| crate::toolbox::find(id)).map(|a| view! {
                        <button class="chip next-step" data-testid="next-toolbox" data-action=a.id
                            on:click=move |_| if let Some(sources) = sources {
                                super::composer::apply_toolbox(a, sources, ctx.chat.context, ctx.draft);
                            }>
                            {icon(Glyph::Sparkle)}{t(a.title)}
                        </button>
                    }).collect_view())}
                </div>
            }
        })
    }
}

/// Puts the caret in the chat composer.
pub(super) fn focus_composer() {
    focus_selector("[data-testid=composer]");
}

/// Focuses the first element matching `selector`.
pub(super) fn focus_selector(selector: &str) {
    use wasm_bindgen::JsCast;
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(selector).ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = el.focus();
    }
}

#[component]
fn PlanList(todos: Vec<crate::chat::Todo>) -> impl IntoView {
    view! {
        <details class="plan" open=true>
            <summary>{icon(Glyph::Plan)}{t(T::Plan)}{icon(Glyph::ChevronDown)}</summary>
            <ol>
                {todos
                    .into_iter()
                    .map(|td| {
                        let (mark, class) = match td.status {
                            TodoStatus::Completed => (icon(Glyph::Check).into_any(), "done"),
                            TodoStatus::InProgress => (view! { <span class="plan-dot"></span> }.into_any(), "doing"),
                            TodoStatus::Pending => (view! { <span class="plan-ring"></span> }.into_any(), "todo"),
                        };
                        view! { <li class=class><span class="plan-mark">{mark}</span><span>{td.content}</span></li> }
                    })
                    .collect_view()}
            </ol>
        </details>
    }
}

pub(super) fn render_block(
    block: Block,
    turn: RwSignal<AssistantTurn>,
    is_last: Signal<bool>,
) -> AnyView {
    match block {
        Block::Text(text) => question_form::split(&crate::chat::without_jury_protocol(&text))
            .into_iter()
            .map(|seg| match seg {
                Segment::Text(t) => {
                    view! { <div class="md" inner_html=markdown::to_html(&t)></div> }.into_any()
                }
                Segment::Form(form) => {
                    let enabled = Signal::derive(move || {
                        is_last.get() && !turn.with(|t| t.state.is_live())
                    });
                    view! { <QuestionFormView form=form enabled=enabled/> }.into_any()
                }
                Segment::PendingForm => {
                    view! { <div class="form-pending"><span class="spinner"></span>{t(T::PreparingQuestions)}</div> }
                        .into_any()
                }
            })
            .collect_view()
            .into_any(),
        Block::Thinking(text) => view! {
            <details class="thinking">
                <summary>{icon(Glyph::Thinking)}{t(T::Thinking)}{icon(Glyph::ChevronDown)}</summary>
                <pre>{text}</pre>
            </details>
        }
        .into_any(),
        Block::Tool(call) => {
            let live = Signal::derive(move || turn.with(|t| t.state.is_live()));
            view! { <ToolRow call=call live=live/> }.into_any()
        }
        Block::Error(message) => view! { <div class="msg-error" role="alert">{icon(Glyph::Warning)}<span>{message}</span></div> }.into_any(),
        Block::Steered(text) => view! { <div class="steered">{icon(Glyph::ArrowRight)}{tf(T::Steered, &[&text])}</div> }.into_any(),
    }
}

/// Whether a tool call shows as the reply's Plan list instead of a row of
/// its own: `TodoWrite` once a plan was read from it (only it fills the list).
fn folded_into_plan(name: &str, has_plan: bool) -> bool {
    has_plan && name == "TodoWrite"
}

/// What a tool call did, in words: the agents' own tool names
/// (`TodoWrite`, `Bash`) mean nothing to most people. Unknown tools keep
/// their name.
fn tool_label(name: &str) -> String {
    let key = match name {
        "TodoWrite" | "update_plan" => T::ToolPlan,
        "Write" | "write_file" => T::ToolWrote,
        "Edit" | "MultiEdit" | "edit_file" => T::ToolEdited,
        "Read" | "read_file" => T::ToolRead,
        "Bash" | "shell" | "command_execution" => T::ToolRan,
        "Glob" | "Grep" | "LS" | "list_directory" => T::ToolSearched,
        other => return other.to_owned(),
    };
    t(key).to_owned()
}

/// One tool call. A call still without a result once the run is over
/// (the agent never reported back) stops spinning and shows a plain ring.
#[component]
fn ToolRow(call: ToolCall, live: Signal<bool>) -> impl IntoView {
    let result = call.result;
    let class = move || match result {
        None if live.get() => "tool running",
        None => "tool ended",
        Some(false) => "tool ok",
        Some(true) => "tool failed",
    };
    let mark = move || match result {
        None if live.get() => icon(Glyph::Loader).into_any(),
        None => view! { <span class="plan-ring"></span> }.into_any(),
        Some(false) => icon(Glyph::Check).into_any(),
        Some(true) => icon(Glyph::Failed).into_any(),
    };
    view! {
        <div class=class>
            <span class="tool-icon">{mark}</span>
            <span class="tool-name" title=call.name.clone()>{tool_label(&call.name)}</span>
            <span class="tool-summary mono">{call.summary}</span>
        </div>
    }
}

#[component]
fn QuestionFormView(form: QuestionForm, enabled: Signal<bool>) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let shell = Shell::get();
    let answers = RwSignal::new(Answers::new());
    let submitted = RwSignal::new(false);
    let missing = RwSignal::new(Vec::<String>::new());
    let form = StoredValue::new(form);
    let active = move || enabled.get() && !submitted.get();

    let submit = move |_| {
        let (msg, gaps) = form.with_value(|f| {
            answers.with_untracked(|a| {
                (
                    format_answers(f, a),
                    missing_required(f, a)
                        .into_iter()
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                )
            })
        });
        if !gaps.is_empty() {
            missing.set(gaps);
            return;
        }
        submitted.set(true);
        ctx.send(msg, shell);
    };

    view! {
        <form class="qform" data-testid="question-form" on:submit=|ev| ev.prevent_default()>
            {form.with_value(|f| (!f.title.is_empty()).then(|| view! { <h3>{f.title.clone()}</h3> }))}
            {form
                .with_value(|f| f.questions.clone())
                .into_iter()
                .map(|q| view! { <QuestionField q=q answers=answers active=Signal::derive(active)/> })
                .collect_view()}
            {move || {
                let m = missing.get();
                (!m.is_empty()).then(|| view! { <p class="error-text">{tf(T::PleaseAnswer, &[&m.join(", ")])}</p> })
            }}
            <div class="row end">
                <button
                    type="button"
                    class="btn primary"
                    data-testid="qform-submit"
                    disabled=move || !active()
                    on:click=submit
                >
                    {move || if submitted.get() {
                        view! { {icon(Glyph::Check)}{t(T::Sent)} }.into_any()
                    } else {
                        view! { {t(T::SendAnswers)}{icon(Glyph::Send)} }.into_any()
                    }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn QuestionField(q: Question, answers: RwSignal<Answers>, active: Signal<bool>) -> impl IntoView {
    let id = q.id.clone();
    let set_one = {
        let id = id.clone();
        move |v: String| {
            answers.update(|a| {
                a.insert(id.clone(), vec![v]);
            })
        }
    };
    let control = match q.kind {
        QuestionKind::Radio | QuestionKind::Checkbox => {
            let multi = q.kind == QuestionKind::Checkbox;
            q.options
                .iter()
                .map(|o| {
                    let id = id.clone();
                    let value = o.value.clone();
                    let value_checked = o.value.clone();
                    let id_checked = id.clone();
                    view! {
                        <label class="opt">
                            <input
                                type=if multi { "checkbox" } else { "radio" }
                                name=id.clone()
                                disabled=move || !active.get()
                                prop:checked=move || answers.with(|a| a.get(&id_checked).is_some_and(|v| v.contains(&value_checked)))
                                on:change=move |_| answers.update(|a| {
                                    let entry = a.entry(id.clone()).or_default();
                                    if multi {
                                        if let Some(i) = entry.iter().position(|v| *v == value) {
                                            entry.remove(i);
                                        } else {
                                            entry.push(value.clone());
                                        }
                                    } else {
                                        *entry = vec![value.clone()];
                                    }
                                })
                            />
                            <span>{o.label.clone()}</span>
                        </label>
                    }
                })
                .collect_view()
                .into_any()
        }
        QuestionKind::Select => view! {
            <select disabled=move || !active.get() on:change=move |ev| set_one(event_target_value(&ev))>
                <option value="">{t(T::Choose)}</option>
                {q.options.iter().map(|o| view! { <option value=o.value.clone()>{o.label.clone()}</option> }).collect_view()}
            </select>
        }
        .into_any(),
        QuestionKind::Textarea => view! {
            <textarea
                class="input"
                rows="3"
                placeholder=q.placeholder.clone()
                disabled=move || !active.get()
                on:input=move |ev| set_one(event_target_value(&ev))
            ></textarea>
        }
        .into_any(),
        QuestionKind::Text => view! {
            <input
                class="input"
                placeholder=q.placeholder.clone()
                disabled=move || !active.get()
                on:input=move |ev| set_one(event_target_value(&ev))
            />
        }
        .into_any(),
    };
    view! {
        <fieldset class="qfield">
            <legend>{q.label.clone()}{q.required.then_some(" *")}</legend>
            <div class=if q.kind.has_options() { "opts" } else { "" }>{control}</div>
        </fieldset>
    }
}

#[cfg(test)]
mod tests {
    use super::{folded_into_plan, tool_label};

    #[test]
    fn plan_updates_fold_into_the_plan_list() {
        assert!(folded_into_plan("TodoWrite", true));
        assert!(!folded_into_plan("TodoWrite", false), "no plan read: the row stays");
        assert!(!folded_into_plan("update_plan", true), "only TodoWrite fills the list");
        assert!(!folded_into_plan("Write", true));
    }

    #[test]
    fn tools_read_as_what_they_did() {
        assert_eq!(tool_label("TodoWrite"), "Updated plan");
        assert_eq!(tool_label("Write"), "Wrote");
        assert_eq!(tool_label("MultiEdit"), "Edited");
        assert_eq!(tool_label("mcp__contour__list"), "mcp__contour__list");
    }
}
