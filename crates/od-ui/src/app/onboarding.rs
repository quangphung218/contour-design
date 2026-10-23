//! First run: pick a local coding agent, or skip (spec 1.5, no cloud step).

use super::Shell;
use super::icons::{Glyph, icon};
use crate::events::AgentInfo;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

#[component]
pub fn Onboarding() -> impl IntoView {
    let shell = Shell::get();
    let navigate = use_navigate();
    let picked = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);

    // Preselect the first available agent once detection answers.
    Effect::new(move |_| {
        if picked.get_untracked().is_none() {
            if let Some(Ok(list)) = shell.agents.get() {
                picked.set(list.iter().find(|a| a.available).map(|a| a.id.clone()));
            }
        }
    });

    let finish = move |agent: Option<String>| {
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let mut next = shell.config.get_untracked().unwrap_or_default();
            if agent.is_some() {
                next.agent_id = agent;
            }
            next.onboarding_completed = true;
            match shell.save_config(next).await {
                Ok(()) => navigate("/", Default::default()),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let finish_skip = finish.clone();

    view! {
        <section class="page onboarding">
            <div class="onboarding-card card">
                <header class="onboarding-head">
                    {super::brand_mark("brand-mark large")}
                    <p class="onboarding-brand"><strong>{super::APP_NAME}</strong><span class="muted">{t(T::Tagline)}</span></p>
                    <h1>{t(T::OnboardingTitle)}</h1>
                    <p class="muted">{t(T::OnboardingIntro)}</p>
                </header>
                <AgentPicker picked=picked/>
                {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                <div class="onboarding-foot">
                    <button class="btn ghost" on:click=move |_| shell.reload_agents()>
                        {icon(Glyph::Refresh)}
                        {t(T::Refresh)}
                    </button>
                    <span class="spacer"></span>
                    <button class="btn ghost" on:click=move |_| finish_skip(None)>{t(T::SkipForNow)}</button>
                    <button
                        class="btn primary"
                        data-testid="onboarding-continue"
                        disabled=move || picked.get().is_none()
                        on:click=move |_| finish(picked.get_untracked())
                    >
                        {t(T::Continue)}
                        {icon(Glyph::ArrowRight)}
                    </button>
                </div>
            </div>
        </section>
    }
}

/// Detected agents: installed ones as cards to pick, the others in a
/// compact list below. Shared with Settings.
#[component]
pub fn AgentPicker(picked: RwSignal<Option<String>>) -> impl IntoView {
    let shell = Shell::get();
    move || {
        match shell.agents.get() {
        None => view! {
            <div class="agent-list" role="radiogroup">
                {(0..3).map(|_| view! { <div class="skeleton agent-skeleton"></div> }).collect_view()}
            </div>
        }
            .into_any(),
        Some(Err(e)) => {
            view! { <p class="error-text">{tf(T::AgentDetectionUnavailable, &[&e])}</p> }.into_any()
        }
        Some(Ok(list)) if list.is_empty() => view! {
            <div class="empty-state compact">
                <span class="empty-icon">{icon(Glyph::Agent)}</span>
                <p>{t(T::NoAgentsFound)}</p>
            </div>
        }
            .into_any(),
        Some(Ok(list)) => {
            let (ready, missing): (Vec<_>, Vec<_>) = list.into_iter().partition(|a| a.available);
            view! {
                <div class="agent-list" role="radiogroup">
                    {ready.into_iter().map(|a| view! { <AgentCard agent=a picked=picked/> }).collect_view()}
                </div>
                {(!missing.is_empty()).then(|| view! {
                    <div class="agent-missing">
                        <span class="eyebrow">{t(T::OtherAgents)}</span>
                        <div class="agent-list compact">
                            {missing.into_iter().map(|a| view! { <AgentCard agent=a picked=picked/> }).collect_view()}
                        </div>
                    </div>
                })}
            }
                .into_any()
        }
    }
    }
}

#[component]
pub fn AgentCard(agent: AgentInfo, picked: RwSignal<Option<String>>) -> impl IntoView {
    let id = agent.id.clone();
    let id_check = id.clone();
    let detail = if agent.available {
        agent.version.clone().map_or_else(
            || t(T::Installed).to_owned(),
            |v| tf(T::InstalledVersion, &[&v]),
        )
    } else {
        agent
            .error
            .clone()
            .unwrap_or_else(|| t(T::NotInstalled).to_owned())
    };
    let available = agent.available;
    let detail_title = detail.clone();
    let selected = move || picked.get().as_deref() == Some(id_check.as_str());
    view! {
        <button
            class="agent-card"
            class:compact=!available
            class:active=selected.clone()
            disabled=!available
            role="radio"
            aria-checked=move || selected().to_string()
            data-agent=agent.id.clone()
            on:click=move |_| picked.set(Some(id.clone()))
        >
            <span class="agent-icon">{icon(if available { Glyph::Agent } else { Glyph::Download })}</span>
            <span class="agent-text">
                <span class="agent-name">{agent.name.clone()}</span>
                <span class="agent-detail" title=detail_title>{detail}</span>
            </span>
            {available.then(|| view! {
                <span class="agent-check" aria-hidden="true">{icon(Glyph::CheckCircle)}</span>
            })}
        </button>
    }
}
