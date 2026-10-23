//! "New design system" on Home (spec 1.4): name + brief go to
//! `POST /api/design-systems/generate`; the job is polled until the agent's
//! package is installed, then the picker reloads and selects it.

use super::icons::{Glyph, icon};
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::DesignSystemSummary;
use serde_json::{Value, json};

/// Poll interval for the generation job.
const POLL_MS: u32 = 400;

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Running,
    Done(String),
    Failed(String),
}

#[component]
pub fn NewDesignSystem(
    systems: LocalResource<Vec<DesignSystemSummary>>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let shell = Shell::get();
    let open = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let brief = RwSignal::new(String::new());
    let phase = RwSignal::new(Phase::Idle);

    let start = move || {
        if phase.get_untracked() == Phase::Running
            || name.with_untracked(|n| n.trim().is_empty())
            || brief.with_untracked(|b| b.trim().is_empty())
        {
            return;
        }
        phase.set(Phase::Running);
        let body = json!({
            "name": name.get_untracked().trim(),
            "brief": brief.get_untracked().trim(),
            "agentId": shell.agent_id(),
        });
        leptos::task::spawn_local(async move {
            let job = match api::post::<_, Value>("/api/design-systems/generate", &body).await {
                Ok(v) => v["job"]["id"].as_str().unwrap_or_default().to_owned(),
                Err(e) => {
                    phase.set(Phase::Failed(e.to_string()));
                    return;
                }
            };
            loop {
                gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
                let v = match api::get::<Value>(&format!("/api/design-systems/jobs/{job}")).await {
                    Ok(v) => v,
                    Err(e) => {
                        phase.set(Phase::Failed(e.to_string()));
                        return;
                    }
                };
                match v["job"]["status"].as_str() {
                    Some("succeeded") => {
                        let id = v["job"]["designSystemId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        systems.refetch();
                        selected.set(Some(id));
                        phase.set(Phase::Done(
                            v["job"]["name"].as_str().unwrap_or_default().to_owned(),
                        ));
                        name.set(String::new());
                        brief.set(String::new());
                        open.set(false);
                        return;
                    }
                    Some("failed") => {
                        phase.set(Phase::Failed(
                            v["job"]["error"].as_str().unwrap_or_default().to_owned(),
                        ));
                        return;
                    }
                    _ => {}
                }
            }
        });
    };

    let status = move || {
        match phase.get() {
        Phase::Idle => None,
        Phase::Running => Some(view! {
            <p class="ds-status" data-testid="ds-status"><span class="spinner"></span>{t(T::GeneratingDesignSystemHint)}</p>
        }.into_any()),
        Phase::Done(n) => Some(view! {
            <p class="ds-status ok" data-testid="ds-status">{icon(Glyph::CheckCircle)}{tf(T::DesignSystemCreated, &[n.as_str()])}</p>
        }.into_any()),
        Phase::Failed(e) => Some(view! {
            <p class="error-text" role="alert" data-testid="ds-status">{tf(T::DesignSystemFailed, &[e.as_str()])}</p>
        }.into_any()),
    }
    };
    let name_ref = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = name_ref.get() {
            let _ = el.focus();
        }
    });

    view! {
        <div class="ds-create">
            <button
                class="btn ghost small"
                data-testid="ds-new"
                aria-haspopup="dialog"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {icon(Glyph::Sparkle)}
                {t(T::NewDesignSystem)}
            </button>
            <Show when=move || !open.get()>{status}</Show>
            <Show when=move || open.get()>
                <div
                    class="overlay center"
                    on:click=move |_| open.set(false)
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Escape" {
                            open.set(false);
                        }
                    }
                >
                    <div
                        class="dialog"
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="ds-dialog-title"
                        data-testid="ds-form"
                        on:click=|ev| ev.stop_propagation()
                    >
                        <div class="dialog-head">
                            <span class="dialog-icon">{icon(Glyph::Sparkle)}</span>
                            <div class="grow">
                                <h2 id="ds-dialog-title">{t(T::DsDialogTitle)}</h2>
                                <p>{t(T::DsDialogBody)}</p>
                            </div>
                            <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| open.set(false)>
                                {icon(Glyph::Close)}
                            </button>
                        </div>
                        <div class="dialog-body">
                            <label class="field">
                                <span>{t(T::DsName)}</span>
                                <input
                                    class="input"
                                    data-testid="ds-name"
                                    node_ref=name_ref
                                    placeholder=t(T::DesignSystemNamePlaceholder)
                                    prop:value=move || name.get()
                                    on:input=move |ev| name.set(event_target_value(&ev))
                                />
                            </label>
                            <label class="field">
                                <span>{t(T::DsBrief)}</span>
                                <textarea
                                    class="input"
                                    data-testid="ds-brief"
                                    rows="4"
                                    placeholder=t(T::DesignSystemBriefPlaceholder)
                                    prop:value=move || brief.get()
                                    on:input=move |ev| brief.set(event_target_value(&ev))
                                ></textarea>
                            </label>
                            {status}
                        </div>
                        <div class="dialog-foot">
                            <button class="btn ghost" on:click=move |_| open.set(false)>{t(T::Cancel)}</button>
                            <button
                                class="btn primary"
                                data-testid="ds-generate"
                                disabled=move || {
                                    phase.get() == Phase::Running
                                        || name.with(|n| n.trim().is_empty())
                                        || brief.with(|b| b.trim().is_empty())
                                }
                                on:click=move |_| start()
                            >
                                {move || if phase.get() == Phase::Running {
                                    view! { <span class="spinner on-accent"></span>{t(T::GeneratingDesignSystem)} }.into_any()
                                } else {
                                    view! { {icon(Glyph::Sparkle)}{t(T::GenerateDesignSystem)} }.into_any()
                                }}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}
