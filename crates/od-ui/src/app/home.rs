//! Home: brief box, output chips, design system and template pickers.

use super::{Shell, ago, api};
use crate::intent::{Chip, HomeIntent, templates_for};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DesignSystemSummary, DesignSystemsResponse, DesignTemplatesResponse,
    Project, ProjectsResponse, SkillSummary, SkillsResponse,
};

/// How many recent projects Home lists.
const RECENT: usize = 6;

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

    let chip_templates = move || -> Vec<SkillSummary> {
        templates
            .get()
            .map(|t| templates_for(chip.get(), &t))
            .unwrap_or_default()
    };

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

    view! {
        <section class="home">
            <h1 class="home-title">"What do you want to design?"</h1>
            <div class="composer-card">
                <textarea
                    class="brief"
                    data-testid="brief"
                    rows="4"
                    placeholder="Describe a page, an app screen or a deck…"
                    prop:value=move || brief.get()
                    on:input=move |ev| brief.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() {
                            ev.prevent_default();
                            submit_key();
                        }
                    }
                ></textarea>
                <div class="composer-row">
                    <div class="chips" role="radiogroup" aria-label="Output type">
                        {Chip::ALL
                            .into_iter()
                            .map(|c| {
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
                                        {c.label()}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                    <button
                        class="btn primary"
                        data-testid="create"
                        disabled=move || busy.get() || brief.with(|b| b.trim().is_empty())
                        on:click=move |_| submit()
                    >
                        {move || if busy.get() { "Creating…" } else { "Create" }}
                    </button>
                </div>
                <div class="pickers">
                    <label class="picker">
                        <span>"Design system"</span>
                        <select
                            data-testid="design-system"
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                design_system.set(Some(v).filter(|v| !v.is_empty()));
                            }
                        >
                            <option value="">"None"</option>
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
                        <span>"Template"</span>
                        <select
                            data-testid="template"
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                template.set(Some(v).filter(|v| !v.is_empty()));
                            }
                        >
                            <option value="" selected=move || template.get().is_none()>
                                "Default for this type"
                            </option>
                            {move || {
                                chip_templates()
                                    .into_iter()
                                    .map(|t| {
                                        view! { <option value=t.info.id.clone()>{t.info.name.clone()}</option> }
                                    })
                                    .collect_view()
                            }}
                        </select>
                    </label>
                </div>
                {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            </div>
            <div class="section-head">
                <h2>"Recent projects"</h2>
                <A href="/projects">"All projects"</A>
            </div>
            <div class="project-grid">
                {move || match recent.get() {
                    None => view! { <p class="empty">"Loading…"</p> }.into_any(),
                    Some(list) if list.is_empty() => {
                        view! { <p class="empty">"No projects yet. Your first brief creates one."</p> }
                            .into_any()
                    }
                    Some(list) => {
                        list.into_iter().map(|p| view! { <ProjectTile project=p/> }).collect_view().into_any()
                    }
                }}
            </div>
        </section>
    }
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

#[component]
pub fn ProjectTile(project: Project) -> impl IntoView {
    let kind = project
        .metadata
        .as_ref()
        .and_then(|m| m.kind)
        .map(|k| format!("{k:?}"))
        .unwrap_or_else(|| "Project".to_owned());
    view! {
        <A href=format!("/projects/{}", project.id) attr:class="tile">
            <span class="tile-kind">{kind}</span>
            <span class="tile-name">{project.name.clone()}</span>
            <span class="tile-meta">{ago(project.updated_at)}</span>
        </A>
    }
}
