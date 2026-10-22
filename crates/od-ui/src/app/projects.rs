//! Project list: open, rename, duplicate, delete (with confirmation), and
//! "Open folder" on desktop.

use super::{ago, api, desktop};
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

    let open_folder = move |_| {
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let Some(dir) = desktop::pick_folder().await else {
                return;
            };
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

    view! {
        <section class="page">
            <div class="section-head">
                <h1>"Projects"</h1>
                <div class="row">
                    <Show when=desktop::is_desktop>
                        <button class="btn" on:click=open_folder.clone()>"Open folder…"</button>
                    </Show>
                    <A href="/" attr:class="btn primary">"New project"</A>
                </div>
            </div>
            {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            <div class="project-list">
                {move || match projects.get() {
                    None => view! { <p class="empty">"Loading…"</p> }.into_any(),
                    Some(list) if list.is_empty() => {
                        view! { <p class="empty">"No projects yet."</p> }.into_any()
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
    let kind = project
        .metadata
        .as_ref()
        .and_then(|m| m.kind)
        .map(|k| format!("{k:?}"))
        .unwrap_or_else(|| "Project".to_owned());
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
                            <span class="confirm">"Delete this project?"</span>
                            <button class="btn danger" on:click=delete>"Delete"</button>
                            <button class="btn" on:click=move |_| confirming.set(false)>"Cancel"</button>
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
                                {move || if editing.get() { "Save" } else { "Rename" }}
                            </button>
                            <button class="btn ghost" on:click=duplicate>"Duplicate"</button>
                            <button class="btn ghost" on:click=move |_| confirming.set(true)>"Delete"</button>
                        }
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
}
