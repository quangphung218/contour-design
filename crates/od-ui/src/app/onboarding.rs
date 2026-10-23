//! First run (spec 1.5, no cloud step), as the original's runtime setup:
//! a local coding agent (picked, then tested once with a smoke prompt) or
//! an API key (BYOK form with its own test). Continue waits until the
//! chosen runtime answered; Skip is always there.

use super::Shell;
use super::api;
use super::byok_form::{ByokForm, TestResultLine, TestState};
use super::icons::{Glyph, icon};
use crate::events::AgentInfo;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use serde_json::{Value, json};

#[component]
pub fn Onboarding() -> impl IntoView {
    let shell = Shell::get();
    let navigate = use_navigate();
    let picked = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let use_key = RwSignal::new(false);
    let test = RwSignal::new(None::<TestState>);
    let byok_ready = RwSignal::new(false);
    // Track C2: step 2, a design system extracted from the brand's site.
    let brand = RwSignal::new(super::onboarding_brand::BrandState::Idle);
    let extracting = move || matches!(brand.get(), super::onboarding_brand::BrandState::Running(_));

    // Preselect the first available agent once detection answers.
    Effect::new(move |_| {
        if picked.get_untracked().is_none() {
            if let Some(Ok(list)) = shell.agents.get() {
                picked.set(
                    list.iter()
                        .find(|a| a.available && a.id != "byok")
                        .map(|a| a.id.clone()),
                );
            }
        }
    });
    // Test the picked agent once per pick (the original auto-tests).
    let seq = StoredValue::new(0u32);
    Effect::new(move |_| {
        let Some(agent) = picked.get() else {
            test.set(None);
            return;
        };
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        test.set(Some(TestState::Running));
        leptos::task::spawn_local(async move {
            let body = json!({ "mode": "agent", "agentId": agent });
            let res = match api::post::<_, Value>("/api/test/connection", &body).await {
                Ok(v) => TestState::Done(v),
                Err(e) => TestState::Failed(e.to_string()),
            };
            if seq.get_value() == mine {
                test.set(Some(res));
            }
        });
    });
    let agent_ready = move || matches!(test.get(), Some(TestState::Done(v)) if v["ok"] == true);
    let ready = move || {
        if use_key.get() {
            byok_ready.get()
        } else {
            agent_ready()
        }
    };

    let finish = move |agent: Option<String>| {
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let mut next = shell.config.get_untracked().unwrap_or_default();
            if agent.is_some() {
                next.agent_id = agent;
            }
            // The extracted system becomes the default for new projects.
            if let super::onboarding_brand::BrandState::Done(id, _) = brand.get_untracked() {
                next.extra.insert(
                    crate::settings::DEFAULT_DESIGN_SYSTEM_KEY.to_owned(),
                    serde_json::json!(id),
                );
            }
            next.onboarding_completed = true;
            match shell.save_config(next).await {
                Ok(()) => navigate("/", Default::default()),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let finish_skip = finish.clone();
    let on_byok_saved = Callback::new(move |()| byok_ready.set(true));

    view! {
        <section class="page onboarding">
            <div class="onboarding-card card">
                <header class="onboarding-head">
                    {super::brand_mark("brand-mark large")}
                    <p class="onboarding-brand"><strong>{super::APP_NAME}</strong><span class="muted">{t(T::Tagline)}</span></p>
                    <h1>{t(T::OnboardingTitle)}</h1>
                    <p class="muted">{t(T::OnboardingIntro)}</p>
                </header>
                <div class="segmented" role="tablist" aria-label=t(T::ExecutionMode)>
                    <button class="seg" role="tab" data-testid="onboarding-local"
                        aria-selected=move || (!use_key.get()).to_string() class:active=move || !use_key.get()
                        on:click=move |_| use_key.set(false)>{icon(Glyph::Terminal)}{t(T::LocalAgent)}</button>
                    <button class="seg" role="tab" data-testid="onboarding-byok"
                        aria-selected=move || use_key.get().to_string() class:active=move || use_key.get()
                        on:click=move |_| use_key.set(true)>{icon(Glyph::Key)}{t(T::UseApiKey)}</button>
                </div>
                <Show
                    when=move || !use_key.get()
                    fallback=move || view! {
                        <p class="muted">{t(T::ByokIntro)}</p>
                        <ByokForm on_saved=on_byok_saved/>
                    }
                >
                    <p class="muted">{t(T::LocalAgentIntro)}</p>
                    <AgentPicker picked=picked/>
                    <div class="row">
                        <TestResultLine state=test testid="onboarding-test"/>
                    </div>
                </Show>
                <super::onboarding_brand::BrandStep state=brand/>
                {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                <div class="onboarding-foot">
                    <Show when=move || !use_key.get()>
                        <button class="btn ghost" on:click=move |_| shell.reload_agents()>
                            {icon(Glyph::Refresh)}
                            {t(T::Refresh)}
                        </button>
                        <span class="muted small">{t(T::RescanHint)}</span>
                    </Show>
                    <span class="spacer"></span>
                    <button class="btn ghost" on:click=move |_| finish_skip(None)>{t(T::SkipForNow)}</button>
                    <span data-tip=move || {
                        if extracting() {
                            Some(t(T::BrandStepWait))
                        } else {
                            (!ready()).then(|| t(if use_key.get() { T::GateByok } else { T::GateAgent }))
                        }
                    }>
                        <button
                            class="btn primary"
                            data-testid="onboarding-continue"
                            disabled=move || !ready() || extracting()
                            on:click=move |_| finish(if use_key.get_untracked() { Some("byok".to_owned()) } else { picked.get_untracked() })
                        >
                            {t(T::Continue)}
                            {icon(Glyph::ArrowRight)}
                        </button>
                    </span>
                </div>
            </div>
        </section>
    }
}

/// Detected agents: installed ones as cards to pick, the others in a
/// compact list below. Shared with Settings. BYOK has its own form.
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
            let (ready, missing): (Vec<_>, Vec<_>) = list
                .into_iter()
                .filter(|a| a.id != "byok")
                .partition(|a| a.available);
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
    // DeepSeek Harness not available yet: the card opens the setup dialog
    // instead of being a dead entry.
    let setup = super::dsh_setup::needs_setup(&agent);
    let cli_path = agent.path.clone();
    let setup_state = RwSignal::new(None::<super::dsh_setup::SetupState>);
    let detail = if agent.available {
        agent.version.clone().map_or_else(
            || t(T::Installed).to_owned(),
            |v| tf(T::InstalledVersion, &[&v]),
        )
    } else if agent.needs_setup {
        t(T::DshSetupRequired).to_owned()
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
            class:needs-setup=setup
            class:active=selected.clone()
            disabled=!available && !setup
            role="radio"
            aria-checked=move || selected().to_string()
            data-agent=agent.id.clone()
            data-needs-setup=setup.then_some("true")
            on:click=move |_| {
                if setup {
                    setup_state.set(Some(super::dsh_setup::SetupState::default()));
                } else {
                    picked.set(Some(id.clone()));
                }
            }
        >
            <span class="agent-icon">{icon(if available { Glyph::Agent } else if setup { Glyph::Settings } else { Glyph::Download })}</span>
            <span class="agent-text">
                <span class="agent-name">{agent.name.clone()}</span>
                <span class="agent-detail" title=detail_title>{detail}</span>
            </span>
            {available.then(|| view! {
                <span class="agent-check" aria-hidden="true">{icon(Glyph::CheckCircle)}</span>
            })}
        </button>
        {move || setup_state.get().is_some().then(|| view! {
            <super::dsh_setup::DshSetupDialog state=setup_state picked=picked cli_path=cli_path.clone()/>
        })}
    }
}
