//! Projects page (spec 1.1): grid, list and board views with the run
//! status of each project, type filter, sort, "Recently viewed", search over
//! projects and conversations, multi-select delete, rename, duplicate,
//! project instructions, the new-project panel, Claude Design ZIP import
//! and "Open folder" (the native picker on desktop, a path field in a
//! browser).

use super::icons::{Glyph, icon};
use super::lazy_frame::LazyFrame;
use super::manage::{self, RunState, Sort, Tag};
use super::new_project::NewProjectPanel;
use super::{ago, api, desktop};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DuplicateProjectResponse, OkResponse, Project, ProjectResponse,
    ProjectsResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// How often run states refresh while the page is open.
const STATUS_POLL: Duration = Duration::from_secs(5);
/// Search waits this long after the last key.
const SEARCH_DEBOUNCE_MS: u32 = 200;
const VIEW_KEY: &str = "od.projects.view";
const SORT_KEY: &str = "od.projects.sort";
/// Longest project instructions, as the original.
const MAX_INSTRUCTIONS: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Grid,
    List,
    Board,
}

impl View {
    fn id(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::List => "list",
            Self::Board => "board",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "list" => Self::List,
            "board" => Self::Board,
            _ => Self::Grid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Collection {
    All,
    Recent,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    kind: String,
    project_id: String,
    project_name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
    updated_at: i64,
}

#[derive(Debug, Deserialize)]
struct HitsResponse {
    hits: Vec<Hit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentView {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct ViewsResponse {
    views: Vec<RecentView>,
}

/// Page state shared by the views.
#[derive(Clone, Copy)]
struct Page {
    projects: RwSignal<Option<Vec<Project>>>,
    statuses: RwSignal<BTreeMap<String, RunState>>,
    live: RwSignal<BTreeMap<String, Vec<Value>>>,
    selecting: RwSignal<bool>,
    selected: RwSignal<BTreeSet<String>>,
    error: RwSignal<Option<String>>,
}

impl Page {
    fn reload(self) {
        leptos::task::spawn_local(async move {
            match api::get::<ProjectsResponse>("/api/projects").await {
                Ok(r) => {
                    let ids: Vec<String> = r.projects.iter().map(|p| p.id.clone()).collect();
                    self.projects.set(Some(r.projects));
                    self.statuses.set(manage::fetch_statuses().await);
                    let mut live = BTreeMap::new();
                    for id in ids {
                        let path = format!("/api/live-artifacts?projectId={}", api::seg(&id));
                        if let Ok(v) = api::get::<Value>(&path).await {
                            let list = v["artifacts"].as_array().cloned().unwrap_or_default();
                            if !list.is_empty() {
                                live.insert(id, list);
                            }
                        }
                    }
                    self.live.set(live);
                }
                Err(e) => self.error.set(Some(e.to_string())),
            }
        });
    }

    fn status(self, id: &str) -> RunState {
        self.statuses
            .with(|s| s.get(id).copied().unwrap_or(RunState::NotStarted))
    }
}

#[component]
pub fn Projects() -> impl IntoView {
    let navigate = use_navigate();
    let page = Page {
        projects: RwSignal::new(None),
        statuses: RwSignal::new(BTreeMap::new()),
        live: RwSignal::new(BTreeMap::new()),
        selecting: RwSignal::new(false),
        selected: RwSignal::new(BTreeSet::new()),
        error: RwSignal::new(None),
    };
    page.reload();
    if let Ok(handle) = set_interval_with_handle(
        move || {
            leptos::task::spawn_local(async move {
                page.statuses.set(manage::fetch_statuses().await);
            });
        },
        STATUS_POLL,
    ) {
        on_cleanup(move || handle.clear());
    }

    let query = RwSignal::new(String::new());
    let tag = RwSignal::new(None::<Tag>);
    let sort = RwSignal::new(Sort::parse(&manage::stored(SORT_KEY).unwrap_or_default()));
    let view = RwSignal::new(View::parse(&manage::stored(VIEW_KEY).unwrap_or_default()));
    let collection = RwSignal::new(Collection::All);
    let recent = RwSignal::new(Vec::<String>::new());
    let hits = RwSignal::new(Vec::<Hit>::new());
    let panel = RwSignal::new(false);
    let confirm_bulk = RwSignal::new(false);
    let notice = RwSignal::new(None::<String>);

    // Search over projects, conversations and messages, debounced.
    let search_seq = StoredValue::new(0u32);
    Effect::new(move |_| {
        let q = query.get();
        search_seq.update_value(|n| *n += 1);
        let seq = search_seq.get_value();
        if q.trim().is_empty() {
            hits.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(SEARCH_DEBOUNCE_MS).await;
            if search_seq.get_value() != seq {
                return;
            }
            let path = format!("/api/search?q={}", api::seg(q.trim()));
            if let Ok(r) = api::get::<HitsResponse>(&path).await {
                if search_seq.get_value() == seq {
                    hits.set(r.hits);
                }
            }
        });
    });
    Effect::new(move |_| {
        if collection.get() == Collection::Recent {
            leptos::task::spawn_local(async move {
                if let Ok(r) = api::get::<ViewsResponse>("/api/recently-viewed").await {
                    recent.set(r.views.into_iter().map(|v| v.project_id).collect());
                }
            });
        }
    });

    let shown = Memo::new(move |_| {
        let list = page.projects.get()?;
        let mut out = manage::filtered(&list, tag.get(), &query.get(), sort.get());
        if collection.get() == Collection::Recent {
            let order = recent.get();
            out.retain(|p| order.contains(&p.id));
            out.sort_by_key(|p| order.iter().position(|id| id == &p.id));
        }
        Some(out)
    });

    // Imports `dir` and opens its Studio.
    let nav = navigate.clone();
    let import = move |dir: String| {
        let navigate = nav.clone();
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateProjectResponse>(
                "/api/import/folder",
                &json!({ "baseDir": dir }),
            )
            .await
            {
                Ok(r) => navigate(&format!("/projects/{}", r.project.id), Default::default()),
                Err(e) => page.error.set(Some(e.to_string())),
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

    // Claude Design ZIP import.
    let zip_input = NodeRef::<leptos::html::Input>::new();
    let importing = RwSignal::new(false);
    let nav_zip = navigate.clone();
    let on_zip = move |_| {
        let Some(input) = zip_input.get_untracked() else {
            return;
        };
        let Some(file) = input.files().and_then(|f| f.get(0)) else {
            return;
        };
        input.set_value("");
        let navigate = nav_zip.clone();
        importing.set(true);
        leptos::task::spawn_local(async move {
            match import_zip(&file).await {
                Ok(id) => navigate(&format!("/projects/{id}"), Default::default()),
                Err(e) => page.error.set(Some(tf(T::ImportZipFailed, &[&e]))),
            }
            importing.set(false);
        });
    };

    let delete_selected = move |_| {
        let ids: Vec<String> = page.selected.get_untracked().into_iter().collect();
        confirm_bulk.set(false);
        leptos::task::spawn_local(async move {
            let mut failed = 0usize;
            for id in &ids {
                let path = format!("/api/projects/{}", api::seg(id));
                if api::delete::<OkResponse>(&path).await.is_err() {
                    failed += 1;
                }
            }
            let deleted = ids.len() - failed;
            notice.set(Some(if failed == 0 {
                tf(T::DeletedProjects, &[&deleted.to_string()])
            } else {
                tf(
                    T::DeletedProjectsPartial,
                    &[&deleted.to_string(), &failed.to_string()],
                )
            }));
            page.selected.set(BTreeSet::new());
            page.selecting.set(false);
            page.reload();
        });
    };

    let set_view = move |v: View| {
        view.set(v);
        manage::store(VIEW_KEY, v.id());
        if v == View::Board {
            page.selecting.set(false);
            page.selected.set(BTreeSet::new());
        }
    };
    let view_button = move |v: View, glyph: Glyph, label: T| {
        view! {
            <button
                class="seg"
                data-testid=format!("view-{}", v.id())
                aria-label=t(label)
                data-tip=t(label)
                aria-pressed=move || (view.get() == v).to_string()
                class:active=move || view.get() == v
                on:click=move |_| set_view(v)
            >
                {icon(glyph)}
            </button>
        }
    };

    view! {
        <section class="page wide projects-page">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::ProjectsTitle)}</h1>
                    {move || page.projects.with(|p| p.as_ref().filter(|l| !l.is_empty()).map(|l| {
                        view! { <span class="count-badge">{l.len()}</span> }
                    }))}
                </div>
                <div class="row">
                    <input
                        type="file"
                        accept=".zip,application/zip"
                        class="visually-hidden"
                        tabindex="-1"
                        aria-hidden="true"
                        aria-label=t(T::ImportZip)
                        data-testid="import-zip-input"
                        node_ref=zip_input
                        on:change=on_zip
                    />
                    <button
                        class="btn"
                        data-testid="import-zip"
                        data-tip=t(T::ImportZipTitle)
                        disabled=move || importing.get()
                        on:click=move |_| {
                            if let Some(input) = zip_input.get_untracked() {
                                input.click();
                            }
                        }
                    >
                        {move || if importing.get() {
                            view! { <span class="spinner"></span>{t(T::ImportingZip)} }.into_any()
                        } else {
                            view! { {icon(Glyph::Zip)}{t(T::ImportZip)} }.into_any()
                        }}
                    </button>
                    <button class="btn" data-testid="open-folder" aria-expanded=move || path_field.with(Option::is_some).to_string() on:click=open_folder>
                        {icon(Glyph::FolderOpen)}
                        {t(T::OpenFolder)}
                    </button>
                    <button class="btn primary" data-testid="new-project" aria-haspopup="dialog" on:click=move |_| panel.set(true)>
                        {icon(Glyph::Add)}{t(T::NewProject)}
                    </button>
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

            <div class="projects-toolbar">
                <div class="segmented small" role="tablist" aria-label=t(T::ProjectCollections)>
                    <button class="seg" role="tab" data-testid="collection-all"
                        aria-selected=move || (collection.get() == Collection::All).to_string()
                        class:active=move || collection.get() == Collection::All
                        on:click=move |_| collection.set(Collection::All)>{t(T::CollectionAll)}</button>
                    <button class="seg" role="tab" data-testid="collection-recent"
                        aria-selected=move || (collection.get() == Collection::Recent).to_string()
                        class:active=move || collection.get() == Collection::Recent
                        on:click=move |_| collection.set(Collection::Recent)>{t(T::CollectionRecent)}</button>
                </div>
                <label class="search-field grow">
                    {icon(Glyph::Search)}
                    <input
                        class="input small"
                        type="search"
                        data-testid="project-search"
                        aria-label=t(T::SearchProjects)
                        placeholder=t(T::SearchProjectsPlaceholder)
                        prop:value=move || query.get()
                        on:input=move |ev| query.set(event_target_value(&ev))
                    />
                </label>
                <select
                    class="small select-auto"
                    data-testid="project-type-filter"
                    aria-label=t(T::FilterByType)
                    on:change=move |ev| tag.set(Tag::parse(&event_target_value(&ev)))
                >
                    <option value="" selected=move || tag.get().is_none()>{t(T::AnyType)}</option>
                    {Tag::ALL.into_iter().map(|tg| view! {
                        <option value=tg.id() selected=move || tag.get() == Some(tg)>{tg.label()}</option>
                    }).collect_view()}
                </select>
                <select
                    class="small select-auto"
                    data-testid="project-sort"
                    aria-label=t(T::SortBy)
                    on:change=move |ev| {
                        let s = Sort::parse(&event_target_value(&ev));
                        sort.set(s);
                        manage::store(SORT_KEY, s.id());
                    }
                >
                    {Sort::ALL.into_iter().map(|s| view! {
                        <option value=s.id() selected=move || sort.get() == s>{s.label()}</option>
                    }).collect_view()}
                </select>
                <div class="segmented small icons" role="group" aria-label=t(T::ViewMode)>
                    {view_button(View::Grid, Glyph::Grid, T::ViewGrid)}
                    {view_button(View::List, Glyph::List, T::ViewList)}
                    {view_button(View::Board, Glyph::Board, T::ViewBoard)}
                </div>
                <Show when=move || view.get() != View::Board>
                    <button
                        class="btn small"
                        data-testid="select-mode"
                        aria-pressed=move || page.selecting.get().to_string()
                        on:click=move |_| {
                            page.selecting.update(|s| *s = !*s);
                            page.selected.set(BTreeSet::new());
                            confirm_bulk.set(false);
                        }
                    >
                        {icon(Glyph::MultiSelect)}{t(T::SelectMode)}
                    </button>
                </Show>
            </div>

            <Show when=move || page.selecting.get()>
                <div class="selection-bar card" data-testid="selection-bar" role="region" aria-label=t(T::SelectMode)>
                    <strong>{move || tf(T::SelectedCount, &[&page.selected.with(BTreeSet::len).to_string()])}</strong>
                    <span class="spacer"></span>
                    {move || if confirm_bulk.get() {
                        view! {
                            <span class="confirm">{tf(T::ConfirmDeleteSelected, &[&page.selected.with(BTreeSet::len).to_string()])}</span>
                            <button class="btn small" on:click=move |_| confirm_bulk.set(false)>{t(T::Cancel)}</button>
                            <button class="btn small danger" data-testid="confirm-delete-selected" on:click=delete_selected>{t(T::Delete)}</button>
                        }.into_any()
                    } else {
                        view! {
                            <button class="btn small ghost" on:click=move |_| {
                                page.selecting.set(false);
                                page.selected.set(BTreeSet::new());
                            }>{t(T::Cancel)}</button>
                            <button
                                class="btn small danger"
                                data-testid="delete-selected"
                                disabled=move || page.selected.with(BTreeSet::is_empty)
                                on:click=move |_| confirm_bulk.set(true)
                            >{icon(Glyph::Delete)}{t(T::DeleteSelected)}</button>
                        }.into_any()
                    }}
                </div>
            </Show>

            {move || notice.get().map(|n| view! { <p class="ok-text" role="status" data-testid="projects-notice">{n}</p> })}
            {move || page.error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
            <SearchHits hits=hits query=query/>

            {move || match (shown.get(), view.get()) {
                (None, _) => view! {
                    <div class="project-grid large">
                        {(0..6).map(|_| view! { <div class="skeleton card-skeleton"></div> }).collect_view()}
                    </div>
                }.into_any(),
                (Some(list), _) if list.is_empty() => {
                    let none_at_all = page.projects.with(|p| p.as_ref().is_some_and(Vec::is_empty));
                    view! {
                        <div class="empty-state">
                            <span class="empty-icon">{icon(Glyph::Projects)}</span>
                            <strong>{if none_at_all { t(T::ProjectsEmptyTitle) } else { t(T::NoProjectsMatch) }}</strong>
                            {none_at_all.then(|| view! {
                                <p>{t(T::ProjectsEmptyBody)}</p>
                                <A href="/" attr:class="btn primary">{icon(Glyph::Add)}{t(T::NewProject)}</A>
                            })}
                        </div>
                    }.into_any()
                }
                (Some(list), View::Board) => view! { <Board list=list page=page/> }.into_any(),
                (Some(list), View::List) => view! {
                    <ul class="project-list" data-testid="project-list">
                        {list.into_iter().map(|p| view! { <ProjectRow project=p page=page compact=true/> }).collect_view()}
                    </ul>
                }.into_any(),
                (Some(list), View::Grid) => view! {
                    <div class="project-grid large" data-testid="project-grid">
                        {list.into_iter().map(|p| view! { <ProjectRow project=p page=page compact=false/> }).collect_view()}
                    </div>
                }.into_any(),
            }}
            <Show when=move || panel.get()>
                <NewProjectPanel open=panel/>
            </Show>
        </section>
    }
}

/// `POST /api/import/claude-design` with `file`; the new project's id.
async fn import_zip(file: &web_sys::File) -> Result<String, String> {
    let form = web_sys::FormData::new().map_err(|_| "FormData".to_owned())?;
    form.append_with_blob_and_filename("file", file, &file.name())
        .map_err(|_| "FormData".to_owned())?;
    let req = gloo_net::http::Request::post("/api/import/claude-design")
        .body(form)
        .map_err(|e| e.to_string())?;
    let res = req.send().await.map_err(|e| e.to_string())?;
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !res.ok() {
        return Err(body["error"]["message"]
            .as_str()
            .map_or_else(|| format!("HTTP {}", res.status()), str::to_owned));
    }
    body["project"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "unexpected answer".to_owned())
}

/// Search results under the toolbar: projects, conversations, messages.
#[component]
fn SearchHits(hits: RwSignal<Vec<Hit>>, query: RwSignal<String>) -> impl IntoView {
    move || {
        let list = hits.get();
        if query.with(|q| q.trim().is_empty()) || list.is_empty() {
            return None;
        }
        Some(view! {
            <div class="card search-hits" data-testid="search-hits">
                <span class="eyebrow">{t(T::SearchResults)}</span>
                <ul>
                    {list.into_iter().map(|h| {
                        let (glyph, kind) = match h.kind.as_str() {
                            "project" => (Glyph::Projects, t(T::HitProject)),
                            "conversation" => (Glyph::Chat, t(T::HitConversation)),
                            _ => (Glyph::Comment, t(T::HitMessage)),
                        };
                        let title = h.title.clone().unwrap_or_else(|| t(T::UntitledConversation).to_owned());
                        let in_conversation = h.kind != "project";
                        view! {
                            <li>
                                <A href=format!("/projects/{}", h.project_id) attr:class="search-hit" attr:data-testid="search-hit" attr:data-kind=h.kind.clone()>
                                    {icon(glyph)}
                                    <span class="grow">
                                        <span class="hit-title">
                                            {h.project_name.clone()}
                                            {in_conversation.then(|| view! { <span class="subtle">" · "{title}</span> })}
                                        </span>
                                        {h.snippet.clone().map(|s| view! { <span class="hit-snippet">{s}</span> })}
                                    </span>
                                    <span class="badge outline">{kind}</span>
                                    <span class="subtle small">{ago(h.updated_at)}</span>
                                </A>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            </div>
        })
    }
}

/// A project's cover: its entry page (`entryFile` from the list) rendered
/// small once it scrolls into view, else the kind's icon.
#[component]
fn ProjectCover(project: Project) -> impl IntoView {
    let glyph = Tag::of(&project).glyph();
    let src = project.entry_file.as_deref().map(|path| {
        format!(
            "/api/projects/{}/raw/{}",
            api::seg(&project.id),
            api::file_path(path)
        )
    });
    view! {
        <span class="cover project-cover" aria-hidden="true">
            {match src {
                Some(src) => view! { <LazyFrame src=src/> }.into_any(),
                None => view! { <span class="cover-glyph">{icon(glyph)}</span> }.into_any(),
            }}
        </span>
    }
}

/// The run-state badge of a card.
#[component]
pub fn StatusBadge(#[prop(into)] state: Signal<RunState>) -> impl IntoView {
    view! {
        <span
            class=move || format!("{} run-status", state.get().badge_class())
            data-testid="project-status"
            data-status=move || state.get().as_str()
        >
            {move || (state.get() == RunState::Running).then(|| view! { <span class="status-dot" aria-hidden="true"></span> })}
            {move || state.get().label()}
        </span>
    }
}

/// "Live", plus refresh and archive state, for projects with live artifacts.
#[component]
fn LiveBadges(artifacts: Vec<Value>) -> impl IntoView {
    if artifacts.is_empty() {
        return None;
    }
    let refreshing = artifacts.iter().any(|a| a["refreshStatus"] == "running");
    let failed = artifacts.iter().any(|a| a["refreshStatus"] == "failed");
    let archived = artifacts.iter().all(|a| a["status"] == "archived");
    Some(view! {
        <span class="live-badges" aria-label=t(T::LiveArtifactBadges) data-testid="live-badges">
            <span class="badge accent">{icon(Glyph::LiveArtifact)}{tf(T::LiveCount, &[&artifacts.len().to_string()])}</span>
            {refreshing.then(|| view! { <span class="badge">{t(T::LiveRefreshing)}</span> })}
            {failed.then(|| view! { <span class="badge danger">{t(T::LiveRefreshFailed)}</span> })}
            {archived.then(|| view! { <span class="badge outline">{t(T::LiveArchived)}</span> })}
        </span>
    })
}

/// How many recent projects Home lists.
const RECENT: usize = 6;

/// Home's "Recent projects": newest first, filtered by type, with run state.
#[component]
pub fn RecentStrip() -> impl IntoView {
    let projects = RwSignal::new(None::<Vec<Project>>);
    let statuses = RwSignal::new(BTreeMap::<String, RunState>::new());
    let tag = RwSignal::new(None::<Tag>);
    leptos::task::spawn_local(async move {
        let list = api::get::<ProjectsResponse>("/api/projects")
            .await
            .map(|r| r.projects)
            .unwrap_or_default();
        projects.set(Some(list));
        statuses.set(manage::fetch_statuses().await);
    });
    view! {
        <section class="home-section" data-testid="recent-projects">
            <div class="section-head">
                <h2>{t(T::RecentProjects)}</h2>
                <span class="spacer"></span>
                <select class="small select-auto" data-testid="recent-type-filter" aria-label=t(T::FilterByType)
                    on:change=move |ev| tag.set(Tag::parse(&event_target_value(&ev)))>
                    <option value="">{t(T::AnyType)}</option>
                    {Tag::ALL.into_iter().map(|tg| view! { <option value=tg.id()>{tg.label()}</option> }).collect_view()}
                </select>
                <A href="/projects" attr:class="btn ghost small">
                    {t(T::AllProjects)}
                    {icon(Glyph::ChevronRight)}
                </A>
            </div>
            <div class="project-grid">
                {move || match projects.get() {
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
                    Some(list) => {
                        let mut shown = manage::filtered(&list, tag.get(), "", Sort::Newest);
                        shown.truncate(RECENT);
                        if shown.is_empty() {
                            return view! { <p class="muted small">{t(T::NoProjectsMatch)}</p> }.into_any();
                        }
                        shown.into_iter()
                            .map(|p| {
                                let status = statuses.with(|s| s.get(&p.id).copied());
                                view! { <ProjectCard project=p status=status.unwrap_or(RunState::NotStarted)/> }
                            })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </section>
    }
}

/// A project card linking to its Studio (Home's recent list).
#[component]
pub fn ProjectCard(project: Project, #[prop(optional)] status: Option<RunState>) -> impl IntoView {
    let tag = Tag::of(&project);
    let href = format!("/projects/{}", project.id);
    view! {
        <A href=href attr:class="project-card card interactive">
            <ProjectCover project=project.clone()/>
            <span class="project-body">
                <span class="project-name">{project.name.clone()}</span>
                <span class="project-meta">
                    <span class="badge">{tag.label()}</span>
                    {status.filter(|s| *s != RunState::NotStarted).map(|s| view! { <StatusBadge state=s/> })}
                    <span>{ago(project.updated_at)}</span>
                </span>
            </span>
        </A>
    }
}

#[component]
fn ProjectRow(project: Project, page: Page, compact: bool) -> impl IntoView {
    let id = StoredValue::new(project.id.clone());
    let name = RwSignal::new(project.name.clone());
    let editing = RwSignal::new(false);
    let confirming = RwSignal::new(false);
    let instructions_open = RwSignal::new(false);
    let instructions = RwSignal::new(project.custom_instructions.clone().unwrap_or_default());
    let tag = Tag::of(&project);
    let imported = project.metadata.as_ref().and_then(|m| m.base_dir.clone());
    let updated = project.updated_at;
    let live = Memo::new(move |_| {
        page.live
            .with(|l| l.get(&id.get_value()).cloned().unwrap_or_default())
    });
    let state = Signal::derive(move || page.status(&id.get_value()));
    let checked = move || page.selected.with(|s| s.contains(&id.get_value()));
    let toggle_select = move || {
        page.selected.update(|s| {
            let key = id.get_value();
            if !s.remove(&key) {
                s.insert(key);
            }
        });
    };

    // The last name the daemon accepted: a refused rename goes back to it.
    let saved = StoredValue::new(project.name.clone());
    let rename = move || {
        editing.set(false);
        let Some(new_name) = rename_input(&name.get_untracked()) else {
            name.set(saved.get_value());
            page.error.set(Some(t(T::ProjectNameRequired).to_string()));
            return;
        };
        name.set(new_name.clone());
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}", api::seg(&id.get_value()));
            match api::patch::<_, ProjectResponse>(&path, &json!({ "name": new_name })).await {
                Ok(_) => {
                    saved.set_value(new_name);
                    page.error.set(None);
                }
                Err(e) => {
                    name.set(saved.get_value());
                    page.error.set(Some(e.to_string()));
                }
            }
            page.reload();
        });
    };
    let rename_key = rename;
    let duplicate = move |_| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}/duplicate", api::seg(&id.get_value()));
            if let Err(e) = api::post::<_, DuplicateProjectResponse>(&path, &json!({})).await {
                page.error.set(Some(e.to_string()));
            }
            page.reload();
        });
    };
    let delete = move |_| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}", api::seg(&id.get_value()));
            if let Err(e) = api::delete::<OkResponse>(&path).await {
                page.error.set(Some(e.to_string()));
            }
            page.reload();
        });
    };
    let href = move || format!("/projects/{}", id.get_value());
    let project_id = project.id.clone();

    view! {
        <article
            class=if compact { "project-row card" } else { "project-card card interactive" }
            class:selected=checked
            data-testid="project-card"
            data-project=project_id
            data-tag=tag.id()
        >
            <Show when=move || page.selecting.get()>
                <label class="select-check" aria-label=t(T::SelectProject)>
                    <input type="checkbox" data-testid="project-select" prop:checked=checked on:change=move |_| toggle_select()/>
                </label>
            </Show>
            <A href=href attr:class="cover-link" attr:tabindex="-1" attr:aria-hidden="true">
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
                                            name.set(saved.get_value());
                                            editing.set(false);
                                        }
                                    }
                                }
                            />
                        }
                            .into_any()
                    } else {
                        view! {
                            <A href=href attr:class="project-name">
                                {move || name.get()}
                            </A>
                        }
                            .into_any()
                    }
                }}
                <span class="project-meta">
                    <span class="badge" data-testid="project-tag">{tag.label()}</span>
                    <StatusBadge state=state/>
                    <span>{ago(updated)}</span>
                </span>
                {move || {
                    let l = live.get();
                    view! { <LiveBadges artifacts=l/> }
                }}
                {move || instructions.with(|i| !i.trim().is_empty()).then(|| view! {
                    <span class="project-path" data-testid="instructions-active">{icon(Glyph::Notes)}{t(T::InstructionsActive)}</span>
                })}
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
                            <button class="icon-btn" data-testid="project-instructions" aria-label=t(T::ProjectInstructions) data-tip=t(T::ProjectInstructions) aria-haspopup="dialog" on:click=move |_| instructions_open.set(true)>
                                {icon(Glyph::Notes)}
                            </button>
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
            <Show when=move || instructions_open.get()>
                <InstructionsDialog id=id.get_value() text=instructions open=instructions_open/>
            </Show>
        </article>
    }
}

/// Project instructions: injected into every message of the project.
#[component]
fn InstructionsDialog(id: String, text: RwSignal<String>, open: RwSignal<bool>) -> impl IntoView {
    let draft = RwSignal::new(text.get_untracked());
    let error = RwSignal::new(None::<String>);
    let id = StoredValue::new(id);
    let save = move |_| {
        let value = draft.get_untracked();
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}", api::seg(&id.get_value()));
            let body = if value.trim().is_empty() {
                json!({ "customInstructions": null })
            } else {
                json!({ "customInstructions": value.trim() })
            };
            match api::patch::<_, ProjectResponse>(&path, &body).await {
                Ok(r) => {
                    text.set(r.project.custom_instructions.unwrap_or_default());
                    open.set(false);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    view! {
        <div class="overlay center" on:click=move |_| open.set(false)
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { open.set(false) }>
            <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="instructions-title" data-testid="instructions-dialog" on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Notes)}</span>
                    <div class="grow">
                        <h2 id="instructions-title">{t(T::ProjectInstructions)}</h2>
                        <p>{t(T::ProjectInstructionsHint)}</p>
                    </div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| open.set(false)>{icon(Glyph::Close)}</button>
                </div>
                <div class="dialog-body">
                    <textarea
                        class="input"
                        rows="8"
                        maxlength=MAX_INSTRUCTIONS.to_string()
                        data-testid="instructions-text"
                        aria-label=t(T::ProjectInstructions)
                        placeholder=t(T::ProjectInstructionsPlaceholder)
                        prop:value=move || draft.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))
                    ></textarea>
                    <span class="muted small">{move || format!("{} / {MAX_INSTRUCTIONS}", draft.with(|d| d.chars().count()))}</span>
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                </div>
                <div class="dialog-foot">
                    <button class="btn ghost" on:click=move |_| open.set(false)>{t(T::Cancel)}</button>
                    <button class="btn primary" data-testid="instructions-save" on:click=save>{t(T::Save)}</button>
                </div>
            </div>
        </div>
    }
}

/// Board view: one column per run state, no drag and drop (as the
/// original: the column is the project's state).
#[component]
fn Board(list: Vec<Project>, page: Page) -> impl IntoView {
    let columns: Vec<(RunState, Vec<Project>)> = RunState::ALL
        .into_iter()
        .map(|state| {
            let cards = list
                .iter()
                .filter(|p| page.status(&p.id) == state)
                .cloned()
                .collect();
            (state, cards)
        })
        .collect();
    view! {
        <div class="board" data-testid="project-board">
            {columns.into_iter().map(|(state, cards)| view! {
                <section class="board-column" data-testid="board-column" data-status=state.as_str()>
                    <header class="board-head">
                        <span class=format!("board-bar {}", state.as_str()) aria-hidden="true"></span>
                        <span class="grow">{state.label()}</span>
                        <span class="count-badge">{cards.len()}</span>
                    </header>
                    {if cards.is_empty() {
                        view! { <p class="muted small board-empty">{t(T::BoardEmptyColumn)}</p> }.into_any()
                    } else {
                        cards.into_iter().map(|p| view! { <BoardCard project=p page=page/> }).collect_view().into_any()
                    }}
                </section>
            }).collect_view()}
        </div>
    }
}

#[component]
fn BoardCard(project: Project, page: Page) -> impl IntoView {
    let id = StoredValue::new(project.id.clone());
    let delete = move |_| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}", api::seg(&id.get_value()));
            let _ = api::delete::<OkResponse>(&path).await;
            page.reload();
        });
    };
    let system = project
        .design_system_id
        .clone()
        .unwrap_or_else(|| t(T::Freeform).to_owned());
    let state = page.status(&project.id).as_str();
    let tag = Tag::of(&project).label();
    view! {
        <article class=format!("board-card card interactive {state}") data-testid="board-card">
            <A href=format!("/projects/{}", project.id) attr:class="board-link">
                <span class="project-name">{project.name.clone()}</span>
                <span class="project-meta">
                    <span class="badge">{tag}</span>
                    <span class="truncate">{system}</span>
                    <span>{ago(project.updated_at)}</span>
                </span>
            </A>
            <button class="icon-btn small danger" aria-label=t(T::Delete) data-tip=t(T::Delete) on:click=delete>{icon(Glyph::Close)}</button>
        </article>
    }
}

/// The name a rename sends: trimmed, `None` when nothing is left (the
/// daemon refuses a blank name).
fn rename_input(typed: &str) -> Option<String> {
    let name = typed.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_input_trims_and_refuses_blank() {
        assert_eq!(rename_input("  Quán phở 🍜 "), Some("Quán phở 🍜".into()));
        assert_eq!(rename_input("   "), None);
        assert_eq!(rename_input(""), None);
    }
}
