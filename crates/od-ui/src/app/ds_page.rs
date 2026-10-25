//! Design systems (spec 1.4): browse "Your systems" and the bundled
//! presets (search, surface, category), and a kit view per system:
//! palette, type, token contract, assets, showcase, tokens and DESIGN.md.
//! User systems can be edited, reviewed (Looks good / Needs work with an
//! agent revision to accept or reject), published, set as the default for
//! new chats, downloaded, copied, edited with an agent and deleted.

use super::ds_create::CreateFlow;
use super::export_menu::download;
use super::icons::{Glyph, icon};
use super::lib_ui::{Modal, copy_text, picked_files, query_param, size_label, upload};
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use crate::settings::DEFAULT_DESIGN_SYSTEM_KEY;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    ContentSource, DesignSystemStatus, DesignSystemSummary, DesignSystemsResponse, Surface,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Poll interval while a revision runs.
const POLL_MS: u32 = 700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Mine,
    Presets,
}

fn is_mine(s: &DesignSystemSummary) -> bool {
    s.source == ContentSource::User || s.is_editable
}

/// The summary line under a system's name. The bundled packages share a
/// note for maintainers ("Bundled … package for X, derived from curated
/// DESIGN.md, tokens.css…"), which says nothing to a user, so it is left out.
fn summary_for_people(summary: &str) -> Option<String> {
    let s = summary.trim();
    let boilerplate = s.starts_with("Bundled ") && s.contains(" package for ");
    (!s.is_empty() && !boilerplate).then(|| crate::i18n::rebrand(s).into_owned())
}

/// Whether `s` matches the search text: the words the page shows (title,
/// description, a summary written for people, category), not the bundled
/// packages' maintainer note.
fn matches(s: &DesignSystemSummary, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    let summary = summary_for_people(&s.summary).unwrap_or_default();
    q.is_empty()
        || format!("{} {} {summary} {}", s.title, s.description, s.category)
            .to_lowercase()
            .contains(&q)
}

fn surface_label(s: Surface) -> &'static str {
    match s {
        Surface::Web => t(T::SurfaceWeb),
        Surface::Image => t(T::SurfaceImage),
        Surface::Video => t(T::SurfaceVideo),
        Surface::Audio => t(T::SurfaceAudio),
    }
}

#[component]
pub fn DesignSystemsPage() -> impl IntoView {
    let shell = Shell::get();
    let systems = RwSignal::new(None::<Vec<DesignSystemSummary>>);
    let scope = RwSignal::new(Scope::Presets);
    let query = RwSignal::new(String::new());
    let surface = RwSignal::new(None::<Surface>);
    let category = RwSignal::new(String::new());
    let selected = RwSignal::new(query_param("id"));
    let creating = RwSignal::new(query_param("new").is_some());
    let error = RwSignal::new(None::<String>);

    let reload = move |select: Option<String>| {
        leptos::task::spawn_local(async move {
            // An agent may have edited a package in place.
            let _ = api::post::<_, Value>("/api/design-systems/reload", &json!({})).await;
            match api::get::<DesignSystemsResponse>("/api/design-systems").await {
                Ok(r) => {
                    if let Some(id) = select {
                        // Show the system asked for, whatever the filters were.
                        query.set(String::new());
                        category.set(String::new());
                        surface.set(None);
                        if let Some(s) = r.design_systems.iter().find(|s| s.id == id) {
                            scope.set(if is_mine(s) {
                                Scope::Mine
                            } else {
                                Scope::Presets
                            });
                        }
                        selected.set(Some(id));
                    }
                    systems.set(Some(r.design_systems));
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    reload(selected.get_untracked());
    let on_change = Callback::new(move |id: Option<String>| reload(id));

    let default_id = Memo::new(move |_| {
        shell.config.with(|c| {
            c.as_ref()
                .and_then(crate::settings::AppConfig::default_design_system)
        })
    });

    let in_scope = Memo::new(move |_| {
        let sc = scope.get();
        systems.with(|list| {
            list.as_ref().map_or_else(Vec::new, |l| {
                l.iter()
                    .filter(|s| (sc == Scope::Mine) == is_mine(s))
                    .cloned()
                    .collect::<Vec<_>>()
            })
        })
    });
    let visible = Memo::new(move |_| {
        let q = query.get();
        let sf = surface.get();
        let cat = category.get();
        in_scope.with(|l| {
            l.iter()
                .filter(|s| matches(s, &q))
                .filter(|s| sf.is_none_or(|f| s.surface == f))
                .filter(|s| cat.is_empty() || s.category == cat)
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let categories = Memo::new(move |_| {
        let mut c: Vec<String> = in_scope.with(|l| l.iter().map(|s| s.category.clone()).collect());
        c.sort();
        c.dedup();
        c
    });
    // Keep a selection inside what is shown.
    Effect::new(move |_| {
        let list = visible.get();
        let current = selected.get_untracked();
        if current
            .as_ref()
            .is_none_or(|id| !list.iter().any(|s| &s.id == id))
        {
            if let Some(first) = list.first() {
                selected.set(Some(first.id.clone()));
            }
        }
    });
    let count = move |sc: Scope| {
        systems.with(|l| {
            l.as_ref().map_or(0, |l| {
                l.iter()
                    .filter(|s| (sc == Scope::Mine) == is_mine(s))
                    .count()
            })
        })
    };
    let surface_count =
        move |sf: Surface| in_scope.with(|l| l.iter().filter(|s| s.surface == sf).count());

    let scope_tab = move |sc: Scope, label: T| {
        view! {
            <button
                class="seg"
                role="tab"
                class:active=move || scope.get() == sc
                aria-selected=move || (scope.get() == sc).to_string()
                data-testid=match sc { Scope::Mine => "ds-scope-mine", Scope::Presets => "ds-scope-presets" }
                on:click=move |_| {
                    scope.set(sc);
                    category.set(String::new());
                    surface.set(None);
                }
            >
                <span class="seg-label">{t(label)}<span class="count-badge">{move || count(sc)}</span></span>
            </button>
        }
    };

    view! {
        <section class="page wide lib-page" data-testid="design-systems">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavDesignSystems)}</h1>
                    <p class="page-sub">{t(T::DsPageSub)}</p>
                </div>
                <button class="btn primary" data-testid="ds-page-create" on:click=move |_| creating.set(true)>
                    {icon(Glyph::Add)}{t(T::Create)}
                </button>
            </header>
            <div class="segmented lib-tabs" role="tablist">
                {scope_tab(Scope::Mine, T::DsYourSystems)}
                {scope_tab(Scope::Presets, T::DsPresets)}
            </div>
            <div class="lib-toolbar">
                <label class="lib-search">
                    {icon(Glyph::Search)}
                    <input
                        class="input small"
                        type="search"
                        data-testid="ds-search"
                        placeholder=t(T::DsSearch)
                        aria-label=t(T::DsSearch)
                        prop:value=move || query.get()
                        on:input=move |ev| query.set(event_target_value(&ev))
                    />
                </label>
                <div class="segmented small" role="group" aria-label=t(T::Surface)>
                    <button class="seg" aria-pressed=move || surface.get().is_none().to_string() on:click=move |_| surface.set(None)>
                        {t(T::AllFilter)}
                    </button>
                    {[Surface::Web, Surface::Image, Surface::Video, Surface::Audio]
                        .into_iter()
                        .map(|sf| view! {
                            <Show when=move || { surface_count(sf) > 0 }>
                                <button
                                    class="seg"
                                    data-testid="ds-surface"
                                    aria-pressed=move || (surface.get() == Some(sf)).to_string()
                                    on:click=move |_| surface.set(Some(sf))
                                >
                                    {surface_label(sf)}
                                    <span class="count-badge">{move || surface_count(sf)}</span>
                                </button>
                            </Show>
                        })
                        .collect_view()}
                </div>
                <select
                    class="select-auto small"
                    data-testid="ds-category"
                    aria-label=t(T::Category)
                    prop:value=move || category.get()
                    on:change=move |ev| category.set(event_target_value(&ev))
                >
                    <option value="">{t(T::AllCategories)}</option>
                    {move || categories.get().into_iter().map(|c| view! { <option value=c.clone()>{c.clone()}</option> }).collect_view()}
                </select>
            </div>
            {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
            <div class="lib-split">
                <ul class="lib-list" role="listbox" aria-label=t(T::NavDesignSystems) data-testid="ds-list">
                    {move || match systems.get() {
                        None => (0..6).map(|_| view! { <li class="skeleton lib-row-skeleton"></li> }).collect_view().into_any(),
                        Some(_) => {
                            let list = visible.get();
                            if list.is_empty() {
                                let mine = scope.get() == Scope::Mine;
                                view! {
                                    <li class="empty-state compact">
                                        <span class="empty-icon">{icon(Glyph::DesignSystem)}</span>
                                        <p>{if mine { t(T::DsEmptyMine) } else { t(T::DsNoMatch) }}</p>
                                        {mine.then(|| view! {
                                            <button class="btn small" on:click=move |_| creating.set(true)>{icon(Glyph::Add)}{t(T::Create)}</button>
                                        })}
                                        {(!mine && !query.with(|q| q.trim().is_empty())).then(|| view! {
                                            <button class="btn small" data-testid="ds-clear-search" on:click=move |_| query.set(String::new())>{icon(Glyph::Close)}{t(T::ClearSearch)}</button>
                                        })}
                                    </li>
                                }.into_any()
                            } else {
                                list.into_iter().map(|s| view! { <DsRow system=s selected=selected default_id=default_id/> }).collect_view().into_any()
                            }
                        }
                    }}
                </ul>
                <div class="lib-detail">
                    {move || {
                        let id = selected.get()?;
                        // Nothing matches the search: no detail of a hidden row.
                        if !query.with(|q| q.trim().is_empty()) && visible.with(|l| l.iter().all(|s| s.id != id)) {
                            return None;
                        }
                        let summary = systems.with(|l| l.as_ref()?.iter().find(|s| s.id == id).cloned())?;
                        Some(view! { <DsDetail summary=summary default_id=default_id on_change=on_change/> })
                    }}
                </div>
            </div>
            <Show when=move || creating.get()>
                <CreateFlow
                    systems=Signal::derive(move || systems.get().unwrap_or_default())
                    on_close=Callback::new(move |()| creating.set(false))
                    on_created=Callback::new(move |id: String| {
                        creating.set(false);
                        reload(Some(id));
                    })
                />
            </Show>
        </section>
    }
}

/// Four swatches as a stripe; a tint from the id when there are none.
fn stripe(s: &DesignSystemSummary) -> impl IntoView + use<> {
    let swatches = s.swatches.clone();
    let fallback = format!("lib-stripe tint-{}", super::lib_ui::tint(&s.id));
    view! {
        <span class=fallback aria-hidden="true">
            {swatches.into_iter().map(|c| view! { <i style=format!("background:{c}")></i> }).collect_view()}
        </span>
    }
}

#[component]
fn DsRow(
    system: DesignSystemSummary,
    selected: RwSignal<Option<String>>,
    default_id: Memo<Option<String>>,
) -> impl IntoView {
    let id = system.id.clone();
    let id_click = id.clone();
    let id_default = id.clone();
    let is_sel = move || selected.get().as_deref() == Some(id.as_str());
    let mine = is_mine(&system);
    let subtitle = if mine && !system.summary.is_empty() {
        crate::i18n::rebrand(&system.summary).into_owned()
    } else {
        system.category.clone()
    };
    let published = system.status == DesignSystemStatus::Published;
    view! {
        <li
            class="lib-row"
            role="option"
            tabindex="0"
            data-testid="ds-row"
            data-id=system.id.clone()
            aria-selected=move || is_sel().to_string()
            on:click=move |_| selected.set(Some(id_click.clone()))
            on:keydown={
                let id = system.id.clone();
                move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Enter" || ev.key() == " " {
                        ev.prevent_default();
                        selected.set(Some(id.clone()));
                    }
                }
            }
        >
            {stripe(&system)}
            <span class="lib-row-text">
                <span class="lib-row-title">
                    {system.title.clone()}
                    <Show when={
                        let id = id_default.clone();
                        move || default_id.get().as_deref() == Some(id.as_str())
                    }>
                        <span class="badge accent">{t(T::ChatDefault)}</span>
                    </Show>
                </span>
                <span class="lib-row-sub">{subtitle}</span>
            </span>
            {mine.then(|| view! {
                <span class="status-dot" class:on=published title=if published { t(T::Published) } else { t(T::Draft) }></span>
            })}
        </li>
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Token {
    pub name: String,
    pub value: String,
    pub group: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub hex: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub score: u32,
    pub grade: String,
    pub missing_a1: Vec<String>,
    pub missing_a2: Vec<String>,
    pub recommend_rebuild: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub kind: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RevisionFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub feedback: Option<String>,
    pub section: Option<String>,
    pub error: Option<String>,
    pub files: Vec<RevisionFile>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kit {
    pub summary: DesignSystemSummary,
    pub body: String,
    pub tokens: Vec<Token>,
    pub palette: Vec<Swatch>,
    pub score: Score,
    pub has_components: bool,
    pub assets: Vec<Asset>,
    pub previews: Vec<String>,
    pub revisions: Vec<Revision>,
    /// Set on systems named after a real brand (`manifest.json` `brand`).
    #[serde(default)]
    pub brand: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KitTab {
    Kit,
    Showcase,
    Tokens,
    Design,
}

/// `## ` headings of a DESIGN.md, for the "Needs work" section picker.
fn sections(md: &str) -> Vec<String> {
    md.lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(|h| {
            h.trim()
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
                .trim()
                .to_owned()
        })
        .filter(|h| !h.is_empty())
        .collect()
}

fn grade_label(grade: &str) -> &'static str {
    match grade {
        "excellent" => t(T::GradeExcellent),
        "usable" => t(T::GradeUsable),
        "needs-review" => t(T::GradeNeedsReview),
        _ => t(T::GradeNeedsRebuild),
    }
}

fn ds_url(id: &str, rest: &str) -> String {
    format!("/api/design-systems/{}/{rest}", api::seg(id))
}

#[component]
fn DsDetail(
    summary: DesignSystemSummary,
    default_id: Memo<Option<String>>,
    on_change: Callback<Option<String>>,
) -> impl IntoView {
    let shell = Shell::get();
    let navigate = use_navigate();
    let id = summary.id.clone();
    let mine = is_mine(&summary);
    let kit = RwSignal::new(None::<Result<Kit, String>>);
    let tab = RwSignal::new(KitTab::Kit);
    let notice = RwSignal::new(None::<Result<String, String>>);
    let editing = RwSignal::new(None::<String>);
    let feedback_open = RwSignal::new(false);
    let feedback = RwSignal::new(String::new());
    let feedback_section = RwSignal::new(String::new());
    let fullscreen = RwSignal::new(None::<String>);
    let menu = RwSignal::new(false);
    let upload_ref = NodeRef::<leptos::html::Input>::new();
    let focused_asset = RwSignal::new(None::<String>);

    let load = {
        let id = id.clone();
        move || {
            let id = id.clone();
            leptos::task::spawn_local(async move {
                let res = api::get::<Kit>(&ds_url(&id, "kit"))
                    .await
                    .map_err(|e| e.to_string());
                let running = res
                    .as_ref()
                    .is_ok_and(|k| k.revisions.iter().any(|r| r.status == "running"));
                kit.set(Some(res));
                if running {
                    gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
                    // Refetch until the agent's revision is ready.
                    if let Ok(k) = api::get::<Kit>(&ds_url(&id, "kit")).await {
                        kit.set(Some(Ok(k)));
                    }
                }
            });
        }
    };
    load();
    // Poll while a revision is running.
    let poll = load.clone();
    Effect::new(move |_| {
        let running = kit.with(
            |k| matches!(k, Some(Ok(k)) if k.revisions.iter().any(|r| r.status == "running")),
        );
        if running {
            let poll = poll.clone();
            leptos::task::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
                poll();
            });
        }
    });

    let say = move |r: Result<String, String>| notice.set(Some(r));
    let post = {
        let load = load.clone();
        move |path: String, body: Value, ok: &'static str, refresh_list: bool| {
            let load = load.clone();
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(&path, &body).await {
                    Ok(_) => {
                        say(Ok(ok.to_owned()));
                        load();
                        if refresh_list {
                            on_change.run(None);
                        }
                    }
                    Err(e) => say(Err(e.to_string())),
                }
            });
        }
    };

    let copy_md = move || {
        let body = kit.with_untracked(|k| {
            k.as_ref()
                .and_then(|k| k.as_ref().ok())
                .map(|k| k.body.clone())
        });
        if let Some(body) = body {
            leptos::task::spawn_local(async move {
                say(copy_text(&body)
                    .await
                    .map(|()| t(T::CopiedDesignMd).to_owned()));
            });
        }
    };
    let start_edit = move || {
        if mine {
            let body = kit.with_untracked(|k| {
                k.as_ref()
                    .and_then(|k| k.as_ref().ok())
                    .map(|k| k.body.clone())
            });
            tab.set(KitTab::Design);
            editing.set(body);
        }
    };
    let save_edit = {
        let id = id.clone();
        let load = load.clone();
        move || {
            let Some(body) = editing.get_untracked() else {
                return;
            };
            let id = id.clone();
            let load = load.clone();
            leptos::task::spawn_local(async move {
                match api::put::<_, Value>(&ds_url(&id, "design-md"), &json!({ "body": body }))
                    .await
                {
                    Ok(_) => {
                        editing.set(None);
                        say(Ok(t(T::Saved).to_owned()));
                        load();
                    }
                    Err(e) => say(Err(e.to_string())),
                }
            });
        }
    };
    let rebuild = {
        let id = id.clone();
        let load = load.clone();
        move || {
            if !mine {
                load();
                return;
            }
            let id = id.clone();
            let load = load.clone();
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(&ds_url(&id, "token-contract/rebuild"), &json!({}))
                    .await
                {
                    Ok(v) if v["decision"] == "up-to-date" => {
                        say(Ok(t(T::TokensUpToDate).to_owned()))
                    }
                    Ok(_) => {
                        say(Ok(t(T::RebuildProposed).to_owned()));
                        load();
                    }
                    Err(e) => say(Err(e.to_string())),
                }
            });
        }
    };
    let pick_upload = move || {
        if mine {
            if let Some(input) = upload_ref.get_untracked() {
                input.click();
            }
        }
    };
    let on_upload = {
        let id = id.clone();
        let load = load.clone();
        move |ev: web_sys::Event| {
            let files = picked_files(&ev);
            if files.is_empty() {
                return;
            }
            let id = id.clone();
            let load = load.clone();
            leptos::task::spawn_local(async move {
                match upload::<Value>(&ds_url(&id, "assets"), &files).await {
                    Ok(_) => {
                        say(Ok(t(T::AssetsUploaded).to_owned()));
                        load();
                    }
                    Err(e) => say(Err(e)),
                }
            });
        }
    };
    let delete_asset = {
        let id = id.clone();
        let post = post.clone();
        move |name: String| {
            if mine {
                post(
                    ds_url(&id, &format!("assets/{}/delete", api::seg(&name))),
                    json!({}),
                    t(T::AssetDeleted),
                    false,
                );
            }
        }
    };
    let delete_focused = delete_asset.clone();
    let keydown = {
        let rebuild = rebuild.clone();
        move |ev: web_sys::KeyboardEvent| {
            if ev.meta_key() || ev.ctrl_key() || ev.alt_key() {
                return;
            }
            let in_field = ev
                .target()
                .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok())
                .is_some_and(|el| {
                    matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                });
            if in_field
                || editing.with_untracked(Option::is_some)
                || super::lib_ui::from_dialog(&ev)
            {
                return;
            }
            match ev.key().as_str() {
                "e" | "E" => start_edit(),
                "c" | "C" => copy_md(),
                "u" | "U" => pick_upload(),
                "r" | "R" => rebuild(),
                "Backspace" | "Delete" => {
                    if let Some(name) = focused_asset.get_untracked() {
                        delete_focused(name);
                    } else {
                        return;
                    }
                }
                _ => return,
            }
            ev.prevent_default();
        }
    };

    let set_default = {
        let id = id.clone();
        move |_| {
            let Some(mut next) = shell.config.get_untracked() else {
                return;
            };
            let is_default = default_id.get_untracked().as_deref() == Some(id.as_str());
            if is_default {
                next.extra.remove(DEFAULT_DESIGN_SYSTEM_KEY);
            } else {
                next.extra
                    .insert(DEFAULT_DESIGN_SYSTEM_KEY.to_owned(), json!(id));
            }
            leptos::task::spawn_local(async move {
                match shell.save_config(next).await {
                    Ok(()) => say(Ok(if is_default {
                        t(T::DefaultCleared)
                    } else {
                        t(T::DefaultSet)
                    }
                    .to_owned())),
                    Err(e) => say(Err(e)),
                }
            });
        }
    };
    let can_default = !mine || summary.status == DesignSystemStatus::Published;
    let id_is_default = {
        let id = StoredValue::new(id.clone());
        move || id.with_value(|id| default_id.get().as_deref() == Some(id.as_str()))
    };

    let edit_with_agent = {
        let id = id.clone();
        let navigate = navigate.clone();
        move |_| {
            let id = id.clone();
            let navigate = navigate.clone();
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(&ds_url(&id, "workspace"), &json!({})).await {
                    Ok(v) => {
                        let pid = v["project"]["id"].as_str().unwrap_or_default().to_owned();
                        navigate(&format!("/projects/{pid}"), Default::default());
                    }
                    Err(e) => say(Err(e.to_string())),
                }
            });
        }
    };
    let copy_system = {
        let id = id.clone();
        move |_| {
            let id = id.clone();
            menu.set(false);
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(&ds_url(&id, "copy"), &json!({})).await {
                    Ok(v) => on_change.run(v["id"].as_str().map(str::to_owned)),
                    Err(e) => say(Err(e.to_string())),
                }
            });
        }
    };
    let download_zip = {
        let id = id.clone();
        let title = summary.title.clone();
        move |_| {
            let id = id.clone();
            let title = title.clone();
            menu.set(false);
            leptos::task::spawn_local(async move {
                match api::get_bytes(&ds_url(&id, "archive")).await {
                    Ok(bytes) => {
                        let _ = download(&bytes, &format!("{title}.zip"), "application/zip");
                    }
                    Err(e) => say(Err(e.to_string())),
                }
            });
        }
    };
    let toggle_status = {
        let id = id.clone();
        let post = post.clone();
        let published = summary.status == DesignSystemStatus::Published;
        move |_| {
            menu.set(false);
            let next = if published { "draft" } else { "published" };
            post(
                ds_url(&id, "status"),
                json!({ "status": next }),
                t(T::Saved),
                true,
            );
        }
    };
    let confirm_delete = RwSignal::new(false);
    let delete_system = {
        let id = id.clone();
        Callback::new(move |()| {
            confirm_delete.set(false);
            let id = id.clone();
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(&ds_url(&id, "delete"), &json!({})).await {
                    Ok(_) => on_change.run(None),
                    Err(e) => say(Err(e.to_string())),
                }
            });
        })
    };
    let use_in_project = {
        let id = id.clone();
        let navigate = navigate.clone();
        move |_| navigate(&format!("/?ds={}", api::seg(&id)), Default::default())
    };
    let looks_good = {
        let id = id.clone();
        let post = post.clone();
        move |_| {
            post(
                ds_url(&id, "status"),
                json!({ "status": "published" }),
                t(T::DsApproved),
                true,
            )
        }
    };
    let send_feedback = {
        let id = id.clone();
        let post = post.clone();
        move |_| {
            let text = feedback.get_untracked();
            if text.trim().is_empty() {
                return;
            }
            let section = feedback_section.get_untracked();
            feedback_open.set(false);
            feedback.set(String::new());
            post(
                ds_url(&id, "revisions"),
                json!({ "feedback": text, "section": section, "agentId": shell.agent_id() }),
                t(T::RevisionStarted),
                false,
            );
        }
    };
    let decide = {
        let id = id.clone();
        let post = post.clone();
        Callback::new(move |(rid, accept): (String, bool)| {
            let status = if accept { "accepted" } else { "rejected" };
            post(
                ds_url(&id, &format!("revisions/{}", api::seg(&rid))),
                json!({ "status": status }),
                if accept {
                    t(T::RevisionAccepted)
                } else {
                    t(T::RevisionRejected)
                },
                false,
            );
        })
    };

    let draft = mine && summary.status == DesignSystemStatus::Draft;
    let title = summary.title.clone();
    let delete_message = tf(T::DsDeleteConfirm, &[&title]);
    let showcase_src = ds_url(&id, "showcase");
    let showcase_full = showcase_src.clone();
    let id_assets = id.clone();

    view! {
        <article class="ds-detail" data-testid="ds-detail" data-id=id.clone() tabindex="0" on:keydown=keydown>
            <header class="ds-detail-head">
                <div class="grow">
                    <h2>{title.clone()}</h2>
                    <div class="row wrap">
                        <span class="badge outline">{summary.category.clone()}</span>
                        <span class="badge outline">{surface_label(summary.surface)}</span>
                        {mine.then(|| {
                            let published = summary.status == DesignSystemStatus::Published;
                            view! {
                                <span class="badge" class:success=published data-testid="ds-status-badge">
                                    {if published { t(T::Published) } else { t(T::Draft) }}
                                </span>
                            }
                        })}
                        <Show when=id_is_default>
                            <span class="badge accent">{t(T::ChatDefault)}</span>
                        </Show>
                    </div>
                    {move || kit.with(|k| k.as_ref().and_then(|r| r.as_ref().ok()).and_then(|k| k.brand.clone())).map(|brand| view! {
                        <p class="subtle small ds-brand-note" data-testid="ds-brand-note">{tf(T::DsBrandNote, &[&brand])}</p>
                    })}
                </div>
                <div class="row wrap ds-actions">
                    <button class="btn small" data-testid="ds-use" on:click=use_in_project>{icon(Glyph::ArrowRight)}{t(T::UseInNewProject)}</button>
                    {if mine {
                        view! { <button class="btn small" data-testid="ds-edit-agent" on:click=edit_with_agent>{icon(Glyph::Agent)}{t(T::EditWithAgent)}</button> }.into_any()
                    } else {
                        view! { <button class="btn small" data-testid="ds-copy" on:click=copy_system.clone()>{icon(Glyph::Duplicate)}{t(T::CopyToYourSystems)}</button> }.into_any()
                    }}
                    <button
                        class="btn small"
                        data-testid="ds-default"
                        aria-pressed=move || id_is_default().to_string()
                        disabled=move || !can_default && !id_is_default()
                        data-tip=(!can_default).then(|| t(T::DefaultNeedsPublished))
                        on:click=set_default
                    >
                        {move || icon(if id_is_default() { Glyph::StarFill } else { Glyph::Star })}
                        {t(T::DefaultForNewChats)}
                    </button>
                    <div class="menu-wrap">
                        <button class="icon-btn" aria-label=t(T::MoreActions) data-testid="ds-more" aria-expanded=move || menu.get().to_string() on:click=move |_| menu.update(|m| *m = !*m)>
                            {icon(Glyph::More)}
                        </button>
                        <Show when=move || menu.get()>
                            <div class="menu" role="menu">
                                <button class="menu-item" role="menuitem" data-testid="ds-download" on:click=download_zip.clone()>{icon(Glyph::Download)}{t(T::DownloadZip)}</button>
                                <button class="menu-item" role="menuitem" on:click=copy_system.clone()>{icon(Glyph::Duplicate)}{t(T::DuplicateAction)}</button>
                                {mine.then(|| view! {
                                    <button class="menu-item" role="menuitem" data-testid="ds-toggle-status" on:click=toggle_status.clone()>
                                        {icon(Glyph::CheckCircle)}
                                        {if summary.status == DesignSystemStatus::Published { t(T::MarkDraft) } else { t(T::Publish) }}
                                    </button>
                                    <button class="menu-item danger" role="menuitem" data-testid="ds-delete" on:click=move |_| { menu.set(false); confirm_delete.set(true); }>{icon(Glyph::Delete)}{t(T::Delete)}</button>
                                })}
                            </div>
                        </Show>
                    </div>
                </div>
            </header>
            {summary_for_people(&summary.description).map(|text| view! { <p class="muted ds-summary">{text}</p> })}
            <Show when=move || confirm_delete.get()>
                <super::lib_ui::ConfirmBar message=delete_message.clone() testid="ds-delete-confirm"
                    on_confirm=delete_system on_cancel=Callback::new(move |()| confirm_delete.set(false))/>
            </Show>
            {move || notice.get().map(|n| match n {
                Ok(m) => view! { <p class="ok-text" role="status" data-testid="ds-notice">{icon(Glyph::Check)}{m}</p> }.into_any(),
                Err(e) => view! { <p class="error-text" role="alert" data-testid="ds-notice">{e}</p> }.into_any(),
            })}
            {draft.then(|| view! {
                <div class="card padded ds-review" data-testid="ds-review">
                    <div class="grow">
                        <strong>{t(T::DsReviewTitle)}</strong>
                        <p class="muted">{t(T::DsReviewBody)}</p>
                    </div>
                    <div class="row">
                        <button class="btn small primary" data-testid="ds-looks-good" on:click=looks_good.clone()>{icon(Glyph::Check)}{t(T::LooksGood)}</button>
                        <button class="btn small" data-testid="ds-needs-work" on:click=move |_| feedback_open.update(|o| *o = !*o)>{icon(Glyph::Comment)}{t(T::NeedsWork)}</button>
                    </div>
                </div>
            })}
            <Show when=move || feedback_open.get()>
                <div class="card padded ds-feedback" data-testid="ds-feedback">
                    <label class="field">
                        <span>{t(T::Section)}</span>
                        <select class="select-auto" prop:value=move || feedback_section.get() on:change=move |ev| feedback_section.set(event_target_value(&ev))>
                            <option value="">{t(T::WholeSystem)}</option>
                            {move || kit.with(|k| match k { Some(Ok(k)) => sections(&k.body), _ => Vec::new() })
                                .into_iter()
                                .map(|s| view! { <option value=s.clone()>{s.clone()}</option> })
                                .collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span>{t(T::WhatNeedsWork)}</span>
                        <textarea class="input" rows="3" data-testid="ds-feedback-text" placeholder=t(T::FeedbackPlaceholder)
                            prop:value=move || feedback.get() on:input=move |ev| feedback.set(event_target_value(&ev))></textarea>
                    </label>
                    <div class="row end">
                        <button class="btn small ghost" on:click=move |_| feedback_open.set(false)>{t(T::Cancel)}</button>
                        <button class="btn small primary" data-testid="ds-feedback-send" disabled=move || feedback.with(|f| f.trim().is_empty()) on:click=send_feedback.clone()>
                            {icon(Glyph::Send)}{t(T::ReviseWithAgent)}
                        </button>
                    </div>
                </div>
            </Show>
            {move || kit.with(|k| match k {
                Some(Ok(k)) => k.revisions.iter().filter(|r| matches!(r.status.as_str(), "running" | "pending" | "failed")).cloned().collect::<Vec<_>>(),
                _ => Vec::new(),
            }).into_iter().map(|r| view! { <RevisionCard revision=r decide=decide/> }).collect_view()}
            <div class="row kit-tabs">
                <div class="segmented" role="tablist">
                {[(KitTab::Kit, T::KitTab, "kit"), (KitTab::Showcase, T::Showcase, "showcase"), (KitTab::Tokens, T::Tokens, "tokens"), (KitTab::Design, T::DesignMd, "design-md")]
                    .into_iter()
                    .map(|(k, label, key)| view! {
                        <button class="seg" role="tab" data-testid=format!("kit-tab-{key}") class:active=move || tab.get() == k aria-selected=move || (tab.get() == k).to_string() on:click=move |_| tab.set(k)>
                            <span class="seg-label">{t(label)}</span>
                        </button>
                    })
                    .collect_view()}
                </div>
                <span class="spacer"></span>
                <span class="subtle small kit-keys">{if mine { t(T::KitKeysMine) } else { t(T::KitKeys) }}</span>
            </div>
            <input type="file" class="visually-hidden" aria-label=t(T::UploadFiles) multiple accept="image/*,.svg,.woff,.woff2,.ttf,.otf" node_ref=upload_ref data-testid="ds-asset-input" on:change=on_upload/>
            {move || match kit.get() {
                None => view! { <div class="skeleton kit-skeleton"></div> }.into_any(),
                Some(Err(e)) => view! { <p class="error-text">{e}</p> }.into_any(),
                Some(Ok(k)) => match tab.get() {
                    KitTab::Kit => view! {
                        // Track C2: no brand font files yet.
                        <super::font_banner::MissingBrandFontsBanner
                            id=id_assets.clone()
                            editable=mine
                            asset_names=k.assets.iter().map(|a| a.name.clone()).collect()
                            on_upload=Callback::new(move |()| pick_upload())
                        />
                        <KitOverview kit=k.clone() id=id_assets.clone() mine=mine focused_asset=focused_asset
                            on_upload=Callback::new(move |()| pick_upload())
                            on_rebuild=Callback::new({ let rebuild = rebuild.clone(); move |()| rebuild() })
                            on_delete_asset=Callback::new({ let d = delete_asset.clone(); move |n: String| d(n) })
                            on_preview=Callback::new(move |src: String| fullscreen.set(Some(src)))
                        />
                    }.into_any(),
                    KitTab::Showcase => {
                        let src = showcase_src.clone();
                        let full = showcase_full.clone();
                        view! {
                            <div class="kit-stage">
                                <div class="row end">
                                    <button class="btn small ghost" data-testid="ds-fullscreen" on:click=move |_| fullscreen.set(Some(full.clone()))>{icon(Glyph::Fullscreen)}{t(T::LargePreview)}</button>
                                </div>
                                <iframe class="kit-frame" sandbox="" src=src title=t(T::Showcase) data-testid="ds-showcase"></iframe>
                            </div>
                        }.into_any()
                    }
                    KitTab::Tokens => view! { <TokenTable tokens=k.tokens.clone()/> }.into_any(),
                    KitTab::Design => {
                        let save = save_edit.clone();
                        match editing.get() {
                            Some(_) => view! {
                                <div class="kit-editor">
                                    <textarea class="input mono" rows="22" data-testid="ds-md-editor" aria-label=t(T::DesignMd)
                                        prop:value=move || editing.get().unwrap_or_default()
                                        on:input=move |ev| editing.set(Some(event_target_value(&ev)))></textarea>
                                    <div class="row end">
                                        <button class="btn small ghost" on:click=move |_| editing.set(None)>{t(T::Cancel)}</button>
                                        <button class="btn small primary" data-testid="ds-md-save" on:click=move |_| save()>{t(T::Save)}</button>
                                    </div>
                                </div>
                            }.into_any(),
                            None => view! {
                                <div class="kit-md">
                                    <div class="row end">
                                        <button class="btn small ghost" data-testid="ds-md-copy" on:click=move |_| copy_md()>{icon(Glyph::Duplicate)}{t(T::Copy)}</button>
                                        {mine.then(|| view! { <button class="btn small" data-testid="ds-md-edit" on:click=move |_| start_edit()>{icon(Glyph::Rename)}{t(T::Edit)}</button> })}
                                    </div>
                                    <div class="md" data-testid="ds-md" inner_html=crate::markdown::to_html(&k.body)></div>
                                </div>
                            }.into_any(),
                        }
                    }
                },
            }}
            {move || fullscreen.get().map(|src| view! {
                <Modal title=title.clone() glyph=Glyph::Fullscreen wide=true testid="ds-preview-modal" on_close=Callback::new(move |()| fullscreen.set(None))>
                    <iframe class="kit-frame large" sandbox="" src=src title=t(T::LargePreview)></iframe>
                </Modal>
            })}
        </article>
    }
}

#[component]
fn RevisionCard(revision: Revision, decide: Callback<(String, bool)>) -> impl IntoView {
    let rid = revision.id.clone();
    let rid2 = rid.clone();
    let title = if revision.kind == "token-rebuild" {
        t(T::TokenRebuildRevision).to_owned()
    } else {
        tf(
            T::RevisionFor,
            &[revision.section.as_deref().unwrap_or(t(T::WholeSystem))],
        )
    };
    let status = revision.status.clone();
    view! {
        <div class="card padded ds-revision" data-testid="ds-revision" data-status=status.clone()>
            <div class="row">
                <span class="grow"><strong>{title}</strong></span>
                {match status.as_str() {
                    "running" => view! { <span class="badge"><span class="spinner"></span>{t(T::AgentRevising)}</span> }.into_any(),
                    "failed" => view! { <span class="badge danger">{t(T::Failed)}</span> }.into_any(),
                    _ => view! { <span class="badge warning">{t(T::AwaitingReview)}</span> }.into_any(),
                }}
            </div>
            {revision.feedback.map(|f| view! { <p class="muted">"“"{f}"”"</p> })}
            {revision.error.map(|e| view! { <p class="error-text">{e}</p> })}
            {(status == "pending").then(|| view! {
                <details class="ds-proposal">
                    <summary>{t(T::ShowProposedChanges)}</summary>
                    {revision.files.into_iter().map(|f| view! {
                        <p class="small strong">{f.name}</p>
                        <pre class="code-block">{f.content}</pre>
                    }).collect_view()}
                </details>
                <div class="row end">
                    <button class="btn small ghost danger" data-testid="ds-revision-reject" on:click=move |_| decide.run((rid.clone(), false))>{t(T::Reject)}</button>
                    <button class="btn small primary" data-testid="ds-revision-accept" on:click=move |_| decide.run((rid2.clone(), true))>{icon(Glyph::Check)}{t(T::Accept)}</button>
                </div>
            })}
        </div>
    }
}

#[component]
fn KitOverview(
    kit: Kit,
    id: String,
    mine: bool,
    focused_asset: RwSignal<Option<String>>,
    on_upload: Callback<()>,
    on_rebuild: Callback<()>,
    on_delete_asset: Callback<String>,
    on_preview: Callback<String>,
) -> impl IntoView {
    let colors: Vec<(String, String)> = if kit.palette.is_empty() {
        kit.tokens
            .iter()
            .filter(|t| t.group == "color")
            .take(12)
            .map(|t| (t.name.clone(), t.value.clone()))
            .collect()
    } else {
        kit.palette
            .iter()
            .map(|s| (s.name.clone(), s.hex.clone()))
            .collect()
    };
    let font = |n: &str| {
        kit.tokens
            .iter()
            .find(|t| t.name == n)
            .map(|t| t.value.clone())
    };
    let display = font("--font-display");
    let body = font("--font-body");
    let score = kit.score.clone();
    let missing = score.missing_a1.len() + score.missing_a2.len();
    let assets = kit.assets.clone();
    let previews = kit.previews.clone();
    view! {
        <div class="kit-grid">
            <section class="card padded kit-module">
                <h3>{icon(Glyph::Palette)}{t(T::Palette)}</h3>
                <div class="kit-swatches" data-testid="kit-palette">
                    {colors.into_iter().map(|(name, value)| view! {
                        <figure class="kit-swatch">
                            <span style=format!("background:{value}")></span>
                            <figcaption><strong>{name}</strong><code>{value}</code></figcaption>
                        </figure>
                    }).collect_view()}
                </div>
            </section>
            <section class="card padded kit-module">
                <h3>{icon(Glyph::EditText)}{t(T::Typography)}</h3>
                {display.map(|f| view! {
                    <p class="kit-type-sample display" style=format!("font-family:{f}")>"Aa Display"</p>
                    <code class="small">{f}</code>
                })}
                {body.map(|f| view! {
                    <p class="kit-type-sample" style=format!("font-family:{f}")>{t(T::TypeSample)}</p>
                    <code class="small">{f}</code>
                })}
            </section>
            <section class="card padded kit-module" data-testid="kit-score">
                <h3>{icon(Glyph::LintOk)}{t(T::TokenContract)}</h3>
                <p class="kit-score">
                    <span class="kit-score-num">{score.score}</span>
                    <span class="badge" class:success=score.grade == "excellent" class:warning=score.recommend_rebuild>{grade_label(&score.grade)}</span>
                </p>
                <p class="muted small">{tf(T::MissingTokens, &[&missing.to_string()])}</p>
                {(mine && missing > 0).then(|| view! {
                    <button class="btn small" data-testid="kit-rebuild" on:click=move |_| on_rebuild.run(())>{icon(Glyph::Refresh)}{t(T::RebuildTokens)}</button>
                })}
            </section>
            <section class="card padded kit-module wide" data-testid="kit-assets">
                <div class="row">
                    <h3 class="grow">{icon(Glyph::Image)}{t(T::BrandAssets)}</h3>
                    {mine.then(|| view! { <button class="btn small" data-testid="kit-upload" on:click=move |_| on_upload.run(())>{icon(Glyph::Upload)}{t(T::UploadLogoOrFont)}</button> })}
                </div>
                {if assets.is_empty() {
                    view! { <p class="muted small">{if mine { t(T::NoAssetsMine) } else { t(T::NoAssets) }}</p> }.into_any()
                } else {
                    view! {
                        <ul class="kit-assets">
                            {assets.into_iter().map(|a| {
                                let src = format!("/api/design-systems/{}/file/assets/{}", api::seg(&id), api::seg(&a.name));
                                let name = a.name.clone();
                                let name_focus = a.name.clone();
                                let name_del = a.name.clone();
                                let is_font = a.kind == "font";
                                view! {
                                    <li class="kit-asset" tabindex="0" data-testid="kit-asset"
                                        on:focus=move |_| focused_asset.set(Some(name_focus.clone()))
                                        on:blur=move |_| focused_asset.set(None)>
                                        {if is_font {
                                            view! { <span class="kit-asset-thumb font">"Aa"</span> }.into_any()
                                        } else {
                                            view! { <img class="kit-asset-thumb" src=src alt=name.clone()/> }.into_any()
                                        }}
                                        <span class="kit-asset-name">{name.clone()}</span>
                                        <span class="subtle small">{size_label(a.size)}</span>
                                        {mine.then(|| view! {
                                            <button class="icon-btn small" aria-label=tf(T::DeleteNamed, &[&name]) on:click=move |_| on_delete_asset.run(name_del.clone())>{icon(Glyph::Delete)}</button>
                                        })}
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }}
            </section>
            {(!previews.is_empty()).then(|| view! {
                <section class="card padded kit-module wide">
                    <h3>{icon(Glyph::Eye)}{t(T::PreviewPages)}</h3>
                    <div class="row wrap">
                        {previews.into_iter().map(|p| {
                            let src = format!("/api/design-systems/{}/file/preview/{}", api::seg(&kit.summary.id), api::seg(&p));
                            view! { <button class="btn small ghost" on:click=move |_| on_preview.run(src.clone())>{icon(Glyph::FileCode)}{p}</button> }
                        }).collect_view()}
                    </div>
                </section>
            })}
        </div>
    }
}

#[component]
fn TokenTable(tokens: Vec<Token>) -> impl IntoView {
    let groups = [
        "color", "font", "type", "space", "radius", "shadow", "motion", "other",
    ];
    view! {
        <div class="token-groups" data-testid="kit-tokens">
            {groups.into_iter().filter_map(|g| {
                let rows: Vec<Token> = tokens.iter().filter(|t| t.group == g).cloned().collect();
                (!rows.is_empty()).then(|| view! {
                    <section class="card padded">
                        <h3 class="token-group">{g.to_owned()}<span class="count-badge">{rows.len()}</span></h3>
                        <table class="token-table">
                            <tbody>
                                {rows.into_iter().map(|t| view! {
                                    <tr>
                                        <td><code>{t.name.clone()}</code></td>
                                        <td class="token-value">
                                            {(g == "color").then(|| view! { <i class="token-chip" style=format!("background:{}", t.value)></i> })}
                                            <code>{t.value.clone()}</code>
                                        </td>
                                    </tr>
                                }).collect_view()}
                            </tbody>
                        </table>
                    </section>
                })
            }).collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_package_note_is_not_shown() {
        assert_eq!(
            summary_for_people(
                "Bundled OpenDesign package for Airbnb, derived from curated DESIGN.md, tokens.css, and components.html fixtures."
            ),
            None
        );
        assert_eq!(summary_for_people("  "), None);
        assert_eq!(
            summary_for_people("Calm and precise.").as_deref(),
            Some("Calm and precise.")
        );
    }

    #[test]
    fn search_reads_the_description_not_the_package_note() {
        let sys: DesignSystemSummary = serde_json::from_value(serde_json::json!({
            "id": "airbnb", "title": "Airbnb", "category": "E-Commerce & Retail",
            "summary": "Bundled OpenDesign package for Airbnb, derived from curated DESIGN.md.",
            "swatches": [], "surface": "web", "source": "built-in", "status": "published",
            "isEditable": false, "description": "Warm coral accent on a clean white canvas."
        }))
        .unwrap();
        assert!(matches(&sys, "Coral"));
        assert!(matches(&sys, "airbnb"));
        assert!(matches(&sys, "retail"));
        assert!(!matches(&sys, "package"));
        assert!(!matches(&sys, "curated"));
    }

    #[test]
    fn sections_strip_numbers() {
        assert_eq!(
            sections("# T\n## 1. Visual Theme\ntext\n## Color\n### sub\n"),
            ["Visual Theme", "Color"]
        );
    }
}
