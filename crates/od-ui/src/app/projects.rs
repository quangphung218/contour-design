//! Project list: open, rename, duplicate, delete (with confirmation), and
//! "Open folder": the native picker on desktop, a path field in a browser.

use super::icons::{Glyph, icon};
use super::{ago, api, desktop, kind_label};
use crate::i18n::{T, t};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DuplicateProjectResponse, FilesResponse, OkResponse, Project,
    ProjectResponse, ProjectsResponse,
};
use serde_json::json;

#[component]
pub fn Projects() -> impl IntoView {
    let navigate = use_navigate();
    let projects = RwSignal::new(None::<Vec<Project>>);
    let error = RwSignal::new(None::<String>);
    let reload = move || {
        leptos::task::spawn_local(async move {
            match api::get::<ProjectsResponse>("/api/projects").await {
                Ok(mut r) => {
                    r.projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                    projects.set(Some(r.projects));
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    reload();

    // Imports `dir` and opens its Studio.
    let import = move |dir: String| {
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateProjectResponse>(
                "/api/import/folder",
                &json!({ "baseDir": dir }),
            )
            .await
            {
                Ok(r) => navigate(&format!("/projects/{}", r.project.id), Default::default()),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    let path_field = RwSignal::new(None::<String>);
    let import_pick = import.clone();
    let open_folder = move |_| {
        if !desktop::is_desktop() {
            path_field.update(|f| {
                *f = if f.is_some() {
                    None
                } else {
                    Some(String::new())
                }
            });
            return;
        }
        let import = import_pick.clone();
        leptos::task::spawn_local(async move {
            if let Some(dir) = desktop::pick_folder().await {
                import(dir);
            }
        });
    };
    let import_typed = move || {
        if let Some(dir) = path_field.get_untracked().filter(|d| !d.trim().is_empty()) {
            import(dir.trim().to_owned());
        }
    };
    let import_key = import_typed.clone();

    view! {
        <section class="page wide">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::ProjectsTitle)}</h1>
                    {move || projects.with(|p| p.as_ref().filter(|l| !l.is_empty()).map(|l| {
                        view! { <span class="count-badge">{l.len()}</span> }
                    }))}
                </div>
                <div class="row">
                    <button class="btn" data-testid="open-folder" aria-expanded=move || path_field.with(Option::is_some).to_string() on:click=open_folder>
                        {icon(Glyph::FolderOpen)}
                        {t(T::OpenFolder)}
                    </button>
                    <A href="/" attr:class="btn primary">{icon(Glyph::Add)}{t(T::NewProject)}</A>
                </div>
            </header>
            {move || {
                path_field
                    .get()
                    .is_some()
                    .then(|| {
                        let import_typed = import_typed.clone();
                        let import_key = import_key.clone();
                        view! {
                            <div class="card padded folder-field">
                                <label class="field">
                                    <span>{t(T::FolderPathLabel)}</span>
                                    <input
                                        class="input mono"
                                        data-testid="folder-path"
                                        placeholder=t(T::FolderPathPlaceholder)
                                        prop:value=move || path_field.get().unwrap_or_default()
                                        on:input=move |ev| path_field.set(Some(event_target_value(&ev)))
                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                            if ev.key() == "Enter" {
                                                import_key();
                                            }
                                        }
                                    />
                                    <span class="muted">{t(T::FolderHint)}</span>
                                </label>
                                <button class="btn primary" data-testid="folder-import" on:click=move |_| import_typed()>
                                    {t(T::OpenAsProject)}
                                </button>
                            </div>
                        }
                    })
            }}
            {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
            <div class="project-grid large">
                {move || match projects.get() {
                    None => (0..6)
                        .map(|_| view! { <div class="skeleton card-skeleton"></div> })
                        .collect_view()
                        .into_any(),
                    Some(list) if list.is_empty() => view! {
                        <div class="empty-state">
                            <span class="empty-icon">{icon(Glyph::Projects)}</span>
                            <strong>{t(T::ProjectsEmptyTitle)}</strong>
                            <p>{t(T::ProjectsEmptyBody)}</p>
                            <A href="/" attr:class="btn primary">{icon(Glyph::Add)}{t(T::NewProject)}</A>
                        </div>
                    }
                        .into_any(),
                    Some(list) => {
                        list.into_iter()
                            .map(|p| view! { <ProjectRow project=p on_change=Callback::new(move |()| reload())/> })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </section>
    }
}

/// A project's cover: its entry page rendered small (no scripts), else the
/// kind's icon. Loads lazily, so long lists stay cheap.
#[component]
fn ProjectCover(project: Project) -> impl IntoView {
    let entry = RwSignal::new(None::<Option<String>>);
    let id = project.id.clone();
    let kind_glyph = match project.metadata.as_ref().and_then(|m| m.kind) {
        Some(od_contracts::ProjectKind::Deck) => Glyph::Deck,
        _ => Glyph::Prototype,
    };
    let base = format!("/api/projects/{}", api::seg(&id));
    let files_url = format!("{base}/files");
    let stored = project.clone();
    leptos::task::spawn_local(async move {
        let found = api::get::<FilesResponse>(&files_url)
            .await
            .ok()
            .and_then(|r| super::studio::pick_entry(Some(&stored), &r.files))
            .filter(|p| super::studio::is_html_path(p));
        entry.set(Some(found));
    });
    view! {
        <span class="cover project-cover" aria-hidden="true">
            {move || match entry.get() {
                Some(Some(path)) => view! {
                    // `loading` before `src`: the frame loads when scrolled to.
                    <iframe
                        class="cover-frame"
                        prop:loading="lazy"
                        sandbox=""
                        src=format!("{base}/raw/{}", api::file_path(&path))
                        tabindex="-1"
                        title=""
                    ></iframe>
                }
                    .into_any(),
                Some(None) => view! { <span class="cover-glyph">{icon(kind_glyph)}</span> }.into_any(),
                None => view! { <span class="skeleton cover-fill"></span> }.into_any(),
            }}
        </span>
    }
}

/// A project card linking to its Studio (Home's recent list).
#[component]
pub fn ProjectCard(project: Project) -> impl IntoView {
    let kind = kind_label(&project);
    let href = format!("/projects/{}", project.id);
    view! {
        <A href=href attr:class="project-card card interactive">
            <ProjectCover project=project.clone()/>
            <span class="project-body">
                <span class="project-name">{project.name.clone()}</span>
                <span class="project-meta">
                    <span class="badge">{kind}</span>
                    <span>{ago(project.updated_at)}</span>
                </span>
            </span>
        </A>
    }
}

#[component]
fn ProjectRow(project: Project, on_change: Callback<()>) -> impl IntoView {
    let id = StoredValue::new(project.id.clone());
    let name = RwSignal::new(project.name.clone());
    let editing = RwSignal::new(false);
    let confirming = RwSignal::new(false);
    let kind = kind_label(&project);
    let imported = project.metadata.as_ref().and_then(|m| m.base_dir.clone());
    let updated = project.updated_at;

    let rename = move || {
        editing.set(false);
        let new_name = name.get_untracked();
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}", api::seg(&id.get_value()));
            let _ = api::patch::<_, ProjectResponse>(&path, &json!({ "name": new_name })).await;
            on_change.run(());
        });
    };
    let rename_key = rename;
    let duplicate = move |_| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}/duplicate", api::seg(&id.get_value()));
            let _ = api::post::<_, DuplicateProjectResponse>(&path, &json!({})).await;
            on_change.run(());
        });
    };
    let delete = move |_| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}", api::seg(&id.get_value()));
            let _ = api::delete::<OkResponse>(&path).await;
            on_change.run(());
        });
    };

    view! {
        <article class="project-card card interactive" data-testid="project-card">
            <A href=format!("/projects/{}", id.get_value()) attr:class="cover-link" attr:tabindex="-1">
                <ProjectCover project=project.clone()/>
            </A>
            <div class="project-body">
                {move || {
                    if editing.get() {
                        view! {
                            <input
                                class="input small"
                                aria-label=t(T::Rename)
                                prop:value=move || name.get()
                                on:input=move |ev| name.set(event_target_value(&ev))
                                on:keydown={
                                    move |ev: web_sys::KeyboardEvent| {
                                        if ev.key() == "Enter" {
                                            rename_key();
                                        } else if ev.key() == "Escape" {
                                            editing.set(false);
                                        }
                                    }
                                }
                            />
                        }
                            .into_any()
                    } else {
                        view! {
                            <A href=format!("/projects/{}", id.get_value()) attr:class="project-name">
                                {move || name.get()}
                            </A>
                        }
                            .into_any()
                    }
                }}
                <span class="project-meta">
                    <span class="badge">{kind}</span>
                    <span>{ago(updated)}</span>
                </span>
                {imported.map(|d| {
                    let full = d.clone();
                    view! { <span class="project-path mono" title=full>{icon(Glyph::FolderOpen)}{d}</span> }
                })}
            </div>
            <div class="project-actions">
                {move || {
                    if confirming.get() {
                        view! {
                            <span class="confirm">{t(T::ConfirmDeleteProject)}</span>
                            <span class="spacer"></span>
                            <button class="btn small" on:click=move |_| confirming.set(false)>{t(T::Cancel)}</button>
                            <button class="btn small danger" on:click=delete>{t(T::Delete)}</button>
                        }
                            .into_any()
                    } else {
                        view! {
                            <button
                                class="btn ghost small"
                                on:click=move |_| {
                                    if editing.get_untracked() { rename() } else { editing.set(true) }
                                }
                            >
                                {move || if editing.get() {
                                    view! { {icon(Glyph::Check)}{t(T::Save)} }.into_any()
                                } else {
                                    view! { {icon(Glyph::Rename)}{t(T::Rename)} }.into_any()
                                }}
                            </button>
                            <span class="spacer"></span>
                            <button class="icon-btn" aria-label=t(T::Duplicate) data-tip=t(T::Duplicate) on:click=duplicate>
                                {icon(Glyph::Duplicate)}
                            </button>
                            <button class="icon-btn danger" aria-label=t(T::Delete) data-tip=t(T::Delete) on:click=move |_| confirming.set(true)>
                                {icon(Glyph::Delete)}
                            </button>
                        }
                            .into_any()
                    }
                }}
            </div>
        </article>
    }
}
