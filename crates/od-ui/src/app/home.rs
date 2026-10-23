//! Home: brief composer (output chips, design system and template pickers),
//! the template gallery for the chosen output type, and recent projects.

use super::ds_create::NewDesignSystem;
use super::icons::{Glyph, icon};
use super::lazy_frame::LazyFrame;
use super::projects::ProjectCard;
use super::{Shell, api};
use crate::i18n::{T, locale, t, tf};
use crate::intent::{Chip, HomeIntent, templates_for};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DesignSystemSummary, DesignSystemsResponse, DesignTemplatesResponse,
    Platform, ProjectsResponse, SkillSummary, SkillsResponse,
};

/// How many recent projects Home lists.
const RECENT: usize = 6;
/// Templates shown before "Show all".
const GALLERY: usize = 8;

#[component]
pub fn Home() -> impl IntoView {
    let _shell = Shell::get();
    let navigate = use_navigate();
    let brief = RwSignal::new(String::new());
    let chip = RwSignal::new(Chip::Prototype);
    let design_system = RwSignal::new(None::<String>);
    let template = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let brief_ref = NodeRef::<leptos::html::Textarea>::new();

    let skills = LocalResource::new(|| async {
        api::get::<SkillsResponse>("/api/skills")
            .await
            .map(|r| r.skills)
            .unwrap_or_default()
    });
    let templates = LocalResource::new(|| async {
        api::get::<DesignTemplatesResponse>("/api/design-templates")
            .await
            .map(|r| r.design_templates)
            .unwrap_or_default()
    });
    let systems = LocalResource::new(|| async {
        api::get::<DesignSystemsResponse>("/api/design-systems")
            .await
            .map(|r| r.design_systems)
            .unwrap_or_default()
    });
    let recent = LocalResource::new(|| async {
        let mut list = api::get::<ProjectsResponse>("/api/projects")
            .await
            .map(|r| r.projects)
            .unwrap_or_default();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        list.truncate(RECENT);
        list
    });

    let chip_templates = Signal::derive(move || -> Option<Vec<SkillSummary>> {
        templates.get().map(|t| templates_for(chip.get(), &t))
    });

    let submit = move || {
        if busy.get_untracked() || brief.with_untracked(|b| b.trim().is_empty()) {
            return;
        }
        let intent = HomeIntent {
            brief: brief.get_untracked(),
            chip: chip.get_untracked(),
            design_system_id: design_system.get_untracked(),
            template_id: template.get_untracked(),
        };
        let req = intent.create_request(&skills.get_untracked().unwrap_or_default());
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateProjectResponse>("/api/projects", &req).await {
                Ok(res) => navigate(&format!("/projects/{}", res.project.id), Default::default()),
                Err(e) => {
                    error.set(Some(e.to_string()));
                    busy.set(false);
                }
            }
        });
    };
    let submit_key = submit.clone();

    // A gallery pick selects the template and, on an empty brief, starts
    // from the template's example prompt.
    let pick_template = Callback::new(move |s: SkillSummary| {
        template.set(Some(s.info.id.clone()));
        if brief.with_untracked(|b| b.trim().is_empty()) && !s.info.example_prompt.is_empty() {
            brief.set(s.info.example_prompt.clone());
        }
        if let Some(el) = brief_ref.get_untracked() {
            let _ = el.focus();
        }
    });

    view! {
        <section class="home">
            <div class="home-inner">
                <header class="home-hero">
                    <h1 class="home-title">{t(T::HomeTitle)}</h1>
                    <p class="home-sub">{t(T::HomeSubtitle)}</p>
                </header>
                <div class="composer-card">
                    <textarea
                        class="brief"
                        data-testid="brief"
                        rows="3"
                        node_ref=brief_ref
                        aria-label=t(T::HomeTitle)
                        placeholder=t(T::BriefPlaceholder)
                        prop:value=move || brief.get()
                        on:input=move |ev| brief.set(event_target_value(&ev))
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() {
                                ev.prevent_default();
                                submit_key();
                            }
                        }
                    ></textarea>
                    <div class="composer-bar">
                        <div class="chips" role="radiogroup" aria-label=t(T::OutputType)>
                            {Chip::ALL
                                .into_iter()
                                .map(|c| {
                                    let (glyph, label) = match c {
                                        Chip::Prototype => (Glyph::Prototype, t(T::ChipPrototype)),
                                        Chip::Deck => (Glyph::Deck, t(T::ChipDeck)),
                                    };
                                    view! {
                                        <button
                                            class="chip"
                                            class:active=move || chip.get() == c
                                            role="radio"
                                            aria-checked=move || (chip.get() == c).to_string()
                                            on:click=move |_| {
                                                chip.set(c);
                                                template.set(None);
                                            }
                                        >
                                            {icon(glyph)}
                                            {label}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                        <span class="bar-divider" aria-hidden="true"></span>
                        <label class="picker">
                            {icon(Glyph::DesignSystem)}
                            <select
                                data-testid="design-system"
                                aria-label=t(T::DesignSystem)
                                on:change=move |ev| {
                                    let v = event_target_value(&ev);
                                    design_system.set(Some(v).filter(|v| !v.is_empty()));
                                }
                            >
                                <option value="">{t(T::NoneOption)}</option>
                                {move || {
                                    systems
                                        .get()
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(|s| {
                                            let selected = design_system.get().as_deref() == Some(s.id.as_str());
                                            view! {
                                                <option value=s.id.clone() selected=selected>
                                                    {format!("{} · {}", s.title, s.category)}
                                                </option>
                                            }
                                        })
                                        .collect_view()
                                }}
                            </select>
                            <Swatches systems=systems selected=design_system/>
                        </label>
                        <label class="picker">
                            {icon(Glyph::Grid)}
                            <select
                                data-testid="template"
                                aria-label=t(T::Template)
                                // A property, not `selected` on the options: an
                                // option the user picked by hand ignores the
                                // attribute, so the gallery could not move it.
                                prop:value=move || {
                                    chip_templates.track();
                                    template.get().unwrap_or_default()
                                }
                                on:change=move |ev| {
                                    let v = event_target_value(&ev);
                                    template.set(Some(v).filter(|v| !v.is_empty()));
                                }
                            >
                                <option value="">{t(T::DefaultTemplate)}</option>
                                {move || {
                                    chip_templates
                                        .get()
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(|s| {
                                            view! { <option value=s.info.id.clone()>{template_name(&s)}</option> }
                                        })
                                        .collect_view()
                                }}
                            </select>
                        </label>
                        <span class="spacer"></span>
                        <button
                            class="btn primary send-btn"
                            data-testid="create"
                            disabled=move || busy.get() || brief.with(|b| b.trim().is_empty())
                            on:click=move |_| submit()
                        >
                            {move || if busy.get() {
                                view! { <span class="spinner on-accent"></span>{t(T::Creating)} }.into_any()
                            } else {
                                view! { {t(T::Create)}{icon(Glyph::ArrowRight)} }.into_any()
                            }}
                        </button>
                    </div>
                </div>
                <div class="composer-foot">
                    <NewDesignSystem systems=systems selected=design_system/>
                    <span class="spacer"></span>
                    <span class="subtle small">{t(T::CreateHint)}</span>
                </div>
                {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}

                <Gallery templates=chip_templates selected=template pick=pick_template/>

                <section class="home-section">
                    <div class="section-head">
                        <h2>{t(T::RecentProjects)}</h2>
                        <A href="/projects" attr:class="btn ghost small">
                            {t(T::AllProjects)}
                            {icon(Glyph::ChevronRight)}
                        </A>
                    </div>
                    <div class="project-grid">
                        {move || match recent.get() {
                            None => (0..3)
                                .map(|_| view! { <div class="skeleton card-skeleton"></div> })
                                .collect_view()
                                .into_any(),
                            Some(list) if list.is_empty() => view! {
                                <div class="empty-state compact">
                                    <span class="empty-icon">{icon(Glyph::Projects)}</span>
                                    <p>{t(T::NoProjectsFirstBrief)}</p>
                                </div>
                            }
                                .into_any(),
                            Some(list) => list
                                .into_iter()
                                .map(|p| view! { <ProjectCard project=p/> })
                                .collect_view()
                                .into_any(),
                        }}
                    </div>
                </section>
            </div>
        </section>
    }
}

/// Featured templates of the chosen output type; a card picks it.
#[component]
fn Gallery(
    templates: Signal<Option<Vec<SkillSummary>>>,
    selected: RwSignal<Option<String>>,
    pick: Callback<SkillSummary>,
) -> impl IntoView {
    let show_all = RwSignal::new(false);
    view! {
        <section class="home-section">
            <div class="section-head">
                <h2>{t(T::TemplatesTitle)}</h2>
                {move || {
                    let n = templates.with(|t| t.as_ref().map_or(0, Vec::len));
                    (n > GALLERY).then(|| view! {
                        <button class="btn ghost small" on:click=move |_| show_all.update(|s| *s = !*s)>
                            {move || if show_all.get() {
                                t(T::TemplatesShowFewer).to_owned()
                            } else {
                                tf(T::TemplatesShowAll, &[&n.to_string()])
                            }}
                        </button>
                    })
                }}
            </div>
            <div class="template-grid">
                {move || match templates.get() {
                    None => (0..4)
                        .map(|_| view! { <div class="skeleton template-skeleton"></div> })
                        .collect_view()
                        .into_any(),
                    Some(list) if list.is_empty() => {
                        view! { <p class="empty">{t(T::NoTemplates)}</p> }.into_any()
                    }
                    Some(list) => {
                        let take = if show_all.get() { list.len() } else { GALLERY };
                        list.into_iter()
                            .take(take)
                            .map(|s| view! { <TemplateCard template=s selected=selected pick=pick/> })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </section>
    }
}

#[component]
fn TemplateCard(
    template: SkillSummary,
    selected: RwSignal<Option<String>>,
    pick: Callback<SkillSummary>,
) -> impl IntoView {
    let id = template.info.id.clone();
    let is_selected = move || selected.get().as_deref() == Some(id.as_str());
    let name = template_name(&template);
    let description = localized(
        template.info.description_i18n.as_ref(),
        &template.info.description,
    );
    let tint = format!("cover tint-{}", tint_of(&template.info.id));
    let tag = scenario_label(&template.info.scenario);
    let glyph = template_glyph(&template);
    let picked = template.clone();
    let picked_key = template.clone();
    let example = template.has_example.then(|| {
        format!(
            "/api/design-templates/{}/example",
            api::seg(&template.info.id)
        )
    });
    let thumb = example.is_some();
    let label = name.clone();
    view! {
        // A div, not a button: the thumbnail frame may not sit in a button.
        <div
            class="template-card"
            class:selected=is_selected.clone()
            role="button"
            tabindex="0"
            aria-pressed=move || is_selected().to_string()
            aria-label=label
            data-testid="template-card"
            on:click=move |_| pick.run(picked.clone())
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() == "Enter" || ev.key() == " " {
                    ev.prevent_default();
                    pick.run(picked_key.clone());
                }
            }
        >
            <span class=tint class:thumb-cover=thumb aria-hidden="true">
                {match example {
                    Some(src) => view! { <LazyFrame src=src/> }.into_any(),
                    None => view! { <span class="cover-glyph">{icon(glyph)}</span> }.into_any(),
                }}
                <Show when=is_selected.clone()>
                    <span class="cover-check">{icon(Glyph::Check)}</span>
                </Show>
            </span>
            <span class="template-body">
                <span class="template-name">{name}</span>
                <span class="template-desc">{description}</span>
                {tag.map(|s| view! { <span class="template-meta">{s}</span> })}
            </span>
        </div>
    }
}

/// The template's name in the UI language, else its own; a bare
/// `kebab-case` id reads as words.
fn template_name(s: &SkillSummary) -> String {
    let name = localized(s.info.display_name.as_ref(), &s.info.name);
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

fn localized(map: Option<&std::collections::BTreeMap<String, String>>, fallback: &str) -> String {
    let text = map
        .and_then(|m| m.get(locale().tag()))
        .filter(|v| !v.trim().is_empty())
        .map_or(fallback, String::as_str);
    crate::i18n::rebrand(text).into_owned()
}

/// One of four quiet cover tints, stable per template.
fn tint_of(id: &str) -> u32 {
    id.bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        % 4
}

/// Placeholder icon for a template without a rendered example, from what
/// it makes: video, deck, mobile screen, then its subject.
fn template_glyph(s: &SkillSummary) -> Glyph {
    let words = format!("{} {}", s.info.id, s.info.name).to_ascii_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| words.contains(k));
    match s.info.mode {
        od_contracts::SkillMode::Video => return Glyph::Present,
        od_contracts::SkillMode::Deck => return Glyph::Deck,
        _ => {}
    }
    if s.info.platform == Some(Platform::Mobile) {
        return Glyph::Phone;
    }
    if has(&["research", "30days"]) {
        return Glyph::Search;
    }
    if has(&["valuation", "dcf", "finance", "dashboard", "table"]) {
        return Glyph::Grid;
    }
    if has(&["live"]) {
        return Glyph::Refresh;
    }
    match s.info.scenario.as_str() {
        "marketing" | "sales" | "creator" => Glyph::Sparkle,
        "design" => Glyph::Palette,
        "education" | "policy" => Glyph::Notes,
        "strategy" | "product" => Glyph::Plan,
        "engineering" | "ai" => Glyph::Braces,
        "personal" => Glyph::FileText,
        _ => Glyph::Desktop,
    }
}

/// The scenario as a short tag, capitalised.
fn scenario_label(scenario: &str) -> Option<String> {
    let s = scenario.trim();
    let mut chars = s.chars();
    let first = chars.next()?;
    Some(
        first
            .to_uppercase()
            .chain(chars)
            .collect::<String>()
            .replace('-', " "),
    )
}

#[component]
fn Swatches(
    systems: LocalResource<Vec<DesignSystemSummary>>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    move || {
        let id = selected.get()?;
        let sys = systems.get()?.into_iter().find(|s| s.id == id)?;
        Some(view! {
            <span class="swatches" title=sys.summary.clone()>
                {sys
                    .swatches
                    .into_iter()
                    .map(|c| view! { <i style=format!("background:{c}")></i> })
                    .collect_view()}
            </span>
        })
    }
}
