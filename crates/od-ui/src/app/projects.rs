//! Project list: open, rename, duplicate, delete (with confirmation), and
//! "Open folder": the native picker on desktop, a path field in a browser.

use super::{ago, api, desktop, kind_label};
use crate::i18n::{T, t};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DuplicateProjectResponse, OkResponse, Project, ProjectResponse,
    ProjectsResponse,
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
        <section class="page">
            <div class="section-head">
                <h1>{t(T::ProjectsTitle)}</h1>
                <div class="row">
                    <button class="btn" data-testid="open-folder" on:click=open_folder>
                        {t(T::OpenFolder)}
                    </button>
                    <A href="/" attr:class="btn primary">{t(T::NewProject)}</A>
                </div>
            </div>
            {move || {
                path_field
                    .get()
                    .is_some()
                    .then(|| {
                        let import_typed = import_typed.clone();
                        let import_key = import_key.clone();
                        view! {
                            <div class="folder-field">
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
            {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            <div class="project-list">
                {move || match projects.get() {
                    None => view! { <p class="empty">{t(T::Loading)}</p> }.into_any(),
                    Some(list) if list.is_empty() => {
                        view! { <p class="empty">{t(T::NoProjects)}</p> }.into_any()
                    }
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

#[component]
fn ProjectRow(project: Project, on_change: Callback<()>) -> impl IntoView {
    let id = StoredValue::new(project.id.clone());
    let name = RwSignal::new(project.name.clone());
    let editing = RwSignal::new(false);
    let confirming = RwSignal::new(false);
    let kind = kind_label(&project);
    let imported = project.metadata.as_ref().and_then(|m| m.base_dir.clone());

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
        <div class="project-row">
            <span class="tile-kind">{kind}</span>
            <div class="project-main">
                {move || {
                    if editing.get() {
                        view! {
                            <input
                                class="input"
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
                <span class="tile-meta">
                    {ago(project.updated_at)}
                    {imported.map(|d| format!(" · {d}"))}
                </span>
            </div>
            <div class="row actions">
                {move || {
                    if confirming.get() {
                        view! {
                            <span class="confirm">{t(T::ConfirmDeleteProject)}</span>
                            <button class="btn danger" on:click=delete>{t(T::Delete)}</button>
                            <button class="btn" on:click=move |_| confirming.set(false)>{t(T::Cancel)}</button>
                        }
                            .into_any()
                    } else {
                        view! {
                            <button
                                class="btn ghost"
                                on:click=move |_| {
                                    if editing.get_untracked() { rename() } else { editing.set(true) }
                                }
                            >
                                {move || if editing.get() { t(T::Save) } else { t(T::Rename) }}
                            </button>
                            <button class="btn ghost" on:click=duplicate>{t(T::Duplicate)}</button>
                            <button class="btn ghost" on:click=move |_| confirming.set(true)>{t(T::Delete)}</button>
                        }
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
}
