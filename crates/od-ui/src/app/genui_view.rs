//! GenUI surfaces over the chat composer (track C4-depth): the questions a
//! plugin run asks (Continue, Authorize, Accept all…), answered in place,
//! and the remembered answers with Revoke (the original's `GenUIInbox`).
//! Decisions and answer shapes live in [`crate::genui`].

use super::api;
use super::icons::{Glyph, icon};
use super::sse::Sse;
use super::studio::StudioCtx;
use crate::genui::{
    self, Field, FieldKind, FilePick, FormError, Shape, SurfaceRow, authorize_answer,
};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Run events after which the surfaces are read again.
const EVENTS: [&str; 3] = [
    "genui_surface_request",
    "genui_surface_response",
    "genui_state_synced",
];

#[derive(Debug, Deserialize)]
struct ProjectSurfaces {
    surfaces: Vec<SurfaceRow>,
}

/// Pending surfaces of the open conversation, then the remembered answers.
#[component]
pub fn GenuiSurfaces(ctx: StudioCtx) -> impl IntoView {
    let rows = RwSignal::new(Vec::<SurfaceRow>::new());
    let base = move || {
        format!(
            "/api/projects/{}/genui",
            api::seg(&ctx.project_id.get_value())
        )
    };
    let reload = move || {
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<ProjectSurfaces>(&base()).await {
                rows.set(r.surfaces);
            }
        });
    };
    // Listen on the live run for its surfaces as they are asked.
    let stream = StoredValue::new(None::<SendWrapper<Sse>>);
    Effect::new(move |_| {
        let live = ctx.live_run.get().filter(|r| !r.is_empty());
        ctx.conversation_id.track();
        stream.set_value(None);
        reload();
        if let Some(run) = live {
            let url = format!("/api/runs/{}/events", api::seg(&run));
            let s = Sse::open(&url, &EVENTS, move |_, _| reload(), || {});
            stream.set_value(s.map(SendWrapper::new));
        }
    });
    on_cleanup(move || stream.set_value(None));

    let pending = Memo::new(move |_| {
        let conversation = ctx.conversation_id.get();
        rows.with(|r| genui::pending_in(r, conversation.as_deref()))
    });
    let remembered = Memo::new(move |_| {
        rows.with(|r| {
            r.iter()
                .filter(|s| s.is_remembered())
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let show_remembered = RwSignal::new(false);
    let revoke = move |surface_id: String| {
        leptos::task::spawn_local(async move {
            let path = format!("{}/{}/revoke", base(), api::seg(&surface_id));
            if let Err(e) = api::post::<_, Value>(&path, &json!({})).await {
                ctx.notice.set(Some(e.to_string()));
            }
            reload();
        });
    };

    view! {
        <Show when=move || !pending.with(Vec::is_empty)>
            <section class="genui-stack" data-testid="genui-surfaces" aria-label=t(T::GenuiWaiting)>
                <For each=move || pending.get() key=|r| r.id.clone() let:row>
                    <SurfaceCard row=row on_done=Callback::new(move |()| reload())/>
                </For>
            </section>
        </Show>
        <Show when=move || !remembered.with(Vec::is_empty)>
            <div class="genui-memory" data-testid="genui-inbox">
                <button class="btn ghost small" data-testid="genui-inbox-toggle"
                    aria-expanded=move || show_remembered.get().to_string()
                    on:click=move |_| show_remembered.update(|s| *s = !*s)>
                    {icon(Glyph::Memory)}
                    {move || tf(T::GenuiRemembered, &[&remembered.with(Vec::len).to_string()])}
                </button>
                <Show when=move || show_remembered.get()>
                    <p class="muted small">{t(T::GenuiRememberedHint)}</p>
                    <ul class="genui-memory-list">
                        <For each=move || remembered.get() key=|r| r.id.clone() let:row>
                            {
                                let sid = StoredValue::new(row.surface_id.clone());
                                let tier = if row.persist == "project" { T::GenuiTierProject } else { T::GenuiTierConversation };
                                view! {
                                    <li class="row small" data-testid="genui-remembered" data-surface=row.surface_id.clone()>
                                        <code class="grow">{row.surface_id.clone()}</code>
                                        <span class="badge outline">{t(tier)}</span>
                                        <button class="btn ghost small danger" data-testid="genui-revoke"
                                            on:click=move |_| revoke(sid.get_value())>{t(T::GenuiRevoke)}</button>
                                    </li>
                                }
                            }
                        </For>
                    </ul>
                </Show>
            </div>
        </Show>
    }
}

/// One pending surface with its controls.
#[component]
fn SurfaceCard(row: SurfaceRow, on_done: Callback<()>) -> impl IntoView {
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let path = StoredValue::new(format!(
        "/api/runs/{}/genui/{}/respond",
        api::seg(row.run_id.as_deref().unwrap_or_default()),
        api::seg(&row.surface_id)
    ));
    let answer = Callback::new(move |value: Value| {
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let res = api::post::<_, Value>(&path.get_value(), &json!({ "value": value })).await;
            busy.set(false);
            match res {
                Ok(_) => on_done.run(()),
                Err(e) => error.set(Some(tf(T::GenuiAnswerFailed, &[&e.to_string()]))),
            }
        });
    });
    let shape = genui::shape(&row);
    let prompt = row.prompt().map(str::to_owned);
    let body = match shape {
        Shape::Confirmation => view! {
            <p class="genui-prompt">{prompt.unwrap_or_else(|| t(T::GenuiConfirmDefault).to_owned())}</p>
            <div class="row genui-actions">
                <button class="btn small primary" data-testid="genui-confirm" disabled=busy
                    on:click=move |_| answer.run(json!(true))>{icon(Glyph::Check)}{t(T::GenuiContinue)}</button>
                <button class="btn small ghost" data-testid="genui-cancel" disabled=busy
                    on:click=move |_| answer.run(json!(false))>{t(T::Cancel)}</button>
            </div>
        }
        .into_any(),
        Shape::OAuth { route, target } => {
            let value = authorize_answer(&row);
            let hint = target.clone().map(|t| {
                tf(if route == "mcp" { T::GenuiMcpHint } else { T::GenuiConnectorHint }, &[&t])
            });
            let fallback = tf(
                T::GenuiAuthorizeDefault,
                &[target.as_deref().unwrap_or(t(T::GenuiTheConnection))],
            );
            view! {
                <p class="genui-prompt">{prompt.unwrap_or(fallback)}</p>
                {hint.map(|h| view! { <p class="muted small">{h}</p> })}
                <div class="row genui-actions">
                    <button class="btn small primary" data-testid="genui-authorize" disabled=busy
                        on:click=move |_| answer.run(value.clone())>{icon(Glyph::Key)}{t(T::GenuiAuthorize)}</button>
                    <button class="btn small ghost" data-testid="genui-skip" disabled=busy
                        on:click=move |_| answer.run(json!({ "authorized": false, "skipped": true }))>{t(T::GenuiSkip)}</button>
                </div>
            }
            .into_any()
        }
        Shape::DiffReview { files } => view! {
            <DiffReview prompt=prompt files=files answer=answer busy=busy error=error/>
        }
        .into_any(),
        Shape::Choice { key, options } => view! {
            <p class="genui-prompt">{prompt.unwrap_or_else(|| t(T::GenuiAnswerDefault).to_owned())}</p>
            <div class="row wrap genui-actions" role="group">
                {options.into_iter().map(|o| {
                    let value = json!({ key.clone(): o.clone() });
                    let testid = format!("genui-choice-{o}");
                    view! {
                        <button class="btn small" data-testid=testid disabled=busy
                            on:click=move |_| answer.run(value.clone())>{o}</button>
                    }
                }).collect_view()}
                <button class="btn small ghost" data-testid="genui-skip" disabled=busy
                    on:click=move |_| answer.run(Value::Null)>{t(T::GenuiSkip)}</button>
            </div>
        }
        .into_any(),
        Shape::Form(fields) => view! {
            <p class="genui-prompt">{prompt.unwrap_or_else(|| t(T::GenuiAnswerDefault).to_owned())}</p>
            <FormFields fields=fields answer=answer busy=busy error=error/>
        }
        .into_any(),
        Shape::Raw => {
            let text = RwSignal::new(String::new());
            view! {
                <p class="genui-prompt">{prompt.unwrap_or_else(|| t(T::GenuiAnswerDefault).to_owned())}</p>
                <textarea class="input small" rows="3" data-testid="genui-raw" placeholder=t(T::GenuiJsonPlaceholder)
                    prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev))></textarea>
                <div class="row genui-actions">
                    <button class="btn small primary" data-testid="genui-submit" disabled=busy
                        on:click=move |_| answer.run(genui::raw_answer(&text.get_untracked()))>{t(T::GenuiSubmit)}</button>
                </div>
            }
            .into_any()
        }
    };
    view! {
        <div class="card genui-surface" role="group" data-testid="genui-surface"
            data-kind=row.kind.clone() data-surface=row.surface_id.clone()>
            <div class="genui-head small">
                {icon(Glyph::Plugin)}
                <span>{t(T::GenuiWaiting)}</span>
            </div>
            {body}
            {move || error.get().map(|e| view! { <p class="error-text small" data-testid="genui-error">{e}</p> })}
        </div>
    }
}

/// Accept all / Reject all / per-file picks, with a note.
#[component]
fn DiffReview(
    prompt: Option<String>,
    files: Vec<String>,
    answer: Callback<Value>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let reason = RwSignal::new(String::new());
    let partial = RwSignal::new(false);
    let picks = RwSignal::new(
        files
            .iter()
            .map(|f| (f.clone(), FilePick::Undecided))
            .collect::<Vec<_>>(),
    );
    let all = StoredValue::new(files.clone());
    let hint = if files.is_empty() {
        t(T::GenuiNoFileList).to_owned()
    } else {
        tf(T::GenuiFilesTouched, &[&files.len().to_string()])
    };
    let pick = move |file: String, p: FilePick| {
        picks.update(|ps| {
            if let Some(slot) = ps.iter_mut().find(|(f, _)| *f == file) {
                slot.1 = p;
            }
        });
    };
    let submit_partial =
        move |_| match genui::partial_answer(&picks.get_untracked(), &reason.get_untracked()) {
            Ok(v) => answer.run(v),
            Err(missing) => error.set(Some(tf(T::GenuiPickEach, &[&missing.join(", ")]))),
        };
    view! {
        <p class="genui-prompt">{prompt.unwrap_or_else(|| t(T::GenuiReviewDefault).to_owned())}</p>
        <p class="muted small">{hint}</p>
        <div class="row wrap genui-actions">
            <button class="btn small primary" data-testid="genui-diff-accept" disabled=busy
                on:click=move |_| answer.run(genui::diff_answer(true, &all.get_value(), &reason.get_untracked()))>
                {icon(Glyph::Check)}{t(T::GenuiAcceptAll)}
            </button>
            <button class="btn small ghost danger" data-testid="genui-diff-reject" disabled=busy
                on:click=move |_| answer.run(genui::diff_answer(false, &all.get_value(), &reason.get_untracked()))>
                {t(T::GenuiRejectAll)}
            </button>
            <Show when=move || !all.with_value(Vec::is_empty)>
                <button class="btn small ghost" data-testid="genui-diff-partial" disabled=busy
                    aria-pressed=move || partial.get().to_string()
                    on:click=move |_| partial.update(|p| *p = !*p)>
                    {icon(Glyph::List)}{t(T::GenuiPartial)}
                </button>
            </Show>
        </div>
        <Show when=move || partial.get()>
            <ul class="genui-files">
                {all.get_value().into_iter().map(|f| {
                    let file = StoredValue::new(f.clone());
                    let current = move || picks.with(|ps| ps.iter().find(|(x, _)| *x == file.get_value()).map_or(FilePick::Undecided, |p| p.1));
                    view! {
                        <li class="row small">
                            <code class="grow">{f.clone()}</code>
                            <div class="segmented small" role="group" aria-label=f.clone()>
                                <button class="seg" data-testid=format!("genui-diff-file-accept-{f}")
                                    aria-pressed=move || (current() == FilePick::Accept).to_string()
                                    on:click=move |_| pick(file.get_value(), FilePick::Accept)>{t(T::GenuiAccept)}</button>
                                <button class="seg" data-testid=format!("genui-diff-file-reject-{f}")
                                    aria-pressed=move || (current() == FilePick::Reject).to_string()
                                    on:click=move |_| pick(file.get_value(), FilePick::Reject)>{t(T::GenuiReject)}</button>
                            </div>
                        </li>
                    }
                }).collect_view()}
            </ul>
            <button class="btn small primary" data-testid="genui-diff-partial-submit" disabled=busy
                on:click=submit_partial>{t(T::GenuiSubmitPartial)}</button>
        </Show>
        <textarea class="input small genui-reason" rows="2" data-testid="genui-diff-reason"
            placeholder=t(T::GenuiPatchNotes)
            prop:value=move || reason.get() on:input=move |ev| reason.set(event_target_value(&ev))></textarea>
    }
}

/// A form surface's fields and Submit.
#[component]
fn FormFields(
    fields: Vec<Field>,
    answer: Callback<Value>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let values = RwSignal::new(Map::<String, Value>::new());
    for f in &fields {
        let seed = match &f.kind {
            FieldKind::Bool => "false".to_owned(),
            FieldKind::Enum(o) => o.first().cloned().unwrap_or_default(),
            _ => String::new(),
        };
        values.update(|v| {
            v.insert(f.key.clone(), json!(seed));
        });
    }
    let set = move |key: String, value: String| {
        values.update(|v| {
            v.insert(key, json!(value));
        });
    };
    let all = StoredValue::new(fields.clone());
    let submit = move |_| match genui::form_answer(&all.get_value(), &values.get_untracked()) {
        Ok(v) => answer.run(v),
        Err(e) => error.set(Some(match e {
            FormError::Required(l) => tf(T::GenuiFieldRequired, &[&l]),
            FormError::NotANumber(l) => tf(T::GenuiFieldNumber, &[&l]),
            FormError::NotWhole(l) => tf(T::GenuiFieldWhole, &[&l]),
            FormError::OutOfRange(l) => tf(T::GenuiFieldRange, &[&l]),
        })),
    };
    view! {
        <div class="genui-form">
            {fields.into_iter().map(|f| {
                let key = StoredValue::new(f.key.clone());
                let control = match f.kind.clone() {
                    FieldKind::Bool => view! {
                        <label class="switch">
                            <input type="checkbox" data-testid=format!("genui-field-{}", f.key)
                                on:change=move |ev| set(key.get_value(), event_target_checked(&ev).to_string())/>
                            <span>{f.label.clone()}</span>
                        </label>
                    }.into_any(),
                    FieldKind::Enum(options) => view! {
                        <select data-testid=format!("genui-field-{}", f.key)
                            on:change=move |ev| set(key.get_value(), event_target_value(&ev))>
                            {options.into_iter().map(|o| view! { <option value=o.clone()>{o.clone()}</option> }).collect_view()}
                        </select>
                    }.into_any(),
                    FieldKind::Text { multiline: true } => view! {
                        <textarea class="input small" rows="3" data-testid=format!("genui-field-{}", f.key)
                            on:input=move |ev| set(key.get_value(), event_target_value(&ev))></textarea>
                    }.into_any(),
                    FieldKind::Text { multiline: false } | FieldKind::Number { .. } => view! {
                        <input class="input small" data-testid=format!("genui-field-{}", f.key)
                            inputmode=if matches!(f.kind, FieldKind::Number { .. }) { "decimal" } else { "text" }
                            on:input=move |ev| set(key.get_value(), event_target_value(&ev))/>
                    }.into_any(),
                };
                let is_bool = matches!(f.kind, FieldKind::Bool);
                view! {
                    <div class="field">
                        {(!is_bool).then(|| view! {
                            <span>{f.label.clone()}{f.required.then_some(" *")}</span>
                        })}
                        {control}
                        {f.description.clone().map(|d| view! { <span class="muted">{d}</span> })}
                    </div>
                }
            }).collect_view()}
            <div class="row genui-actions">
                <button class="btn small primary" data-testid="genui-submit" disabled=busy on:click=submit>
                    {t(T::GenuiSubmit)}
                </button>
            </div>
        </div>
    }
}
