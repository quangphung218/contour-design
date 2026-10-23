//! Settings ▸ MCP servers (spec 1.5): declare stdio or HTTP servers for the
//! agent, test them, sign in with OAuth. Servers are edited as JSON so env
//! and header entries (shown masked by the daemon) round-trip untouched.

use super::icons::{Glyph, icon};
use super::{api, desktop};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
enum Probe {
    Ok(usize),
    NeedsAuth,
    Failed(String),
    Browser,
}

fn field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The server as `PUT /api/mcp/servers` takes it (no status fields).
fn for_save(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("oauth");
    }
    v
}

#[component]
pub fn McpSection() -> impl IntoView {
    let servers = RwSignal::new(Vec::<Value>::new());
    let status = RwSignal::new(None::<Result<(), String>>);
    // While a save is in flight the daemon still has the old list: a test
    // then would probe a server it does not know yet.
    let saving = RwSignal::new(false);
    let reload = move || {
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<Value>("/api/mcp/servers").await {
                servers.set(r["servers"].as_array().cloned().unwrap_or_default());
            }
        });
    };
    reload();
    let save = move |_| {
        let body =
            json!({"servers": servers.get_untracked().iter().map(for_save).collect::<Vec<_>>()});
        saving.set(true);
        leptos::task::spawn_local(async move {
            match api::put::<_, Value>("/api/mcp/servers", &body).await {
                Ok(r) => {
                    servers.set(r["servers"].as_array().cloned().unwrap_or_default());
                    status.set(Some(Ok(())));
                }
                Err(e) => status.set(Some(Err(e.to_string()))),
            }
            saving.set(false);
        });
    };
    let rows = Memo::new(move |_| servers.with(Vec::len));
    let add = move |_| {
        servers.update(|s| {
            let n = s.len() + 1;
            s.push(json!({"id": format!("server-{n}"), "transport": "stdio", "command": "", "enabled": true}));
        });
    };

    view! {
        <div class="section-intro">
            <h2>{icon(Glyph::Mcp)}{t(T::McpTitle)}</h2>
            <p>{t(T::McpIntro)}</p>
        </div>
        <div class="mcp-list" data-testid="mcp-servers">
            // Rows are rebuilt only when their number changes: an edit or a
            // save must not reset a row (and the test result it shows).
            {move || {
                let count = rows.get();
                if count == 0 {
                    return view! {
                        <div class="card empty-state compact">
                            <span class="empty-icon">{icon(Glyph::Mcp)}</span>
                            <p>{t(T::McpNoServers)}</p>
                        </div>
                    }.into_any();
                }
                (0..count).map(|i| view! { <McpRow servers=servers index=i saving=saving/> }).collect_view().into_any()
            }}
        </div>
        <div class="row end">
            {move || match status.get() {
                Some(Ok(())) => view! { <span class="ok-text">{t(T::Saved)}</span> }.into_any(),
                Some(Err(e)) => view! { <span class="error-text" data-testid="mcp-error">{e}</span> }.into_any(),
                None => ().into_any(),
            }}
            <button class="btn" data-testid="mcp-add" on:click=add>{icon(Glyph::Add)}{t(T::McpAdd)}</button>
            <button class="btn primary" data-testid="mcp-save" disabled=move || saving.get() on:click=save>{t(T::Save)}</button>
        </div>
    }
}

#[component]
fn McpRow(servers: RwSignal<Vec<Value>>, index: usize, saving: RwSignal<bool>) -> impl IntoView {
    let probe = RwSignal::new(None::<Probe>);
    let get = move |key: &'static str| {
        move || servers.with(|s| s.get(index).map(|v| field(v, key)).unwrap_or_default())
    };
    let set = move |key: &'static str, value: Value| {
        servers.update(|s| {
            if let Some(o) = s.get_mut(index).and_then(Value::as_object_mut) {
                o.insert(key.to_owned(), value);
            }
        });
    };
    let id = move || get("id")();
    let is_http = move || get("transport")() == "http";
    let oauth = move || get("auth")() == "oauth";
    let connected = move || {
        servers.with(|s| {
            s.get(index)
                .and_then(|v| v["oauth"]["connected"].as_bool())
                .unwrap_or(false)
        })
    };
    let switch_transport = move |http: bool| {
        servers.update(|s| {
            if let Some(o) = s.get_mut(index).and_then(Value::as_object_mut) {
                for k in ["command", "args", "env", "url", "headers", "auth"] {
                    o.remove(k);
                }
                o.insert(
                    "transport".into(),
                    json!(if http { "http" } else { "stdio" }),
                );
                o.insert(if http { "url" } else { "command" }.into(), json!(""));
            }
        });
    };
    let test = move |_| {
        let url = format!("/api/mcp/servers/{}/test", api::seg(&id()));
        leptos::task::spawn_local(async move {
            probe.set(Some(match api::post::<_, Value>(&url, &json!({})).await {
                Ok(r) if r["ok"] == true => Probe::Ok(r["tools"].as_array().map_or(0, Vec::len)),
                Ok(r) if r["needsAuth"] == true => Probe::NeedsAuth,
                Ok(r) => Probe::Failed(field(&r, "error")),
                Err(e) => Probe::Failed(e.to_string()),
            }));
        });
    };
    let sign_in = move |_| {
        let url = format!("/api/mcp/servers/{}/oauth/start", api::seg(&id()));
        let on_desktop = desktop::is_desktop();
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>(&url, &json!({"open": on_desktop})).await {
                Ok(r) => {
                    if !on_desktop {
                        if let Some(w) = web_sys::window() {
                            let _ =
                                w.open_with_url_and_target(&field(&r, "authorizeUrl"), "_blank");
                        }
                    }
                    probe.set(Some(Probe::Browser));
                }
                Err(e) => probe.set(Some(Probe::Failed(e.to_string()))),
            }
        });
    };
    let sign_out = move |_| {
        let url = format!("/api/mcp/servers/{}/oauth", api::seg(&id()));
        leptos::task::spawn_local(async move {
            let _ = api::delete::<Value>(&url).await;
            servers.update(|s| {
                if let Some(v) = s.get_mut(index) {
                    v["oauth"] = json!({"connected": false});
                }
            });
        });
    };
    let remove = move |_| {
        servers.update(|s| {
            if index < s.len() {
                s.remove(index);
            }
        })
    };

    view! {
        <div class="card padded mcp-server" data-testid="mcp-server">
            <div class="row mcp-head">
                <label class="field grow">
                    <span>{t(T::McpId)}</span>
                    <input class="input mono" data-testid="mcp-id" prop:value=id on:change=move |ev| set("id", json!(event_target_value(&ev)))/>
                </label>
                <div class="segmented small transport">
                    <button class="seg" class:active=move || !is_http() on:click=move |_| switch_transport(false)>"stdio"</button>
                    <button class="seg" class:active=is_http data-testid="mcp-http" on:click=move |_| switch_transport(true)>"HTTP"</button>
                </div>
                <label class="switch">
                    <input type="checkbox"
                        prop:checked=move || servers.with(|s| s.get(index).and_then(|v| v["enabled"].as_bool()).unwrap_or(true))
                        on:change=move |ev| set("enabled", json!(event_target_checked(&ev)))/>
                    <span>{t(T::RoutineEnabled)}</span>
                </label>
            </div>
            <Show
                when=is_http
                fallback=move || view! {
                    <label class="field">
                        <span>{t(T::McpCommand)}</span>
                        <input class="input mono" data-testid="mcp-command" prop:value=get("command") on:change=move |ev| set("command", json!(event_target_value(&ev)))/>
                    </label>
                    <label class="field">
                        <span>{t(T::McpArgs)}</span>
                        <textarea class="input mono" rows="2" data-testid="mcp-args"
                            prop:value=move || servers.with(|s| s.get(index).and_then(|v| v["args"].as_array().cloned()).unwrap_or_default()
                                .iter().filter_map(|a| a.as_str().map(str::to_owned)).collect::<Vec<_>>().join("\n"))
                            on:change=move |ev| {
                                let args: Vec<String> = event_target_value(&ev).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
                                set("args", json!(args));
                            }></textarea>
                    </label>
                }
            >
                <label class="field">
                    <span>{t(T::McpUrl)}</span>
                    <input class="input mono" data-testid="mcp-url" prop:value=get("url") on:change=move |ev| set("url", json!(event_target_value(&ev)))/>
                </label>
                <label class="radio-row">
                    <input type="checkbox" prop:checked=oauth on:change=move |ev| set("auth", json!(if event_target_checked(&ev) { "oauth" } else { "none" }))/>
                    <span>{t(T::McpUseOauth)}</span>
                </label>
            </Show>
            <div class="row mcp-actions">
                <button class="btn small" data-testid="mcp-test" disabled=move || saving.get() on:click=test>{icon(Glyph::Play)}{t(T::McpTest)}</button>
                <Show when=move || is_http() && oauth()>
                    {move || if connected() {
                        view! {
                            <span class="ok-text small">{t(T::McpSignedIn)}</span>
                            <button class="btn small" on:click=sign_out>{t(T::McpSignOut)}</button>
                        }.into_any()
                    } else {
                        view! { <button class="btn small" data-testid="mcp-connect" on:click=sign_in>{t(T::McpConnect)}</button> }.into_any()
                    }}
                </Show>
                <span class="grow small" data-testid="mcp-probe">
                    {move || probe.get().map(|p| match p {
                        Probe::Ok(n) => view! { <span class="ok-text">{tf(T::McpTestOk, &[&n.to_string()])}</span> }.into_any(),
                        Probe::NeedsAuth => view! { <span class="error-text">{t(T::McpNeedsAuth)}</span> }.into_any(),
                        Probe::Browser => view! { <span class="muted">{t(T::McpFinishInBrowser)}</span> }.into_any(),
                        Probe::Failed(e) => view! { <span class="error-text">{e}</span> }.into_any(),
                    })}
                </span>
                <button class="btn small ghost danger" on:click=remove>{icon(Glyph::Delete)}{t(T::Remove)}</button>
            </div>
        </div>
    }
}
