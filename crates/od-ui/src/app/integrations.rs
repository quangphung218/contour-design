//! Integrations (the original's `IntegrationsView`): external MCP servers
//! with a template catalog and which agents receive them; "Connect Open
//! Design to your coding agent" (a snippet per client, one-click install
//! for claude, codex and cursor); "Use everywhere" (CLI, MCP, HTTP API).

use super::icons::{Glyph, icon};
use super::mcp_settings::McpSection;
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Mcp,
    Agents,
    Everywhere,
}

/// How runs hand external MCP servers to an agent (see the daemon's
/// `platform::mcp::for_agent`): `None` = not forwarded.
#[must_use]
pub fn forwarding(agent: &str) -> Option<bool> {
    match agent {
        "claude" | "opencode" | "codebuddy" | "mimo" => Some(false),
        // ACP adapters take stdio servers only.
        "kimi" | "hermes" | "kiro" | "kilo" | "vibe" | "trae-cli" | "reasonix" | "devin" => {
            Some(true)
        }
        _ => None,
    }
}

fn copy(text: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.navigator().clipboard().write_text(text);
    }
}

#[component]
pub fn Integrations() -> impl IntoView {
    let tab = RwSignal::new(Tab::Mcp);
    let tab_button = move |tb: Tab, glyph: Glyph, label: T, hint: T, testid: &'static str| {
        view! {
            <button class="seg" role="tab" data-testid=testid data-tip=t(hint)
                aria-selected=move || (tab.get() == tb).to_string() tabindex=move || super::focus::roving(tab.get() == tb)
                class:active=move || tab.get() == tb
                on:click=move |_| tab.set(tb)>
                {icon(glyph)}{t(label)}
            </button>
        }
    };
    view! {
        <section class="page narrow integrations" data-testid="integrations">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavIntegrations)}</h1>
                    <p class="page-sub">{t(T::IntegrationsIntro)}</p>
                </div>
            </header>
            <div class="segmented integration-tabs" role="tablist" on:keydown=move |ev| super::focus::group_keys(&ev, true)>
                {tab_button(Tab::Mcp, Glyph::Mcp, T::McpTitle, T::ExternalTools, "tab-mcp")}
                {tab_button(Tab::Agents, Glyph::Agent, T::ConnectAgentTab, T::ConnectAgents, "tab-agents")}
                {tab_button(Tab::Everywhere, Glyph::Globe, T::UseEverywhere, T::UseEverywhereShort, "tab-everywhere")}
            </div>
            {move || match tab.get() {
                Tab::Mcp => view! {
                    <ForwardedNote/>
                    <section class="settings-section"><McpSection/></section>
                    <McpCatalog/>
                }.into_any(),
                Tab::Agents => view! { <InstallIntoAgents/> }.into_any(),
                Tab::Everywhere => view! { <UseEverywhere/> }.into_any(),
            }}
        </section>
    }
}

/// "Forwarded to: Claude Code · Kimi (stdio only)" from the installed agents.
#[component]
fn ForwardedNote() -> impl IntoView {
    let shell = Shell::get();
    move || {
        let agents = match shell.agents.get() {
            Some(Ok(list)) => list,
            _ => return None,
        };
        let installed: Vec<_> = agents
            .into_iter()
            .filter(|a| a.available && a.id != "byok")
            .collect();
        let mut to: Vec<String> = Vec::new();
        let mut not: Vec<String> = Vec::new();
        let mut acp = false;
        for a in &installed {
            match forwarding(&a.id) {
                Some(stdio_only) => {
                    acp |= stdio_only;
                    to.push(if stdio_only {
                        tf(T::StdioOnly, &[&a.name])
                    } else {
                        a.name.clone()
                    });
                }
                None => not.push(a.name.clone()),
            }
        }
        to.sort();
        not.sort();
        Some(view! {
            <div class="card padded forwarded" data-testid="mcp-forwarded">
                <p><strong>{t(T::ForwardedTo)}</strong>" "<span data-testid="mcp-forwarded-to">{if to.is_empty() { t(T::NoAgents).to_owned() } else { to.join(" · ") }}</span></p>
                {acp.then(|| view! { <p class="muted small">{t(T::ForwardedAcpNote)}</p> })}
                {(!not.is_empty()).then(|| view! {
                    <p><strong>{t(T::NotForwardedTo)}</strong>" "<span data-testid="mcp-not-forwarded">{not.join(" · ")}</span></p>
                    <p class="muted small">{t(T::NotForwardedNote)}</p>
                })}
            </div>
        })
    }
}

fn group_label(group: &str) -> &'static str {
    t(match group {
        "image-generation" => T::GroupImageGeneration,
        "image-editing" => T::GroupImageEditing,
        "web-capture" => T::GroupWebCapture,
        "design-systems" => T::GroupDesignSystems,
        "ui-components" => T::GroupUiComponents,
        "data-viz" => T::GroupDataViz,
        "publishing" => T::GroupPublishing,
        _ => T::GroupUtilities,
    })
}

/// Template servers; "Add" appends one to the saved list.
#[component]
fn McpCatalog() -> impl IntoView {
    let catalog = LocalResource::new(|| async {
        api::get::<Value>("/api/mcp/catalog")
            .await
            .ok()
            .and_then(|v| v["templates"].as_array().cloned())
            .unwrap_or_default()
    });
    let state = RwSignal::new(None::<Result<String, String>>);
    let add = move |entry: Value| {
        leptos::task::spawn_local(async move {
            let current = api::get::<Value>("/api/mcp/servers")
                .await
                .unwrap_or(Value::Null);
            let mut servers: Vec<Value> =
                current["servers"].as_array().cloned().unwrap_or_default();
            for s in &mut servers {
                if let Some(o) = s.as_object_mut() {
                    o.remove("oauth");
                }
            }
            let id = entry["id"].as_str().unwrap_or_default().to_owned();
            if servers.iter().any(|s| s["id"] == id.as_str()) {
                state.set(Some(Err(tf(T::McpAlreadyAdded, &[&id]))));
                return;
            }
            let mut server = entry["server"].clone();
            server["id"] = json!(id);
            server["enabled"] = json!(true);
            servers.push(server);
            match api::put::<_, Value>("/api/mcp/servers", &json!({ "servers": servers })).await {
                Ok(_) => {
                    state.set(Some(Ok(tf(T::McpAdded, &[&id]))));
                    // The editor above reads the list once: show the new row.
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().reload();
                    }
                }
                Err(e) => state.set(Some(Err(e.to_string()))),
            }
        });
    };
    view! {
        <section class="settings-section" data-testid="mcp-catalog">
            <div class="section-intro">
                <h2>{icon(Glyph::Grid)}{t(T::McpCatalog)}</h2>
                <p>{t(T::McpCatalogHint)}</p>
            </div>
            {move || state.get().map(|s| match s {
                Ok(m) => view! { <p class="ok-text small">{m}</p> }.into_any(),
                Err(e) => view! { <p class="error-text small">{e}</p> }.into_any(),
            })}
            <div class="catalog-grid">
                {move || catalog.get().unwrap_or_default().into_iter().map(|e| {
                    let id = e["id"].as_str().unwrap_or_default().to_owned();
                    let server = &e["server"];
                    let line = if server["transport"] == "http" {
                        server["url"].as_str().unwrap_or_default().to_owned()
                    } else {
                        let args: Vec<String> = server["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                        format!("{} {}", server["command"].as_str().unwrap_or_default(), args.join(" "))
                    };
                    let entry = e.clone();
                    view! {
                        <div class="card catalog-card" data-testid="mcp-template" data-template=id.clone()>
                            <div class="row">
                                <strong class="grow">{id.clone()}</strong>
                                <span class="badge outline">{group_label(e["group"].as_str().unwrap_or_default())}</span>
                            </div>
                            <code class="small truncate">{line}</code>
                            <button class="btn small" data-testid="mcp-template-add" on:click=move |_| add(entry.clone())>{icon(Glyph::Add)}{t(T::Add)}</button>
                        </div>
                    }
                }).collect_view()}
            </div>
        </section>
    }
}

/// A snippet per client, copy buttons, one-click install where possible.
#[component]
fn InstallIntoAgents() -> impl IntoView {
    let info =
        LocalResource::new(|| async { api::get::<Value>("/api/mcp/install-info").await.ok() });
    let client = RwSignal::new("claude".to_owned());
    let result = RwSignal::new(None::<Result<String, String>>);
    let busy = RwSignal::new(false);
    // The daemon's message names the client by id ("installed contour into
    // claude"); success is told here with the client's name and what to do
    // next. Failures keep the daemon's message: it says what went wrong.
    let install = move |uninstall: bool, name: String| {
        let id = client.get_untracked();
        result.set(None);
        busy.set(true);
        leptos::task::spawn_local(async move {
            let url = format!("/api/mcp/install/{}", api::seg(&id));
            result.set(Some(
                match api::post::<_, Value>(&url, &json!({ "uninstall": uninstall })).await {
                    Ok(v) if v["ok"] == true => Ok(tf(
                        if uninstall {
                            T::McpUninstalledIn
                        } else {
                            T::McpInstalledIn
                        },
                        &[&name],
                    )),
                    Ok(v) => Err(v["message"].as_str().unwrap_or_default().to_owned()),
                    Err(e) => Err(e.to_string()),
                },
            ));
            busy.set(false);
        });
    };
    view! {
        <section class="settings-section" data-testid="mcp-install">
            <div class="section-intro">
                <h2>{icon(Glyph::Agent)}{t(T::ConnectAgents)}</h2>
                <p>{t(T::ConnectAgentsHint)}</p>
            </div>
            {move || info.get().flatten().map(|v| {
                let clients = v["clients"].as_array().cloned().unwrap_or_default();
                let list = clients.clone();
                let current = move || {
                    let id = client.get();
                    list.iter().find(|c| c["id"] == id.as_str()).cloned().unwrap_or(Value::Null)
                };
                let current2 = current.clone();
                let current3 = current.clone();
                view! {
                    <div class="card padded">
                        <label class="field">
                            <span>{t(T::Client)}</span>
                            <select data-testid="mcp-client" on:change=move |ev| { client.set(event_target_value(&ev)); result.set(None); }>
                                {clients.iter().map(|c| {
                                    let id = c["id"].as_str().unwrap_or_default().to_owned();
                                    let id2 = id.clone();
                                    view! { <option value=id selected=move || client.get() == id2>{c["name"].as_str().unwrap_or_default().to_owned()}</option> }
                                }).collect_view()}
                            </select>
                        </label>
                        {move || {
                            let c = current();
                            let snippet = c["snippet"].as_str().unwrap_or_default().to_owned();
                            let to_copy = snippet.clone();
                            let method = match c["format"].as_str() {
                                Some("command") => t(T::MethodCommand),
                                Some("toml") => t(T::MethodToml),
                                _ => t(T::MethodJson),
                            };
                            view! {
                                <p class="muted small">
                                    {method}
                                    {c["configPath"].as_str().map(|p| view! { " · " <code>{p.to_owned()}</code> })}
                                </p>
                                <pre class="snippet" data-testid="mcp-snippet"><code>{snippet}</code></pre>
                                <div class="row">
                                    <button class="btn small" data-testid="mcp-snippet-copy" on:click=move |_| copy(&to_copy)>{icon(Glyph::Duplicate)}{t(T::Copy)}</button>
                                </div>
                            }
                        }}
                        {move || {
                            let c = current2();
                            c["deeplink"].as_str().map(|link| {
                                let link = link.to_owned();
                                view! { <a class="btn small" href=link data-testid="mcp-deeplink">{icon(Glyph::Link)}{t(T::InstallInCursor)}</a> }
                            })
                        }}
                        {move || {
                            let c = current3();
                            (c["installable"] == true).then(|| {
                                let name = c["name"].as_str().unwrap_or_default().to_owned();
                                let name2 = name.clone();
                                view! {
                                    <div class="row">
                                        <button class="btn primary small" data-testid="mcp-install-run" disabled=move || busy.get() on:click=move |_| install(false, name.clone())>{icon(Glyph::Download)}{t(T::InstallNow)}</button>
                                        <button class="btn ghost small" data-testid="mcp-uninstall-run" disabled=move || busy.get() on:click=move |_| install(true, name2.clone())>{t(T::Uninstall)}</button>
                                    </div>
                                }
                            })
                        }}
                        {move || busy.get().then(|| view! { <p class="muted small" role="status" data-testid="mcp-install-busy"><span class="spinner"></span>{t(T::McpInstalling)}</p> })}
                        {move || result.get().map(|r| match r {
                            Ok(m) => view! { <p class="ok-text small" role="status" data-testid="mcp-install-result">{icon(Glyph::CheckCircle)}{m}</p> }.into_any(),
                            Err(e) => view! { <p class="error-text small" role="alert" data-testid="mcp-install-result">{e}</p> }.into_any(),
                        })}
                        <p class="muted small">{t(T::McpLaunchNote)}" "<code class="mono">{format!("{} {}", v["command"].as_str().unwrap_or_default(), v["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default())}</code></p>
                    </div>
                }
            })}
        </section>
    }
}

/// The original's Use everywhere guide: CLI, MCP server, HTTP API, skills.
#[component]
fn UseEverywhere() -> impl IntoView {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://127.0.0.1:7456".to_owned());
    let sections: Vec<(T, T, String)> = vec![
        (T::EverywhereCli, T::EverywhereCliBody, "od daemon start\nod project list\nod run start --project <id> --message \"Make the hero bolder\"\nod export <id> --format pdf".to_owned()),
        (T::EverywhereMcp, T::EverywhereMcpBody, "od mcp install claude   # or codex, cursor\nod mcp                  # stdio server for any client".to_owned()),
        (T::EverywhereHttp, T::EverywhereHttpBody, format!("curl -s {origin}/api/projects\ncurl -s -X POST {origin}/api/runs -H 'content-type: application/json' \\\n  -d '{{\"agentId\":\"claude\",\"projectId\":\"<id>\",\"message\":\"Add a pricing section\"}}'")),
        (T::EverywhereSkills, T::EverywhereSkillsBody, "od plugin list\nod plugin apply <plugin> -i name=value\nod mcp live-artifacts --project <id>".to_owned()),
    ];
    let guide: String = std::iter::once(format!(
        "# Contour: use it everywhere\n\nDaemon: {origin}\n"
    ))
    .chain(sections.iter().map(|(title, body, code)| {
        format!("\n## {}\n\n{}\n\n```sh\n{code}\n```\n", t(*title), t(*body))
    }))
    .collect();
    let copied = RwSignal::new(false);
    view! {
        <section class="settings-section" data-testid="use-everywhere">
            <div class="section-intro">
                <h2>{icon(Glyph::Globe)}{t(T::UseEverywhere)}</h2>
                <p>{t(T::UseEverywhereHint)}</p>
            </div>
            {sections.into_iter().map(|(title, body, code)| {
                let to_copy = code.clone();
                view! {
                    <div class="card padded guide-card">
                        <h3>{t(title)}</h3>
                        <p class="muted">{t(body)}</p>
                        <pre class="snippet"><code>{code}</code></pre>
                        <button class="btn small ghost" on:click=move |_| copy(&to_copy)>{icon(Glyph::Duplicate)}{t(T::Copy)}</button>
                    </div>
                }
            }).collect_view()}
            <div class="row">
                <button class="btn" data-testid="copy-agent-guide" on:click=move |_| { copy(&guide); copied.set(true); }>{icon(Glyph::Guide)}{t(T::CopyAgentGuide)}</button>
                {move || copied.get().then(|| view! { <span class="ok-text small">{t(T::Copied)}</span> })}
            </div>
        </section>
    }
}
