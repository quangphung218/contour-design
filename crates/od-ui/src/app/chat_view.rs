//! Chat pane: conversations, message list, assistant blocks, question
//! forms, and the composer with its queue, plan pill and comment chips.

use super::Shell;
use super::icons::{Glyph, icon};
use super::preview_comments::comment_label;
use super::preview_tools::{AttachmentChips, DeliverableWarning};
use super::studio::{StudioCtx, conversation_title};
use crate::chat::{AssistantTurn, Block, TodoStatus, ToolCall, plan_progress};
use crate::guide::{Followup, NextStep, RunPhase, next_steps, run_phase};
use crate::i18n::{T, t, tf};
use crate::markdown;
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
        }
    }

    pub fn assistant(id: String, turn: RwSignal<AssistantTurn>) -> Self {
        Self {
            id,
            kind: ItemKind::Assistant(turn),
        }
    }
}

#[component]
pub fn ChatPane(ctx: StudioCtx) -> impl IntoView {
    let shell = Shell::get();
    let draft = ctx.draft;
    let list_ref = NodeRef::<leptos::html::Div>::new();

    // Keep the newest output in view while streaming.
    Effect::new(move |_| {
        ctx.items.with(|items| {
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
        if text.trim().is_empty() {
            return;
        }
        draft.set(String::new());
        // While a run is going this queues the message.
        ctx.send(text, shell);
    };
    let send_key = send;

    view! {
        <section class="chat" aria-label=t(T::Chat)>
            <ConversationBar ctx=ctx/>
            <super::agent_w3::ChatTools ctx=ctx/>
            <div class="messages" node_ref=list_ref data-testid="messages">
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
                <Show when=move || ctx.items.with(Vec::is_empty)>
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
            <PlanPill ctx=ctx/>
            <QueueStack ctx=ctx/>
            <CommentChips ctx=ctx/>
            <AttachmentChips ctx=ctx/>
            <DeliverableWarning ctx=ctx/>
            <div class="composer">
                <textarea
                    class="composer-input"
                    data-testid="composer"
                    aria-label=t(T::Chat)
                    rows="3"
                    placeholder=t(T::ComposerPlaceholder)
                    prop:value=move || draft.get()
                    on:input=move |ev| draft.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() {
                            ev.prevent_default();
                            send_key();
                        }
                    }
                ></textarea>
                <div class="composer-actions">
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
                                disabled=move || draft.with(|d| d.trim().is_empty())
                                on:click=move |_| send()
                            >
                                {icon(if live { Glyph::Queue } else { Glyph::Send })}
                                {label}
                            </button>
                        }
                    }}
                </div>
            </div>
        </section>
    }
}

#[component]
fn ConversationBar(ctx: StudioCtx) -> impl IntoView {
    view! {
        <div class="pane-head conversation-bar">
            <span class="conversation-ico">{icon(Glyph::Chat)}</span>
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
            <super::agent_w3::TranscriptMenu ctx=ctx/>
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
                                        on:click=move |_| ctx.queue.update(|q| q.retain(|x| x.id != id_del))>{icon(Glyph::Close)}</button>
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
                                <strong>{comment_label(&c)}</strong>
                                {format!(": {}", c.note)}
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
            {move || {
                turn.with(|t| {
                    t.blocks
                        .iter()
                        .cloned()
                        .map(|b| render_block(b, turn, is_last))
                        .collect_view()
                })
            }}
            <RunLine turn=turn/>
            <Show when=move || is_last.get() && ctx.live_run.get().is_none()>
                <NextSteps turn=turn item_id=item_id.get_value()/>
            </Show>
            <Show when=move || !turn.with(|t| t.state.is_live()) && ctx.live_run.get().is_none()>
                <div class="msg-actions">
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
/// written), failed or stopped. `data-phase` mirrors it for tests.
#[component]
fn RunLine(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    move || {
        turn.with(|tn| {
            let (phase, glyph, label, detail) = match run_phase(tn) {
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
                    (files > 0).then(|| tf(T::RunFilesWritten, &[&files.to_string()])),
                ),
                RunPhase::Failed => ("failed", Some(Glyph::Warning), t(T::RunFailed).to_owned(), Some(t(T::RunFailedHint).to_owned())),
                RunPhase::Stopped => ("stopped", Some(Glyph::Stop), t(T::Stopped).to_owned(), None),
            };
            view! {
                <div class=format!("run-status {phase}") role="status" data-testid="run-state" data-phase=phase>
                    {match glyph {
                        Some(g) => icon(g).into_any(),
                        None => view! { <span class="spinner"></span> }.into_any(),
                    }}
                    <strong>{label}</strong>
                    {detail.map(|d| view! { <span class="muted">{d}</span> })}
                </div>
            }
        })
    }
}

/// Suggested next steps under the latest finished turn.
#[component]
fn NextSteps(turn: RwSignal<AssistantTurn>, item_id: String) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let shell = Shell::get();
    let navigate = leptos_router::hooks::use_navigate();
    let item_id = StoredValue::new(item_id);
    // The user message this turn answered, for a retry.
    let asked = move || {
        ctx.items.with_untracked(|items| {
            let i = items.iter().position(|i| i.id == item_id.get_value())?;
            items[..i].iter().rev().find_map(|i| match &i.kind {
                ItemKind::User(text) => Some(text.clone()),
                ItemKind::Assistant(_) => None,
            })
        })
    };
    move || {
        let steps = turn.with(next_steps);
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
                                    if let Some(text) = asked() {
                                        ctx.send(text, shell);
                                    }
                                }
                                NextStep::Settings => navigate("/settings", Default::default()),
                            };
                            view! {
                                <button class="chip next-step" data-testid=testid on:click=run>
                                    {icon(glyph)}{label}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            }
        })
    }
}

/// Puts the caret in the chat composer.
pub(super) fn focus_composer() {
    use wasm_bindgen::JsCast;
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("[data-testid=composer]").ok().flatten())
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
        Block::Tool(call) => view! { <ToolRow call=call/> }.into_any(),
        Block::Error(message) => view! { <div class="msg-error" role="alert">{icon(Glyph::Warning)}<span>{message}</span></div> }.into_any(),
        Block::Steered(text) => view! { <div class="steered">{icon(Glyph::ArrowRight)}{tf(T::Steered, &[&text])}</div> }.into_any(),
    }
}

#[component]
fn ToolRow(call: ToolCall) -> impl IntoView {
    let (glyph, class) = match call.result {
        None => (Glyph::Loader, "tool running"),
        Some(false) => (Glyph::Check, "tool ok"),
        Some(true) => (Glyph::Failed, "tool failed"),
    };
    view! {
        <div class=class>
            <span class="tool-icon">{icon(glyph)}</span>
            <span class="tool-name">{call.name}</span>
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
