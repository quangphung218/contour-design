//! Home below the composer (track C2-chat-home): the entry tabs: Designs,
//! Templates, Design systems, Image and Video templates. Connectors are
//! N/A (no cloud). The tab is remembered per browser.

use super::api;
use super::icons::{Glyph, icon};
use crate::i18n::{T, t};
use crate::intent::Chip;
use leptos::prelude::*;
use od_contracts::{DesignSystemSummary, ProjectsResponse};
use serde_json::Value;

const TAB_KEY: &str = "od.home.tab";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Designs,
    Templates,
    Systems,
    Image,
    Video,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::Designs,
        Self::Templates,
        Self::Systems,
        Self::Image,
        Self::Video,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::Designs => "designs",
            Self::Templates => "templates",
            Self::Systems => "design-systems",
            Self::Image => "image",
            Self::Video => "video",
        }
    }

    fn label(self) -> &'static str {
        t(match self {
            Self::Designs => T::HomeTabDesigns,
            Self::Templates => T::HomeTabTemplates,
            Self::Systems => T::HomeTabSystems,
            Self::Image => T::HomeTabImage,
            Self::Video => T::HomeTabVideo,
        })
    }

    fn glyph(self) -> Glyph {
        match self {
            Self::Designs => Glyph::Projects,
            Self::Templates => Glyph::Grid,
            Self::Systems => Glyph::DesignSystem,
            Self::Image => Glyph::Image,
            Self::Video => Glyph::Video,
        }
    }

    fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// The tabs; `templates` is the existing gallery.
#[component]
pub fn HomeTabs(
    #[prop(into)] templates: ViewFn,
    systems: LocalResource<Vec<DesignSystemSummary>>,
    design_system: RwSignal<Option<String>>,
    brief: RwSignal<String>,
    set_chip: Callback<Chip>,
    brief_ref: NodeRef<leptos::html::Textarea>,
) -> impl IntoView {
    let tab = RwSignal::new(
        storage()
            .and_then(|s| s.get_item(TAB_KEY).ok().flatten())
            .and_then(|k| Tab::parse(&k))
            .unwrap_or(Tab::Templates),
    );
    let pick = move |k: Tab| {
        tab.set(k);
        if let Some(s) = storage() {
            let _ = s.set_item(TAB_KEY, k.key());
        }
    };
    let focus = move || {
        if let Some(el) = brief_ref.get_untracked() {
            let _ = el.focus();
        }
    };
    view! {
        <section class="home-section home-tabs" data-testid="home-tabs">
            <div class="segmented" role="tablist" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::HomeTabsLabel)>
                {Tab::ALL.into_iter().map(|k| view! {
                    <button class="seg" role="tab" data-testid=format!("home-tab-{}", k.key())
                        aria-selected=move || (tab.get() == k).to_string() tabindex=move || super::focus::roving(tab.get() == k)
                        class:active=move || tab.get() == k
                        on:click=move |_| pick(k)>
                        {icon(k.glyph())}<span class="seg-label">{k.label()}</span>
                    </button>
                }).collect_view()}
            </div>
            <div class="home-tab-panel" role="tabpanel" data-testid="home-tab-panel" data-tab=move || tab.get().key()>
                {move || match tab.get() {
                    Tab::Templates => templates.run(),
                    Tab::Designs => view! { <DesignsTab/> }.into_any(),
                    Tab::Systems => view! { <SystemsTab systems=systems selected=design_system focus=Callback::new(move |()| focus())/> }.into_any(),
                    Tab::Image => view! { <PromptsTab surface="image" brief=brief set_chip=set_chip focus=Callback::new(move |()| focus())/> }.into_any(),
                    Tab::Video => view! { <PromptsTab surface="video" brief=brief set_chip=set_chip focus=Callback::new(move |()| focus())/> }.into_any(),
                }}
            </div>
        </section>
    }
}

/// The user's designs, newest first (the full list lives on Projects).
#[component]
fn DesignsTab() -> impl IntoView {
    let projects = LocalResource::new(|| async {
        let mut list = api::get::<ProjectsResponse>("/api/projects")
            .await
            .map(|r| r.projects)
            .unwrap_or_default();
        list.retain(|p| {
            p.metadata.as_ref().and_then(|m| m.intent.as_deref()) != Some("media-library")
        });
        list.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        list
    });
    move || {
        let list = projects.get().unwrap_or_default();
        if list.is_empty() {
            return view! {
                <div class="empty-state compact">
                    <span class="empty-icon">{icon(Glyph::Projects)}</span>
                    <p>{t(T::HomeNoDesigns)}</p>
                </div>
            }
            .into_any();
        }
        view! {
            <div class="home-ds-grid">
                {list.into_iter().take(12).map(|p| view! {
                    <a class="card interactive home-ds-card" data-testid="home-design" href=format!("/projects/{}", p.id)>
                        <strong>{p.name.clone()}</strong>
                        <span class="muted small">{super::ago(p.updated_at)}</span>
                    </a>
                }).collect_view()}
                <a class="btn ghost small home-all" href="/projects">{t(T::HomeAllDesigns)}{icon(Glyph::ArrowRight)}</a>
            </div>
        }
        .into_any()
    }
}

/// Design systems as cards; picking one sets the composer's system.
#[component]
fn SystemsTab(
    systems: LocalResource<Vec<DesignSystemSummary>>,
    selected: RwSignal<Option<String>>,
    focus: Callback<()>,
) -> impl IntoView {
    move || {
        let mut list = systems.get().unwrap_or_default();
        // The user's own systems first, then the bundled ones.
        list.sort_by_key(|s| !s.id.starts_with("user:"));
        if list.is_empty() {
            return view! {
                <div class="empty-state compact">
                    <span class="empty-icon">{icon(Glyph::DesignSystem)}</span>
                    <p>{t(T::HomeNoSystems)}</p>
                </div>
            }
            .into_any();
        }
        view! {
            <div class="home-ds-grid">
                {list.into_iter().take(24).map(|s| {
                    let id = s.id.clone();
                    let id_on = s.id.clone();
                    view! {
                        <button class="card interactive home-ds-card" data-testid="home-ds-card" data-id=s.id.clone()
                            aria-pressed=move || (selected.get().as_deref() == Some(id_on.as_str())).to_string()
                            on:click=move |_| { selected.set(Some(id.clone())); focus.run(()); }>
                            <strong>{crate::i18n::rebrand(&s.title).into_owned()}</strong>
                            <span class="muted small">{s.category.clone()}</span>
                        </button>
                    }
                }).collect_view()}
            </div>
        }
        .into_any()
    }
}

/// Image or video prompt templates; picking one fills the brief.
#[component]
fn PromptsTab(
    surface: &'static str,
    brief: RwSignal<String>,
    set_chip: Callback<Chip>,
    focus: Callback<()>,
) -> impl IntoView {
    let list = LocalResource::new(move || async move {
        api::get::<Value>(&format!("/api/prompt-templates?surface={surface}"))
            .await
            .ok()
            .and_then(|v| v["promptTemplates"].as_array().cloned())
            .unwrap_or_default()
    });
    let use_one = move |id: String| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/prompt-templates/{surface}/{}", api::seg(&id));
            if let Ok(v) = api::get::<Value>(&path).await {
                set_chip.run(if surface == "video" {
                    Chip::Video
                } else {
                    Chip::Image
                });
                brief.set(
                    v["promptTemplate"]["prompt"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                );
                focus.run(());
            }
        });
    };
    move || {
        let items = list.get().unwrap_or_default();
        view! {
            <div class="prompt-grid home-prompts">
                {items.into_iter().take(12).map(|p| {
                    let id = p["id"].as_str().unwrap_or_default().to_owned();
                    view! {
                        <button class="card interactive prompt-card home-prompt" data-testid="home-prompt" data-surface=surface
                            on:click=move |_| use_one(id.clone())>
                            <span class=format!("prompt-cover tint-{}", super::lib_ui::tint(p["id"].as_str().unwrap_or_default())) aria-hidden="true">
                                {icon(if surface == "video" { Glyph::Video } else { Glyph::Image })}
                            </span>
                            <strong>{p["title"].as_str().unwrap_or_default().to_owned()}</strong>
                            <span class="muted small">{p["summary"].as_str().unwrap_or_default().to_owned()}</span>
                        </button>
                    }
                }).collect_view()}
            </div>
        }
    }
}
