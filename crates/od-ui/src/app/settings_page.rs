//! Settings: agent, model, permission mode, theme, language.

use super::Shell;
use super::icons::{Glyph, icon};
use super::onboarding::AgentPicker;
use crate::i18n::{Locale, T, t};
use crate::settings::{AppConfig, PermissionMode, Theme};
use leptos::prelude::*;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let shell = Shell::get();
    let agent = RwSignal::new(None::<String>);
    let model = RwSignal::new(String::new());
    let permission = RwSignal::new(PermissionMode::Auto);
    let theme = RwSignal::new(Theme::System);
    let language = RwSignal::new(shell.locale.get_untracked());
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
            locale: Some(language.get_untracked().tag().to_owned()),
            onboarding_completed: true,
            ..base
        };
        leptos::task::spawn_local(async move {
            status.set(Some(shell.save_config(next).await));
        });
    };

    let perm_option = move |mode: PermissionMode, title: T, detail: T| {
        view! {
            <label class="radio-row">
                <input
                    type="radio"
                    name="permission"
                    prop:checked=move || permission.get() == mode
                    on:change=move |_| permission.set(mode)
                />
                <span>
                    <strong>{t(title)}</strong>
                    <span class="muted block">{t(detail)}</span>
                </span>
            </label>
        }
    };

    view! {
        <section class="page narrow settings">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::SettingsTitle)}</h1>
                </div>
            </header>

            <section class="settings-section">
                <div class="section-intro">
                    <h2>{icon(Glyph::Agent)}{t(T::Agent)}</h2>
                    <p>{t(T::AgentSectionHint)}</p>
                </div>
                <div class="card padded">
                    <AgentPicker picked=agent/>
                    {move || matches!(shell.agents.get(), Some(Err(_))).then(|| view! {
                        <label class="field">
                            <span>{t(T::AgentId)}</span>
                            <input
                                class="input"
                                prop:value=move || agent.get().unwrap_or_default()
                                on:input=move |ev| {
                                    let v = event_target_value(&ev);
                                    agent.set(Some(v).filter(|v| !v.trim().is_empty()));
                                }
                            />
                        </label>
                    })}
                    <label class="field">
                        <span>{t(T::Model)}</span>
                        <input
                            class="input"
                            data-testid="model"
                            placeholder=t(T::ModelPlaceholder)
                            prop:value=move || model.get()
                            on:input=move |ev| model.set(event_target_value(&ev))
                        />
                        <span class="muted">{t(T::ModelHint)}</span>
                    </label>
                </div>
            </section>

            <section class="settings-section">
                <div class="section-intro">
                    <h2>{icon(Glyph::LintOk)}{t(T::Permissions)}</h2>
                    <p>{t(T::PermissionsHint)}</p>
                </div>
                <div class="card padded">
                    <div class="radio-group">
                        {perm_option(
                            PermissionMode::Auto,
                            T::PermAuto,
                            T::PermAutoDetail,
                        )}
                        {perm_option(
                            PermissionMode::Ask,
                            T::PermAsk,
                            T::PermAskDetail,
                        )}
                    </div>
                    <super::agent_w3::AskModeWarning/>
                </div>
            </section>

            <section class="settings-section">
                <div class="section-intro">
                    <h2>{icon(Glyph::System)}{t(T::Appearance)}</h2>
                    <p>{t(T::AppearanceHint)}</p>
                </div>
                <div class="card padded settings-inline">
                    <div class="segmented" role="group" aria-label=t(T::Appearance)>
                        {Theme::ALL
                            .into_iter()
                            .map(|th| {
                                let (glyph, label) = match th {
                                    Theme::System => (Glyph::System, t(T::ThemeSystem)),
                                    Theme::Light => (Glyph::Sun, t(T::ThemeLight)),
                                    Theme::Dark => (Glyph::Moon, t(T::ThemeDark)),
                                };
                                view! {
                                    <button
                                        class="seg"
                                        aria-pressed=move || (theme.get() == th).to_string()
                                        class:active=move || theme.get() == th
                                        on:click=move |_| theme.set(th)
                                    >
                                        {icon(glyph)}
                                        {label}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
            </section>

            <section class="settings-section">
                <div class="section-intro">
                    <h2>{icon(Glyph::Language)}{t(T::Language)}</h2>
                    <p>{t(T::LanguageHint)}</p>
                </div>
                <div class="card padded settings-inline">
                    <select
                        class="select-auto"
                        data-testid="language"
                        aria-label=t(T::Language)
                        on:change=move |ev| language.set(Locale::parse(&event_target_value(&ev)))
                    >
                        {Locale::ALL
                            .into_iter()
                            .map(|l| {
                                view! {
                                    <option value=l.tag() selected=move || language.get() == l>
                                        {l.native_name()}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </div>
            </section>

            <div class="save-bar">
                {move || match status.get() {
                    Some(Ok(())) => view! { <span class="ok-text save-status">{icon(Glyph::Check)}{t(T::Saved)}</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                    None => ().into_any(),
                }}
                <button class="btn primary" data-testid="save-settings" on:click=save>{t(T::Save)}</button>
            </div>

            // Saved on its own: the servers live in mcp-config.json.
            <section class="settings-section">
                <super::mcp_settings::McpSection/>
            </section>
        </section>
    }
}
