//! Settings: agent, model, permission mode, theme.

use super::Shell;
use super::onboarding::AgentCard;
use crate::settings::{AppConfig, PermissionMode, Theme};
use leptos::prelude::*;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let shell = Shell::get();
    let agent = RwSignal::new(None::<String>);
    let model = RwSignal::new(String::new());
    let permission = RwSignal::new(PermissionMode::Auto);
    let theme = RwSignal::new(Theme::System);
    let status = RwSignal::new(None::<Result<(), String>>);

    // Fill the form once settings arrive.
    Effect::new(move |loaded: Option<bool>| {
        if loaded == Some(true) {
            return true;
        }
        let Some(c) = shell.config.get() else {
            return false;
        };
        agent.set(c.agent_id.clone());
        model.set(c.model.clone().unwrap_or_default());
        permission.set(c.agent_permission_mode);
        theme.set(c.theme);
        true
    });

    let save = move |_| {
        let base: AppConfig = shell.config.get_untracked().unwrap_or_default();
        let next = AppConfig {
            agent_id: agent.get_untracked(),
            model: Some(model.get_untracked().trim().to_owned()).filter(|m| !m.is_empty()),
            agent_permission_mode: permission.get_untracked(),
            theme: theme.get_untracked(),
            onboarding_completed: true,
            ..base
        };
        leptos::task::spawn_local(async move {
            status.set(Some(shell.save_config(next).await));
        });
    };

    let perm_option = move |mode: PermissionMode, title: &'static str, detail: &'static str| {
        view! {
            <label class="radio-row">
                <input
                    type="radio"
                    name="permission"
                    prop:checked=move || permission.get() == mode
                    on:change=move |_| permission.set(mode)
                />
                <span>
                    <strong>{title}</strong>
                    <span class="muted block">{detail}</span>
                </span>
            </label>
        }
    };

    view! {
        <section class="page narrow">
            <h1>"Settings"</h1>

            <h2>"Agent"</h2>
            <div class="agent-list">
                {move || match shell.agents.get() {
                    None => view! { <p class="empty">"Detecting agents…"</p> }.into_any(),
                    Some(Err(e)) => {
                        view! {
                            <p class="error-text">{format!("Agent detection unavailable: {e}")}</p>
                            <label class="field">
                                <span>"Agent id"</span>
                                <input
                                    class="input"
                                    prop:value=move || agent.get().unwrap_or_default()
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        agent.set(Some(v).filter(|v| !v.trim().is_empty()));
                                    }
                                />
                            </label>
                        }
                            .into_any()
                    }
                    Some(Ok(list)) => {
                        list.into_iter()
                            .map(|a| view! { <AgentCard agent=a picked=agent/> })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>

            <label class="field">
                <span>"Model"</span>
                <input
                    class="input"
                    data-testid="model"
                    placeholder="Agent default"
                    prop:value=move || model.get()
                    on:input=move |ev| model.set(event_target_value(&ev))
                />
                <span class="muted">"Leave empty to use the agent's own default model."</span>
            </label>

            <h2>"Permissions"</h2>
            <div class="radio-group">
                {perm_option(
                    PermissionMode::Auto,
                    "Automatic",
                    "The agent edits files and runs commands in the project folder without asking.",
                )}
                {perm_option(
                    PermissionMode::Ask,
                    "Ask first",
                    "Claude only accepts file edits; other agents run without their auto-approve flag.",
                )}
            </div>

            <h2>"Appearance"</h2>
            <div class="segmented" role="radiogroup" aria-label="Theme">
                {Theme::ALL
                    .into_iter()
                    .map(|t| {
                        view! {
                            <button
                                class="seg"
                                class:active=move || theme.get() == t
                                on:click=move |_| theme.set(t)
                            >
                                {match t {
                                    Theme::System => "System",
                                    Theme::Light => "Light",
                                    Theme::Dark => "Dark",
                                }}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>

            <div class="row end">
                {move || match status.get() {
                    Some(Ok(())) => view! { <span class="ok-text">"Saved"</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                    None => ().into_any(),
                }}
                <button class="btn primary" data-testid="save-settings" on:click=save>"Save"</button>
            </div>
        </section>
    }
}
