//! "New design system" on Home (spec 1.4): name + brief go to
//! `POST /api/design-systems/generate`; the job is polled until the agent's
//! package is installed, then the picker reloads and selects it.

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
                        let id = v["job"]["designSystemId"].as_str().unwrap_or_default().to_owned();
                        systems.refetch();
                        selected.set(Some(id));
                        phase.set(Phase::Done(v["job"]["name"].as_str().unwrap_or_default().to_owned()));
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

    view! {
        <div class="ds-create">
            <button
                class="btn ghost"
                data-testid="ds-new"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {t(T::NewDesignSystem)}
            </button>
            <Show when=move || open.get()>
                <div class="ds-create-form" data-testid="ds-form">
                    <input
                        class="input"
                        data-testid="ds-name"
                        placeholder=t(T::DesignSystemNamePlaceholder)
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev))
                    />
                    <textarea
                        class="input"
                        data-testid="ds-brief"
                        rows="3"
                        placeholder=t(T::DesignSystemBriefPlaceholder)
                        prop:value=move || brief.get()
                        on:input=move |ev| brief.set(event_target_value(&ev))
                    ></textarea>
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
                        {move || if phase.get() == Phase::Running { t(T::GeneratingDesignSystem) } else { t(T::GenerateDesignSystem) }}
                    </button>
                </div>
            </Show>
            {move || match phase.get() {
                Phase::Idle => None,
                Phase::Running => Some(view! { <p class="ds-status" data-testid="ds-status">{t(T::GeneratingDesignSystemHint)}</p> }.into_any()),
                Phase::Done(n) => Some(view! { <p class="ds-status ok" data-testid="ds-status">{tf(T::DesignSystemCreated, &[n.as_str()])}</p> }.into_any()),
                Phase::Failed(e) => Some(view! { <p class="error-text" data-testid="ds-status">{tf(T::DesignSystemFailed, &[e.as_str()])}</p> }.into_any()),
            }}
        </div>
    }
}
