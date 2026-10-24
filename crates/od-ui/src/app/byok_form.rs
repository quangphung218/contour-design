//! API provider (BYOK) form, shared by Settings and onboarding: preset,
//! base URL, key (never shown back: the daemon keeps it and answers with a
//! hint), model with "Fetch models", "Test" and save. Saving also makes
//! the BYOK agent the one runs use.

use super::icons::{Glyph, icon};
use super::{Shell, api};
use crate::byok::{self, PRESETS};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::{Value, json};

/// What a connection test answered (`POST /api/test/connection`).
#[derive(Debug, Clone, PartialEq)]
pub enum TestState {
    Running,
    Done(Value),
    Failed(String),
}

/// One line describing a test result, as the original's copy.
#[must_use]
pub fn describe_test(v: &Value) -> (bool, String) {
    let ms = v["latencyMs"].as_u64().unwrap_or(0).to_string();
    let sample = v["sample"].as_str().unwrap_or_default();
    let detail = v["detail"].as_str().unwrap_or_default();
    let model = v["model"].as_str().unwrap_or_default();
    let name = v["agentName"].as_str().unwrap_or_default();
    if v["ok"] == true {
        let text = if name.is_empty() {
            tf(T::TestSuccessApi, &[&ms, sample])
        } else {
            tf(T::TestSuccessCli, &[name, &ms, sample])
        };
        return (true, text);
    }
    let text = match v["kind"].as_str().unwrap_or_default() {
        "auth_failed" => t(T::TestAuthFailed).to_owned(),
        "forbidden" => tf(T::TestForbidden, &[detail]),
        "not_found_model" => tf(T::TestNotFoundModel, &[model]),
        "invalid_base_url" => tf(T::TestInvalidBaseUrl, &[detail]),
        "rate_limited" => t(T::TestRateLimited).to_owned(),
        "timeout" => tf(T::TestTimeout, &[&ms]),
        "agent_not_installed" => tf(T::TestAgentMissing, &[name]),
        "agent_spawn_failed" => tf(T::TestAgentSpawn, &[name, detail]),
        _ => tf(T::TestUnknown, &[detail]),
    };
    (false, text)
}

/// The test result line.
#[component]
pub fn TestResultLine(state: RwSignal<Option<TestState>>, testid: &'static str) -> impl IntoView {
    view! {
        <span class="test-result small" data-testid=testid role="status">
            {move || state.get().map(|s| match s {
                TestState::Running => view! { <span class="muted"><span class="spinner"></span>{t(T::TestRunning)}</span> }.into_any(),
                TestState::Done(v) => {
                    let (ok, text) = describe_test(&v);
                    view! {
                        <span class=if ok { "ok-text" } else { "error-text" } data-ok=ok.to_string()>
                            {icon(if ok { Glyph::CheckCircle } else { Glyph::Warning })}{text}
                        </span>
                    }.into_any()
                }
                TestState::Failed(e) => view! { <span class="error-text" data-ok="false">{e}</span> }.into_any(),
            })}
        </span>
    }
}

/// The form. `on_saved` runs after a successful save.
#[component]
pub fn ByokForm(#[prop(optional, into)] on_saved: Option<Callback<()>>) -> impl IntoView {
    let shell = Shell::get();
    let stored = shell
        .config
        .with_untracked(|c| c.as_ref().and_then(|c| c.extra.get("byok").cloned()))
        .unwrap_or(Value::Null);
    let s_protocol = stored["protocol"]
        .as_str()
        .unwrap_or("anthropic")
        .to_owned();
    let s_base = stored["baseUrl"].as_str().map(str::to_owned);
    let has_key = stored["hasApiKey"] == true;
    let key_hint = stored["apiKeyHint"].as_str().unwrap_or_default().to_owned();
    let initial = byok::preset_for(&s_protocol, s_base.as_deref())
        .map_or("custom", |p| p.id)
        .to_owned();

    let preset_id = RwSignal::new(initial);
    let protocol = RwSignal::new(s_protocol);
    let base_url = RwSignal::new(s_base.unwrap_or_default());
    let api_key = RwSignal::new(String::new());
    let show_key = RwSignal::new(false);
    let cleaned = RwSignal::new(false);
    let model = RwSignal::new(stored["model"].as_str().unwrap_or_default().to_owned());
    let api_version = RwSignal::new(stored["apiVersion"].as_str().unwrap_or_default().to_owned());
    let azure = move || protocol.get() == "azure";
    let fetched = RwSignal::new(Vec::<String>::new());
    let fetch_state = RwSignal::new(None::<Result<usize, String>>);
    let test = RwSignal::new(None::<TestState>);
    let saved = RwSignal::new(None::<Result<(), String>>);

    let pick = move |id: String| {
        if let Some(p) = byok::preset(&id) {
            protocol.set(p.protocol.to_owned());
            base_url.set(p.base_url.to_owned());
            model.set(p.models[0].to_owned());
        }
        fetched.set(Vec::new());
        test.set(None);
        preset_id.set(id);
    };
    let suggested = move || {
        let mut list: Vec<String> = fetched.get();
        if let Some(p) = byok::preset(&preset_id.get()) {
            for m in p.models {
                if !list.iter().any(|x| x == m) {
                    list.push((*m).to_owned());
                }
            }
        }
        list
    };
    // The key is sent only when typed; otherwise the daemon uses the
    // stored one (for the stored base URL only).
    let body = move || {
        let mut b = json!({
            "protocol": protocol.get_untracked(),
            "baseUrl": base_url.get_untracked().trim(),
            "model": model.get_untracked().trim(),
        });
        let key = api_key.get_untracked();
        if !key.is_empty() {
            b["apiKey"] = json!(key);
        }
        // An emptied field clears a saved version (back to the default).
        let version = api_version.get_untracked();
        let version = version.trim();
        b["apiVersion"] = if protocol.get_untracked() == "azure" && !version.is_empty() {
            json!(version)
        } else {
            Value::Null
        };
        b
    };
    let run_test = move |_| {
        let mut b = body();
        b["mode"] = json!("provider");
        test.set(Some(TestState::Running));
        leptos::task::spawn_local(async move {
            test.set(Some(
                match api::post::<_, Value>("/api/test/connection", &b).await {
                    Ok(v) => TestState::Done(v),
                    Err(e) => TestState::Failed(e.to_string()),
                },
            ));
        });
    };
    let fetch = move |_| {
        let b = body();
        fetch_state.set(None);
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/provider/models", &b).await {
                Ok(v) => {
                    let list: Vec<String> = v["models"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|m| m.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    fetch_state.set(Some(Ok(list.len())));
                    fetched.set(list);
                }
                Err(e) => fetch_state.set(Some(Err(e.to_string()))),
            }
        });
    };
    let save = move |_| {
        let mut byok = body();
        if byok["baseUrl"] == "" {
            byok["baseUrl"] = Value::Null;
        }
        if byok["model"] == "" {
            byok["model"] = Value::Null;
        }
        let mut next = shell.config.get_untracked().unwrap_or_default();
        let merged = match next.extra.get("byok") {
            Some(Value::Object(old)) => {
                let mut o = old.clone();
                o.remove("hasApiKey");
                o.remove("apiKeyHint");
                if let Value::Object(new) = &byok {
                    for (k, v) in new {
                        o.insert(k.clone(), v.clone());
                    }
                }
                Value::Object(o)
            }
            _ => byok,
        };
        next.extra.insert("byok".to_owned(), merged);
        next.agent_id = Some("byok".to_owned());
        leptos::task::spawn_local(async move {
            let res = shell.save_config(next).await;
            if res.is_ok() {
                api_key.set(String::new());
                shell.reload_agents();
                if let Some(cb) = on_saved {
                    cb.run(());
                }
            }
            saved.set(Some(res));
        });
    };
    let needs_key = move || byok::preset(&preset_id.get()).is_none_or(|p| p.needs_key);
    let shape_warning = move || {
        let key = api_key.get();
        !key.is_empty() && byok::key_shape_warning(&preset_id.get(), &key)
    };

    view! {
        <div class="byok-form" data-testid="byok-form">
            <div class="row wrap">
                <label class="field grow">
                    <span>{t(T::Provider)}</span>
                    <select data-testid="byok-preset" on:change=move |ev| pick(event_target_value(&ev))>
                        {PRESETS.iter().map(|p| {
                            let id = p.id;
                            view! { <option value=id selected=move || preset_id.get() == id>{p.label}</option> }
                        }).collect_view()}
                        <option value="custom" selected=move || preset_id.get() == "custom">{t(T::CustomProvider)}</option>
                    </select>
                </label>
                <Show when=move || preset_id.get() == "custom">
                    <label class="field">
                        <span>{t(T::Protocol)}</span>
                        <select data-testid="byok-protocol" on:change=move |ev| protocol.set(event_target_value(&ev))>
                            <option value="anthropic" selected=move || protocol.get() == "anthropic">"Anthropic Messages"</option>
                            <option value="openai" selected=move || protocol.get() == "openai">{t(T::OpenAiCompatible)}</option>
                            <option value="azure" selected=move || protocol.get() == "azure">"Azure OpenAI"</option>
                        </select>
                    </label>
                </Show>
            </div>
            <label class="field">
                <span>{t(T::BaseUrl)}</span>
                <input class="input mono" data-testid="byok-base-url"
                    placeholder=move || if azure() { "https://<resource>.openai.azure.com" } else { "https://…" }
                    prop:value=move || base_url.get()
                    on:input=move |ev| { base_url.set(event_target_value(&ev)); test.set(None); }/>
            </label>
            <Show when=needs_key>
                <label class="field">
                    <span>{t(T::ApiKey)}</span>
                    <span class="row key-row">
                        <input class="input mono grow" data-testid="byok-key" autocomplete="off"
                            type=move || if show_key.get() { "text" } else { "password" }
                            placeholder=if has_key { tf(T::ApiKeyStored, &[&key_hint]) } else { t(T::ApiKeyPlaceholder).to_owned() }
                            prop:value=move || api_key.get()
                            on:input=move |ev| {
                                let (key, changed) = byok::clean_key(&event_target_value(&ev));
                                cleaned.set(changed);
                                api_key.set(key);
                            }/>
                        <button class="icon-btn" aria-label=move || if show_key.get() { t(T::HideKey) } else { t(T::ShowKey) }
                            data-tip=move || if show_key.get() { t(T::HideKey) } else { t(T::ShowKey) }
                            aria-pressed=move || show_key.get().to_string()
                            on:click=move |_| show_key.update(|s| *s = !*s)>
                            {move || icon(if show_key.get() { Glyph::EyeOff } else { Glyph::Eye })}
                        </button>
                    </span>
                    {move || cleaned.get().then(|| view! { <span class="muted">{t(T::ApiKeyCleaned)}</span> })}
                    {move || shape_warning().then(|| view! { <span class="warning-text">{t(T::ApiKeyShape)}</span> })}
                    <span class="muted">{t(T::ApiKeyLocalHint)}</span>
                </label>
            </Show>
            <Show when=azure>
                <label class="field">
                    <span>{t(T::ApiVersion)}</span>
                    <input class="input mono" data-testid="byok-api-version" placeholder="2024-10-21"
                        prop:value=move || api_version.get()
                        on:input=move |ev| { api_version.set(event_target_value(&ev)); test.set(None); }/>
                </label>
            </Show>
            <label class="field">
                <span>{move || if azure() { t(T::AzureDeployment) } else { t(T::Model) }}</span>
                <span class="row">
                    <input class="input mono grow" data-testid="byok-model" list="byok-models"
                        prop:value=move || model.get()
                        on:input=move |ev| model.set(event_target_value(&ev))/>
                    <button class="btn small" data-testid="byok-fetch-models" on:click=fetch>{icon(Glyph::Refresh)}{t(T::FetchModels)}</button>
                </span>
                <datalist id="byok-models">
                    {move || suggested().into_iter().map(|m| view! { <option value=m></option> }).collect_view()}
                </datalist>
                <span class="muted" data-testid="byok-models-state">{move || match fetch_state.get() {
                    Some(Ok(n)) => tf(T::FetchedModels, &[&n.to_string()]),
                    Some(Err(e)) => e,
                    None => t(T::ModelSourceSuggested).to_owned(),
                }}</span>
            </label>
            <div class="row byok-actions">
                <button class="btn" data-testid="byok-test" on:click=run_test>{icon(Glyph::Play)}{t(T::Test)}</button>
                <TestResultLine state=test testid="byok-test-result"/>
                <span class="spacer"></span>
                {move || match saved.get() {
                    Some(Ok(())) => view! { <span class="ok-text">{icon(Glyph::Check)}{t(T::Saved)}</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                    None => ().into_any(),
                }}
                // Azure has no shared host: nothing to save without the endpoint.
                <button class="btn primary" data-testid="byok-save" on:click=save
                    disabled=move || azure() && base_url.with(|b| b.trim().is_empty())>{t(T::SaveProvider)}</button>
            </div>
        </div>
    }
}
