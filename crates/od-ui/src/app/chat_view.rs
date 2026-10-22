//! Chat pane: conversations, message list, assistant blocks, question
//! forms, and the composer with its queue, plan pill and comment chips.

use super::Shell;
use super::preview_comments::comment_label;
use super::studio::{StudioCtx, conversation_title};
use crate::chat::{AssistantTurn, Block, TodoStatus, ToolCall, TurnState, plan_progress};
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
    let draft = RwSignal::new(String::new());
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
            <div class="messages" node_ref=list_ref data-testid="messages">
                {move || ctx.notice.get().map(|n| view! { <p class="error-text">{n}</p> })}
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
                    <p class="empty">{t(T::DescribeChange)}</p>
                </Show>
            </div>
            <PlanPill ctx=ctx/>
            <QueueStack ctx=ctx/>
            <CommentChips ctx=ctx/>
            <div class="composer">
                <textarea
                    class="composer-input"
                    data-testid="composer"
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
                    {move || {
                        let live = ctx.live_run.get().is_some();
                        view! {
                            <button
                                class=if live { "btn" } else { "btn primary" }
                                data-testid="send"
                                disabled=move || draft.with(|d| d.trim().is_empty())
                                on:click=move |_| send()
                            >
                                {if live { t(T::Queue) } else { t(T::Send) }}
                            </button>
                        }
                    }}
                    <Show when=move || ctx.live_run.get().is_some()>
                        <button class="btn danger" data-testid="stop" on:click=move |_| ctx.cancel()>
                            {t(T::Stop)}
                        </button>
                    </Show>
                </div>
            </div>
        </section>
    }
}

#[component]
fn ConversationBar(ctx: StudioCtx) -> impl IntoView {
    view! {
        <div class="pane-head conversation-bar">
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
                title=t(T::NewConversation)
                aria-label=t(T::NewConversation)
                data-testid="new-conversation"
                disabled=move || ctx.live_run.get().is_some()
                on:click=move |_| ctx.new_conversation(None)
            >
                "+"
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
                <span class="queue-title muted">{format!("{} · {}", t(T::Queued), t(T::QueuedHint))}</span>
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
                                    <button class="icon-btn" title=t(T::MoveUp) aria-label=t(T::MoveUp) disabled=i == 0
                                        on:click=move |_| ctx.queue.update(|q| move_item(q, i, i.saturating_sub(1)))>"↑"</button>
                                    <button class="icon-btn" title=t(T::MoveDown) aria-label=t(T::MoveDown) disabled=i + 1 == n
                                        on:click=move |_| ctx.queue.update(|q| move_item(q, i, i + 1))>"↓"</button>
                                    <button class="icon-btn" title=t(T::Edit) aria-label=t(T::Edit)
                                        on:click=move |_| editing.set(Some(id_edit.clone()))>"✎"</button>
                                    {can_steer.then(|| view! {
                                        <button class="btn small" title=t(T::SteerHint) data-testid="steer"
                                            on:click=move |_| ctx.steer(id_steer.clone())>{t(T::Steer)}</button>
                                    })}
                                    <button class="icon-btn" title=t(T::Delete) aria-label=t(T::Delete)
                                        on:click=move |_| ctx.queue.update(|q| q.retain(|x| x.id != id_del))>"×"</button>
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
                                <strong>{comment_label(&c)}</strong>
                                {format!(": {}", c.note)}
                                <button
                                    class="chip-x"
                                    title=t(T::Detach)
                                    aria-label=t(T::Detach)
                                    on:click=move |_| ctx.set_comment_status(&id, CommentStatus::Open)
                                >
                                    "×"
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
                <span class="answers-label">{t(T::Answers)}</span>
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
            {move || {
                turn.with(|t| {
                    t.blocks
                        .iter()
                        .cloned()
                        .map(|b| render_block(b, turn, is_last))
                        .collect_view()
                })
            }}
            {move || {
                turn.with(|t| match t.state {
                    TurnState::Pending | TurnState::Running => {
                        let label = t.status.clone().unwrap_or_else(|| crate::i18n::t(T::Working).to_owned());
                        Some(view! { <div class="run-status"><span class="spinner"></span>{label}</div> }.into_any())
                    }
                    TurnState::Canceled => Some(view! { <div class="run-status muted">{crate::i18n::t(T::Stopped)}</div> }.into_any()),
                    TurnState::Succeeded | TurnState::Failed => None,
                })
            }}
            <Show when=move || !turn.with(|t| t.state.is_live()) && ctx.live_run.get().is_none()>
                <div class="msg-actions">
                    <button
                        class="btn ghost small"
                        data-testid="fork"
                        on:click=move |_| ctx.new_conversation(Some(item_id.get_value()))
                    >
                        {t(T::Fork)}
                    </button>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn PlanList(todos: Vec<crate::chat::Todo>) -> impl IntoView {
    view! {
        <details class="plan" open=true>
            <summary>{t(T::Plan)}</summary>
            <ol>
                {todos
                    .into_iter()
                    .map(|td| {
                        let (mark, class) = match td.status {
                            TodoStatus::Completed => ("✓", "done"),
                            TodoStatus::InProgress => ("●", "doing"),
                            TodoStatus::Pending => ("○", "todo"),
                        };
                        view! { <li class=class><span class="plan-mark">{mark}</span>{td.content}</li> }
                    })
                    .collect_view()}
            </ol>
        </details>
    }
}

fn render_block(block: Block, turn: RwSignal<AssistantTurn>, is_last: Signal<bool>) -> AnyView {
    match block {
        Block::Text(text) => question_form::split(&text)
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
                <summary>{t(T::Thinking)}</summary>
                <pre>{text}</pre>
            </details>
        }
        .into_any(),
        Block::Tool(call) => view! { <ToolRow call=call/> }.into_any(),
        Block::Error(message) => view! { <div class="msg-error" role="alert">{message}</div> }.into_any(),
        Block::Steered(text) => view! { <div class="steered">{tf(T::Steered, &[&text])}</div> }.into_any(),
    }
}

#[component]
fn ToolRow(call: ToolCall) -> impl IntoView {
    let (icon, class) = match call.result {
        None => ("…", "tool running"),
        Some(false) => ("✓", "tool ok"),
        Some(true) => ("✕", "tool failed"),
    };
    view! {
        <div class=class>
            <span class="tool-icon">{icon}</span>
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
                    {move || if submitted.get() { t(T::Sent) } else { t(T::SendAnswers) }}
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
