//! Onboarding ▸ step 2, "Your brand" (track C2-chat-home): a website the
//! agent extracts a design system from, through the same job as Design
//! systems ▸ New ▸ Website (`POST /api/design-systems/generate`). Optional:
//! Continue works without it, and waits while an extraction runs.

use super::api;
use super::ds_create::{STEPS, step_index};
use super::icons::{Glyph, icon};
use crate::brand_step::{brand_name, normalize_url};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::{Value, json};

const POLL_MS: u32 = 400;

/// Where the extraction stands.
#[derive(Debug, Clone, PartialEq)]
pub enum BrandState {
    Idle,
    Running(usize),
    /// The installed design system: id and name.
    Done(String, String),
    Failed(String),
}

/// `agent`: the agent that runs the extraction, the one picked and tested.
#[component]
pub fn BrandStep(state: RwSignal<BrandState>, agent: RwSignal<Option<String>>) -> impl IntoView {
    let url = RwSignal::new(String::new());
    let valid = move || url.with(|u| normalize_url(u).is_some());
    let start = move || {
        let Some(target) = normalize_url(&url.get_untracked()) else {
            return;
        };
        if matches!(state.get_untracked(), BrandState::Running(_)) {
            return;
        }
        state.set(BrandState::Running(0));
        let name = brand_name(&target);
        let body = json!({ "name": name, "brief": "", "urls": [target], "agentId": agent.get_untracked() });
        leptos::task::spawn_local(async move {
            let job = match api::post::<_, Value>("/api/design-systems/generate", &body).await {
                Ok(v) => v["job"]["id"].as_str().unwrap_or_default().to_owned(),
                Err(e) => {
                    state.set(BrandState::Failed(e.to_string()));
                    return;
                }
            };
            loop {
                gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
                let v = match api::get::<Value>(&format!(
                    "/api/design-systems/jobs/{}",
                    api::seg(&job)
                ))
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        state.set(BrandState::Failed(e.to_string()));
                        return;
                    }
                };
                let job = &v["job"];
                match job["status"].as_str() {
                    Some("succeeded") => {
                        state.set(BrandState::Done(
                            job["designSystemId"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            job["name"].as_str().unwrap_or(&name).to_owned(),
                        ));
                        return;
                    }
                    Some("failed") => {
                        state.set(BrandState::Failed(
                            job["error"].as_str().unwrap_or_default().to_owned(),
                        ));
                        return;
                    }
                    _ => state.set(BrandState::Running(step_index(
                        job["phase"].as_str().unwrap_or("sources"),
                    ))),
                }
            }
        });
    };
    view! {
        <section class="onboarding-step brand-step" data-testid="onboarding-brand">
            <div class="onboarding-step-head">
                <span class="step-num" aria-hidden="true">"2"</span>
                <div class="grow">
                    <strong>{t(T::BrandStepTitle)}</strong>
                    <span class="badge outline">{t(T::Optional)}</span>
                    <p class="muted small">{t(T::BrandStepBody)}</p>
                </div>
            </div>
            {move || match state.get() {
                BrandState::Done(_, name) => view! {
                    <p class="ds-status ok" data-testid="onboarding-brand-done">
                        {icon(Glyph::CheckCircle)}{tf(T::BrandStepDone, &[&name])}
                    </p>
                }.into_any(),
                BrandState::Running(step) => view! {
                    <ol class="ds-steps" data-testid="onboarding-brand-steps">
                        {STEPS.into_iter().enumerate().map(|(i, label)| view! {
                            <li class=("done", step > i) class=("active", step == i)>
                                {if step > i { icon(Glyph::CheckCircle).into_any() } else if step == i { view! { <span class="spinner"></span> }.into_any() } else { view! { <span class="step-dot"></span> }.into_any() }}
                                {t(label)}
                            </li>
                        }).collect_view()}
                    </ol>
                }.into_any(),
                other => view! {
                    <div class="row">
                        <input class="input grow" type="url" data-testid="onboarding-brand-url"
                            aria-label=t(T::BrandStepUrl)
                            placeholder="acme.com"
                            prop:value=move || url.get()
                            on:input=move |ev| url.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { start() }/>
                        <button class="btn" data-testid="onboarding-brand-extract" disabled=move || !valid() on:click=move |_| start()>
                            {icon(Glyph::Sparkle)}{t(T::BrandStepExtract)}
                        </button>
                    </div>
                    {match other {
                        BrandState::Failed(e) => Some(view! {
                            <p class="error-text small" role="alert" data-testid="onboarding-brand-error">{tf(T::DesignSystemFailed, &[&e])}</p>
                        }),
                        _ => None,
                    }}
                }.into_any(),
            }}
        </section>
    }
}
