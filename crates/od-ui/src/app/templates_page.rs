//! Templates (spec 1.4): design templates with filters (type, platform,
//! scenario, search) and a large preview (fullscreen, Use, Remix, open in a
//! new tab, download); image and video prompt templates (Copy prompt,
//! Generate); and templates the user saved from a project.

use super::api;
use super::export_menu::download;
use super::icons::{Glyph, icon};
use super::lazy_frame::LazyFrame;
use super::lib_ui::{Modal, copy_text, query_param};
use crate::i18n::{T, locale, t, tf};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    DesignTemplatesResponse, Platform, Project, ProjectsResponse, SkillMode, SkillSummary,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Design,
    Image,
    Video,
    Mine,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PromptTemplate {
    pub id: String,
    pub surface: String,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub aspect: Option<String>,
    pub source: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source_project_id: Option<String>,
    pub file_count: usize,
    pub created_at: i64,
}

fn mode_key(m: SkillMode) -> String {
    serde_json::to_value(m)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn mode_label(m: &str) -> String {
    match m {
        "prototype" => t(T::ChipPrototype).to_owned(),
        "deck" => t(T::ChipDeck).to_owned(),
        "template" => t(T::TemplateKind).to_owned(),
        "image" => t(T::Image).to_owned(),
        "video" => t(T::Video).to_owned(),
        "audio" => t(T::AudioKind).to_owned(),
        other => other.to_owned(),
    }
}

fn name_of(s: &SkillSummary) -> String {
    let name = s
        .info
        .display_name
        .as_ref()
        .and_then(|m| m.get(locale().tag()))
        .cloned()
        .unwrap_or_else(|| s.info.name.clone());
    if name.contains(' ') || !name.contains('-') {
        return name;
    }
    let words = name.replace('-', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or(words)
}

fn description_of(s: &SkillSummary) -> String {
    s.info
        .description_i18n
        .as_ref()
        .and_then(|m| m.get(locale().tag()))
        .cloned()
        .unwrap_or_else(|| s.info.description.clone())
}

fn example_url(id: &str) -> String {
    format!("/api/design-templates/{}/example", api::seg(id))
}

#[component]
pub fn TemplatesPage() -> impl IntoView {
    let tab = RwSignal::new(match query_param("tab").as_deref() {
        Some("image") => Tab::Image,
        Some("video") => Tab::Video,
        Some("mine") => Tab::Mine,
        _ => Tab::Design,
    });
    let templates = LocalResource::new(|| async {
        api::get::<DesignTemplatesResponse>("/api/design-templates")
            .await
            .map(|r| r.design_templates)
            .unwrap_or_default()
    });
    let prompts = LocalResource::new(|| async {
        api::get::<Value>("/api/prompt-templates")
            .await
            .ok()
            .and_then(|v| {
                serde_json::from_value::<Vec<PromptTemplate>>(v["promptTemplates"].clone()).ok()
            })
            .unwrap_or_default()
    });
    let tab_button = move |k: Tab, label: T, key: &'static str| {
        view! {
            <button class="seg" role="tab" data-testid=format!("tpl-tab-{key}") class:active=move || tab.get() == k aria-selected=move || (tab.get() == k).to_string() on:click=move |_| tab.set(k)>
                <span class="seg-label">{t(label)}</span>
            </button>
        }
    };
    view! {
        <section class="page wide lib-page" data-testid="templates-page">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavTemplates)}</h1>
                    <p class="page-sub">{t(T::TemplatesPageSub)}</p>
                </div>
            </header>
            <div class="segmented lib-tabs" role="tablist">
                {tab_button(Tab::Design, T::DesignTemplates, "design")}
                {tab_button(Tab::Image, T::ImagePrompts, "image")}
                {tab_button(Tab::Video, T::VideoPrompts, "video")}
                {tab_button(Tab::Mine, T::MyTemplates, "mine")}
            </div>
            {move || match tab.get() {
                Tab::Design => view! { <DesignTemplates templates=templates/> }.into_any(),
                Tab::Image => view! { <PromptGallery prompts=prompts surface="image"/> }.into_any(),
                Tab::Video => view! { <PromptGallery prompts=prompts surface="video"/> }.into_any(),
                Tab::Mine => view! { <MyTemplates/> }.into_any(),
            }}
        </section>
    }
}

#[component]
fn DesignTemplates(templates: LocalResource<Vec<SkillSummary>>) -> impl IntoView {
    let mode = RwSignal::new(String::new());
    let platform = RwSignal::new(String::new());
    let scenario = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let preview = RwSignal::new(None::<SkillSummary>);

    let all = move || templates.get().unwrap_or_default();
    let modes = Memo::new(move |_| {
        let mut m: Vec<String> = all().iter().map(|s| mode_key(s.info.mode)).collect();
        m.sort();
        m.dedup();
        m
    });
    let scenarios = Memo::new(move |_| {
        let mut m: Vec<String> = all()
            .iter()
            .map(|s| s.info.scenario.clone())
            .filter(|s| !s.is_empty())
            .collect();
        m.sort();
        m.dedup();
        m
    });
    let visible = Memo::new(move |_| {
        let (md, pf, sc) = (mode.get(), platform.get(), scenario.get());
        let q = query.get().to_lowercase();
        let mut list: Vec<SkillSummary> = all()
            .into_iter()
            .filter(|s| md.is_empty() || mode_key(s.info.mode) == md)
            .filter(|s| match pf.as_str() {
                "mobile" => s.info.platform == Some(Platform::Mobile),
                "desktop" => s.info.platform != Some(Platform::Mobile),
                _ => true,
            })
            .filter(|s| sc.is_empty() || s.info.scenario == sc)
            .filter(|s| {
                q.is_empty()
                    || format!("{} {}", name_of(s), description_of(s))
                        .to_lowercase()
                        .contains(&q)
            })
            .collect();
        list.sort_by(|a, b| {
            let fa = a.info.featured.unwrap_or(f64::MAX);
            let fb = b.info.featured.unwrap_or(f64::MAX);
            fa.total_cmp(&fb)
                .then_with(|| a.info.name.cmp(&b.info.name))
        });
        list
    });

    view! {
        <div class="chips lib-chips" role="radiogroup" aria-label=t(T::TemplateType)>
            <button class="chip" role="radio" aria-checked=move || mode.get().is_empty().to_string() class:active=move || mode.get().is_empty() on:click=move |_| mode.set(String::new())>{t(T::AllFilter)}</button>
            {move || modes.get().into_iter().map(|m| {
                let a = m.clone();
                let b = m.clone();
                let c = m.clone();
                view! {
                    <button class="chip" role="radio" data-testid="tpl-type" aria-checked=move || (mode.get() == a).to_string() class:active=move || mode.get() == b on:click=move |_| mode.set(c.clone())>{mode_label(&m)}</button>
                }
            }).collect_view()}
        </div>
        <div class="lib-toolbar">
            <label class="lib-search">
                {icon(Glyph::Search)}
                <input class="input small" type="search" data-testid="tpl-search" placeholder=t(T::SearchTemplates) aria-label=t(T::SearchTemplates)
                    prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            </label>
            <div class="segmented small" role="group" aria-label=t(T::Platform)>
                {[("", T::AllFilter, Glyph::Grid), ("desktop", T::Desktop, Glyph::Desktop), ("mobile", T::Mobile, Glyph::Phone)].into_iter().map(|(p, label, glyph)| view! {
                    <button class="seg" data-testid=format!("tpl-platform-{}", if p.is_empty() { "all" } else { p }) aria-pressed=move || (platform.get() == p).to_string() on:click=move |_| platform.set(p.to_owned())>{icon(glyph)}{t(label)}</button>
                }).collect_view()}
            </div>
            <select class="select-auto small" data-testid="tpl-scenario" aria-label=t(T::Industry) prop:value=move || scenario.get() on:change=move |ev| scenario.set(event_target_value(&ev))>
                <option value="">{t(T::AllIndustries)}</option>
                {move || scenarios.get().into_iter().map(|s| {
                    let label = super::lib_ui::localized(&json!(s.replace('-', " "))).unwrap_or_default();
                    view! { <option value=s.clone()>{label}</option> }
                }).collect_view()}
            </select>
            <span class="count-badge">{move || tf(T::TemplatesCount, &[&visible.with(Vec::len).to_string()])}</span>
        </div>
        <div class="template-grid lib-template-grid" data-testid="tpl-grid">
            {move || match templates.get() {
                None => (0..8).map(|_| view! { <div class="skeleton template-skeleton"></div> }).collect_view().into_any(),
                Some(_) if visible.with(Vec::is_empty) => view! { <p class="empty">{t(T::NoMatches)}</p> }.into_any(),
                Some(_) => visible.get().into_iter().map(|s| {
                    let open = s.clone();
                    let open_key = s.clone();
                    let name = name_of(&s);
                    let label = name.clone();
                    let thumb = s.has_example.then(|| example_url(&s.info.id));
                    let tint = format!("cover tint-{}", super::lib_ui::tint(&s.info.id));
                    let has_thumb = thumb.is_some();
                    view! {
                        <div class="template-card" role="button" tabindex="0" data-testid="tpl-card" aria-label=label
                            on:click=move |_| preview.set(Some(open.clone()))
                            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" || ev.key() == " " { ev.prevent_default(); preview.set(Some(open_key.clone())); }>
                            <span class=tint class:thumb-cover=has_thumb aria-hidden="true">
                                {match thumb {
                                    Some(src) => view! { <LazyFrame src=src/> }.into_any(),
                                    None => view! { <span class="cover-glyph">{icon(Glyph::Grid)}</span> }.into_any(),
                                }}
                            </span>
                            <span class="template-body">
                                <span class="template-name">{name}</span>
                                <span class="template-desc">{description_of(&s)}</span>
                                <span class="template-meta">{mode_label(&mode_key(s.info.mode))}</span>
                            </span>
                        </div>
                    }
                }).collect_view().into_any(),
            }}
        </div>
        {move || preview.get().map(|s| view! { <TemplatePreview template=s on_close=Callback::new(move |()| preview.set(None))/> })}
    }
}

#[component]
fn TemplatePreview(template: SkillSummary, on_close: Callback<()>) -> impl IntoView {
    let navigate = use_navigate();
    let id = template.info.id.clone();
    let name = name_of(&template);
    let src = example_url(&id);
    let stage = NodeRef::<leptos::html::Div>::new();
    let notice = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let prompt = template
        .info
        .example_prompt_i18n
        .as_ref()
        .and_then(|m| m.get(locale().tag()))
        .cloned()
        .unwrap_or_else(|| template.info.example_prompt.clone());

    let use_it = {
        let id = id.clone();
        let navigate = navigate.clone();
        move |_| navigate(&format!("/?template={}", api::seg(&id)), Default::default())
    };
    let remix = {
        let id = id.clone();
        let navigate = navigate.clone();
        move |_| {
            let id = id.clone();
            let navigate = navigate.clone();
            busy.set(true);
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(
                    &format!("/api/design-templates/{}/remix", api::seg(&id)),
                    &json!({}),
                )
                .await
                {
                    Ok(v) => navigate(
                        &format!(
                            "/projects/{}",
                            v["project"]["id"].as_str().unwrap_or_default()
                        ),
                        Default::default(),
                    ),
                    Err(e) => {
                        busy.set(false);
                        notice.set(Some(e.to_string()));
                    }
                }
            });
        }
    };
    let open_tab = {
        let src = src.clone();
        move |_| {
            if let Some(w) = web_sys::window() {
                let _ = w.open_with_url_and_target(&src, "_blank");
            }
        }
    };
    let save_html = {
        let src = src.clone();
        let name = name.clone();
        move |_| {
            let src = src.clone();
            let name = name.clone();
            leptos::task::spawn_local(async move {
                match api::get_bytes(&src).await {
                    Ok(bytes) => {
                        let _ = download(&bytes, &format!("{name}.html"), "text/html");
                    }
                    Err(e) => notice.set(Some(e.to_string())),
                }
            });
        }
    };
    let fullscreen = move |_| {
        if let Some(el) = stage.get_untracked() {
            let _ = el.request_fullscreen();
        }
    };
    let copy_prompt = {
        let prompt = prompt.clone();
        move |_| {
            let p = prompt.clone();
            leptos::task::spawn_local(async move {
                notice.set(Some(match copy_text(&p).await {
                    Ok(()) => t(T::Copied).to_owned(),
                    Err(e) => e,
                }));
            });
        }
    };
    view! {
        <Modal title=name.clone() subtitle=description_of(&template) glyph=Glyph::Grid wide=true testid="tpl-preview" on_close=on_close>
            <div class="dialog-body tpl-preview-body">
                <div class="tpl-stage" node_ref=stage>
                    {if template.has_example {
                        view! { <iframe class="kit-frame large" sandbox="" src=src.clone() title=name.clone() data-testid="tpl-frame"></iframe> }.into_any()
                    } else {
                        view! { <div class="empty-state"><span class="empty-icon">{icon(Glyph::Grid)}</span><p>{t(T::NoExample)}</p></div> }.into_any()
                    }}
                </div>
                <aside class="tpl-side">
                    <div class="row wrap">
                        <span class="badge outline">{mode_label(&mode_key(template.info.mode))}</span>
                        {(!template.info.scenario.is_empty()).then(|| view! { <span class="badge outline">{template.info.scenario.clone()}</span> })}
                    </div>
                    {(!prompt.is_empty()).then(|| view! {
                        <h4>{t(T::ExampleBrief)}</h4>
                        <p class="prompt-preview">{prompt.clone()}</p>
                        <button class="btn small ghost" on:click=copy_prompt.clone()>{icon(Glyph::Duplicate)}{t(T::CopyPrompt)}</button>
                    })}
                    {move || notice.get().map(|n| view! { <p class="subtle small" role="status">{n}</p> })}
                </aside>
            </div>
            <div class="dialog-foot">
                {template.has_example.then(|| view! {
                    <button class="btn ghost" data-testid="tpl-fullscreen" on:click=fullscreen>{icon(Glyph::Fullscreen)}{t(T::Fullscreen)}</button>
                    <button class="btn ghost" data-testid="tpl-open-tab" on:click=open_tab.clone()>{icon(Glyph::ExternalLink)}{t(T::OpenInNewTab)}</button>
                    <button class="btn ghost" data-testid="tpl-download" on:click=save_html.clone()>{icon(Glyph::Download)}{t(T::DownloadHtml)}</button>
                })}
                <span class="spacer"></span>
                {template.has_example.then(|| view! {
                    <button class="btn" data-testid="tpl-remix" disabled=move || busy.get() on:click=remix.clone()>{icon(Glyph::Fork)}{t(T::Remix)}</button>
                })}
                <button class="btn primary" data-testid="tpl-use" on:click=use_it>{icon(Glyph::ArrowRight)}{t(T::UseTemplate)}</button>
            </div>
        </Modal>
    }
}

#[component]
fn PromptGallery(
    prompts: LocalResource<Vec<PromptTemplate>>,
    surface: &'static str,
) -> impl IntoView {
    let navigate = use_navigate();
    let category = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let copied = RwSignal::new(None::<String>);
    let mine = move || {
        prompts
            .get()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.surface == surface)
            .collect::<Vec<_>>()
    };
    let categories = Memo::new(move |_| {
        let mut c: Vec<String> = mine().into_iter().map(|p| p.category).collect();
        c.sort();
        c.dedup();
        c
    });
    view! {
        <div class="lib-toolbar">
            <label class="lib-search">
                {icon(Glyph::Search)}
                <input class="input small" type="search" data-testid="prompt-search" placeholder=t(T::SearchPrompts) aria-label=t(T::SearchPrompts)
                    prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            </label>
            <select class="select-auto small" aria-label=t(T::Category) prop:value=move || category.get() on:change=move |ev| category.set(event_target_value(&ev))>
                <option value="">{t(T::AllCategories)}</option>
                {move || categories.get().into_iter().map(|c| view! { <option value=c.clone()>{c.clone()}</option> }).collect_view()}
            </select>
        </div>
        <div class="prompt-grid" data-testid="prompt-grid">
            {move || {
                let q = query.get().to_lowercase();
                let cat = category.get();
                mine().into_iter()
                    .filter(|p| cat.is_empty() || p.category == cat)
                    .filter(|p| q.is_empty() || format!("{} {} {}", p.title, p.summary, p.tags.join(" ")).to_lowercase().contains(&q))
                    .map(|p| {
                        let id = p.id.clone();
                        let path = format!("/api/prompt-templates/{}/{}", surface, api::seg(&p.id));
                        let go = format!("/media?template={}/{}", surface, api::seg(&p.id));
                        let navigate = navigate.clone();
                        let license = p.source["license"].as_str().unwrap_or_default().to_owned();
                        let author = p.source["author"].as_str().map(str::to_owned);
                        let id_copied = id.clone();
                        view! {
                            <article class="card padded prompt-card" data-testid="prompt-card">
                                <span class=format!("prompt-cover tint-{}", super::lib_ui::tint(&p.id)) aria-hidden="true">{icon(if surface == "video" { Glyph::Video } else { Glyph::Image })}</span>
                                <h3>{p.title.clone()}</h3>
                                <p class="muted small">{p.summary.clone()}</p>
                                <div class="row wrap">
                                    <span class="badge outline">{p.category.clone()}</span>
                                    {p.aspect.clone().map(|a| view! { <span class="badge outline">{a}</span> })}
                                    {p.model.clone().map(|m| view! { <span class="badge outline">{m}</span> })}
                                </div>
                                <p class="subtle small">{match author { Some(a) => format!("{a} · {license}"), None => license }}</p>
                                <div class="row end">
                                    <button class="btn small ghost" data-testid="prompt-copy" on:click=move |_| {
                                        let path = path.clone();
                                        let id = id.clone();
                                        leptos::task::spawn_local(async move {
                                            if let Ok(v) = api::get::<Value>(&path).await {
                                                if copy_text(v["promptTemplate"]["prompt"].as_str().unwrap_or_default()).await.is_ok() {
                                                    copied.set(Some(id));
                                                }
                                            }
                                        });
                                    }>
                                        {move || if copied.get().as_deref() == Some(id_copied.as_str()) { view! { {icon(Glyph::Check)}{t(T::Copied)} }.into_any() } else { view! { {icon(Glyph::Duplicate)}{t(T::CopyPrompt)} }.into_any() }}
                                    </button>
                                    <button class="btn small" data-testid="prompt-generate" on:click=move |_| navigate(&go, Default::default())>{icon(Glyph::Sparkle)}{t(T::Generate)}</button>
                                </div>
                            </article>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
}

#[component]
fn MyTemplates() -> impl IntoView {
    let navigate = use_navigate();
    let saved = RwSignal::new(None::<Vec<Saved>>);
    let projects = RwSignal::new(Vec::<Project>::new());
    let project = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let reload = move || {
        leptos::task::spawn_local(async move {
            match api::get::<Value>("/api/templates").await {
                Ok(v) => saved.set(serde_json::from_value(v["templates"].clone()).ok()),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    reload();
    leptos::task::spawn_local(async move {
        if let Ok(r) = api::get::<ProjectsResponse>("/api/projects").await {
            projects.set(r.projects);
        }
    });
    let save = move |_| {
        let body = json!({
            "name": name.get_untracked(),
            "description": description.get_untracked(),
            "sourceProjectId": project.get_untracked(),
        });
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/templates", &body).await {
                Ok(_) => {
                    error.set(None);
                    name.set(String::new());
                    description.set(String::new());
                    reload();
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    view! {
        <div class="card padded form-card" data-testid="tpl-save-form">
            <h2 class="card-title">{icon(Glyph::Add)}{t(T::SaveProjectAsTemplate)}</h2>
            <div class="gen-options">
                <label class="field">
                    <span>{t(T::ProjectLabel)}</span>
                    <select class="input" data-testid="tpl-save-project" prop:value=move || project.get() on:change=move |ev| {
                        let v = event_target_value(&ev);
                        if name.with_untracked(String::is_empty) {
                            if let Some(p) = projects.with_untracked(|l| l.iter().find(|p| p.id == v).cloned()) {
                                name.set(p.name);
                            }
                        }
                        project.set(v);
                    }>
                        <option value="">{t(T::ChooseProject)}</option>
                        {move || projects.get().into_iter().map(|p| view! { <option value=p.id.clone()>{p.name.clone()}</option> }).collect_view()}
                    </select>
                </label>
                <label class="field">
                    <span>{t(T::TemplateName)}</span>
                    <input class="input" data-testid="tpl-save-name" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev))/>
                </label>
            </div>
            <label class="field">
                <span>{t(T::Description)}</span>
                <input class="input" data-testid="tpl-save-description" prop:value=move || description.get() on:input=move |ev| description.set(event_target_value(&ev))/>
            </label>
            {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
            <div class="row end">
                <button class="btn primary" data-testid="tpl-save" disabled=move || project.with(String::is_empty) || name.with(|n| n.trim().is_empty()) on:click=save>{t(T::SaveAsTemplate)}</button>
            </div>
        </div>
        <ul class="routine-list saved-list" data-testid="tpl-saved">
            {move || match saved.get() {
                None => view! { <li class="skeleton routine-skeleton"></li> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <li class="empty-state compact"><span class="empty-icon">{icon(Glyph::Grid)}</span><p>{t(T::NoSavedTemplates)}</p></li>
                }.into_any(),
                Some(list) => list.into_iter().map(|s| {
                    let id = s.id.clone();
                    let id_del = s.id.clone();
                    let navigate = navigate.clone();
                    view! {
                        <li class="card routine" data-testid="tpl-saved-row">
                            <div class="row routine-head">
                                <span class="routine-icon">{icon(Glyph::Grid)}</span>
                                <div class="grow">
                                    <strong>{s.name.clone()}</strong>
                                    <p class="subtle small">{tf(T::FilesCount, &[&s.file_count.to_string()])}" · "{super::ago(s.created_at)}</p>
                                </div>
                                <button class="btn small primary" data-testid="tpl-saved-use" on:click=move |_| {
                                    let id = id.clone();
                                    let navigate = navigate.clone();
                                    leptos::task::spawn_local(async move {
                                        match api::post::<_, Value>(&format!("/api/templates/{}/use", api::seg(&id)), &json!({})).await {
                                            Ok(v) => navigate(&format!("/projects/{}", v["project"]["id"].as_str().unwrap_or_default()), Default::default()),
                                            Err(e) => error.set(Some(e.to_string())),
                                        }
                                    });
                                }>{icon(Glyph::ArrowRight)}{t(T::UseTemplate)}</button>
                                <button class="icon-btn danger" aria-label=t(T::Delete) data-testid="tpl-saved-delete" on:click=move |_| {
                                    let id = id_del.clone();
                                    leptos::task::spawn_local(async move {
                                        let _ = api::delete::<Value>(&format!("/api/templates/{}", api::seg(&id))).await;
                                        reload();
                                    });
                                }>{icon(Glyph::Delete)}</button>
                            </div>
                            {s.description.clone().filter(|d| !d.is_empty()).map(|d| view! { <p class="muted">{d}</p> })}
                        </li>
                    }
                }).collect_view().into_any(),
            }}
        </ul>
    }
}
