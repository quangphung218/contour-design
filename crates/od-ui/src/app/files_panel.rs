//! The Studio files panel: folder tree (collapsible, breadcrumb, search)
//! or files by kind (filter, "show more"), upload by button, drop or
//! paste, a row menu per file and folder (open, new tab, download, copy
//! path, rename, move, delete), selection (ZIP, move, delete), and new
//! documents, pages and folders. Also the tab launcher ("+", Cmd/Ctrl+T)
//! and the panel's info toast. Tree logic lives in `crate::file_tree`.

use super::api;
use super::export_menu::{copy_text, download};
use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, file_glyph};
use super::terminal;
use crate::file_tree::{self as ft, GROUP_PAGE, Group, Row};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::{
    ArchiveSelectionRequest, CreateFolderRequest, FileKind, FileResponse, FolderResponse,
    FoldersResponse, HandoffResponse, MoveFolderRequest, MoveFolderResponse, OkResponse,
    ProjectFile, UploadResponse, WriteFileRequest,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen::JsCast;

/// How long an info toast stays, ms.
const TOAST_MS: u32 = 4000;
/// `localStorage` key: tree or by-kind view.
const BY_KIND_KEY: &str = "od.files.by-kind";
/// `localStorage` key: chat hidden (focus workspace).
const FOCUS_KEY: &str = "od.studio.focus";

/// A dialog of the files panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    NewFolder(String),
    NewDocument(String),
    NewPage(String),
    PasteText(String),
    Rename(String),
    /// Files and folders to move (folders end with `/`).
    Move(Vec<String>),
    Delete(Vec<String>),
    DeleteFolder(String),
}

/// Files panel state, provided as context by the Studio.
#[derive(Clone, Copy)]
pub struct FilesUi {
    pub folders: RwSignal<Vec<String>>,
    /// Expanded folders.
    pub open: RwSignal<BTreeSet<String>>,
    /// Folder in view (breadcrumb); `""` is the project root.
    pub scope: RwSignal<String>,
    pub query: RwSignal<String>,
    pub by_kind: RwSignal<bool>,
    pub kinds: RwSignal<BTreeSet<Group>>,
    /// Rows shown per kind group (paging).
    pub shown: RwSignal<BTreeMap<Group, usize>>,
    pub selected: RwSignal<BTreeSet<String>>,
    pub dialog: RwSignal<Option<Dialog>>,
    /// Path whose row menu is open.
    pub menu: RwSignal<Option<String>>,
    pub dragging: RwSignal<bool>,
    pub launcher: RwSignal<bool>,
    /// Chat hidden: the workspace takes the window.
    pub focus: RwSignal<bool>,
    /// The project's live artifacts (summaries from the daemon).
    pub live: RwSignal<Vec<serde_json::Value>>,
    /// Design system being made from this project.
    pub ds_job: RwSignal<Option<super::project_actions::DsJob>>,
    /// Info toast (copied, uploaded…); errors go to the Studio notice.
    pub toast: RwSignal<Option<String>>,
    toast_seq: StoredValue<u32>,
}

impl FilesUi {
    #[must_use]
    pub fn new() -> Self {
        Self {
            folders: RwSignal::new(Vec::new()),
            open: RwSignal::new(BTreeSet::new()),
            scope: RwSignal::new(String::new()),
            query: RwSignal::new(String::new()),
            by_kind: RwSignal::new(super::stored_flag(BY_KIND_KEY).unwrap_or(false)),
            kinds: RwSignal::new(BTreeSet::new()),
            shown: RwSignal::new(BTreeMap::new()),
            selected: RwSignal::new(BTreeSet::new()),
            dialog: RwSignal::new(None),
            menu: RwSignal::new(None),
            dragging: RwSignal::new(false),
            launcher: RwSignal::new(false),
            focus: RwSignal::new(super::stored_flag(FOCUS_KEY).unwrap_or(false)),
            ds_job: RwSignal::new(None),
            live: RwSignal::new(Vec::new()),
            toast: RwSignal::new(None),
            toast_seq: StoredValue::new(0),
        }
    }

    #[must_use]
    pub fn get() -> Self {
        expect_context::<Self>()
    }

    /// Shows `text` for a few seconds.
    pub fn say(self, text: String) {
        let n = self.toast_seq.get_value().wrapping_add(1);
        self.toast_seq.set_value(n);
        self.toast.set(Some(text));
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(TOAST_MS).await;
            if self.toast_seq.try_get_value() == Some(n) {
                self.toast.set(None);
            }
        });
    }

    /// Re-reads the folder list (empty folders are not in the file list)
    /// and the live artifacts.
    pub fn refresh(self, ctx: StudioCtx) {
        super::live_view::refresh_list(ctx, self);
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<FoldersResponse>(&format!("{}/folders", ctx.base())).await {
                self.folders.set(r.folders);
            }
        });
    }

    /// Files and folders changed: both lists, and the selection keeps only
    /// what still exists.
    pub fn changed(self, ctx: StudioCtx) {
        ctx.refresh_files();
        self.refresh(ctx);
        self.selected.update(|s| s.clear());
    }

    /// Closes the dialog on the next task (see design-system.md: closing
    /// inside a click drops listeners the click still reaches).
    fn close_dialog(self) {
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            self.dialog.set(None);
        });
    }

    /// Opens the folders holding `path`, so a revealed file is visible.
    fn reveal(self, path: &str) {
        let mut dir = ft::parent(path).to_owned();
        self.open.update(|o| {
            while !dir.is_empty() {
                o.insert(dir.clone());
                dir = ft::parent(&dir).to_owned();
            }
        });
    }
}

impl Default for FilesUi {
    fn default() -> Self {
        Self::new()
    }
}

/// Overwrites existing file `path` with `body` (exact path, never renamed);
/// `label` names the version an HTML file gets.
pub async fn replace_content(
    ctx: StudioCtx,
    path: &str,
    body: &str,
    label: &str,
) -> Result<ProjectFile, String> {
    let url = format!(
        "{}/content/{}?label={}",
        ctx.base(),
        api::file_path(path),
        api::seg(label)
    );
    api::put_text::<FileResponse>(&url, body)
        .await
        .map(|r| r.file)
        .map_err(|e| e.to_string())
}

/// Inline style placing a menu under (or, near the bottom of the window,
/// over) the control that opened it, aligned to its right edge unless
/// `left`. Fixed, so scrolling lists and tab bars do not clip it.
pub fn anchor_style(ev: &web_sys::MouseEvent, left: bool) -> String {
    let Some(el) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return String::new();
    };
    let r = el.get_bounding_client_rect();
    let (w, h) = web_sys::window()
        .map(|w| {
            let num = |v: Result<wasm_bindgen::JsValue, _>| {
                v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
            };
            (num(w.inner_width()), num(w.inner_height()))
        })
        .unwrap_or((0.0, 0.0));
    let vertical = if r.bottom() + 320.0 > h && r.top() > 320.0 {
        format!("bottom:{}px", h - r.top() + 6.0)
    } else {
        format!("top:{}px", r.bottom() + 6.0)
    };
    let horizontal = if left {
        format!("left:{}px", r.left())
    } else {
        format!("right:{}px", w - r.right())
    };
    format!("position:fixed;{vertical};{horizontal}")
}

/// `/api/projects/<id>/raw/<path>`.
pub fn raw_url(ctx: StudioCtx, path: &str) -> String {
    format!("{}/raw/{}", ctx.base(), api::file_path(path))
}

/// Opens a project file in a new browser tab (or window).
pub fn open_in_new_tab(ctx: StudioCtx, path: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target(&raw_url(ctx, path), "_blank");
    }
}

/// Saves a project file through the browser's download.
pub fn download_file(ctx: StudioCtx, path: &str) {
    let Some(a) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.create_element("a").ok())
        .and_then(|a| a.dyn_into::<web_sys::HtmlAnchorElement>().ok())
    else {
        return;
    };
    a.set_href(&raw_url(ctx, path));
    a.set_download(ft::base(path));
    a.click();
}

/// Copies the project path, or with `full` the path on disk.
pub fn copy_path(ctx: StudioCtx, ui: FilesUi, path: &str, full: bool) {
    let path = path.to_owned();
    leptos::task::spawn_local(async move {
        let text = if full {
            match api::get::<HandoffResponse>(&format!("{}/handoff", ctx.base())).await {
                Ok(h) => format!("{}/{path}", h.dir.trim_end_matches('/')),
                Err(e) => {
                    ctx.notice.set(Some(e.to_string()));
                    return;
                }
            }
        } else {
            path
        };
        match copy_text(&text).await {
            Ok(()) => ui.say(tf(T::PathCopied, &[&text])),
            Err(e) => ctx.notice.set(Some(e)),
        }
    });
}

/// Uploads browser files into folder `dir`; never overwrites (the daemon
/// picks `name-1.ext` on a clash). Returns the stored paths.
pub async fn upload(
    ctx: StudioCtx,
    files: Vec<web_sys::File>,
    dir: &str,
) -> Result<Vec<String>, String> {
    let form = web_sys::FormData::new().map_err(|e| format!("{e:?}"))?;
    if !dir.is_empty() {
        form.append_with_str("dir", dir)
            .map_err(|e| format!("{e:?}"))?;
    }
    for f in &files {
        form.append_with_blob_and_filename("file", f, &f.name())
            .map_err(|e| format!("{e:?}"))?;
    }
    let res: UploadResponse = api::post_form(&format!("{}/upload", ctx.base()), form)
        .await
        .map_err(|e| e.to_string())?;
    Ok(res.files.into_iter().map(|f| f.path).collect())
}

/// Uploads, reports, refreshes; the single uploaded file opens.
pub fn upload_and_report(ctx: StudioCtx, ui: FilesUi, files: Vec<web_sys::File>, dir: String) {
    if files.is_empty() {
        return;
    }
    leptos::task::spawn_local(async move {
        match upload(ctx, files, &dir).await {
            Ok(paths) => {
                ui.changed(ctx);
                if !dir.is_empty() {
                    ui.open.update(|o| {
                        o.insert(dir.clone());
                    });
                }
                if let [one] = paths.as_slice() {
                    ui.reveal(one);
                    ctx.open_file(one);
                }
                ui.say(tf(T::FilesUploaded, &[&paths.len().to_string()]));
            }
            Err(e) => ctx.notice.set(Some(tf(T::UploadFailed, &[&e]))),
        }
    });
}

fn file_list(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
    list.map(|l| (0..l.length()).filter_map(|i| l.get(i)).collect())
        .unwrap_or_default()
}

/// Writes a new text file (never over an existing one) and opens it.
async fn create_text(ctx: StudioCtx, name: String, content: String) -> Result<String, String> {
    let req = WriteFileRequest {
        name,
        content,
        encoding: od_contracts::ContentEncoding::Utf8,
        overwrite: Some(false),
        artifact_manifest: None,
    };
    let res = api::post::<_, FileResponse>(&format!("{}/files", ctx.base()), &req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(res.file.path)
}

/// Starting Markdown of "New document" (spec: the original's template).
#[must_use]
pub fn document_template(title: &str) -> String {
    format!(
        "# {title}\n\n## {}\n\n{}\n\n## {}\n\n- {}\n- {}\n- {}\n\n## {}\n\n{}\n",
        t(T::DocGoalHeading),
        t(T::DocGoalBody),
        t(T::DocUseHeading),
        t(T::DocUseMarkdown),
        t(T::DocUseAgent),
        t(T::DocUseMention),
        t(T::DocNextHeading),
        t(T::DocNextBody),
    )
}

/// A blank page for "New page".
#[must_use]
pub fn page_template(title: &str) -> String {
    let title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n<style>\n  body {{ margin: 0; font-family: system-ui, sans-serif; }}\n  main {{ max-width: 960px; margin: 0 auto; padding: 64px 24px; }}\n</style>\n</head>\n<body>\n<main>\n  <h1>{title}</h1>\n</main>\n</body>\n</html>\n"
    )
}

fn group_label(g: Group) -> &'static str {
    t(match g {
        Group::Pages => T::GroupPages,
        Group::Documents => T::GroupDocuments,
        Group::Images => T::GroupImages,
        Group::Sketches => T::GroupSketches,
        Group::Stylesheets => T::GroupStylesheets,
        Group::Scripts => T::GroupScripts,
        Group::Media => T::GroupMedia,
        Group::Other => T::GroupOther,
    })
}

fn paths_of(files: &[ProjectFile]) -> Vec<String> {
    files.iter().map(|f| f.path.clone()).collect()
}

/// Folder paths in a move or delete list end with `/`.
fn folder_of(item: &str) -> Option<&str> {
    item.strip_suffix('/')
}

#[component]
pub fn FilesPanel(ctx: StudioCtx) -> impl IntoView {
    let ui = FilesUi::get();
    let input = NodeRef::<leptos::html::Input>::new();
    let new_open = RwSignal::new(false);
    ui.refresh(ctx);
    // A file event (agent write, upload) may also add folders.
    Effect::new(move |_| {
        ctx.files.track();
        ui.refresh(ctx);
    });
    let on_drop = move |ev: web_sys::DragEvent, dir: String| {
        ev.prevent_default();
        ev.stop_propagation();
        ui.dragging.set(false);
        let files = file_list(ev.data_transfer().and_then(|d| d.files()));
        upload_and_report(ctx, ui, files, dir);
    };
    let has_files = move |ev: &web_sys::DragEvent| {
        ev.data_transfer().is_some_and(|d| {
            d.types()
                .iter()
                .any(|t| t.as_string().as_deref() == Some("Files"))
        })
    };
    let pick_upload = move |_| {
        new_open.set(false);
        if let Some(el) = input.get_untracked() {
            el.click();
        }
    };
    view! {
        <aside
            class="files"
            class:dragging=move || ui.dragging.get()
            aria-label=t(T::ProjectFiles)
            data-testid="files-panel"
            on:dragover=move |ev: web_sys::DragEvent| {
                if has_files(&ev) {
                    ev.prevent_default();
                    ui.dragging.set(true);
                }
            }
            on:dragleave=move |ev: web_sys::DragEvent| {
                // Leaving to a child is not leaving the panel.
                let inside = ev
                    .related_target()
                    .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                    .zip(ev.current_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()))
                    .is_some_and(|(to, panel)| panel.contains(Some(&to)));
                if !inside {
                    ui.dragging.set(false);
                }
            }
            on:drop=move |ev| on_drop(ev, ui.scope.get_untracked())
            on:paste=move |ev: web_sys::ClipboardEvent| {
                if super::studio::is_editing_target(ev.target()) {
                    return;
                }
                let files = file_list(ev.clipboard_data().and_then(|d| d.files()));
                if !files.is_empty() {
                    ev.prevent_default();
                    upload_and_report(ctx, ui, files, ui.scope.get_untracked());
                }
            }
        >
            <div class="pane-head files-head">
                <span class="pane-title" title=move || ctx.project.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default())>
                    {move || ctx.project.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default())}
                </span>
                <input
                    type="file"
                    multiple
                    class="visually-hidden"
                    tabindex="-1"
                    aria-hidden="true"
                    data-testid="upload-input"
                    node_ref=input
                    on:change=move |_| {
                        let Some(el) = input.get_untracked() else { return };
                        let files = file_list(el.files());
                        el.set_value("");
                        upload_and_report(ctx, ui, files, ui.scope.get_untracked());
                    }
                />
                <div class="menu-wrap">
                    <button
                        class="icon-btn"
                        aria-label=t(T::NewFileMenu)
                        data-tip=t(T::NewFileMenu)
                        data-tip-pos="left"
                        aria-haspopup="menu"
                        aria-expanded=move || new_open.get().to_string()
                        data-testid="files-new"
                        on:click=move |_| new_open.update(|o| *o = !*o)
                    >
                        {icon(Glyph::Add)}
                    </button>
                    <Show when=move || new_open.get()>
                        <div class="menu-scrim" on:click=move |_| new_open.set(false)></div>
                        <div class="menu" role="menu" data-testid="files-new-menu">
                            <button class="menu-item" role="menuitem" data-testid="files-upload" on:click=pick_upload>
                                {icon(Glyph::Upload)}{t(T::UploadFiles)}
                            </button>
                            <button class="menu-item" role="menuitem" data-testid="files-paste" on:click=move |_| { new_open.set(false); ui.dialog.set(Some(Dialog::PasteText(ui.scope.get_untracked()))); }>
                                {icon(Glyph::Clipboard)}{t(T::PasteAsFile)}
                            </button>
                            <div class="menu-sep">{t(T::CreateNew)}</div>
                            <button class="menu-item" role="menuitem" data-testid="files-new-document" on:click=move |_| { new_open.set(false); ui.dialog.set(Some(Dialog::NewDocument(ui.scope.get_untracked()))); }>
                                {icon(Glyph::Markdown)}{t(T::NewDocument)}
                            </button>
                            <button class="menu-item" role="menuitem" data-testid="files-new-page" on:click=move |_| { new_open.set(false); ui.dialog.set(Some(Dialog::NewPage(ui.scope.get_untracked()))); }>
                                {icon(Glyph::Pages)}{t(T::NewPage)}
                            </button>
                            <button class="menu-item" role="menuitem" data-testid="files-new-sketch" on:click=move |_| { new_open.set(false); super::sketch::new_sketch(ctx, ui, ui.scope.get_untracked()); }>
                                {icon(Glyph::Sketch)}{t(T::NewSketch)}
                            </button>
                            <button class="menu-item" role="menuitem" data-testid="files-new-folder" on:click=move |_| { new_open.set(false); ui.dialog.set(Some(Dialog::NewFolder(ui.scope.get_untracked()))); }>
                                {icon(Glyph::FolderAdd)}{t(T::NewFolder)}
                            </button>
                        </div>
                    </Show>
                </div>
                <button class="icon-btn" aria-label=t(T::QuickOpen) data-tip=t(T::QuickOpen) data-tip-pos="left" data-testid="open-switcher" on:click=move |_| ctx.switcher_open.set(true)>
                    {icon(Glyph::Search)}
                </button>
                <super::project_actions::ProjectMenu ctx=ctx/>
                <button class="icon-btn" aria-label=t(T::HideFiles) data-tip=t(T::HideFiles) data-tip-pos="left" data-testid="hide-files" on:click=move |_| ctx.toggle_files()>
                    {icon(Glyph::SidebarUnfold)}
                </button>
            </div>
            <div class="files-tools">
                <input
                    class="input small files-search"
                    type="search"
                    data-testid="files-search"
                    placeholder=t(T::SearchFiles)
                    aria-label=t(T::SearchFiles)
                    prop:value=move || ui.query.get()
                    on:input=move |ev| ui.query.set(event_target_value(&ev))
                />
                <div class="segmented small icons" role="radiogroup" aria-label=t(T::FilesViewAs)>
                    <button class="seg" role="radio" data-testid="files-view-tree" aria-label=t(T::FilesViewTree) data-tip=t(T::FilesViewTree)
                        aria-checked=move || (!ui.by_kind.get()).to_string() class:active=move || !ui.by_kind.get()
                        on:click=move |_| { ui.by_kind.set(false); super::store_flag(BY_KIND_KEY, false); }>
                        {icon(Glyph::Folder)}
                    </button>
                    <button class="seg" role="radio" data-testid="files-view-kind" aria-label=t(T::FilesViewKind) data-tip=t(T::FilesViewKind)
                        aria-checked=move || ui.by_kind.get().to_string() class:active=move || ui.by_kind.get()
                        on:click=move |_| { ui.by_kind.set(true); super::store_flag(BY_KIND_KEY, true); }>
                        {icon(Glyph::Grid)}
                    </button>
                </div>
            </div>
            <SelectionBar ctx=ctx ui=ui/>
            <Show when=move || ui.query.with(|q| q.trim().is_empty()) && ui.scope.with(String::is_empty)>
                <super::live_view::LiveList ctx=ctx/>
            </Show>
            {move || if ui.by_kind.get() {
                view! { <KindView ctx=ctx ui=ui/> }.into_any()
            } else {
                view! { <TreeView ctx=ctx ui=ui on_drop=on_drop/> }.into_any()
            }}
            <Show when=move || ui.dragging.get()>
                <div class="files-drop" data-testid="files-drop" aria-hidden="true">
                    {icon(Glyph::Upload)}
                    <strong>{t(T::DropFilesTitle)}</strong>
                    <span>{t(T::DropFilesHint)}</span>
                </div>
            </Show>
        </aside>
        {move || ui.dialog.get().map(|d| view! { <FileDialog ctx=ctx ui=ui dialog=d/> })}
    }
}

/// Breadcrumb and folder tree.
#[component]
fn TreeView(
    ctx: StudioCtx,
    ui: FilesUi,
    on_drop: impl Fn(web_sys::DragEvent, String) + Copy + Send + 'static,
) -> impl IntoView {
    let rows = Memo::new(move |_| {
        let files = ctx.files.with(|f| paths_of(f));
        ui.folders.with(|folders| {
            ui.open.with(|open| {
                ui.scope.with(|scope| {
                    ui.query
                        .with(|q| ft::tree_rows(&files, folders, scope, open, q))
                })
            })
        })
    });
    view! {
        <Show when=move || !ui.scope.with(String::is_empty)>
            <nav class="files-crumbs" aria-label=t(T::FolderPath) data-testid="files-crumbs">
                <button class="icon-btn small" aria-label=t(T::FolderUp) data-tip=t(T::FolderUp)
                    on:click=move |_| ui.scope.update(|s| *s = ft::parent(s).to_owned())>
                    {icon(Glyph::ChevronLeft)}
                </button>
                {move || {
                    let crumbs = ft::crumbs(&ui.scope.get(), t(T::ProjectRoot));
                    let last = crumbs.len() - 1;
                    crumbs.into_iter().enumerate().map(|(i, (label, dir))| view! {
                        {(i > 0).then(|| view! { <span class="crumb-sep" aria-hidden="true">"/"</span> })}
                        <button class="crumb" aria-current=(i == last).then_some("location")
                            on:click=move |_| ui.scope.set(dir.clone())>{label}</button>
                    }).collect_view()
                }}
            </nav>
        </Show>
        <ul class="file-tree" data-testid="file-tree" role="tree">
            {move || {
                let rows = rows.get();
                if rows.is_empty() {
                    let searching = ui.query.with(|q| !q.trim().is_empty());
                    return view! {
                        <li class="files-empty">
                            <span class="files-empty-title">{icon(Glyph::File)}{if searching { t(T::NoMatchingFiles) } else { t(T::NoFilesYet) }}</span>
                            {(!searching).then(|| view! { <span class="subtle">{t(T::NoFilesYetHint)}</span> })}
                        </li>
                    }.into_any();
                }
                rows.into_iter().map(|row| view! { <TreeRow ctx=ctx ui=ui row=row on_drop=on_drop/> }).collect_view().into_any()
            }}
        </ul>
    }
}

#[component]
fn TreeRow(
    ctx: StudioCtx,
    ui: FilesUi,
    row: Row,
    on_drop: impl Fn(web_sys::DragEvent, String) + Copy + Send + 'static,
) -> impl IntoView {
    let path = row.path().to_owned();
    let (depth, is_folder) = match &row {
        Row::Folder { depth, .. } => (*depth, true),
        Row::File { depth, .. } => (*depth, false),
    };
    let pad = format!("padding-left:{}px", 6 + depth * 14);
    let key = if is_folder {
        format!("{path}/")
    } else {
        path.clone()
    };
    let checked = {
        let key = key.clone();
        Memo::new(move |_| ui.selected.with(|s| s.contains(&key)))
    };
    let check = {
        let key = key.clone();
        move |_| {
            ui.selected.update(|s| {
                if !s.remove(&key) {
                    s.insert(key.clone());
                }
            });
        }
    };
    let main = match row {
        Row::Folder {
            name, open, count, ..
        } => {
            let p = path.clone();
            let drop_dir = path.clone();
            view! {
                <button
                    class="file folder"
                    style=pad
                    role="treeitem"
                    aria-expanded=open.to_string()
                    data-testid="folder-row"
                    data-path=path.clone()
                    title=path.clone()
                    on:click=move |_| ui.open.update(|o| if !o.remove(&p) { o.insert(p.clone()); })
                    on:dblclick=move |_| ui.scope.set(path.clone())
                    on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                    on:drop=move |ev| on_drop(ev, drop_dir.clone())
                >
                    <span class="file-caret" class:open=open>{icon(Glyph::ChevronRight)}</span>
                    <span class="file-ico">{icon(if open { Glyph::FolderOpen } else { Glyph::Folder })}</span>
                    <span class="file-name">{name}</span>
                    <span class="file-count mono">{count}</span>
                </button>
            }
            .into_any()
        }
        Row::File { name, .. } => {
            let p = path.clone();
            let active = path.clone();
            let kind = ctx
                .files
                .with_untracked(|f| f.iter().find(|f| f.path == path).map(|f| f.kind));
            view! {
                <button
                    class="file"
                    class:active=move || ctx.active().as_deref() == Some(active.as_str())
                    style=pad
                    role="treeitem"
                    data-testid="file-row"
                    data-path=path.clone()
                    title=path.clone()
                    on:click=move |_| ctx.open_file(&p)
                >
                    <span class="file-caret" aria-hidden="true"></span>
                    <span class=format!("file-ico kind-{:?}", kind.unwrap_or(FileKind::Binary)).to_lowercase()>{icon(file_glyph(&path))}</span>
                    <span class="file-name">{name}</span>
                </button>
            }
            .into_any()
        }
    };
    view! {
        <li class="file-row" class:selected=move || checked.get() class:selecting=move || ui.selected.with(|s| !s.is_empty())>
            <input
                type="checkbox"
                class="file-check"
                aria-label=t(T::SelectFile)
                data-testid="file-check"
                prop:checked=move || checked.get()
                on:change=check
            />
            {main}
            <RowMenu ctx=ctx ui=ui key=key/>
        </li>
    }
}

/// Files by kind: filter chips, then one collapsible section per kind.
#[component]
fn KindView(ctx: StudioCtx, ui: FilesUi) -> impl IntoView {
    let collapsed = RwSignal::new(BTreeSet::<Group>::new());
    let all = Memo::new(move |_| {
        let q = ui.query.with(|q| q.trim().to_lowercase());
        let files: Vec<(String, FileKind)> = ctx.files.with(|f| {
            f.iter()
                .filter(|f| q.is_empty() || f.path.to_lowercase().contains(&q))
                .map(|f| (f.path.clone(), f.kind))
                .collect()
        });
        files
    });
    let present = Memo::new(move |_| {
        all.with(|f| {
            ft::grouped(f, &BTreeSet::new())
                .into_iter()
                .map(|(g, _)| g)
                .collect::<Vec<_>>()
        })
    });
    view! {
        <div class="files-kinds" role="group" aria-label=t(T::FilterByKind) data-testid="kind-filter">
            {move || present.get().into_iter().map(|g| view! {
                <button class="chip small-chip" aria-checked=move || ui.kinds.with(|k| k.contains(&g)).to_string()
                    on:click=move |_| ui.kinds.update(|k| if !k.remove(&g) { k.insert(g); })>
                    {group_label(g)}
                </button>
            }).collect_view()}
        </div>
        <div class="file-tree kind-groups" data-testid="kind-groups">
            {move || {
                let groups = all.with(|f| ui.kinds.with(|k| ft::grouped(f, k)));
                if groups.is_empty() {
                    return view! { <p class="files-empty subtle">{t(T::NoMatchingFiles)}</p> }.into_any();
                }
                groups.into_iter().map(|(g, paths)| {
                    let total = paths.len();
                    let limit = ui.shown.with(|s| s.get(&g).copied().unwrap_or(GROUP_PAGE));
                    let closed = collapsed.with(|c| c.contains(&g));
                    view! {
                        <section class="kind-group" data-testid="kind-group">
                            <button class="kind-head" aria-expanded=(!closed).to_string()
                                on:click=move |_| collapsed.update(|c| if !c.remove(&g) { c.insert(g); })>
                                <span class="file-caret" class:open=!closed>{icon(Glyph::ChevronRight)}</span>
                                <span class="eyebrow">{group_label(g)}</span>
                                <span class="file-count mono">{total}</span>
                            </button>
                            {(!closed).then(|| view! {
                                <ul class="kind-list">
                                    {paths.into_iter().take(limit).map(|p| view! {
                                        <TreeRow ctx=ctx ui=ui row=Row::File { name: ft::base(&p).to_owned(), path: p, depth: 0 } on_drop=|_, _| {}/>
                                    }).collect_view()}
                                </ul>
                                {(total > limit).then(|| view! {
                                    <button class="btn small ghost show-more" data-testid="show-more"
                                        on:click=move |_| ui.shown.update(|s| { s.insert(g, limit + GROUP_PAGE); })>
                                        {tf(T::ShowMore, &[&(total - limit).to_string()])}
                                    </button>
                                })}
                            })}
                        </section>
                    }
                }).collect_view().into_any()
            }}
        </div>
    }
}

/// What the selection can do: ZIP, move, delete, clear.
#[component]
fn SelectionBar(ctx: StudioCtx, ui: FilesUi) -> impl IntoView {
    let zip = move |_| {
        let items: Vec<String> = ui.selected.get_untracked().into_iter().collect();
        let files = ctx.files.with_untracked(|all| expand(&items, all));
        leptos::task::spawn_local(async move {
            let req = ArchiveSelectionRequest { files };
            match api::post_bytes(&format!("{}/archive/selection", ctx.base()), &req).await {
                Ok(bytes) => {
                    let name = ctx
                        .project
                        .with_untracked(|p| p.as_ref().map(|p| p.name.clone()))
                        .unwrap_or_else(|| "files".to_owned());
                    download(&bytes, &format!("{name}.zip"), "application/zip");
                }
                Err(e) => ctx.notice.set(Some(e.to_string())),
            }
        });
    };
    move || {
        let n = ui.selected.with(BTreeSet::len);
        (n > 0).then(|| view! {
            <div class="files-selection" data-testid="files-selection">
                <span class="grow">{tf(T::NSelected, &[&n.to_string()])}</span>
                <button class="icon-btn small" aria-label=t(T::DownloadZip) data-tip=t(T::DownloadZip) data-testid="selection-zip" on:click=zip>{icon(Glyph::Download)}</button>
                <button class="icon-btn small" aria-label=t(T::MoveTo) data-tip=t(T::MoveTo) data-testid="selection-move"
                    on:click=move |_| ui.dialog.set(Some(Dialog::Move(ui.selected.get_untracked().into_iter().collect())))>{icon(Glyph::FolderMove)}</button>
                <button class="icon-btn small danger" aria-label=t(T::Delete) data-tip=t(T::Delete) data-testid="selection-delete"
                    on:click=move |_| ui.dialog.set(Some(Dialog::Delete(ui.selected.get_untracked().into_iter().collect())))>{icon(Glyph::Delete)}</button>
                <button class="icon-btn small" aria-label=t(T::ClearSelection) data-tip=t(T::ClearSelection) on:click=move |_| ui.selected.update(|s| s.clear())>{icon(Glyph::Close)}</button>
            </div>
        })
    }
}

/// Selection items (folders end with `/`) as the files they hold.
fn expand(items: &[String], files: &[ProjectFile]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        match folder_of(item) {
            Some(dir) => out.extend(
                files
                    .iter()
                    .filter(|f| ft::is_inside(&f.path, dir))
                    .map(|f| f.path.clone()),
            ),
            None => out.push(item.clone()),
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The "…" menu of a row; `key` is a file path or a folder path + `/`.
#[component]
fn RowMenu(ctx: StudioCtx, ui: FilesUi, key: String) -> impl IntoView {
    let k = key.clone();
    let open = Memo::new(move |_| ui.menu.with(|m| m.as_deref() == Some(k.as_str())));
    let pos = RwSignal::new(String::new());
    let toggle = {
        let key = key.clone();
        move |ev: web_sys::MouseEvent| {
            pos.set(anchor_style(&ev, false));
            ui.menu.update(|m| {
                *m = if m.as_deref() == Some(key.as_str()) {
                    None
                } else {
                    Some(key.clone())
                }
            });
        }
    };
    let close = move || ui.menu.set(None);
    let folder = folder_of(&key).map(str::to_owned);
    let items = move || {
        let key = key.clone();
        match folder.clone() {
            Some(dir) => {
                let (d1, d2, d3, d4, d5, d6) = (
                    dir.clone(),
                    dir.clone(),
                    dir.clone(),
                    dir.clone(),
                    dir.clone(),
                    dir.clone(),
                );
                view! {
                    <button class="menu-item" role="menuitem" data-testid="row-open-folder" on:click=move |_| { close(); ui.scope.set(d1.clone()); }>{icon(Glyph::FolderOpen)}{t(T::OpenThisFolder)}</button>
                    <button class="menu-item" role="menuitem" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::NewDocument(d2.clone()))); }>{icon(Glyph::Markdown)}{t(T::NewDocumentHere)}</button>
                    <button class="menu-item" role="menuitem" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::NewFolder(d3.clone()))); }>{icon(Glyph::FolderAdd)}{t(T::NewFolderHere)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-zip-folder" on:click=move |_| {
                        close();
                        let dir = d4.clone();
                        leptos::task::spawn_local(async move {
                            let url = format!("{}/archive?root={}", ctx.base(), api::seg(&dir));
                            match api::get_bytes(&url).await {
                                Ok(bytes) => { download(&bytes, &format!("{}.zip", ft::base(&dir)), "application/zip"); }
                                Err(e) => ctx.notice.set(Some(e.to_string())),
                            }
                        });
                    }>{icon(Glyph::Download)}{t(T::DownloadZip)}</button>
                    <button class="menu-item" role="menuitem" on:click=move |_| { close(); copy_path(ctx, ui, &d5, false); }>{icon(Glyph::Link)}{t(T::CopyPath)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-move-folder" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::Move(vec![format!("{d6}/")]))); }>{icon(Glyph::FolderMove)}{t(T::MoveTo)}</button>
                    <div class="menu-sep"></div>
                    <button class="menu-item danger" role="menuitem" data-testid="row-delete-folder" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::DeleteFolder(dir.clone()))); }>{icon(Glyph::Delete)}{t(T::DeleteFolder)}</button>
                }
                .into_any()
            }
            None => {
                let (p1, p2, p3, p4, p5, p6, p7, p8) = (
                    key.clone(),
                    key.clone(),
                    key.clone(),
                    key.clone(),
                    key.clone(),
                    key.clone(),
                    key.clone(),
                    key.clone(),
                );
                view! {
                    <button class="menu-item" role="menuitem" data-testid="row-open" on:click=move |_| { close(); ctx.open_file(&p1); }>{icon(Glyph::Eye)}{t(T::OpenInTab)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-new-tab" on:click=move |_| { close(); open_in_new_tab(ctx, &p2); }>{icon(Glyph::ExternalLink)}{t(T::OpenInNewTab)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-download" on:click=move |_| { close(); download_file(ctx, &p3); }>{icon(Glyph::Download)}{t(T::DownloadFile)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-copy-path" on:click=move |_| { close(); copy_path(ctx, ui, &p4, false); }>{icon(Glyph::Link)}{t(T::CopyPath)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-copy-full-path" on:click=move |_| { close(); copy_path(ctx, ui, &p5, true); }>{icon(Glyph::Clipboard)}{t(T::CopyFullPath)}</button>
                    <div class="menu-sep"></div>
                    <button class="menu-item" role="menuitem" data-testid="row-rename" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::Rename(p6.clone()))); }>{icon(Glyph::Rename)}{t(T::Rename)}</button>
                    <button class="menu-item" role="menuitem" data-testid="row-move" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::Move(vec![p7.clone()]))); }>{icon(Glyph::FolderMove)}{t(T::MoveTo)}</button>
                    <button class="menu-item danger" role="menuitem" data-testid="row-delete" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::Delete(vec![p8.clone()]))); }>{icon(Glyph::Delete)}{t(T::Delete)}</button>
                }
                .into_any()
            }
        }
    };
    view! {
        <div class="menu-wrap file-more">
            <button class="icon-btn small" aria-label=t(T::RowMenu) aria-haspopup="menu" aria-expanded=move || open.get().to_string() data-testid="row-menu" on:click=toggle>
                {icon(Glyph::More)}
            </button>
            <Show when=move || open.get()>
                <div class="menu-scrim" on:click=move |_| close()></div>
                <div class="menu" role="menu" data-testid="row-menu-list" style=move || pos.get()>{items.clone()}</div>
            </Show>
        </div>
    }
}

/// Moves `items` (files, folders ending in `/`) into folder `dest`. A
/// folder moves whole on the server: what the files list skips (hidden
/// files, `dist/`, empty folders) moves with it.
async fn move_items(ctx: StudioCtx, items: Vec<String>, dest: String) -> Result<usize, String> {
    let mut moved = 0;
    for item in &items {
        match folder_of(item) {
            Some(dir) => {
                let r = api::post::<_, MoveFolderResponse>(
                    &format!("{}/move-folder", ctx.base()),
                    &MoveFolderRequest {
                        from: dir.to_owned(),
                        to: ft::join(&dest, ft::base(dir)),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
                moved += r.files;
            }
            None => {
                api::post::<_, FileResponse>(
                    &format!("{}/files/rename", ctx.base()),
                    &json!({ "from": item, "to": ft::join(&dest, ft::base(item)) }),
                )
                .await
                .map_err(|e| e.to_string())?;
                moved += 1;
            }
        }
    }
    Ok(moved)
}

#[component]
fn FileDialog(ctx: StudioCtx, ui: FilesUi, dialog: Dialog) -> impl IntoView {
    let initial = match &dialog {
        Dialog::Rename(p) => ft::base(p).to_owned(),
        Dialog::NewDocument(_) => t(T::UntitledDocument).to_owned(),
        Dialog::NewPage(_) => t(T::UntitledPage).to_owned(),
        Dialog::PasteText(_) => "pasted.txt".to_owned(),
        _ => String::new(),
    };
    let name = RwSignal::new(initial);
    let text = RwSignal::new(String::new());
    let dest = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
            el.select();
        }
    });
    let (title, glyph, confirm, danger) = match &dialog {
        Dialog::NewFolder(_) => (t(T::NewFolder), Glyph::FolderAdd, t(T::Create), false),
        Dialog::NewDocument(_) => (t(T::NewDocument), Glyph::Markdown, t(T::Create), false),
        Dialog::NewPage(_) => (t(T::NewPage), Glyph::Pages, t(T::Create), false),
        Dialog::PasteText(_) => (t(T::PasteAsFile), Glyph::Clipboard, t(T::Create), false),
        Dialog::Rename(_) => (t(T::RenameFile), Glyph::Rename, t(T::Rename), false),
        Dialog::Move(_) => (t(T::MoveTo), Glyph::FolderMove, t(T::Move), false),
        Dialog::Delete(_) => (t(T::DeleteFiles), Glyph::Delete, t(T::Delete), true),
        Dialog::DeleteFolder(_) => (t(T::DeleteFolder), Glyph::Delete, t(T::Delete), true),
    };
    let body = match &dialog {
        Dialog::Delete(items) => {
            let n = ctx.files.with_untracked(|f| expand(items, f)).len();
            Some(tf(T::ConfirmDeleteFiles, &[&n.to_string()]))
        }
        Dialog::DeleteFolder(dir) => Some(tf(T::ConfirmDeleteFolder, &[dir])),
        Dialog::Move(items) => Some(tf(T::MoveItems, &[&items.len().to_string()])),
        _ => None,
    };
    let needs_name = matches!(
        dialog,
        Dialog::NewFolder(_)
            | Dialog::NewDocument(_)
            | Dialog::NewPage(_)
            | Dialog::PasteText(_)
            | Dialog::Rename(_)
    );
    let is_paste = matches!(dialog, Dialog::PasteText(_));
    let targets = match &dialog {
        Dialog::Move(items) => {
            let files = ctx.files.with_untracked(|f| paths_of(f));
            let folders = ui.folders.get_untracked();
            let mut set: Option<BTreeSet<String>> = None;
            for item in items {
                let (p, is_folder) = match folder_of(item) {
                    Some(d) => (d, true),
                    None => (item.as_str(), false),
                };
                let these: BTreeSet<String> = ft::move_targets(p, is_folder, &files, &folders)
                    .into_iter()
                    .collect();
                set = Some(match set {
                    Some(s) => s.intersection(&these).cloned().collect(),
                    None => these,
                });
            }
            let list: Vec<String> = set.unwrap_or_default().into_iter().collect();
            if let Some(first) = list.first() {
                dest.set(first.clone());
            }
            Some(list)
        }
        _ => None,
    };
    let dialog = StoredValue::new(dialog);
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let d = dialog.get_value();
        let n = name.get_untracked().trim().to_owned();
        if needs_name && n.is_empty() {
            return;
        }
        busy.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            let files = ctx.files.get_untracked();
            let existing = paths_of(&files);
            let result: Result<Option<String>, String> = match d {
                Dialog::NewFolder(dir) => api::post::<_, FolderResponse>(
                    &format!("{}/folders", ctx.base()),
                    &CreateFolderRequest {
                        path: ft::join(&dir, &n),
                    },
                )
                .await
                .map(|r| {
                    ui.open.update(|o| {
                        o.insert(r.folder.clone());
                    });
                    ui.reveal(&format!("{}/x", r.folder));
                    Some(tf(T::FolderCreated, &[&r.folder]))
                })
                .map_err(|e| e.to_string()),
                Dialog::NewDocument(dir) => {
                    let path = ft::free_name(&existing, &dir, &ft::stem_of_title(&n), "md");
                    create_text(ctx, path, document_template(&n))
                        .await
                        .map(|p| {
                            ui.reveal(&p);
                            ctx.open_file(&p);
                            None
                        })
                }
                Dialog::NewPage(dir) => {
                    let path = ft::free_name(&existing, &dir, &ft::stem_of_title(&n), "html");
                    create_text(ctx, path, page_template(&n)).await.map(|p| {
                        ui.reveal(&p);
                        ctx.open_file(&p);
                        None
                    })
                }
                Dialog::PasteText(dir) => {
                    let content = text.get_untracked();
                    create_text(ctx, ft::join(&dir, &n), content)
                        .await
                        .map(|p| {
                            ui.reveal(&p);
                            ctx.open_file(&p);
                            None
                        })
                }
                Dialog::Rename(from) => {
                    let to = ft::join(ft::parent(&from), &n);
                    api::post::<_, FileResponse>(
                        &format!("{}/files/rename", ctx.base()),
                        &json!({ "from": from, "to": to }),
                    )
                    .await
                    .map(|r| {
                        let was_open = ctx.tabs.with_untracked(|t| t.open.contains(&from));
                        if was_open {
                            ctx.tabs.update(|t| t.close(&from));
                            ctx.open_file(&r.file.path);
                        }
                        None
                    })
                    .map_err(|e| e.to_string())
                }
                Dialog::Move(items) => {
                    let to = dest.get_untracked();
                    move_items(ctx, items, to.clone()).await.map(|n| {
                        if !to.is_empty() {
                            ui.open.update(|o| {
                                o.insert(to.clone());
                            });
                        }
                        let shown = if to.is_empty() {
                            t(T::ProjectRoot).to_owned()
                        } else {
                            to
                        };
                        Some(tf(T::FilesMoved, &[&n.to_string(), &shown]))
                    })
                }
                Dialog::Delete(items) => {
                    let mut out: Result<(), String> = Ok(());
                    for item in &items {
                        let res = match folder_of(item) {
                            Some(dir) => api::delete::<FolderResponse>(&format!(
                                "{}/folders/{}",
                                ctx.base(),
                                api::file_path(dir)
                            ))
                            .await
                            .map(|_| ()),
                            None => api::delete::<OkResponse>(&format!(
                                "{}/files/{}",
                                ctx.base(),
                                api::file_path(item)
                            ))
                            .await
                            .map(|_| ()),
                        };
                        if let Err(e) = res {
                            out = Err(e.to_string());
                            break;
                        }
                    }
                    out.map(|()| Some(tf(T::FilesDeleted, &[&items.len().to_string()])))
                }
                Dialog::DeleteFolder(dir) => api::delete::<FolderResponse>(&format!(
                    "{}/folders/{}",
                    ctx.base(),
                    api::file_path(&dir)
                ))
                .await
                .map(|_| {
                    if ui
                        .scope
                        .with_untracked(|s| ft::is_inside(s, &dir) || *s == dir)
                    {
                        ui.scope.set(ft::parent(&dir).to_owned());
                    }
                    Some(tf(T::FolderDeleted, &[&dir]))
                })
                .map_err(|e| e.to_string()),
            };
            busy.set(false);
            match result {
                Ok(message) => {
                    ui.changed(ctx);
                    if let Some(m) = message {
                        ui.say(m);
                    }
                    ui.close_dialog();
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let cancel = move || ui.close_dialog();
    view! {
        <div class="overlay center" on:click=move |_| cancel()
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { cancel(); }>
            <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="file-dialog-title" data-testid="file-dialog"
                on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <span class="dialog-icon" class:danger=danger>{icon(glyph)}</span>
                    <div class="grow">
                        <h2 id="file-dialog-title">{title}</h2>
                        {body.map(|b| view! { <p>{b}</p> })}
                    </div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| cancel()>{icon(Glyph::Close)}</button>
                </div>
                <div class="dialog-body">
                    {needs_name.then(|| view! {
                        <label class="field">
                            <span>{if is_paste { t(T::FileName) } else { t(T::Name) }}</span>
                            <input class="input" data-testid="file-dialog-name" node_ref=input
                                prop:value=move || name.get()
                                on:input=move |ev| name.set(event_target_value(&ev))
                                on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" && !is_paste { ev.prevent_default(); submit(); }/>
                        </label>
                    })}
                    {is_paste.then(|| view! {
                        <label class="field">
                            <span>{t(T::PastedText)}</span>
                            <textarea class="input mono" rows="8" data-testid="file-dialog-text"
                                prop:value=move || text.get()
                                on:input=move |ev| text.set(event_target_value(&ev))></textarea>
                        </label>
                    })}
                    {targets.map(|list| view! {
                        <label class="field">
                            <span>{t(T::DestinationFolder)}</span>
                            <select class="input" data-testid="file-dialog-dest" on:change=move |ev| dest.set(event_target_value(&ev))>
                                {list.into_iter().map(|d| {
                                    let label = if d.is_empty() { t(T::ProjectRoot).to_owned() } else { d.clone() };
                                    view! { <option value=d.clone() selected=move || dest.get() == d>{label}</option> }
                                }).collect_view()}
                            </select>
                        </label>
                    })}
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                </div>
                <div class="dialog-foot">
                    <button class="btn ghost" on:click=move |_| cancel()>{t(T::Cancel)}</button>
                    <button class="btn" class:primary=!danger class:danger=danger data-testid="file-dialog-confirm"
                        disabled=move || busy.get() || (needs_name && name.with(|n| n.trim().is_empty()))
                        on:click=move |_| submit()>
                        {move || busy.get().then(|| view! { <span class="spinner on-accent"></span> })}
                        {confirm}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// The tab bar's "+": open a file, new document, page, sketch, terminal.
#[component]
pub fn TabLauncher(ctx: StudioCtx) -> impl IntoView {
    let ui = FilesUi::get();
    let close = move || ui.launcher.set(false);
    let button = NodeRef::<leptos::html::Button>::new();
    let pos = RwSignal::new(String::new());
    // Opened by click or Cmd/Ctrl+T: place the menu under the "+".
    Effect::new(move |_| {
        if ui.launcher.get() {
            if let Some(b) = button.get_untracked() {
                let r = b.get_bounding_client_rect();
                pos.set(format!(
                    "position:fixed;top:{}px;left:{}px",
                    r.bottom() + 6.0,
                    r.left()
                ));
            }
        }
    });
    view! {
        <div class="menu-wrap tab-add">
            <button class="icon-btn" aria-label=t(T::NewTab) data-tip=t(T::NewTab) aria-haspopup="menu" node_ref=button
                aria-expanded=move || ui.launcher.get().to_string() data-testid="tab-launcher"
                on:click=move |_| ui.launcher.update(|o| *o = !*o)>
                {icon(Glyph::Add)}
            </button>
            <Show when=move || ui.launcher.get()>
                <div class="menu-scrim" on:click=move |_| close()></div>
                <div class="menu left" role="menu" data-testid="tab-launcher-menu" style=move || pos.get()>
                    <button class="menu-item" role="menuitem" data-testid="launch-open-file" on:click=move |_| { close(); ctx.switcher_open.set(true); }>
                        {icon(Glyph::Search)}{t(T::OpenFileEllipsis)}<span class="menu-hint">{format!("{}P", super::shortcuts::modifier())}</span>
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="launch-new-document" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::NewDocument(String::new()))); }>
                        {icon(Glyph::Markdown)}{t(T::NewDocument)}
                    </button>
                    <button class="menu-item" role="menuitem" on:click=move |_| { close(); ui.dialog.set(Some(Dialog::NewPage(String::new()))); }>
                        {icon(Glyph::Pages)}{t(T::NewPage)}
                    </button>
                    <button class="menu-item" role="menuitem" on:click=move |_| { close(); super::sketch::new_sketch(ctx, ui, String::new()); }>
                        {icon(Glyph::Sketch)}{t(T::NewSketch)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="new-terminal" on:click=move |_| { close(); terminal::open(ctx); }>
                        {icon(Glyph::Terminal)}{t(T::NewTerminal)}
                    </button>
                </div>
            </Show>
        </div>
        // The panel's dialogs work while the files panel is hidden too.
        <Show when=move || !ctx.files_open.get()>
            {move || ui.dialog.get().map(|d| view! { <FileDialog ctx=ctx ui=ui dialog=d/> })}
        </Show>
    }
}

/// Info toast of the files panel (and of the file actions elsewhere).
#[component]
pub fn FilesToast() -> impl IntoView {
    let ui = FilesUi::get();
    view! {
        <div class="toast-region" aria-live="polite">
            {move || ui.toast.get().map(|text| view! {
                <div class="toast" role="status" data-testid="files-toast">
                    <span class="toast-ico">{icon(Glyph::CheckCircle)}</span>
                    <span class="toast-text">{text}</span>
                    <button class="chip-x" aria-label=t(T::Close) on:click=move |_| ui.toast.set(None)>{icon(Glyph::Close)}</button>
                </div>
            })}
        </div>
    }
}

/// Focus workspace (hide the chat) / Show chat, at the end of the tab bar.
#[component]
pub fn FocusToggle() -> impl IntoView {
    let ui = FilesUi::get();
    let label = move || {
        if ui.focus.get() {
            t(T::ShowChat)
        } else {
            t(T::FocusWorkspace)
        }
    };
    view! {
        <button class="icon-btn" data-testid="focus-workspace" aria-pressed=move || ui.focus.get().to_string()
            aria-label=label data-tip=label data-tip-pos="left"
            on:click=move |_| {
                let next = !ui.focus.get_untracked();
                ui.focus.set(next);
                super::store_flag(FOCUS_KEY, next);
            }>
            {move || icon(if ui.focus.get() { Glyph::Chat } else { Glyph::Focus })}
        </button>
    }
}

/// Copies the hand-off prompt for a coding agent (Continue in CLI).
pub fn continue_in_cli(ctx: StudioCtx, ui: FilesUi) {
    leptos::task::spawn_local(async move {
        let res = match api::get::<HandoffResponse>(&format!("{}/handoff", ctx.base())).await {
            Ok(h) => copy_text(&h.prompt).await,
            Err(e) => Err(e.to_string()),
        };
        match res {
            Ok(()) => ui.say(t(T::ContinueInCliCopied).to_owned()),
            Err(e) => ctx.notice.set(Some(tf(T::HandoffFailed, &[&e]))),
        }
    });
}

/// Studio keys of the files and tabs track: Cmd/Ctrl+T tab launcher,
/// Ctrl+PageUp/PageDown and Cmd/Ctrl+Alt+Left/Right previous/next tab,
/// Cmd/Ctrl+Shift+K continue in CLI, R back to the first slide.
pub fn on_studio_key(ctx: StudioCtx, ui: FilesUi, ev: &web_sys::KeyboardEvent) {
    let key = ev.key();
    let primary = ev.ctrl_key() || ev.meta_key();
    let handled = if primary && !ev.alt_key() && !ev.shift_key() && key.eq_ignore_ascii_case("t") {
        ui.launcher.set(true);
        true
    } else if ev.ctrl_key()
        && !ev.meta_key()
        && !ev.alt_key()
        && !ev.shift_key()
        && (key == "PageDown" || key == "PageUp")
    {
        ctx.tabs.update(|t| t.cycle(key == "PageDown"));
        true
    } else if primary
        && ev.alt_key()
        && !ev.shift_key()
        && (key == "ArrowRight" || key == "ArrowLeft")
    {
        ctx.tabs.update(|t| t.cycle(key == "ArrowRight"));
        true
    } else if primary && ev.shift_key() && !ev.alt_key() && key.eq_ignore_ascii_case("k") {
        continue_in_cli(ctx, ui);
        true
    } else if !primary
        && !ev.alt_key()
        && key.eq_ignore_ascii_case("r")
        && ctx.deck.get_untracked().is_some()
        && !ctx.switcher_open.get_untracked()
        && !super::studio::is_editing(ev)
    {
        ctx.slide(crate::preview_bridge::Slide::First);
        true
    } else {
        false
    };
    if handled {
        ev.prevent_default();
    }
}
