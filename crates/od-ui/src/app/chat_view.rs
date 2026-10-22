//! Chat pane: message list, assistant blocks, question forms, composer.

use super::Shell;
use super::studio::StudioCtx;
use crate::chat::{AssistantTurn, Block, ToolCall, TurnState};
use crate::markdown;
use crate::question_form::{
    self, Answers, Question, QuestionForm, QuestionKind, Segment, format_answers, missing_required,
};
use leptos::prelude::*;

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
        if text.trim().is_empty() || ctx.live_run.get_untracked().is_some() {
            return;
        }
        draft.set(String::new());
        ctx.send(text, shell);
    };
    let send_key = send;

    view! {
        <section class="chat" aria-label="Chat">
            <div class="messages" node_ref=list_ref data-testid="messages">
                {move || ctx.notice.get().map(|n| view! { <p class="error-text">{n}</p> })}
                <For
                    each=move || ctx.items.get()
                    key=|item| item.id.clone()
                    children=move |item| match item.kind {
                        ItemKind::User(text) => view! { <UserBubble text=text/> }.into_any(),
                        ItemKind::Assistant(turn) => {
                            let id = item.id.clone();
                            // A form can be answered only on the latest turn.
                            let is_last = Signal::derive(move || {
                                ctx.items.with(|items| items.last().is_some_and(|i| i.id == id))
                            });
                            view! { <AssistantView turn=turn is_last=is_last/> }.into_any()
                        }
                    }
                />
                <Show when=move || ctx.items.with(Vec::is_empty)>
                    <p class="empty">"Describe what to build or change."</p>
                </Show>
            </div>
            <div class="composer">
                <textarea
                    class="composer-input"
                    data-testid="composer"
                    rows="3"
                    placeholder="Ask for a change…  (Enter to send, Shift+Enter for a new line)"
                    prop:value=move || draft.get()
                    on:input=move |ev| draft.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() {
                            ev.prevent_default();
                            send_key();
                        }
                    }
                ></textarea>
                {move || {
                    if ctx.live_run.get().is_some() {
                        view! {
                            <button class="btn danger" data-testid="stop" on:click=move |_| ctx.cancel()>
                                "Stop"
                            </button>
                        }
                            .into_any()
                    } else {
                        view! {
                            <button
                                class="btn primary"
                                data-testid="send"
                                disabled=move || draft.with(|d| d.trim().is_empty())
                                on:click=move |_| send()
                            >
                                "Send"
                            </button>
                        }
                            .into_any()
                    }
                }}
            </div>
        </section>
    }
}

#[component]
fn UserBubble(text: String) -> impl IntoView {
    if question_form::is_answers_message(&text) {
        let body: String = text.lines().skip(1).collect::<Vec<_>>().join("\n");
        return view! {
            <div class="msg user answers">
                <span class="answers-label">"Answers"</span>
                <pre>{body}</pre>
            </div>
        }
        .into_any();
    }
    view! { <div class="msg user" data-testid="user-msg">{text}</div> }.into_any()
}

#[component]
fn AssistantView(turn: RwSignal<AssistantTurn>, is_last: Signal<bool>) -> impl IntoView {
    let state = move || turn.with(|t| t.state);
    view! {
        <div class="msg assistant" data-testid="assistant-msg" data-state=move || format!("{:?}", state()).to_lowercase()>
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
                        let label = t.status.clone().unwrap_or_else(|| "Working".to_owned());
                        Some(view! { <div class="run-status"><span class="spinner"></span>{label}</div> }.into_any())
                    }
                    TurnState::Canceled => Some(view! { <div class="run-status muted">"Stopped"</div> }.into_any()),
                    TurnState::Succeeded | TurnState::Failed => None,
                })
            }}
        </div>
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
                    view! { <div class="form-pending"><span class="spinner"></span>"Preparing questions…"</div> }
                        .into_any()
                }
            })
            .collect_view()
            .into_any(),
        Block::Thinking(text) => view! {
            <details class="thinking">
                <summary>"Thinking"</summary>
                <pre>{text}</pre>
            </details>
        }
        .into_any(),
        Block::Tool(call) => view! { <ToolRow call=call/> }.into_any(),
        Block::Error(message) => view! { <div class="msg-error" role="alert">{message}</div> }.into_any(),
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
                (!m.is_empty()).then(|| view! { <p class="error-text">{format!("Please answer: {}", m.join(", "))}</p> })
            }}
            <div class="row end">
                <button
                    type="button"
                    class="btn primary"
                    data-testid="qform-submit"
                    disabled=move || !active()
                    on:click=submit
                >
                    {move || if submitted.get() { "Sent" } else { "Send answers" }}
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
                <option value="">"Choose…"</option>
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
