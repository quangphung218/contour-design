//! Export menu dialogs: save the shown page as a design template, and hand
//! the design off with a chosen stack and coding agent or editor.

use super::api;
use super::export_menu::copy_text;
use super::icons::{Glyph, icon};
use super::studio::StudioCtx;
use super::terminal;
use crate::handoff::{self, Framework, Target};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::{HandoffResponse, SaveTemplateRequest, SaveTemplateResponse};

type Status = RwSignal<Option<Result<String, String>>>;

/// Closes on the next task (design-system.md, shell dialogs).
fn close_later(open: RwSignal<bool>) {
    leptos::task::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        open.set(false);
    });
}

#[component]
pub fn SaveTemplateDialog(ctx: StudioCtx, open: RwSignal<bool>, status: Status) -> impl IntoView {
    let file = ctx.active_untracked().unwrap_or_default();
    let default_name = ctx
        .project
        .with_untracked(|p| p.as_ref().map(|p| p.name.clone()))
        .unwrap_or_default();
    let name = RwSignal::new(default_name);
    let description = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
            el.select();
        }
    });
    let file = StoredValue::new(file);
    let save = move || {
        let n = name.get_untracked().trim().to_owned();
        if n.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let req = SaveTemplateRequest {
                name: n.clone(),
                description: Some(description.get_untracked()).filter(|d| !d.trim().is_empty()),
                file_name: file.get_value(),
            };
            let res = api::post::<_, SaveTemplateResponse>(
                &format!("{}/save-template", ctx.base()),
                &req,
            )
            .await;
            busy.set(false);
            match res {
                Ok(_) => {
                    status.set(Some(Ok(tf(T::TemplateSaved, &[&n]))));
                    close_later(open);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    view! {
        <div class="overlay center" on:click=move |_| close_later(open)
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { close_later(open); }>
            <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="template-title" data-testid="template-dialog"
                on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Grid)}</span>
                    <div class="grow">
                        <h2 id="template-title">{t(T::SaveAsTemplate)}</h2>
                        <p>{t(T::SaveTemplateBody)}</p>
                    </div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| close_later(open)>{icon(Glyph::Close)}</button>
                </div>
                <div class="dialog-body">
                    <label class="field">
                        <span>{t(T::TemplateName)}</span>
                        <input class="input" data-testid="template-name" node_ref=input
                            prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { ev.prevent_default(); save(); }/>
                    </label>
                    <label class="field">
                        <span>{t(T::TemplateDescription)}</span>
                        <textarea class="input" rows="3" data-testid="template-description" placeholder=t(T::TemplateDescriptionHint)
                            prop:value=move || description.get() on:input=move |ev| description.set(event_target_value(&ev))></textarea>
                    </label>
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                </div>
                <div class="dialog-foot">
                    <button class="btn ghost" on:click=move |_| close_later(open)>{t(T::Cancel)}</button>
                    <button class="btn primary" data-testid="template-save" disabled=move || busy.get() || name.with(|n| n.trim().is_empty())
                        on:click=move |_| save()>
                        {move || busy.get().then(|| view! { <span class="spinner on-accent"></span> })}
                        {t(T::Save)}
                    </button>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn HandoffDialog(ctx: StudioCtx, open: RwSignal<bool>, status: Status) -> impl IntoView {
    let framework = RwSignal::new(Framework::Existing);
    let target = RwSignal::new(Target::ClaudeCode);
    let info = LocalResource::new(move || async move {
        api::get::<HandoffResponse>(&format!("{}/handoff", ctx.base()))
            .await
            .map_err(|e| e.to_string())
    });
    let prompt = move || {
        info.get()
            .and_then(Result::ok)
            .map(|h| handoff::prompt(&h.prompt, framework.get(), target.get()))
    };
    let go = move || {
        let Some(text) = prompt() else {
            return;
        };
        let dir = info
            .get_untracked()
            .and_then(Result::ok)
            .map(|h| h.dir)
            .unwrap_or_default();
        let target = target.get_untracked();
        leptos::task::spawn_local(async move {
            match copy_text(&text).await {
                Ok(()) => {
                    if target.is_cli() {
                        terminal::open(ctx);
                        status.set(Some(Ok(tf(T::HandoffCliReady, &[target.name()]))));
                    } else if let Some(url) = target.open_url(&dir) {
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().set_href(&url);
                        }
                        status.set(Some(Ok(tf(T::HandoffEditorOpened, &[target.name()]))));
                    }
                    close_later(open);
                }
                Err(e) => status.set(Some(Err(tf(T::HandoffFailed, &[&e])))),
            }
        });
    };
    view! {
        <div class="overlay center" on:click=move |_| close_later(open)
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { close_later(open); }>
            <div class="dialog wide" role="dialog" aria-modal="true" aria-labelledby="handoff-title" data-testid="handoff-dialog"
                on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Handoff)}</span>
                    <div class="grow">
                        <h2 id="handoff-title">{t(T::HandoffChoose)}</h2>
                        <p>{t(T::HandoffBody)}</p>
                    </div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| close_later(open)>{icon(Glyph::Close)}</button>
                </div>
                <div class="dialog-body">
                    <div class="row tight">
                        <label class="field grow">
                            <span>{t(T::HandoffFramework)}</span>
                            <select class="input" data-testid="handoff-framework" on:change=move |ev| framework.set(Framework::from_key(&event_target_value(&ev)))>
                                {Framework::ALL.into_iter().map(|f| view! {
                                    <option value=f.key() selected=move || framework.get() == f>{f.name().unwrap_or(t(T::HandoffExistingStack))}</option>
                                }).collect_view()}
                            </select>
                        </label>
                        <label class="field grow">
                            <span>{t(T::HandoffTarget)}</span>
                            <select class="input" data-testid="handoff-target" on:change=move |ev| target.set(Target::from_key(&event_target_value(&ev)))>
                                {Target::ALL.into_iter().map(|x| view! {
                                    <option value=x.key() selected=move || target.get() == x>{x.name()}</option>
                                }).collect_view()}
                            </select>
                        </label>
                    </div>
                    <label class="field">
                        <span>{t(T::HandoffPrompt)}</span>
                        <textarea class="input mono" rows="9" readonly data-testid="handoff-prompt"
                            prop:value=move || prompt().unwrap_or_default()></textarea>
                    </label>
                    {move || info.get().and_then(Result::err).map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                </div>
                <div class="dialog-foot">
                    <button class="btn ghost" on:click=move |_| close_later(open)>{t(T::Cancel)}</button>
                    <button class="btn primary" data-testid="handoff-go" disabled=move || prompt().is_none() on:click=move |_| go()>
                        {move || if target.get().is_cli() {
                            view! { {icon(Glyph::Terminal)}{t(T::HandoffCopyOpenTerminal)} }.into_any()
                        } else {
                            view! { {icon(Glyph::ExternalLink)}{tf(T::HandoffCopyOpenEditor, &[target.get().name()])} }.into_any()
                        }}
                    </button>
                </div>
            </div>
        </div>
    }
}
