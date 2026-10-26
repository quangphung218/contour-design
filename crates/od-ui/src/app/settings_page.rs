//! Settings (spec 1.5), in the original's section order: models and
//! providers (local CLI with model, reasoning, test and advanced env, or an
//! API provider), permissions, appearance, language, notifications, project
//! locations, instructions and rules, memory, media providers, MCP servers,
//! about (version, diagnostics, reset onboarding).

use super::byok_form::{ByokForm, TestResultLine, TestState};
use super::icons::{Glyph, icon};
use super::onboarding::{AgentPicker, CheckAgainButton};
use super::{Shell, api, desktop};
use crate::i18n::{Locale, T, t, tf};
use crate::settings::{AppConfig, PermissionMode, Theme};
use leptos::prelude::*;
use serde_json::{Map, Value, json};

/// Longest global rules, as the original.
const MAX_RULES: usize = 5000;

use crate::models::{models_for, reasoning_for};

/// Service tiers the agent accepts, `(value, label)` (empty: no picker).
fn tiers_for(agent: &str) -> &'static [(&'static str, T)] {
    match agent {
        "codex" => &[("default", T::TierDefault), ("priority", T::TierFast)],
        _ => &[],
    }
}

/// Sections, for the in-page navigation.
const SECTIONS: [(&str, T); 11] = [
    ("execution", T::SettingsModels),
    ("permissions", T::Permissions),
    ("appearance", T::Appearance),
    ("language", T::Language),
    ("notifications", T::Notifications),
    ("locations", T::ProjectLocations),
    ("instructions", T::InstructionsRules),
    ("memory", T::GlobalMemory),
    ("media", T::MediaProviders),
    ("mcp", T::McpTitle),
    ("about", T::About),
];

fn obj(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    provide_context(super::memory_settings::MemoryRev(RwSignal::new(0)));
    let shell = Shell::get();
    let agent = RwSignal::new(None::<String>);
    // Agent ▸ Check again: an agent installed after the app started.
    let checking_agents = RwSignal::new(false);
    let check_agents = Callback::new(move |()| {
        if checking_agents.get_untracked() {
            return;
        }
        checking_agents.set(true);
        leptos::task::spawn_local(async move {
            shell.rescan_agents().await;
            checking_agents.set(false);
        });
    });
    let model = RwSignal::new(String::new());
    let reasoning = RwSignal::new(String::new());
    let tier = RwSignal::new(String::new());
    let permission = RwSignal::new(PermissionMode::Auto);
    let theme = RwSignal::new(Theme::System);
    let language = RwSignal::new(shell.locale.get_untracked());
    let status = RwSignal::new(None::<Result<(), String>>);
    let api_mode = RwSignal::new(false);

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
        let per_agent = obj(c.extra.get("agentModels"));
        let id = c.agent_id.clone().unwrap_or_default();
        reasoning.set(
            per_agent
                .get(&id)
                .and_then(|m| m["reasoning"].as_str())
                .unwrap_or_default()
                .to_owned(),
        );
        tier.set(
            per_agent
                .get(&id)
                .and_then(|m| m["serviceTier"].as_str())
                .unwrap_or_default()
                .to_owned(),
        );
        api_mode.set(c.agent_id.as_deref() == Some("byok"));
        permission.set(c.agent_permission_mode);
        theme.set(c.theme);
        true
    });

    let save = move |_| {
        let base: AppConfig = shell.config.get_untracked().unwrap_or_default();
        let model_value = Some(model.get_untracked().trim().to_owned()).filter(|m| !m.is_empty());
        let mut extra = base.extra.clone();
        if let Some(id) = agent.get_untracked().filter(|a| a != "byok") {
            let mut per_agent = obj(extra.get("agentModels"));
            let r = reasoning.get_untracked();
            let st = tier.get_untracked();
            per_agent.insert(
                id,
                json!({
                    "model": model_value,
                    "reasoning": Some(r).filter(|r| !r.is_empty() && r != "default"),
                    "serviceTier": Some(st).filter(|s| !s.is_empty() && s != "default"),
                }),
            );
            extra.insert("agentModels".to_owned(), Value::Object(per_agent));
        }
        let next = AppConfig {
            agent_id: agent.get_untracked(),
            model: model_value,
            agent_permission_mode: permission.get_untracked(),
            theme: theme.get_untracked(),
            locale: Some(language.get_untracked().tag().to_owned()),
            onboarding_completed: true,
            extra,
        };
        leptos::task::spawn_local(async move {
            status.set(Some(shell.save_config(next).await));
        });
    };
    // Permissions, appearance and language save as soon as they change; the
    // agent card and the API provider form each have their own Save.
    let prefs_saved = RwSignal::new(None::<(&'static str, Result<(), String>)>);
    let save_prefs = move |section: &'static str| {
        let mut next: AppConfig = shell.config.get_untracked().unwrap_or_default();
        next.agent_permission_mode = permission.get_untracked();
        next.theme = theme.get_untracked();
        next.locale = Some(language.get_untracked().tag().to_owned());
        next.onboarding_completed = true;
        leptos::task::spawn_local(async move {
            prefs_saved.set(Some((section, shell.save_config(next).await)));
        });
    };
    let prefs_status = move |section: &'static str| {
        move || {
            match prefs_saved.get() {
            Some((s, Ok(()))) if s == section => view! {
                <span class="ok-text save-status small" data-testid=format!("saved-{section}")>{icon(Glyph::Check)}{t(T::Saved)}</span>
            }.into_any(),
            Some((s, Err(e))) if s == section => view! { <span class="error-text small">{e}</span> }.into_any(),
            _ => ().into_any(),
        }
        }
    };
    // Switching agents shows that agent's stored model and effort.
    Effect::new(move |prev: Option<Option<String>>| {
        let now = agent.get();
        if prev.is_some_and(|p| p != now) {
            let per_agent = shell
                .config
                .with_untracked(|c| obj(c.as_ref().and_then(|c| c.extra.get("agentModels"))));
            let entry = now.as_ref().and_then(|id| per_agent.get(id).cloned());
            model.set(
                entry
                    .as_ref()
                    .and_then(|e| e["model"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
            );
            reasoning.set(
                entry
                    .as_ref()
                    .and_then(|e| e["reasoning"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
            );
            tier.set(
                entry
                    .as_ref()
                    .and_then(|e| e["serviceTier"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
            );
        }
        now
    });

    let agent_test = RwSignal::new(None::<TestState>);
    let run_agent_test = move |_| {
        let Some(id) = agent.get_untracked() else {
            return;
        };
        let body = json!({
            "mode": "agent",
            "agentId": id,
            "model": model.get_untracked().trim(),
            "reasoning": reasoning.get_untracked(),
        });
        agent_test.set(Some(TestState::Running));
        leptos::task::spawn_local(async move {
            agent_test.set(Some(
                match api::post::<_, Value>("/api/test/connection", &body).await {
                    Ok(v) => TestState::Done(v),
                    Err(e) => TestState::Failed(e.to_string()),
                },
            ));
        });
    };

    let perm_option = move |mode: PermissionMode, title: T, detail: T| {
        view! {
            <label class="radio-row">
                <input
                    type="radio"
                    name="permission"
                    prop:checked=move || permission.get() == mode
                    on:change=move |_| {
                        permission.set(mode);
                        save_prefs("permissions");
                    }
                />
                <span>
                    <strong>{t(title)}</strong>
                    <span class="muted block">{t(detail)}</span>
                </span>
            </label>
        }
    };
    let section_head = |glyph: Glyph, title: T, hint: T| {
        view! {
            <div class="section-intro">
                <h2>{icon(glyph)}{t(title)}</h2>
                <p>{t(hint)}</p>
            </div>
        }
    };

    view! {
        <section class="page settings settings-layout">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::SettingsTitle)}</h1>
                </div>
            </header>
            <div class="settings-columns">
                <nav class="settings-nav" aria-label=t(T::SettingsSections)>
                    {SECTIONS.into_iter().map(|(id, label)| view! {
                        <a class="settings-nav-link" href=format!("#settings-{id}") data-testid=format!("settings-nav-{id}")>{t(label)}</a>
                    }).collect_view()}
                </nav>
                <div class="settings-main">
                    <section class="settings-section" id="settings-execution">
                        {section_head(Glyph::Agent, T::SettingsModels, T::SettingsModelsHint)}
                        <div class="segmented" role="group" aria-label=t(T::ExecutionMode)>
                            <button class="seg" data-testid="mode-local"
                                aria-pressed=move || (!api_mode.get()).to_string()
                                class:active=move || !api_mode.get()
                                on:click=move |_| {
                                    api_mode.set(false);
                                    if agent.get_untracked().as_deref() == Some("byok") {
                                        agent.set(None);
                                    }
                                }>
                                {icon(Glyph::Terminal)}{t(T::LocalCli)}
                                <span class="muted small">{move || {
                                    let n = shell.agents.with(|a| match a {
                                        Some(Ok(l)) => l.iter().filter(|a| a.available && a.id != "byok").count(),
                                        _ => 0,
                                    });
                                    tf(T::InstalledCount, &[&n.to_string()])
                                }}</span>
                            </button>
                            <button class="seg" data-testid="mode-api"
                                aria-pressed=move || api_mode.get().to_string()
                                class:active=move || api_mode.get()
                                on:click=move |_| api_mode.set(true)>
                                {icon(Glyph::Key)}{t(T::ApiProvider)}
                            </button>
                        </div>
                        <Show
                            when=move || !api_mode.get()
                            fallback=move || view! { <div class="card padded"><ByokForm/></div> }
                        >
                            <div class="card padded">
                                <AgentPicker picked=agent/>
                                <div class="row agent-rescan">
                                    <CheckAgainButton checking=checking_agents on_click=check_agents testid="settings-check-again"/>
                                </div>
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
                                <div class="row wrap">
                                    <label class="field grow">
                                        <span>{t(T::Model)}</span>
                                        <input
                                            class="input"
                                            data-testid="model"
                                            list="agent-models"
                                            placeholder=t(T::ModelPlaceholder)
                                            prop:value=move || model.get()
                                            on:input=move |ev| model.set(event_target_value(&ev))
                                        />
                                        <datalist id="agent-models">
                                            {move || models_for(&agent.get().unwrap_or_default()).iter().map(|m| view! {
                                                <option value=*m>{if *m == "default" { t(T::ModelCliDefault) } else { m }}</option>
                                            }).collect_view()}
                                        </datalist>
                                        <span class="muted">{t(T::ModelHint)}" "<span class="badge outline">{t(T::ModelSourceBuiltIn)}</span></span>
                                    </label>
                                    <Show when=move || !reasoning_for(&agent.get().unwrap_or_default()).is_empty()>
                                        <label class="field">
                                            <span>{t(T::ReasoningEffort)}</span>
                                            <select data-testid="reasoning" on:change=move |ev| reasoning.set(event_target_value(&ev))>
                                                {move || reasoning_for(&agent.get().unwrap_or_default()).iter().map(|r| {
                                                    let r = *r;
                                                    view! { <option value=r selected=move || reasoning.get() == r || (r == "default" && reasoning.get().is_empty())>{r}</option> }
                                                }).collect_view()}
                                            </select>
                                        </label>
                                    </Show>
                                    <Show when=move || !tiers_for(&agent.get().unwrap_or_default()).is_empty()>
                                        <label class="field">
                                            <span>{t(T::ServiceTier)}</span>
                                            <select data-testid="service-tier" on:change=move |ev| tier.set(event_target_value(&ev))>
                                                {move || tiers_for(&agent.get().unwrap_or_default()).iter().map(|(v, label)| {
                                                    let v = *v;
                                                    view! { <option value=v selected=move || tier.get() == v || (v == "default" && tier.get().is_empty())>{t(*label)}</option> }
                                                }).collect_view()}
                                            </select>
                                        </label>
                                    </Show>
                                </div>
                                <div class="row">
                                    <button class="btn" data-testid="agent-test" disabled=move || agent.get().is_none() on:click=run_agent_test>
                                        {icon(Glyph::Play)}{t(T::Test)}
                                    </button>
                                    <TestResultLine state=agent_test testid="agent-test-result"/>
                                </div>
                                <AdvancedEnv agent=agent/>
                                // Saves the agent and its model; the sections
                                // below save on their own.
                                <div class="row agent-save">
                                    <span class="spacer"></span>
                                    {move || match status.get() {
                                        Some(Ok(())) => view! { <span class="ok-text save-status">{icon(Glyph::Check)}{t(T::Saved)}</span> }.into_any(),
                                        Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                                        None => ().into_any(),
                                    }}
                                    <button class="btn primary" data-testid="save-settings" on:click=save>{t(T::SaveAgent)}</button>
                                </div>
                            </div>
                        </Show>
                    </section>

                    <section class="settings-section" id="settings-permissions">
                        {section_head(Glyph::LintOk, T::Permissions, T::PermissionsHint)}
                        <div class="card padded">
                            <div class="radio-group">
                                {perm_option(PermissionMode::Auto, T::PermAuto, T::PermAutoDetail)}
                                {perm_option(PermissionMode::Ask, T::PermAsk, T::PermAskDetail)}
                            </div>
                            <super::agent_w3::AskModeWarning/>
                            {prefs_status("permissions")}
                        </div>
                    </section>

                    <section class="settings-section" id="settings-appearance">
                        {section_head(Glyph::System, T::Appearance, T::AppearanceHint)}
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
                                                on:click=move |_| {
                                                    theme.set(th);
                                                    save_prefs("appearance");
                                                }
                                            >
                                                {icon(glyph)}
                                                {label}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                            {prefs_status("appearance")}
                        </div>
                    </section>

                    <section class="settings-section" id="settings-language">
                        {section_head(Glyph::Language, T::Language, T::LanguageHint)}
                        <div class="card padded settings-inline">
                            <select
                                class="select-auto"
                                data-testid="language"
                                aria-label=t(T::Language)
                                on:change=move |ev| {
                                    language.set(Locale::parse(&event_target_value(&ev)));
                                    save_prefs("language");
                                }
                            >
                                {Locale::ALL
                                    .iter()
                                    .copied()
                                    .map(|l| {
                                        view! {
                                            <option value=l.tag() selected=move || language.get() == l>
                                                {l.native_name()}
                                            </option>
                                        }
                                    })
                                    .collect_view()}
                            </select>
                            {prefs_status("language")}
                        </div>
                    </section>

                    <section class="settings-section" id="settings-notifications">
                        {section_head(Glyph::Bell, T::Notifications, T::NotificationsHint)}
                        <super::notify::NotificationSettings/>
                    </section>
                    <section class="settings-section" id="settings-locations">
                        {section_head(Glyph::FolderOpen, T::ProjectLocations, T::ProjectLocationsHint)}
                        <ProjectLocations/>
                    </section>
                    <section class="settings-section" id="settings-instructions">
                        {section_head(Glyph::Notes, T::InstructionsRules, T::InstructionsRulesHint)}
                        <GlobalRules/>
                    </section>
                    <section class="settings-section" id="settings-memory">
                        {section_head(Glyph::Memory, T::GlobalMemory, T::GlobalMemoryHint)}
                        // Track C2: switch, hooks, import and MEMORY.md around the list.
                        <super::memory_settings::MemoryControls/>
                        <GlobalMemory/>
                        <super::memory_settings::MemoryAdvanced/>
                    </section>
                    <section class="settings-section" id="settings-media">
                        {section_head(Glyph::FileImage, T::MediaProviders, T::MediaProvidersHint)}
                        <MediaProviders/>
                    </section>
                    // Saved on its own: the servers live in mcp-config.json.
                    <section class="settings-section" id="settings-mcp">
                        <super::mcp_settings::McpSection/>
                    </section>
                    <section class="settings-section" id="settings-about">
                        {section_head(Glyph::Info, T::About, T::AboutHint)}
                        <About/>
                    </section>
                </div>
            </div>
        </section>
    }
}

/// "Advanced: proxy & custom paths": extra env for the selected agent.
#[component]
fn AdvancedEnv(agent: RwSignal<Option<String>>) -> impl IntoView {
    let data = RwSignal::new(Value::Null);
    let edits = RwSignal::new(Map::<String, Value>::new());
    let state = RwSignal::new(None::<Result<(), String>>);
    leptos::task::spawn_local(async move {
        if let Ok(v) = api::get::<Value>("/api/agent-cli-env").await {
            data.set(v);
        }
    });
    let keys = move || {
        let id = agent.get().unwrap_or_default();
        data.with(|d| {
            d["keys"][&id]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let save = move |_| {
        let Some(id) = agent.get_untracked() else {
            return;
        };
        let body = json!({ "env": { id: Value::Object(edits.get_untracked()) } });
        leptos::task::spawn_local(async move {
            match api::put::<_, Value>("/api/agent-cli-env", &body).await {
                Ok(v) => {
                    data.set(v);
                    edits.set(Map::new());
                    state.set(Some(Ok(())));
                }
                Err(e) => state.set(Some(Err(e.to_string()))),
            }
        });
    };
    view! {
        <Show when=move || !keys().is_empty()>
            <details class="advanced-env" data-testid="advanced-env">
                <summary>{t(T::AdvancedEnv)}</summary>
                <p class="muted small">{t(T::AdvancedEnvHint)}</p>
                {move || {
                    let id = agent.get().unwrap_or_default();
                    keys().into_iter().map(|k| {
                        let stored = data.with(|d| d["env"][&id][&k].clone());
                        let secret = k.ends_with("_API_KEY");
                        let placeholder = match &stored {
                            Value::Object(o) => o.get("hint").and_then(Value::as_str).map(|h| tf(T::ApiKeyStored, &[h])).unwrap_or_default(),
                            _ => String::new(),
                        };
                        let value = stored.as_str().unwrap_or_default().to_owned();
                        let key = k.clone();
                        view! {
                            <label class="field">
                                <span class="mono">{k.clone()}</span>
                                <input class="input mono" data-testid=format!("env-{k}")
                                    type=if secret { "password" } else { "text" }
                                    placeholder=placeholder
                                    prop:value=value
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        edits.update(|e| { e.insert(key.clone(), json!(v)); });
                                    }/>
                            </label>
                        }
                    }).collect_view()
                }}
                <div class="row end">
                    {move || match state.get() {
                        Some(Ok(())) => view! { <span class="ok-text">{t(T::Saved)}</span> }.into_any(),
                        Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                        None => ().into_any(),
                    }}
                    <button class="btn small" data-testid="advanced-env-save" on:click=save>{t(T::Save)}</button>
                </div>
            </details>
        </Show>
    }
}

/// Global rules (`customInstructions`), added to every prompt.
#[component]
fn GlobalRules() -> impl IntoView {
    let shell = Shell::get();
    let text = RwSignal::new(
        shell
            .config
            .with_untracked(|c| {
                c.as_ref()
                    .and_then(|c| c.extra.get("customInstructions"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default(),
    );
    let state = RwSignal::new(None::<Result<(), String>>);
    let save = move |_| {
        let mut next = shell.config.get_untracked().unwrap_or_default();
        let v = text.get_untracked();
        let v: String = v.trim().chars().take(MAX_RULES).collect();
        next.extra.insert(
            "customInstructions".to_owned(),
            if v.is_empty() { Value::Null } else { json!(v) },
        );
        leptos::task::spawn_local(async move {
            state.set(Some(shell.save_config(next).await));
        });
    };
    view! {
        <div class="card padded">
            <label class="field">
                <span>{t(T::GlobalRules)}</span>
                <textarea class="input" rows="6" maxlength=MAX_RULES.to_string() data-testid="global-rules"
                    placeholder=t(T::GlobalRulesPlaceholder)
                    prop:value=move || text.get()
                    on:input=move |ev| text.set(event_target_value(&ev))></textarea>
                <span class="muted">{move || format!("{} / {MAX_RULES}", text.with(|v| v.chars().count()))}</span>
            </label>
            <div class="row end">
                {move || match state.get() {
                    Some(Ok(())) => view! { <span class="ok-text">{t(T::Saved)}</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                    None => ().into_any(),
                }}
                <button class="btn primary" data-testid="global-rules-save" on:click=save>{t(T::Save)}</button>
            </div>
        </div>
    }
}

pub(super) fn memory_kind_label(kind: &str) -> &'static str {
    t(match kind {
        "feedback" => T::MemoryFeedback,
        "project" => T::MemoryProject,
        "reference" => T::MemoryReference,
        "profile" => T::MemoryProfile,
        "rule" => T::MemoryRule,
        _ => T::MemoryUser,
    })
}

/// Settings ▸ Memory: facts for every project.
#[component]
fn GlobalMemory() -> impl IntoView {
    let list = RwSignal::new(Vec::<Value>::new());
    let rev = expect_context::<super::memory_settings::MemoryRev>();
    let kind = RwSignal::new("user".to_owned());
    let text = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let reload = move || {
        leptos::task::spawn_local(async move {
            if let Ok(v) = api::get::<Value>("/api/memory").await {
                list.set(v["memories"].as_array().cloned().unwrap_or_default());
            }
        });
    };
    // An import saved entries: list them.
    Effect::new(move |prev: Option<u32>| {
        let n = rev.0.get();
        if prev.is_some() {
            reload();
        }
        n
    });
    reload();
    let add = move || {
        let body = json!({ "kind": kind.get_untracked(), "text": text.get_untracked() });
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/memory", &body).await {
                Ok(_) => {
                    text.set(String::new());
                    error.set(None);
                    rev.0.update(|n| *n += 1);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    let add_key = add;
    view! {
        <div class="card padded">
            <ul class="memory-list" data-testid="global-memory">
                {move || {
                    let items = list.get();
                    if items.is_empty() {
                        return view! { <li class="muted small">{t(T::NoGlobalMemory)}</li> }.into_any();
                    }
                    items.into_iter().map(|m| {
                        let id = m["id"].as_str().unwrap_or_default().to_owned();
                        let remove = move |_| {
                            let id = id.clone();
                            leptos::task::spawn_local(async move {
                                let _ = api::delete::<Value>(&format!("/api/memory/{}", api::seg(&id))).await;
                                rev.0.update(|n| *n += 1);
                            });
                        };
                        view! {
                            <li class="memory-item" data-testid="global-memory-item">
                                <span class="badge">{memory_kind_label(m["kind"].as_str().unwrap_or_default())}</span>
                                <span class="grow">{m["text"].as_str().unwrap_or_default().to_owned()}</span>
                                <button class="icon-btn small danger" aria-label=t(T::Delete) data-tip=t(T::Delete) on:click=remove>{icon(Glyph::Delete)}</button>
                            </li>
                        }
                    }).collect_view().into_any()
                }}
            </ul>
            <div class="row">
                <select class="small select-auto" aria-label=t(T::MemoryKind) data-testid="global-memory-kind" on:change=move |ev| kind.set(event_target_value(&ev))>
                    {["user", "feedback", "project", "reference", "profile", "rule"].into_iter().map(|k| view! {
                        <option value=k selected=move || kind.get() == k>{memory_kind_label(k)}</option>
                    }).collect_view()}
                </select>
                <input class="input small grow" data-testid="global-memory-text" placeholder=t(T::GlobalMemoryPlaceholder)
                    prop:value=move || text.get()
                    on:input=move |ev| text.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { add_key() }/>
                <button class="btn small" data-testid="global-memory-add" disabled=move || text.with(|t| t.trim().is_empty()) on:click=move |_| add()>{icon(Glyph::Add)}{t(T::Add)}</button>
            </div>
            {move || error.get().map(|e| view! { <p class="error-text small">{e}</p> })}
        </div>
    }
}

/// Project locations: work bases for new projects, the default one, scan.
#[component]
fn ProjectLocations() -> impl IntoView {
    let shell = Shell::get();
    let locations = RwSignal::new(Vec::<Value>::new());
    let default_id = RwSignal::new("default".to_owned());
    let path = RwSignal::new(String::new());
    let state = RwSignal::new(None::<Result<String, String>>);
    let apply = move |v: Value| {
        locations.set(v["locations"].as_array().cloned().unwrap_or_default());
        default_id.set(v["defaultId"].as_str().unwrap_or("default").to_owned());
    };
    leptos::task::spawn_local(async move {
        if let Ok(v) = api::get::<Value>("/api/project-locations").await {
            apply(v);
        }
    });
    let put = move |list: Vec<Value>, default: String| {
        let custom: Vec<Value> = list.into_iter().filter(|l| l["id"] != "default").collect();
        let body = json!({ "locations": custom, "defaultId": default });
        leptos::task::spawn_local(async move {
            match api::put::<_, Value>("/api/project-locations", &body).await {
                Ok(v) => {
                    apply(v);
                    shell.reload_config().await;
                    path.set(String::new());
                    state.set(Some(Ok(t(T::Saved).to_owned())));
                }
                Err(e) => state.set(Some(Err(e.to_string()))),
            }
        });
    };
    let add_path = move |p: String| {
        if p.trim().is_empty() {
            return;
        }
        let mut list = locations.get_untracked();
        list.push(json!({ "id": "", "name": "", "path": p.trim() }));
        put(list, default_id.get_untracked());
    };
    let pick = move |_| {
        if desktop::is_desktop() {
            leptos::task::spawn_local(async move {
                if let Some(dir) = desktop::pick_folder().await {
                    add_path(dir);
                }
            });
        } else {
            add_path(path.get_untracked());
        }
    };
    let scan = move |_| {
        leptos::task::spawn_local(async move {
            state.set(Some(
                match api::post::<_, Value>("/api/project-locations/scan", &json!({})).await {
                    Ok(v) => Ok(tf(
                        T::ScanDone,
                        &[
                            &v["imported"].as_u64().unwrap_or(0).to_string(),
                            &v["existing"].as_u64().unwrap_or(0).to_string(),
                        ],
                    )),
                    Err(e) => Err(e.to_string()),
                },
            ));
        });
    };
    view! {
        <div class="card padded">
            <ul class="location-list" data-testid="locations">
                {move || locations.get().into_iter().map(|l| {
                    let id = l["id"].as_str().unwrap_or_default().to_owned();
                    let is_builtin = id == "default";
                    let id_default = id.clone();
                    let id_check = id.clone();
                    let id_remove = id.clone();
                    let name = if is_builtin { t(T::BuiltInLocation).to_owned() } else { l["name"].as_str().unwrap_or_default().to_owned() };
                    view! {
                        <li class="location-item" data-testid="location" data-location=id.clone()>
                            {icon(Glyph::FolderOpen)}
                            <span class="grow">
                                <strong>{name}</strong>
                                <span class="muted mono block small">{l["path"].as_str().unwrap_or_default().to_owned()}</span>
                            </span>
                            {move || if default_id.get() == id_check {
                                view! { <span class="badge accent">{t(T::DefaultLocation)}</span> }.into_any()
                            } else {
                                let id = id_default.clone();
                                view! { <button class="btn small ghost" data-testid="location-default" on:click=move |_| put(locations.get_untracked(), id.clone())>{t(T::MakeDefault)}</button> }.into_any()
                            }}
                            {(!is_builtin).then(|| view! {
                                <button class="icon-btn small danger" aria-label=t(T::Remove) data-tip=t(T::Remove) on:click=move |_| {
                                    let list: Vec<Value> = locations.get_untracked().into_iter().filter(|x| x["id"] != id_remove.as_str()).collect();
                                    let d = default_id.get_untracked();
                                    put(list, if d == id_remove { "default".to_owned() } else { d });
                                }>{icon(Glyph::Delete)}</button>
                            })}
                        </li>
                    }
                }).collect_view()}
            </ul>
            <div class="row">
                {(!desktop::is_desktop()).then(|| view! {
                    <input class="input small mono grow" data-testid="location-path" placeholder=t(T::FolderPathPlaceholder)
                        prop:value=move || path.get()
                        on:input=move |ev| path.set(event_target_value(&ev))/>
                })}
                <button class="btn small" data-testid="location-add" on:click=pick>{icon(Glyph::Add)}{t(T::AddFolder)}</button>
                <button class="btn small ghost" data-testid="location-scan" on:click=scan>{icon(Glyph::Search)}{t(T::ScanLocations)}</button>
            </div>
            {move || state.get().map(|s| match s {
                Ok(m) => view! { <p class="ok-text small" data-testid="location-state">{m}</p> }.into_any(),
                Err(e) => view! { <p class="error-text small" data-testid="location-state">{e}</p> }.into_any(),
            })}
        </div>
    }
}

/// Media providers: key, base URL and model per provider.
#[component]
fn MediaProviders() -> impl IntoView {
    let entries = super::media_page::PROVIDERS.map(|(id, label, _)| (id, label));
    let stored = RwSignal::new(Value::Null);
    let edits = RwSignal::new(Map::<String, Value>::new());
    let state = RwSignal::new(None::<Result<(), String>>);
    leptos::task::spawn_local(async move {
        if let Ok(v) = api::get::<Value>("/api/media/config").await {
            stored.set(v);
        }
    });
    let field = move |id: &'static str, key: &'static str| {
        edits
            .with(|e| {
                e.get(id)
                    .and_then(|p| p.get(key))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| {
                stored.with(|s| {
                    s["providers"][id][key]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
            })
    };
    let set = move |id: &'static str, key: &'static str, v: String| {
        edits.update(|e| {
            let entry = e.entry(id.to_owned()).or_insert_with(|| json!({}));
            entry[key] = json!(v);
        });
    };
    let save = move |clear: Option<&'static str>| {
        let mut providers = Map::new();
        for (id, _) in entries {
            if clear == Some(id) {
                continue;
            }
            let configured = stored.with(|s| s["providers"][id]["source"] == "stored");
            let key = edits
                .with(|e| {
                    e.get(id)
                        .and_then(|p| p["apiKey"].as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            providers.insert(
                id.to_owned(),
                json!({
                    "apiKey": key,
                    "preserveApiKey": key.is_empty() && configured,
                    "baseUrl": field(id, "baseUrl"),
                    "model": field(id, "model"),
                }),
            );
        }
        let body = json!({ "providers": providers });
        leptos::task::spawn_local(async move {
            match api::put::<_, Value>("/api/media/config", &body).await {
                Ok(v) => {
                    stored.set(v);
                    edits.set(Map::new());
                    state.set(Some(Ok(())));
                }
                Err(e) => state.set(Some(Err(e.to_string()))),
            }
        });
    };
    view! {
        <div class="media-providers">
            {entries.into_iter().map(|(id, label)| view! {
                <div class="card padded media-provider" data-testid="media-provider" data-provider=id>
                    <div class="row">
                        <strong class="grow">{label}</strong>
                        {move || if stored.with(|s| s["providers"][id]["configured"] == true) {
                            view! { <span class="badge success" data-testid="media-configured">{t(T::Configured)}</span> }.into_any()
                        } else {
                            view! { <span class="badge outline">{t(T::Unset)}</span> }.into_any()
                        }}
                        <button class="btn small ghost danger" data-testid="media-clear" on:click=move |_| save(Some(id))>{t(T::ClearConfiguration)}</button>
                    </div>
                    <div class="row wrap">
                        <label class="field grow">
                            <span>{t(T::ApiKey)}</span>
                            <input class="input mono" type="password" autocomplete="off" data-testid="media-key"
                                placeholder=move || {
                                    let tail = stored.with(|s| s["providers"][id]["apiKeyTail"].as_str().unwrap_or_default().to_owned());
                                    if tail.is_empty() { t(T::PasteApiKey).to_owned() } else { tf(T::ApiKeyStored, &[&format!("…{tail}")]) }
                                }
                                on:input=move |ev| set(id, "apiKey", event_target_value(&ev))/>
                        </label>
                        <label class="field grow">
                            <span>{t(T::BaseUrl)}</span>
                            <input class="input mono" data-testid="media-base-url" placeholder=t(T::OverrideBaseUrl)
                                prop:value=move || field(id, "baseUrl")
                                on:input=move |ev| set(id, "baseUrl", event_target_value(&ev))/>
                        </label>
                        <label class="field grow">
                            <span>{t(T::Model)}</span>
                            <input class="input mono" data-testid="media-model" placeholder=t(T::ProviderDefault)
                                prop:value=move || field(id, "model")
                                on:input=move |ev| set(id, "model", event_target_value(&ev))/>
                        </label>
                    </div>
                </div>
            }).collect_view()}
            <div class="row end">
                {move || match state.get() {
                    Some(Ok(())) => view! { <span class="ok-text">{t(T::Saved)}</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                    None => view! { <span class="muted small">{t(T::MediaSaveHint)}</span> }.into_any(),
                }}
                <button class="btn primary" data-testid="media-save" on:click=move |_| save(None)>{t(T::Save)}</button>
            </div>
        </div>
    }
}

/// About: version, local diagnostics export, reset onboarding.
#[component]
fn About() -> impl IntoView {
    let shell = Shell::get();
    let version = LocalResource::new(|| async { api::get::<Value>("/api/version").await.ok() });
    let exporting = RwSignal::new(false);
    let state = RwSignal::new(None::<Result<String, String>>);
    let export = move |_| {
        exporting.set(true);
        leptos::task::spawn_local(async move {
            match api::get_bytes("/api/diagnostics/export").await {
                Ok(bytes) => {
                    // Same name as the daemon's Content-Disposition.
                    let d = js_sys::Date::new_0();
                    let name = format!(
                        "contour-diagnostics-{:04}{:02}{:02}-{:02}{:02}{:02}.zip",
                        d.get_full_year(),
                        d.get_month() + 1,
                        d.get_date(),
                        d.get_hours(),
                        d.get_minutes(),
                        d.get_seconds()
                    );
                    let _ = super::export_menu::download(&bytes, &name, "application/zip");
                    state.set(Some(Ok(tf(T::DiagnosticsSaved, &[&name]))));
                }
                Err(e) => state.set(Some(Err(e.to_string()))),
            }
            exporting.set(false);
        });
    };
    let navigate = leptos_router::hooks::use_navigate();
    let reset = move |_| {
        let mut next = shell.config.get_untracked().unwrap_or_default();
        next.onboarding_completed = false;
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            if shell.save_config(next).await.is_ok() {
                navigate("/onboarding", Default::default());
            }
        });
    };
    view! {
        <div class="card padded about">
            <dl class="about-grid" data-testid="about">
                {move || version.get().flatten().map(|v| view! {
                    <dt>{t(T::Version)}</dt><dd class="mono" data-testid="about-version">{v["version"].as_str().unwrap_or_default().to_owned()}</dd>
                    <dt>{t(T::Runtime)}</dt><dd>{if desktop::is_desktop() { t(T::RuntimePackaged) } else { t(T::RuntimeBrowser) }}</dd>
                    <dt>{t(T::Platform)}</dt><dd class="mono">{format!("{} · {}", v["platform"].as_str().unwrap_or_default(), v["arch"].as_str().unwrap_or_default())}</dd>
                })}
            </dl>
            <div class="row">
                <button class="btn" data-testid="export-diagnostics" disabled=move || exporting.get() on:click=export>
                    {move || if exporting.get() { view! { <span class="spinner"></span> }.into_any() } else { icon(Glyph::Download).into_any() }}
                    {t(T::ExportDiagnostics)}
                </button>
                <button class="btn ghost" data-testid="reset-onboarding" on:click=reset>{icon(Glyph::Reset)}{t(T::ResetOnboarding)}</button>
            </div>
            <p class="muted small">{t(T::DiagnosticsHint)}</p>
            {move || state.get().map(|s| match s {
                Ok(m) => view! { <p class="ok-text small" data-testid="diagnostics-state">{m}</p> }.into_any(),
                Err(e) => view! { <p class="error-text small" data-testid="diagnostics-state">{e}</p> }.into_any(),
            })}
        </div>
    }
}
