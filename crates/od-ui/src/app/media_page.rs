//! Media (spec 1.5): the library of every image, video and audio file
//! across projects (filter, search, upload, preview, delete, "Chat to
//! design"); generation through the providers the user configured, queued
//! (`/api/media/tasks`) with a live queue; provider keys; and HyperFrames
//! → MP4 for a project.

use super::export_menu::download;
use super::icons::{Glyph, icon};
use super::lib_ui::{Modal, query_param, size_label, upload};
use super::{ago, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use od_contracts::{Project, ProjectsResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// How often the queue refreshes while tasks are open.
const QUEUE_POLL: Duration = Duration::from_millis(1500);
/// The project that holds library uploads (daemon `LIBRARY_PROJECT_ID`).
const LIBRARY_PROJECT: &str = "media-library";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub project_id: String,
    pub project_name: String,
    pub path: String,
    pub kind: String,
    pub mime: String,
    pub size: u64,
    pub mtime: f64,
    pub uploaded: bool,
}

impl Asset {
    fn src(&self) -> String {
        format!("/api/projects/{}/raw/{}", api::seg(&self.project_id), api::file_path(&self.path))
    }

    fn key(&self) -> String {
        format!("{}/{}", self.project_id, self.path)
    }

    fn file_name(&self) -> String {
        self.path.rsplit('/').next().unwrap_or(&self.path).to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub request: Value,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Model {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub surface: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Library,
    Generate,
    Queue,
    Providers,
    Mp4,
}

fn kind_glyph(kind: &str) -> Glyph {
    match kind {
        "video" => Glyph::Video,
        "audio" => Glyph::Audio,
        _ => Glyph::Image,
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "queued" => t(T::TaskQueued),
        "running" => t(T::RunStateRunning),
        "done" => t(T::TaskDone),
        "canceled" => t(T::RunStateCanceled),
        "interrupted" => t(T::TaskInterrupted),
        _ => t(T::RunStateFailed),
    }
}

/// Aspect ratios the providers accept.
const ASPECTS: [&str; 5] = ["1:1", "16:9", "9:16", "4:3", "3:4"];
/// Video lengths (seconds).
const VIDEO_LENGTHS: [u32; 6] = [3, 5, 8, 10, 15, 30];
/// Audio durations (seconds).
const AUDIO_DURATIONS: [u32; 6] = [5, 10, 15, 30, 60, 120];

#[component]
pub fn MediaPage() -> impl IntoView {
    let template = query_param("template");
    let tab = RwSignal::new(if template.is_some() { Tab::Generate } else { Tab::Library });
    let assets = RwSignal::new(None::<Vec<Asset>>);
    let tasks = RwSignal::new(Vec::<Task>::new());
    let projects = RwSignal::new(Vec::<Project>::new());
    let error = RwSignal::new(None::<String>);

    let reload_assets = move || {
        leptos::task::spawn_local(async move {
            match api::get::<Value>("/api/library/assets").await {
                Ok(v) => assets.set(serde_json::from_value(v["assets"].clone()).ok()),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    let reload_tasks = move || {
        leptos::task::spawn_local(async move {
            if let Ok(v) = api::get::<Value>("/api/media/tasks").await {
                let mut list: Vec<Task> = serde_json::from_value(v["tasks"].clone()).unwrap_or_default();
                list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                let finished = tasks.with_untracked(|old| {
                    list.iter().any(|n| n.status == "done" && old.iter().any(|o| o.id == n.id && o.status != "done"))
                });
                tasks.set(list);
                if finished {
                    reload_assets();
                }
            }
        });
    };
    let reload_projects = move || {
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<ProjectsResponse>("/api/projects").await {
                projects.set(r.projects);
            }
        });
    };
    reload_assets();
    reload_tasks();
    reload_projects();
    if let Ok(handle) = set_interval_with_handle(
        move || {
            if tasks.with_untracked(|l| l.iter().any(|t| matches!(t.status.as_str(), "queued" | "running"))) {
                reload_tasks();
            }
        },
        QUEUE_POLL,
    ) {
        on_cleanup(move || handle.clear());
    }
    let active = move || tasks.with(|l| l.iter().filter(|t| matches!(t.status.as_str(), "queued" | "running")).count());

    let tab_button = move |k: Tab, label: T, glyph: Glyph, key: &'static str| {
        view! {
            <button class="tab" role="tab" data-testid=format!("media-tab-{key}") class:active=move || tab.get() == k aria-selected=move || (tab.get() == k).to_string() on:click=move |_| tab.set(k)>
                <span class="tab-name">{icon(glyph)}{t(label)}
                    {(k == Tab::Queue).then(|| view! { <Show when=move || { active() > 0 }><span class="badge accent">{active}</span></Show> })}
                </span>
            </button>
        }
    };

    view! {
        <section class="page wide lib-page" data-testid="media-page">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavMedia)}</h1>
                    <p class="page-sub">{t(T::MediaPageSub)}</p>
                </div>
                <button class="btn primary" data-testid="media-new" on:click=move |_| tab.set(Tab::Generate)>{icon(Glyph::Sparkle)}{t(T::GenerateMedia)}</button>
            </header>
            <div class="tabs lib-tabs" role="tablist">
                {tab_button(Tab::Library, T::Library, Glyph::Grid, "library")}
                {tab_button(Tab::Generate, T::Generate, Glyph::Sparkle, "generate")}
                {tab_button(Tab::Queue, T::MediaQueue, Glyph::Queue, "queue")}
                {tab_button(Tab::Providers, T::Providers, Glyph::Settings, "providers")}
                {tab_button(Tab::Mp4, T::HyperframesMp4, Glyph::Video, "mp4")}
            </div>
            {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
            {move || match tab.get() {
                Tab::Library => view! { <LibraryView assets=assets projects=projects on_change=Callback::new(move |()| { reload_assets(); reload_projects(); })/> }.into_any(),
                Tab::Generate => view! {
                    <GenerateView projects=projects template=template.clone()
                        on_queued=Callback::new(move |()| { reload_tasks(); tab.set(Tab::Queue); })
                        on_providers=Callback::new(move |()| tab.set(Tab::Providers))/>
                }.into_any(),
                Tab::Queue => view! { <QueueView tasks=tasks projects=projects on_change=Callback::new(move |()| reload_tasks())/> }.into_any(),
                Tab::Providers => view! { <ProvidersView/> }.into_any(),
                Tab::Mp4 => view! { <Mp4View projects=projects on_saved=Callback::new(move |()| reload_assets())/> }.into_any(),
            }}
        </section>
    }
}

#[component]
fn LibraryView(assets: RwSignal<Option<Vec<Asset>>>, projects: RwSignal<Vec<Project>>, on_change: Callback<()>) -> impl IntoView {
    let navigate = use_navigate();
    let kind = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let project = RwSignal::new(String::new());
    let selected = RwSignal::new(Vec::<String>::new());
    let preview = RwSignal::new(None::<usize>);
    let chat_prompt = RwSignal::new(String::new());
    let notice = RwSignal::new(None::<Result<String, String>>);
    let upload_ref = NodeRef::<leptos::html::Input>::new();

    let visible = Memo::new(move |_| {
        let k = kind.get();
        let q = query.get().to_lowercase();
        let p = project.get();
        assets.with(|l| {
            l.iter()
                .flatten()
                .filter(|a| k.is_empty() || a.kind == k)
                .filter(|a| p.is_empty() || a.project_id == p)
                .filter(|a| q.is_empty() || format!("{} {}", a.path, a.project_name).to_lowercase().contains(&q))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let on_upload = move |ev: web_sys::Event| {
        let files = super::lib_ui::picked_files(&ev);
        if files.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match upload::<Value>("/api/library/upload", &files).await {
                Ok(_) => {
                    notice.set(Some(Ok(t(T::AssetsUploaded).to_owned())));
                    on_change.run(());
                }
                Err(e) => notice.set(Some(Err(e))),
            }
        });
    };
    let chat = move |_| {
        let picks: Vec<Value> = visible.with_untracked(|l| {
            l.iter()
                .filter(|a| selected.with_untracked(|s| s.contains(&a.key())))
                .map(|a| json!({ "projectId": a.project_id, "path": a.path }))
                .collect()
        });
        if picks.is_empty() {
            return;
        }
        let body = json!({ "assets": picks, "prompt": chat_prompt.get_untracked() });
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/library/chat", &body).await {
                Ok(v) => navigate(&format!("/projects/{}", v["project"]["id"].as_str().unwrap_or_default()), Default::default()),
                Err(e) => notice.set(Some(Err(e.to_string()))),
            }
        });
    };
    let delete_selected = move |_| {
        let picks: Vec<Asset> = visible.with_untracked(|l| {
            l.iter().filter(|a| selected.with_untracked(|s| s.contains(&a.key()))).cloned().collect()
        });
        let ok = web_sys::window()
            .and_then(|w| w.confirm_with_message(&tf(T::DeleteAssetsConfirm, &[&picks.len().to_string()])).ok())
            .unwrap_or(false);
        if !ok {
            return;
        }
        leptos::task::spawn_local(async move {
            for a in &picks {
                let path = format!("/api/projects/{}/files/{}", api::seg(&a.project_id), api::file_path(&a.path));
                if let Err(e) = api::delete::<Value>(&path).await {
                    notice.set(Some(Err(e.to_string())));
                }
            }
            selected.set(Vec::new());
            on_change.run(());
        });
    };
    let toggle = move |key: String| selected.update(|s| if let Some(i) = s.iter().position(|k| *k == key) { s.remove(i); } else { s.push(key); });

    view! {
        <div class="lib-toolbar">
            <div class="segmented small" role="group" aria-label=t(T::Kind)>
                {[("", T::AllFilter), ("image", T::Images), ("video", T::Videos), ("audio", T::AudioKind)].into_iter().map(|(k, label)| view! {
                    <button class="seg" data-testid=format!("media-kind-{}", if k.is_empty() { "all" } else { k }) aria-pressed=move || (kind.get() == k).to_string() on:click=move |_| kind.set(k.to_owned())>{t(label)}</button>
                }).collect_view()}
            </div>
            <label class="lib-search">
                {icon(Glyph::Search)}
                <input class="input small" type="search" data-testid="media-search" placeholder=t(T::SearchMedia) aria-label=t(T::SearchMedia)
                    prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            </label>
            <select class="select-auto small" aria-label=t(T::ProjectLabel) prop:value=move || project.get() on:change=move |ev| project.set(event_target_value(&ev))>
                <option value="">{t(T::AllProjectsFilter)}</option>
                {move || projects.get().into_iter().map(|p| view! { <option value=p.id.clone()>{p.name.clone()}</option> }).collect_view()}
            </select>
            <span class="spacer"></span>
            <input type="file" class="visually-hidden" multiple accept="image/*,video/*,audio/*" node_ref=upload_ref data-testid="media-upload-input" on:change=on_upload/>
            <button class="btn small" data-testid="media-upload" on:click=move |_| { if let Some(i) = upload_ref.get_untracked() { i.click(); } }>{icon(Glyph::Upload)}{t(T::Upload)}</button>
        </div>
        {move || notice.get().map(|n| match n {
            Ok(m) => view! { <p class="ok-text" role="status" data-testid="media-notice">{icon(Glyph::Check)}{m}</p> }.into_any(),
            Err(e) => view! { <p class="error-text" role="alert" data-testid="media-notice">{e}</p> }.into_any(),
        })}
        <Show when=move || selected.with(|s| !s.is_empty())>
            <div class="card padded selection-bar" data-testid="media-selection">
                <strong>{move || tf(T::SelectedCount, &[&selected.with(Vec::len).to_string()])}</strong>
                <input class="input small grow" data-testid="media-chat-prompt" placeholder=t(T::ChatToDesignPlaceholder)
                    prop:value=move || chat_prompt.get() on:input=move |ev| chat_prompt.set(event_target_value(&ev))/>
                <button class="btn small primary" data-testid="media-chat" on:click=chat.clone()>{icon(Glyph::Chat)}{t(T::ChatToDesign)}</button>
                <button class="btn small ghost danger" data-testid="media-delete" on:click=delete_selected>{icon(Glyph::Delete)}{t(T::Delete)}</button>
                <button class="btn small ghost" on:click=move |_| selected.set(Vec::new())>{t(T::ClearSelection)}</button>
            </div>
        </Show>
        <div class="media-grid" data-testid="media-grid">
            {move || match assets.get() {
                None => (0..8).map(|_| view! { <div class="skeleton media-skeleton"></div> }).collect_view().into_any(),
                Some(_) => {
                    let list = visible.get();
                    if list.is_empty() {
                        return view! {
                            <div class="empty-state">
                                <span class="empty-icon">{icon(Glyph::Image)}</span>
                                <strong>{t(T::MediaEmptyTitle)}</strong>
                                <p>{t(T::MediaEmptyBody)}</p>
                            </div>
                        }.into_any();
                    }
                    list.into_iter().enumerate().map(|(i, a)| {
                        let key = a.key();
                        let key_sel = key.clone();
                        let is_sel = move || selected.with(|s| s.contains(&key_sel));
                        let is_sel2 = is_sel.clone();
                        let src = a.src();
                        let thumb = match a.kind.as_str() {
                            "image" => view! { <img src=src.clone() alt=a.file_name() loading="lazy"/> }.into_any(),
                            "video" => view! { <video src=src.clone() preload="metadata" muted=true></video> }.into_any(),
                            _ => view! { <span class="media-audio-thumb">{icon(Glyph::Audio)}</span> }.into_any(),
                        };
                        view! {
                            <figure class="media-card" class:selected=is_sel data-testid="media-card" data-kind=a.kind.clone() data-path=a.path.clone()>
                                <button class="media-thumb" aria-label=a.file_name() on:click=move |_| preview.set(Some(i))>{thumb}</button>
                                <label class="media-check" aria-label=t(T::SelectItem)>
                                    <input type="checkbox" data-testid="media-select" prop:checked=is_sel2 on:change=move |_| toggle(key.clone())/>
                                </label>
                                <figcaption>
                                    <span class="media-name">{icon(kind_glyph(&a.kind))}{a.file_name()}</span>
                                    <span class="subtle small">{if a.uploaded { t(T::Uploaded).to_owned() } else { a.project_name.clone() }}" · "{size_label(a.size)}</span>
                                </figcaption>
                            </figure>
                        }
                    }).collect_view().into_any()
                }
            }}
        </div>
        {move || preview.get().and_then(|i| visible.with(|l| l.get(i).cloned())).map(|a| {
            let count = visible.with(Vec::len);
            let i = preview.get_untracked().unwrap_or(0);
            view! { <AssetPreview asset=a index=i count=count preview=preview on_change=on_change/> }
        })}
    }
}

#[component]
fn AssetPreview(asset: Asset, index: usize, count: usize, preview: RwSignal<Option<usize>>, on_change: Callback<()>) -> impl IntoView {
    let navigate = use_navigate();
    let src = asset.src();
    let stage = match asset.kind.as_str() {
        "image" => view! { <img class="media-stage" src=src.clone() alt=asset.file_name()/> }.into_any(),
        "video" => view! { <video class="media-stage" src=src.clone() controls=true></video> }.into_any(),
        _ => view! { <audio class="media-audio" data-testid="media-audio" src=src.clone() controls=true></audio> }.into_any(),
    };
    let pid = asset.project_id.clone();
    let open_project = move |_| navigate(&format!("/projects/{}", api::seg(&pid)), Default::default());
    let dl = {
        let src = src.clone();
        let name = asset.file_name();
        let mime = asset.mime.clone();
        move |_| {
            let src = src.clone();
            let name = name.clone();
            let mime = mime.clone();
            leptos::task::spawn_local(async move {
                if let Ok(bytes) = api::get_bytes(&src).await {
                    let _ = download(&bytes, &name, &mime);
                }
            });
        }
    };
    let remove = {
        let a = asset.clone();
        move |_| {
            let path = format!("/api/projects/{}/files/{}", api::seg(&a.project_id), api::file_path(&a.path));
            leptos::task::spawn_local(async move {
                if api::delete::<Value>(&path).await.is_ok() {
                    preview.set(None);
                    on_change.run(());
                }
            });
        }
    };
    let keydown = move |ev: web_sys::KeyboardEvent| match ev.key().as_str() {
        "ArrowRight" if index + 1 < count => preview.set(Some(index + 1)),
        "ArrowLeft" if index > 0 => preview.set(Some(index - 1)),
        _ => {}
    };
    view! {
        <div on:keydown=keydown>
            <Modal title=asset.file_name() subtitle=asset.project_name.clone() glyph=kind_glyph(&asset.kind) wide=true testid="media-preview" on_close=Callback::new(move |()| preview.set(None))>
                <div class="dialog-body media-preview-body">
                    {stage}
                    <p class="subtle small">{asset.mime.clone()}" · "{size_label(asset.size)}" · "{ago(asset.mtime as i64)}</p>
                </div>
                <div class="dialog-foot">
                    <button class="icon-btn" aria-label=t(T::Previous) disabled=index == 0 on:click=move |_| preview.set(Some(index.saturating_sub(1)))>{icon(Glyph::ChevronLeft)}</button>
                    <button class="icon-btn" aria-label=t(T::Next) disabled=index + 1 >= count on:click=move |_| preview.set(Some(index + 1))>{icon(Glyph::ChevronRight)}</button>
                    <span class="spacer"></span>
                    <button class="btn ghost danger" on:click=remove>{icon(Glyph::Delete)}{t(T::Delete)}</button>
                    <button class="btn" on:click=dl>{icon(Glyph::Download)}{t(T::Download)}</button>
                    <button class="btn primary" on:click=open_project>{icon(Glyph::Projects)}{t(T::OpenProject)}</button>
                </div>
            </Modal>
        </div>
    }
}

#[component]
fn GenerateView(projects: RwSignal<Vec<Project>>, template: Option<String>, on_queued: Callback<()>, on_providers: Callback<()>) -> impl IntoView {
    let models = RwSignal::new(Vec::<Model>::new());
    let configured = RwSignal::new(Vec::<String>::new());
    let surface = RwSignal::new("image".to_owned());
    let model = RwSignal::new(String::new());
    let prompt = RwSignal::new(String::new());
    let aspect = RwSignal::new("1:1".to_owned());
    let length = RwSignal::new(5u32);
    let duration = RwSignal::new(10u32);
    let voice = RwSignal::new(String::new());
    let target = RwSignal::new(LIBRARY_PROJECT.to_owned());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    leptos::task::spawn_local(async move {
        if let Ok(v) = api::get::<Value>("/api/media/models").await {
            models.set(serde_json::from_value(v["models"].clone()).unwrap_or_default());
            configured.set(
                v["providers"].as_array().into_iter().flatten()
                    .filter(|p| p["configured"] == true)
                    .filter_map(|p| p["id"].as_str().map(str::to_owned))
                    .collect(),
            );
        }
        if let Some((sf, id)) = template.as_deref().and_then(|t| t.split_once('/')) {
            let path = format!("/api/prompt-templates/{}/{}", api::seg(sf), api::seg(id));
            if let Ok(v) = api::get::<Value>(&path).await {
                let tpl = &v["promptTemplate"];
                surface.set(sf.to_owned());
                prompt.set(tpl["prompt"].as_str().unwrap_or_default().to_owned());
                if let Some(a) = tpl["aspect"].as_str() {
                    aspect.set(a.to_owned());
                }
                if let Some(m) = tpl["model"].as_str() {
                    model.set(m.to_owned());
                }
            }
        }
    });
    // Keep the model on one of the surface's models.
    Effect::new(move |_| {
        let sf = surface.get();
        let list = models.get();
        let current = model.get_untracked();
        if !list.iter().any(|m| m.surface == sf && m.id == current) {
            let pick = list.iter().filter(|m| m.surface == sf).find(|m| configured.with_untracked(|c| c.contains(&m.provider)))
                .or_else(|| list.iter().find(|m| m.surface == sf));
            if let Some(m) = pick {
                model.set(m.id.clone());
            }
        }
    });
    let provider_ready = move || {
        let id = model.get();
        models.with(|l| l.iter().find(|m| m.id == id).map(|m| m.provider.clone()))
            .is_some_and(|p| configured.with(|c| c.contains(&p)))
    };

    let submit = move |_| {
        let sf = surface.get_untracked();
        let mut body = json!({ "surface": sf, "model": model.get_untracked(), "prompt": prompt.get_untracked() });
        match sf.as_str() {
            "image" => body["aspect"] = json!(aspect.get_untracked()),
            "video" => {
                body["aspect"] = json!(aspect.get_untracked());
                body["length"] = json!(length.get_untracked());
            }
            _ => {
                body["duration"] = json!(duration.get_untracked());
                if !voice.with_untracked(|v| v.trim().is_empty()) {
                    body["voice"] = json!(voice.get_untracked().trim());
                }
            }
        }
        let pid = target.get_untracked();
        busy.set(true);
        leptos::task::spawn_local(async move {
            // The library project may not exist yet: uploading nothing would
            // fail, so ensure it through a tiny create-if-missing call.
            if pid == LIBRARY_PROJECT && api::get::<Value>(&format!("/api/projects/{LIBRARY_PROJECT}")).await.is_err() {
                let _ = api::post::<_, Value>("/api/projects", &json!({ "id": LIBRARY_PROJECT, "name": t(T::MediaLibraryProject) })).await;
            }
            match api::post::<_, Value>(&format!("/api/projects/{}/media/tasks", api::seg(&pid)), &body).await {
                Ok(_) => on_queued.run(()),
                Err(e) => error.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="media-generate card padded" data-testid="media-generate">
            <div class="segmented" role="group" aria-label=t(T::Kind)>
                {[("image", T::Image, Glyph::Image), ("video", T::Video, Glyph::Video), ("audio", T::AudioKind, Glyph::Audio)].into_iter().map(|(k, label, glyph)| view! {
                    <button class="seg" data-testid=format!("gen-surface-{k}") aria-pressed=move || (surface.get() == k).to_string() on:click=move |_| surface.set(k.to_owned())>{icon(glyph)}{t(label)}</button>
                }).collect_view()}
            </div>
            <label class="field">
                <span>{t(T::Prompt)}</span>
                <textarea class="input" rows="5" data-testid="gen-prompt" placeholder=t(T::MediaPromptPlaceholder)
                    prop:value=move || prompt.get() on:input=move |ev| prompt.set(event_target_value(&ev))></textarea>
            </label>
            <div class="gen-options">
                <label class="field">
                    <span>{t(T::Model)}</span>
                    <select class="input" data-testid="gen-model" prop:value=move || model.get() on:change=move |ev| model.set(event_target_value(&ev))>
                        {move || {
                            let sf = surface.get();
                            models.get().into_iter().filter(|m| m.surface == sf).map(|m| {
                                let ok = configured.with(|c| c.contains(&m.provider));
                                let label = if ok { format!("{} · {}", m.label, m.provider) } else { format!("{} · {} ({})", m.label, m.provider, t(T::NoKey)) };
                                view! { <option value=m.id.clone()>{label}</option> }
                            }).collect_view()
                        }}
                    </select>
                </label>
                <Show when=move || surface.get() != "audio">
                    <label class="field">
                        <span>{t(T::Ratio)}</span>
                        <select class="input" data-testid="gen-aspect" prop:value=move || aspect.get() on:change=move |ev| aspect.set(event_target_value(&ev))>
                            {ASPECTS.into_iter().map(|a| view! { <option value=a>{a}</option> }).collect_view()}
                        </select>
                    </label>
                </Show>
                <Show when=move || surface.get() == "video">
                    <label class="field">
                        <span>{t(T::Duration)}</span>
                        <select class="input" prop:value=move || length.get().to_string() on:change=move |ev| length.set(event_target_value(&ev).parse().unwrap_or(5))>
                            {VIDEO_LENGTHS.into_iter().map(|s| view! { <option value=s.to_string()>{tf(T::Seconds, &[&s.to_string()])}</option> }).collect_view()}
                        </select>
                    </label>
                </Show>
                <Show when=move || surface.get() == "audio">
                    <label class="field">
                        <span>{t(T::Duration)}</span>
                        <select class="input" prop:value=move || duration.get().to_string() on:change=move |ev| duration.set(event_target_value(&ev).parse().unwrap_or(10))>
                            {AUDIO_DURATIONS.into_iter().map(|s| view! { <option value=s.to_string()>{tf(T::Seconds, &[&s.to_string()])}</option> }).collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span>{t(T::Voice)}</span>
                        <input class="input" placeholder=t(T::VoicePlaceholder) prop:value=move || voice.get() on:input=move |ev| voice.set(event_target_value(&ev))/>
                    </label>
                </Show>
                <label class="field">
                    <span>{t(T::SaveTo)}</span>
                    <select class="input" data-testid="gen-target" prop:value=move || target.get() on:change=move |ev| target.set(event_target_value(&ev))>
                        <option value=LIBRARY_PROJECT>{t(T::MediaLibraryProject)}</option>
                        {move || projects.get().into_iter().filter(|p| p.id != LIBRARY_PROJECT).map(|p| view! { <option value=p.id.clone()>{p.name.clone()}</option> }).collect_view()}
                    </select>
                </label>
            </div>
            <Show when=move || !provider_ready() && !models.with(Vec::is_empty)>
                <p class="callout" data-testid="gen-no-key">{icon(Glyph::Info)}{t(T::ProviderNeedsKey)}
                    <button class="btn small ghost" on:click=move |_| on_providers.run(())>{t(T::Providers)}</button>
                </p>
            </Show>
            {move || error.get().map(|e| view! { <p class="error-text" role="alert" data-testid="gen-error">{e}</p> })}
            <div class="row end">
                <button class="btn primary" data-testid="gen-submit" disabled=move || busy.get() || prompt.with(|p| p.trim().is_empty()) || model.with(String::is_empty) on:click=submit>
                    {icon(Glyph::Sparkle)}{t(T::AddToQueue)}
                </button>
            </div>
        </div>
    }
}

#[component]
fn QueueView(tasks: RwSignal<Vec<Task>>, projects: RwSignal<Vec<Project>>, on_change: Callback<()>) -> impl IntoView {
    let project_name = move |id: &str| projects.with(|l| l.iter().find(|p| p.id == id).map_or_else(|| id.to_owned(), |p| p.name.clone()));
    view! {
        <ul class="task-list" data-testid="media-queue">
            {move || {
                let list = tasks.get();
                if list.is_empty() {
                    return view! {
                        <li class="empty-state compact">
                            <span class="empty-icon">{icon(Glyph::Queue)}</span>
                            <p>{t(T::QueueEmpty)}</p>
                        </li>
                    }.into_any();
                }
                list.into_iter().map(|task| {
                    let id = task.id.clone();
                    let open = matches!(task.status.as_str(), "queued" | "running");
                    let file = task.result.as_ref().and_then(|r| r["name"].as_str()).map(str::to_owned);
                    let surface = task.request["surface"].as_str().unwrap_or("image").to_owned();
                    let state = format!("badge run-state {}", match task.status.as_str() { "done" => "succeeded", "queued" => "running", s => s });
                    let raw = file.as_ref().map(|f| format!("/api/projects/{}/raw/{}", api::seg(&task.project_id), api::file_path(f)));
                    view! {
                        <li class="card task-row" data-testid="media-task" data-status=task.status.clone()>
                            <span class="routine-icon">{icon(kind_glyph(&surface))}</span>
                            <div class="grow">
                                <p class="task-prompt">{task.request["prompt"].as_str().unwrap_or_default().to_owned()}</p>
                                <p class="subtle small">{task.request["model"].as_str().unwrap_or_default().to_owned()}" · "{project_name(&task.project_id)}" · "{ago(task.created_at)}</p>
                                {task.error.clone().map(|e| view! { <p class="error-text small">{e}</p> })}
                                {raw.filter(|_| surface == "audio").map(|src| view! { <audio src=src controls=true class="media-audio"></audio> })}
                            </div>
                            <span class=state>{status_label(&task.status)}</span>
                            {open.then(|| view! {
                                <button class="btn small ghost" data-testid="media-task-cancel" on:click=move |_| {
                                    let id = id.clone();
                                    leptos::task::spawn_local(async move {
                                        let _ = api::post::<_, Value>(&format!("/api/media/tasks/{}/cancel", api::seg(&id)), &json!({})).await;
                                        on_change.run(());
                                    });
                                }>{icon(Glyph::Stop)}{t(T::Cancel)}</button>
                            })}
                        </li>
                    }
                }).collect_view().into_any()
            }}
        </ul>
    }
}

/// Provider display names and where their keys come from.
const PROVIDERS: [(&str, &str, &str); 3] = [
    ("openai", "OpenAI", "https://api.openai.com/v1"),
    ("fal", "fal.ai", "https://queue.fal.run"),
    ("elevenlabs", "ElevenLabs", "https://api.elevenlabs.io"),
];

#[component]
fn ProvidersView() -> impl IntoView {
    let config = RwSignal::new(None::<Value>);
    let keys = RwSignal::new(std::collections::HashMap::<String, String>::new());
    let urls = RwSignal::new(std::collections::HashMap::<String, String>::new());
    let status = RwSignal::new(None::<Result<(), String>>);
    let load = move || {
        leptos::task::spawn_local(async move {
            if let Ok(v) = api::get::<Value>("/api/media/config").await {
                urls.set(PROVIDERS.iter().map(|(id, _, _)| ((*id).to_owned(), v["providers"][id]["baseUrl"].as_str().unwrap_or_default().to_owned())).collect());
                config.set(Some(v));
            }
        });
    };
    load();
    let save = move |_| {
        let mut providers = serde_json::Map::new();
        for (id, _, _) in PROVIDERS {
            let key = keys.with_untracked(|k| k.get(id).cloned().unwrap_or_default());
            let url = urls.with_untracked(|u| u.get(id).cloned().unwrap_or_default());
            let model = config.with_untracked(|c| c.as_ref().and_then(|c| c["providers"][id]["model"].as_str().map(str::to_owned)));
            providers.insert(id.to_owned(), json!({ "apiKey": key, "preserveApiKey": key.is_empty(), "baseUrl": url, "model": model }));
        }
        let body = json!({ "providers": providers });
        leptos::task::spawn_local(async move {
            match api::put::<_, Value>("/api/media/config", &body).await {
                Ok(v) => {
                    keys.set(Default::default());
                    config.set(Some(v));
                    status.set(Some(Ok(())));
                }
                Err(e) => status.set(Some(Err(e.to_string()))),
            }
        });
    };
    view! {
        <div class="providers" data-testid="media-providers">
            <p class="muted">{t(T::ProvidersHint)}</p>
            {PROVIDERS.into_iter().map(|(id, name, default_url)| {
                let configured = move || config.with(|c| c.as_ref().is_some_and(|c| c["providers"][id]["configured"] == true));
                let tail = move || config.with(|c| c.as_ref().and_then(|c| c["providers"][id]["apiKeyTail"].as_str().map(str::to_owned)).unwrap_or_default());
                let from_env = move || config.with(|c| c.as_ref().is_some_and(|c| c["providers"][id]["source"] == "env"));
                view! {
                    <section class="card padded provider-card" data-testid=format!("provider-{id}")>
                        <div class="row">
                            <h3 class="grow">{name}</h3>
                            {move || if configured() {
                                view! { <span class="badge success">{if from_env() { t(T::KeyFromEnv).to_owned() } else { tf(T::KeySaved, &[&tail()]) }}</span> }.into_any()
                            } else {
                                view! { <span class="badge outline">{t(T::NoKey)}</span> }.into_any()
                            }}
                        </div>
                        <div class="gen-options">
                            <label class="field">
                                <span>{t(T::ApiKey)}</span>
                                <input class="input mono" type="password" autocomplete="off" data-testid=format!("provider-key-{id}") placeholder=t(T::ApiKeyPlaceholder)
                                    prop:value=move || keys.with(|k| k.get(id).cloned().unwrap_or_default())
                                    on:input=move |ev| { let v = event_target_value(&ev); keys.update(|k| { k.insert(id.to_owned(), v); }); }/>
                            </label>
                            <label class="field">
                                <span>{t(T::BaseUrl)}</span>
                                <input class="input mono" data-testid=format!("provider-url-{id}") placeholder=default_url
                                    prop:value=move || urls.with(|u| u.get(id).cloned().unwrap_or_default())
                                    on:input=move |ev| { let v = event_target_value(&ev); urls.update(|u| { u.insert(id.to_owned(), v); }); }/>
                            </label>
                        </div>
                    </section>
                }
            }).collect_view()}
            <div class="save-bar">
                {move || match status.get() {
                    Some(Ok(())) => view! { <span class="ok-text save-status" data-testid="providers-saved">{icon(Glyph::Check)}{t(T::Saved)}</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text">{e}</span> }.into_any(),
                    None => ().into_any(),
                }}
                <button class="btn primary" data-testid="providers-save" on:click=save>{t(T::Save)}</button>
            </div>
        </div>
    }
}

#[component]
fn Mp4View(projects: RwSignal<Vec<Project>>, on_saved: Callback<()>) -> impl IntoView {
    let project = RwSignal::new(String::new());
    let fps = RwSignal::new(30u32);
    let busy = RwSignal::new(false);
    let status = RwSignal::new(None::<Result<String, String>>);
    let render = move |_| {
        let pid = project.get_untracked();
        if pid.is_empty() {
            return;
        }
        busy.set(true);
        status.set(None);
        let body = json!({ "fps": fps.get_untracked(), "output": "media/hyperframes.mp4" });
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>(&format!("/api/projects/{}/export/mp4", api::seg(&pid)), &body).await {
                Ok(v) => {
                    status.set(Some(Ok(tf(T::Mp4Saved, &[v["file"]["name"].as_str().unwrap_or("media/hyperframes.mp4")]))));
                    on_saved.run(());
                }
                Err(e) => status.set(Some(Err(e.to_string()))),
            }
            busy.set(false);
        });
    };
    view! {
        <div class="card padded media-generate" data-testid="media-mp4">
            <p class="muted">{t(T::Mp4Hint)}</p>
            <div class="gen-options">
                <label class="field">
                    <span>{t(T::ProjectLabel)}</span>
                    <select class="input" data-testid="mp4-project" prop:value=move || project.get() on:change=move |ev| project.set(event_target_value(&ev))>
                        <option value="">{t(T::ChooseProject)}</option>
                        {move || projects.get().into_iter().map(|p| view! { <option value=p.id.clone()>{p.name.clone()}</option> }).collect_view()}
                    </select>
                </label>
                <label class="field">
                    <span>{t(T::FramesPerSecond)}</span>
                    <select class="input" prop:value=move || fps.get().to_string() on:change=move |ev| fps.set(event_target_value(&ev).parse().unwrap_or(30))>
                        {[24u32, 30, 60].into_iter().map(|f| view! { <option value=f.to_string()>{f}</option> }).collect_view()}
                    </select>
                </label>
            </div>
            {move || status.get().map(|s| match s {
                Ok(m) => view! { <p class="ok-text" role="status" data-testid="mp4-status">{icon(Glyph::Check)}{m}</p> }.into_any(),
                Err(e) => view! { <p class="error-text" role="alert" data-testid="mp4-status">{e}</p> }.into_any(),
            })}
            <div class="row end">
                <button class="btn primary" data-testid="mp4-render" disabled=move || busy.get() || project.with(String::is_empty) on:click=render>
                    {move || if busy.get() { view! { <span class="spinner on-accent"></span> }.into_any() } else { icon(Glyph::Video).into_any() }}
                    {t(T::RenderMp4)}
                </button>
            </div>
        </div>
    }
}
