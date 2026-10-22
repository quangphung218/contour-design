//! First run: pick a local coding agent, or skip (spec 1.5, no cloud step).

use super::Shell;
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
        <section class="page narrow">
            <h1>{t(T::OnboardingTitle)}</h1>
            <p class="muted">{t(T::OnboardingIntro)}</p>
            <div class="agent-list" role="radiogroup">
                {move || match shell.agents.get() {
                    None => view! { <p class="empty">{t(T::DetectingAgents)}</p> }.into_any(),
                    Some(Err(e)) => {
                        view! { <p class="error-text">{tf(T::AgentDetectionUnavailable, &[&e])}</p> }
                            .into_any()
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        view! { <p class="empty">{t(T::NoAgentsFound)}</p> }
                            .into_any()
                    }
                    Some(Ok(list)) => {
                        list.into_iter()
                            .map(|a| view! { <AgentCard agent=a picked=picked/> })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
            {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            <div class="row end">
                <button class="btn" on:click=move |_| shell.reload_agents()>{t(T::Refresh)}</button>
                <button class="btn ghost" on:click=move |_| finish_skip(None)>{t(T::SkipForNow)}</button>
                <button
                    class="btn primary"
                    data-testid="onboarding-continue"
                    disabled=move || picked.get().is_none()
                    on:click=move |_| finish(picked.get_untracked())
                >
                    {t(T::Continue)}
                </button>
            </div>
        </section>
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
    view! {
        <button
            class="agent-card"
            class:active=move || picked.get().as_deref() == Some(id_check.as_str())
            disabled=!agent.available
            role="radio"
            data-agent=agent.id.clone()
            on:click=move |_| picked.set(Some(id.clone()))
        >
            <span class="agent-name">{agent.name.clone()}</span>
            <span class="tile-meta">{detail}</span>
        </button>
    }
}
