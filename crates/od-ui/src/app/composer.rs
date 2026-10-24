//! Composer pieces shared by Home and the Studio chat (track B3-chat): the
//! "+" context menu, attaching files (button, paste, drop) and uploading
//! them, the `@` mention and `/` command palette, the staged context chips
//! and the model picker. The rules live in [`crate::mention`] and
//! [`crate::models`]; this module is the browser side.

use super::icons::{Glyph, icon};
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use crate::mention::{self, Candidate, MentionKind, Slash, Trigger};
use crate::models::{self, Selection};
use leptos::ev;
use leptos::html::{Div, Input, Textarea};
use leptos::prelude::*;
use od_contracts::{SkillsResponse, UploadResponse};
use send_wrapper::SendWrapper;
use serde_json::Value;
use wasm_bindgen::JsCast;

/// Files per batch and size per file the daemon accepts (the original's
/// limits).
pub const MAX_FILES: usize = 12;
const MAX_FILE_BYTES: f64 = 200.0 * 1024.0 * 1024.0;

/// A file picked, pasted or dropped, until it is uploaded.
#[derive(Clone)]
pub struct Upload {
    pub id: String,
    pub name: String,
    pub file: SendWrapper<web_sys::File>,
    /// Local preview of an image (`blob:` URL).
    pub preview: Option<String>,
    pub state: UploadState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadState {
    /// Kept until there is a project to upload to (Home).
    Waiting,
    Uploading,
    Failed(String),
}

impl Upload {
    fn new(file: web_sys::File) -> Self {
        let preview = file
            .type_()
            .starts_with("image/")
            .then(|| web_sys::Url::create_object_url_with_blob(&file).ok())
            .flatten();
        Self {
            id: api::random_id(),
            name: file.name(),
            file: SendWrapper::new(file),
            preview,
            state: UploadState::Waiting,
        }
    }
}

/// Chat composer state the Studio keeps for the page's life.
#[derive(Clone, Copy)]
pub struct ChatState {
    /// Skills, plugins and MCP servers mentioned for the next message.
    pub context: RwSignal<Vec<Candidate>>,
    /// Files not uploaded yet (uploading or failed).
    pub uploads: RwSignal<Vec<Upload>>,
    /// The chat pane is folded away (focus on the workspace).
    pub collapsed: RwSignal<bool>,
    /// Chat pane width in px, once dragged.
    pub width: RwSignal<Option<f64>>,
}

/// `localStorage` key of the chat pane width.
const CHAT_WIDTH_KEY: &str = "od.studio.chat-width";

impl ChatState {
    #[must_use]
    pub fn new() -> Self {
        let width = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(CHAT_WIDTH_KEY).ok().flatten())
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|w| w.is_finite() && *w > 0.0);
        Self {
            context: RwSignal::new(Vec::new()),
            uploads: RwSignal::new(Vec::new()),
            collapsed: RwSignal::new(false),
            width: RwSignal::new(width),
        }
    }

    pub fn set_width(self, px: f64) {
        self.width.set(Some(px));
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(CHAT_WIDTH_KEY, &format!("{px:.0}"));
        }
    }
}

/// `metadata.extra` keys of what Home stages for the first turn.
pub const PENDING_ATTACHMENTS: &str = "pendingAttachments";
pub const PENDING_CONTEXT: &str = "pendingContext";

/// Removes the files and mentions Home staged in `meta`; `None` when there
/// were none.
pub fn take_pending(
    meta: &mut od_contracts::ProjectMetadata,
) -> Option<(Vec<String>, Vec<Candidate>)> {
    let files = meta.extra.remove(PENDING_ATTACHMENTS);
    let context = meta.extra.remove(PENDING_CONTEXT);
    if files.is_none() && context.is_none() {
        return None;
    }
    Some((
        files
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        context
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
    ))
}

/// The files of a `FileList` (input, paste, drop).
pub fn files_of(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
    let Some(list) = list else {
        return Vec::new();
    };
    (0..list.length()).filter_map(|i| list.get(i)).collect()
}

/// The files that may go: at most [`MAX_FILES`], none over 200 MB; the
/// second value says what was left out.
pub fn admit(files: Vec<web_sys::File>) -> (Vec<Upload>, Option<String>) {
    let mut notice = None;
    let mut out = Vec::new();
    for f in files {
        if f.size() > MAX_FILE_BYTES {
            notice = Some(tf(T::FileTooLarge, &[&f.name()]));
            continue;
        }
        if out.len() == MAX_FILES {
            notice = Some(tf(T::TooManyFiles, &[&MAX_FILES.to_string()]));
            break;
        }
        out.push(Upload::new(f));
    }
    (out, notice)
}

/// Uploads one file to the project at `base` (`/api/projects/:id`); the
/// project-relative path it got.
pub async fn upload(base: &str, file: &web_sys::File) -> Result<String, String> {
    let form = web_sys::FormData::new().map_err(|e| format!("{e:?}"))?;
    form.append_with_blob_and_filename("files", file, &file.name())
        .map_err(|e| format!("{e:?}"))?;
    let res: UploadResponse = api::post_form(&format!("{base}/upload"), form)
        .await
        .map_err(|e| e.to_string())?;
    res.files
        .into_iter()
        .next()
        .map(|f| f.path)
        .ok_or_else(|| "no file in the response".to_owned())
}

/// Uploads the queued files one by one; each uploaded path goes into
/// `ready` and its card leaves `uploads`, a failed one stays with its error.
/// A card removed meanwhile is not attached.
pub async fn upload_all(
    base: String,
    uploads: RwSignal<Vec<Upload>>,
    ready: RwSignal<Vec<String>>,
) {
    let todo: Vec<Upload> = uploads.with_untracked(|u| {
        u.iter()
            .filter(|u| u.state != UploadState::Uploading)
            .cloned()
            .collect()
    });
    // Claimed before the first await, so a second batch started meanwhile
    // (paste, then drop) does not upload these again.
    for item in &todo {
        set_state(uploads, &item.id, UploadState::Uploading);
    }
    for item in todo {
        let result = upload(&base, &item.file).await;
        let still = uploads.with_untracked(|u| u.iter().any(|x| x.id == item.id));
        if !still {
            continue;
        }
        match result {
            Ok(path) => {
                uploads.update(|u| u.retain(|x| x.id != item.id));
                if let Some(url) = &item.preview {
                    let _ = web_sys::Url::revoke_object_url(url);
                }
                ready.update(|r| crate::workspace::add_missing(r, vec![path]));
            }
            Err(e) => set_state(uploads, &item.id, UploadState::Failed(e)),
        }
    }
}

fn set_state(uploads: RwSignal<Vec<Upload>>, id: &str, state: UploadState) {
    uploads.update(|u| {
        if let Some(x) = u.iter_mut().find(|x| x.id == id) {
            x.state = state;
        }
    });
}

/// Folders cannot be uploaded; tells whether a drop carries one.
fn drops_a_folder(dt: &web_sys::DataTransfer) -> bool {
    let items = dt.items();
    (0..items.length()).any(|i| {
        items
            .get(i)
            .and_then(|item| item.webkit_get_as_entry().ok().flatten())
            .is_some_and(|entry| entry.is_directory())
    })
}

/// What the `@` palette can mention, loaded once per composer.
#[derive(Clone, Copy)]
pub struct Sources {
    skills: LocalResource<Vec<Candidate>>,
    plugins: LocalResource<Vec<Candidate>>,
    mcp: LocalResource<Vec<Candidate>>,
    /// Media library assets (track C2), offered where the composer can
    /// import them.
    library: LocalResource<Vec<Candidate>>,
}

impl Sources {
    #[must_use]
    pub fn load() -> Self {
        let skills = LocalResource::new(|| async {
            api::get::<SkillsResponse>("/api/skills")
                .await
                .map(|r| r.skills)
                .unwrap_or_default()
                .into_iter()
                .map(|s| Candidate {
                    kind: MentionKind::Skill,
                    label: s.info.id.clone(),
                    detail: crate::i18n::rebrand(&s.info.description)
                        .chars()
                        .take(90)
                        .collect(),
                    id: s.info.id,
                    inputs: None,
                })
                .collect()
        });
        let plugins = LocalResource::new(|| async {
            api::get::<Value>("/api/plugins")
                .await
                .map(|v| {
                    v["plugins"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(mention::plugin_candidate)
                        .collect()
                })
                .unwrap_or_default()
        });
        let mcp = LocalResource::new(|| async {
            api::get::<Value>("/api/mcp/servers")
                .await
                .map(|v| {
                    v["servers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|s| s["enabled"].as_bool() != Some(false))
                        .filter_map(|s| {
                            let id = s["id"].as_str()?.to_owned();
                            Some(Candidate {
                                kind: MentionKind::Mcp,
                                label: id.clone(),
                                detail: s["label"].as_str().unwrap_or_default().to_owned(),
                                id,
                                inputs: None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        let library = LocalResource::new(|| async {
            super::library_picker::load_assets()
                .await
                .into_iter()
                .map(|a| mention::library_candidate(&a.project_id, &a.path, &a.project_name))
                .collect()
        });
        Self {
            skills,
            plugins,
            mcp,
            library,
        }
    }

    fn of(self, kind: MentionKind) -> Vec<Candidate> {
        match kind {
            MentionKind::Skill => self.skills.get().unwrap_or_default(),
            MentionKind::Plugin => self.plugins.get().unwrap_or_default(),
            MentionKind::Mcp => self.mcp.get().unwrap_or_default(),
            MentionKind::Library => self.library.get().unwrap_or_default(),
            MentionKind::File => Vec::new(),
        }
    }

    fn all(self, files: &[String], library: bool) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = files
            .iter()
            .map(|p| Candidate {
                kind: MentionKind::File,
                id: p.clone(),
                label: p.clone(),
                detail: String::new(),
                inputs: None,
            })
            .collect();
        for kind in [MentionKind::Plugin, MentionKind::Skill, MentionKind::Mcp] {
            out.extend(self.of(kind));
        }
        if library {
            out.extend(self.of(MentionKind::Library));
        }
        out
    }

    /// A listed plugin by id.
    #[must_use]
    pub fn plugin(self, id: &str) -> Option<Candidate> {
        self.plugins
            .get_untracked()
            .unwrap_or_default()
            .into_iter()
            .find(|c| c.id == id)
    }

    fn mcp_ids(self) -> Vec<String> {
        self.of(MentionKind::Mcp)
            .into_iter()
            .map(|c| c.id)
            .collect()
    }
}

/// Adds a mention to the staged context once.
pub fn stage(context: RwSignal<Vec<Candidate>>, c: Candidate) {
    context.update(|list| {
        if !list.iter().any(|x| x.kind == c.kind && x.id == c.id) {
            list.push(c);
        }
    });
}

fn kind_label(kind: MentionKind) -> &'static str {
    match kind {
        MentionKind::File => t(T::MentionFiles),
        MentionKind::Skill => t(T::MenuSkills),
        MentionKind::Plugin => t(T::MenuPlugins),
        MentionKind::Mcp => t(T::MenuMcp),
        MentionKind::Library => t(T::MentionLibrary),
    }
}

fn kind_glyph(kind: MentionKind) -> Glyph {
    match kind {
        MentionKind::File => Glyph::File,
        MentionKind::Skill => Glyph::Magic,
        MentionKind::Plugin => Glyph::Plugin,
        MentionKind::Mcp => Glyph::Mcp,
        MentionKind::Library => Glyph::Image,
    }
}

fn kind_key(kind: MentionKind) -> &'static str {
    match kind {
        MentionKind::File => "file",
        MentionKind::Skill => "skill",
        MentionKind::Plugin => "plugin",
        MentionKind::Mcp => "mcp",
        MentionKind::Library => "library",
    }
}

/// A palette row.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Mention(Candidate),
    Command(Slash),
}

/// The text area of a composer with its `@` / `/` palette, paste and drop.
#[component]
pub fn MentionArea(
    draft: RwSignal<String>,
    #[prop(into)] class: String,
    #[prop(into)] testid: String,
    #[prop(into)] aria_label: String,
    #[prop(into)] placeholder: Signal<String>,
    rows: u32,
    sources: Sources,
    /// Project files offered as mentions.
    #[prop(into)]
    files: Signal<Vec<String>>,
    context: RwSignal<Vec<Candidate>>,
    /// A project file was mentioned: attach it.
    on_file: Callback<String>,
    on_submit: Callback<()>,
    /// Files pasted or dropped.
    on_files: Callback<Vec<web_sys::File>>,
    /// `/mcp` alone was picked.
    on_mcp_settings: Callback<()>,
    on_notice: Callback<String>,
    #[prop(optional)] node_ref: Option<NodeRef<Textarea>>,
    #[prop(optional)] on_focus: Option<Callback<bool>>,
    /// Library assets picked with `@` (track C2); without it the palette
    /// offers no library rows.
    #[prop(optional)]
    on_library: Option<Callback<Vec<super::library_picker::LibraryAsset>>>,
) -> impl IntoView {
    let area = node_ref.unwrap_or_default();
    let open = RwSignal::new(None::<(bool, Trigger)>);
    let index = RwSignal::new(0usize);
    let dragging = RwSignal::new(false);

    let rows_now = Memo::new(move |_| -> Vec<Row> {
        let Some((slash, trigger)) = open.get() else {
            return Vec::new();
        };
        if slash {
            mention::slash_commands(&trigger.query, &sources.mcp_ids())
                .into_iter()
                .map(Row::Command)
                .collect()
        } else {
            let all = files.with(|f| sources.all(f, on_library.is_some()));
            mention::rank(&trigger.query, &all)
                .into_iter()
                .map(Row::Mention)
                .collect()
        }
    });

    let caret = move || -> Option<(String, usize)> {
        let el = area.get_untracked()?;
        let text = el.value();
        let at = el.selection_start().ok().flatten().unwrap_or(0) as usize;
        let byte = mention::byte_offset(&text, at);
        Some((text, byte))
    };
    let refresh = move || {
        let Some((text, at)) = caret() else {
            return;
        };
        let next = mention::slash_at(&text, at)
            .map(|t| (true, t))
            .or_else(|| mention::mention_at(&text, at).map(|t| (false, t)));
        if next.as_ref().map(|(s, t)| (*s, t.start))
            != open.get_untracked().map(|(s, t)| (s, t.start))
        {
            index.set(0);
        }
        open.set(next);
    };
    let place_caret = move |byte: usize| {
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            if let Some(el) = area.get_untracked() {
                let text = el.value();
                let at = u32::try_from(mention::utf16_offset(&text, byte)).unwrap_or(u32::MAX);
                let _ = el.focus();
                let _ = el.set_selection_range(at, at);
            }
        });
    };
    let pick = move |row: Row| {
        let Some((_, trigger)) = open.get_untracked() else {
            return;
        };
        let Some((text, at)) = caret() else {
            return;
        };
        let insert = match &row {
            // An asset becomes an attachment chip, not text.
            Row::Mention(c) if c.kind == MentionKind::Library => String::new(),
            Row::Mention(c) => mention::token(c),
            Row::Command(cmd) => mention::slash_insert(cmd),
        };
        let (next, byte) = mention::apply(&text, &trigger, at, &insert);
        open.set(None);
        match row {
            Row::Command(Slash::McpSettings) => {
                draft.set(String::new());
                on_mcp_settings.run(());
                return;
            }
            Row::Mention(c) if c.kind == MentionKind::File => on_file.run(c.id.clone()),
            Row::Mention(c) if c.kind == MentionKind::Library => {
                if let (Some(cb), Some((project_id, path))) = (on_library, mention::library_ref(&c))
                {
                    cb.run(vec![super::library_picker::LibraryAsset {
                        project_id: project_id.to_owned(),
                        project_name: c.detail.clone(),
                        path: path.to_owned(),
                        kind: String::new(),
                        mtime: 0.0,
                    }]);
                }
            }
            Row::Mention(c) => stage(context, c),
            Row::Command(_) => {}
        }
        draft.set(next);
        place_caret(byte);
    };

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        if open.with_untracked(Option::is_some) {
            let n = rows_now.with_untracked(Vec::len);
            match key.as_str() {
                "ArrowDown" if n > 0 => {
                    ev.prevent_default();
                    index.update(|i| *i = (*i + 1) % n);
                    return;
                }
                "ArrowUp" if n > 0 => {
                    ev.prevent_default();
                    index.update(|i| *i = (*i + n - 1) % n);
                    return;
                }
                "Enter" | "Tab" if n > 0 && !ev.is_composing() => {
                    ev.prevent_default();
                    let row = rows_now
                        .with_untracked(|r| r.get(index.get_untracked().min(n - 1)).cloned());
                    if let Some(row) = row {
                        pick(row);
                    }
                    return;
                }
                "Escape" => {
                    ev.prevent_default();
                    ev.stop_propagation();
                    open.set(None);
                    return;
                }
                _ => {}
            }
        }
        if key == "Enter" && !ev.shift_key() && !ev.is_composing() {
            ev.prevent_default();
            open.set(None);
            on_submit.run(());
        }
    };

    let on_paste = move |ev: web_sys::ClipboardEvent| {
        let files = files_of(ev.clipboard_data().and_then(|d| d.files()));
        if !files.is_empty() {
            ev.prevent_default();
            on_files.run(files);
        }
    };
    let on_drop = move |ev: web_sys::DragEvent| {
        dragging.set(false);
        let Some(dt) = ev.data_transfer() else {
            return;
        };
        if dt.types().length() == 0 || !dt.types().includes(&"Files".into(), 0) {
            return;
        }
        ev.prevent_default();
        if drops_a_folder(&dt) {
            on_notice.run(t(T::FolderNotAttachable).to_owned());
            return;
        }
        let files = files_of(dt.files());
        if !files.is_empty() {
            on_files.run(files);
        }
    };
    let carries_files = |ev: &web_sys::DragEvent| {
        ev.data_transfer()
            .is_some_and(|dt| dt.types().includes(&"Files".into(), 0))
    };

    view! {
        <div
            class="mention-wrap"
            class:dragging=move || dragging.get()
            on:dragover=move |ev: web_sys::DragEvent| {
                if carries_files(&ev) {
                    ev.prevent_default();
                    dragging.set(true);
                }
            }
            on:dragleave=move |_| dragging.set(false)
            on:drop=on_drop
        >
            <textarea
                class=class
                data-od-composer=""
                data-testid=testid
                aria-label=aria_label
                rows=rows
                node_ref=area
                placeholder=move || placeholder.get()
                aria-autocomplete="list"
                prop:value=move || draft.get()
                on:input=move |ev| {
                    draft.set(event_target_value(&ev));
                    refresh();
                }
                on:click=move |_| refresh()
                on:keyup=move |ev: web_sys::KeyboardEvent| {
                    if matches!(ev.key().as_str(), "ArrowLeft" | "ArrowRight" | "Home" | "End") {
                        refresh();
                    }
                }
                on:keydown=on_key
                on:paste=on_paste
                on:focus=move |_| if let Some(cb) = on_focus { cb.run(true) }
                on:blur=move |_| {
                    if let Some(cb) = on_focus {
                        cb.run(false);
                    }
                    // After a click on a row has landed.
                    leptos::task::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(150).await;
                        open.set(None);
                    });
                }
            ></textarea>
            {move || open.get().map(|(slash, trigger)| {
                let rows = rows_now.get();
                view! { <Palette slash=slash query=trigger.query rows=rows index=index pick=Callback::new(pick)/> }
            })}
        </div>
    }
}

#[component]
fn Palette(
    slash: bool,
    query: String,
    rows: Vec<Row>,
    index: RwSignal<usize>,
    pick: Callback<Row>,
) -> impl IntoView {
    let empty = rows.is_empty();
    let mut last_kind = None;
    let items = rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let head = match &row {
                Row::Mention(c) if last_kind != Some(c.kind) => {
                    last_kind = Some(c.kind);
                    Some(view! { <div class="menu-sep" role="presentation">{kind_label(c.kind)}</div> })
                }
                _ => None,
            };
            let (glyph, label, detail, kind) = match &row {
                Row::Mention(c) => (kind_glyph(c.kind), format!("@{}", c.label), c.detail.clone(), kind_key(c.kind)),
                Row::Command(cmd) => (
                    Glyph::Slash,
                    cmd.label(),
                    match cmd {
                        Slash::McpSettings => t(T::SlashMcp),
                        Slash::McpUse(_) => t(T::SlashMcpUse),
                        Slash::Search => t(T::SlashSearch),
                    }
                    .to_owned(),
                    "command",
                ),
            };
            view! {
                {head}
                <button
                    type="button"
                    class="menu-item mention-item"
                    role="option"
                    data-testid="mention-item"
                    data-kind=kind
                    aria-selected=move || (index.get() == i).to_string()
                    on:mousedown=|ev| ev.prevent_default()
                    on:mouseenter=move |_| index.set(i)
                    on:click=move |_| pick.run(row.clone())
                >
                    {icon(glyph)}
                    <span class="mention-label">{label}</span>
                    {(!detail.is_empty()).then(|| view! { <span class="menu-hint">{detail}</span> })}
                </button>
            }
        })
        .collect_view();
    let message = empty.then(|| {
        let text = if query.is_empty() {
            t(T::MentionEmpty).to_owned()
        } else {
            tf(T::MentionNoResults, &[&query])
        };
        view! { <p class="mention-empty">{text}</p> }
    });
    view! {
        <div class="popover mention-pop" role="listbox" data-testid="mention-palette" aria-label=if slash { t(T::CommandsTitle) } else { t(T::AddContext) }>
            {slash.then(|| view! { <div class="menu-sep first" role="presentation">{t(T::CommandsTitle)}</div> })}
            {items}
            {message}
            <div class="mention-hint subtle">{t(T::MentionHint)}</div>
        </div>
    }
}

/// Mentioned skills, plugins and MCP servers waiting for the next message.
#[component]
pub fn ContextChips(context: RwSignal<Vec<Candidate>>, draft: RwSignal<String>) -> impl IntoView {
    move || {
        let list = context.get();
        (!list.is_empty()).then(|| {
            view! {
                <div class="comment-chips" aria-label=t(T::AddContext)>
                    {list
                        .into_iter()
                        .map(|c| {
                            let (id, kind) = (c.id.clone(), c.kind);
                            let token = format!("@{}", c.label);
                            view! {
                                <span class="chip comment-chip" data-testid="context-chip" data-kind=kind_key(kind) title=c.detail.clone()>
                                    {icon(kind_glyph(kind))}
                                    {c.label.clone()}
                                    <button
                                        type="button"
                                        class="chip-x"
                                        aria-label=tf(T::RemoveNamed, &[&c.label])
                                        on:click=move |_| {
                                            context.update(|l| l.retain(|x| !(x.kind == kind && x.id == id)));
                                            draft.update(|d| *d = d.replace(&format!("{token} "), "").replace(&token, ""));
                                        }
                                    >
                                        {icon(Glyph::Close)}
                                    </button>
                                </span>
                            }
                        })
                        .collect_view()}
                </div>
            }
        })
    }
}

/// Attached files: uploaded ones (`ready`, with a thumbnail for images) and
/// the ones still uploading or failed (`uploads`).
#[component]
pub fn UploadTray(
    uploads: RwSignal<Vec<Upload>>,
    ready: RwSignal<Vec<String>>,
    /// `/api/projects/:id` for thumbnails of uploaded images.
    #[prop(into)]
    base: Signal<Option<String>>,
    on_retry: Callback<String>,
) -> impl IntoView {
    let preview = RwSignal::new(None::<(String, String)>);
    view! {
        <Show when=move || !ready.with(Vec::is_empty) || !uploads.with(Vec::is_empty)>
            <div class="upload-tray" aria-label=t(T::FilesAttached) data-testid="upload-tray">
                {move || {
                    let base = base.get();
                    ready
                        .get()
                        .into_iter()
                        .map(|path| {
                            let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                            let remove = path.clone();
                            let src = base
                                .as_ref()
                                .filter(|_| is_image(&path))
                                .map(|b| format!("{b}/raw/{}", api::file_path(&path)));
                            let (open_name, open_src) = (name.clone(), src.clone());
                            view! {
                                <span class="chip comment-chip attachment" data-testid="attachment-chip" title=path.clone()>
                                    {match src {
                                        Some(s) => view! {
                                            <button type="button" class="thumb" aria-label=tf(T::PreviewNamed, &[&name])
                                                on:click=move |_| preview.set(open_src.clone().map(|s| (open_name.clone(), s)))>
                                                <img src=s alt=""/>
                                            </button>
                                        }.into_any(),
                                        None => icon(Glyph::Attachment).into_any(),
                                    }}
                                    <span class="attachment-name">{name.clone()}</span>
                                    <button type="button" class="chip-x" aria-label=tf(T::RemoveNamed, &[&name])
                                        on:click=move |_| ready.update(|a| a.retain(|p| p != &remove))>
                                        {icon(Glyph::Close)}
                                    </button>
                                </span>
                            }
                        })
                        .collect_view()
                }}
                {move || {
                    uploads
                        .get()
                        .into_iter()
                        .map(|u| {
                            let (id_retry, id_remove) = (u.id.clone(), u.id.clone());
                            let (state, failed) = match &u.state {
                                UploadState::Waiting => ("waiting", None),
                                UploadState::Uploading => ("uploading", None),
                                UploadState::Failed(e) => ("failed", Some(e.clone())),
                            };
                            let preview_url = u.preview.clone();
                            view! {
                                <span class="chip comment-chip attachment" data-testid="upload-chip" data-state=state title=failed.clone().unwrap_or_else(|| u.name.clone())>
                                    {match (state, preview_url) {
                                        ("uploading", _) => view! { <span class="spinner"></span> }.into_any(),
                                        ("failed", _) => icon(Glyph::Warning).into_any(),
                                        (_, Some(url)) => view! { <span class="thumb"><img src=url alt=""/></span> }.into_any(),
                                        _ => icon(Glyph::Attachment).into_any(),
                                    }}
                                    <span class="attachment-name">{u.name.clone()}</span>
                                    {failed.is_some().then(|| view! {
                                        <button type="button" class="chip-x" aria-label=t(T::NextRetry) data-testid="upload-retry"
                                            on:click=move |_| on_retry.run(id_retry.clone())>
                                            {icon(Glyph::Refresh)}
                                        </button>
                                    })}
                                    <button type="button" class="chip-x" aria-label=tf(T::RemoveNamed, &[&u.name])
                                        on:click=move |_| uploads.update(|l| l.retain(|x| x.id != id_remove))>
                                        {icon(Glyph::Close)}
                                    </button>
                                </span>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </Show>
        {move || preview.get().map(|(name, src)| view! {
            <div class="overlay center" role="dialog" aria-modal="true" aria-label=tf(T::PreviewNamed, &[&name])
                data-testid="attachment-preview"
                on:click=move |_| preview.set(None)
                on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { preview.set(None) }>
                <img class="attachment-full" src=src alt=name.clone()/>
            </div>
        })}
    }
}

fn is_image(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".avif"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// A hidden `<input type=file multiple>`; `trigger` opens it.
#[component]
pub fn FilePicker(trigger: RwSignal<u32>, on_files: Callback<Vec<web_sys::File>>) -> impl IntoView {
    let input = NodeRef::<Input>::new();
    Effect::new(move |prev: Option<u32>| {
        let n = trigger.get();
        if prev.is_some_and(|p| p != n) {
            if let Some(el) = input.get_untracked() {
                el.click();
            }
        }
        n
    });
    view! {
        <input
            type="file"
            multiple=true
            hidden=true
            data-testid="file-input"
            node_ref=input
            on:change=move |ev| {
                let el = event_target::<web_sys::HtmlInputElement>(&ev);
                let files = files_of(el.files());
                el.set_value("");
                if !files.is_empty() {
                    on_files.run(files);
                }
            }
        />
    }
}

/// Closes `open` on a press outside `wrap` or on Esc.
pub fn close_outside(open: RwSignal<bool>, wrap: NodeRef<Div>) {
    let down = window_event_listener(ev::mousedown, move |e| {
        if !open.get_untracked() {
            return;
        }
        let inside = wrap.get_untracked().is_some_and(|w| {
            e.target()
                .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                .is_some_and(|n| w.contains(Some(&n)))
        });
        if !inside {
            open.set(false);
        }
    });
    let key = window_event_listener(ev::keydown, move |e| {
        if e.key() == "Escape" && open.get_untracked() {
            open.set(false);
        }
    });
    on_cleanup(move || {
        down.remove();
        key.remove();
    });
}

/// Which part of the "+" menu shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlusView {
    Main,
    List(MentionKind),
    Toolbox,
}

/// Seeds the composer with a design toolbox action: its request, and a
/// mention of the bundled skill that fits it.
pub fn apply_toolbox(
    action: &crate::toolbox::Action,
    sources: Sources,
    context: RwSignal<Vec<Candidate>>,
    draft: RwSignal<String>,
) {
    let skills = sources.skills.get_untracked().unwrap_or_default();
    let ids: Vec<String> = skills.iter().map(|c| c.id.clone()).collect();
    let skill = crate::toolbox::skill_for(action, &ids)
        .and_then(|id| skills.iter().find(|c| c.id == id).cloned());
    let mut text = String::new();
    if let Some(c) = skill {
        text.push_str(&mention::token(&c));
        stage(context, c);
    }
    text.push_str(t(action.prompt));
    draft.set(text);
    super::chat_view::focus_selector("[data-od-composer]");
}

/// "+": attach files, mention a plugin, skill or MCP server, and the
/// caller's own items (`children`: working directory, linked folders…).
#[component]
pub fn PlusMenu(
    sources: Sources,
    context: RwSignal<Vec<Candidate>>,
    draft: RwSignal<String>,
    on_attach: Callback<()>,
    /// Open the menu above the button (chat composer at the bottom).
    #[prop(optional)]
    up: bool,
    /// Offer the design toolbox (Studio).
    #[prop(optional)]
    toolbox: bool,
    /// "Import from library" (track C2): the picked assets.
    #[prop(optional)]
    on_library: Option<Callback<Vec<super::library_picker::LibraryAsset>>>,
    #[prop(optional)] children: Option<ChildrenFn>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let picker = RwSignal::new(false);
    let view_of = RwSignal::new(PlusView::Main);
    let search = RwSignal::new(String::new());
    let wrap = NodeRef::<Div>::new();
    close_outside(open, wrap);
    let navigate = leptos_router::hooks::use_navigate();
    let to_mcp = Callback::new(move |()| navigate("/settings#mcp", Default::default()));
    let children = StoredValue::new(children);
    let add = move |c: Candidate| {
        open.set(false);
        draft.update(|d| {
            if !d.is_empty() && !d.ends_with(char::is_whitespace) {
                d.push(' ');
            }
            d.push_str(&mention::token(&c));
        });
        stage(context, c);
        super::chat_view::focus_selector("[data-od-composer]");
    };
    view! {
        <div class="menu-wrap" node_ref=wrap>
            <button
                type="button"
                class="icon-btn plus-btn"
                aria-label=t(T::AddContext)
                data-tip=t(T::AddContext)
                data-tip-pos="top"
                aria-expanded=move || open.get().to_string()
                data-testid="plus-menu"
                on:click=move |_| {
                    view_of.set(PlusView::Main);
                    open.update(|o| *o = !*o);
                }
            >
                {icon(Glyph::Add)}
            </button>
            <Show when=move || open.get()>
                <div class=if up { "menu left up plus-menu" } else { "menu left plus-menu" } role="menu">
                    {move || match view_of.get() {
                        PlusView::Main => {
                            view! {
                                <button type="button" class="menu-item" role="menuitem" data-testid="attach-files"
                                    on:click=move |_| { open.set(false); on_attach.run(()); }>
                                    {icon(Glyph::Attachment)}{t(T::AttachFiles)}
                                </button>
                                {on_library.is_some().then(|| view! {
                                    <button type="button" class="menu-item" role="menuitem" data-testid="plus-library"
                                        on:click=move |_| { open.set(false); picker.set(true); }>
                                        {icon(Glyph::Image)}{t(T::ImportFromLibrary)}
                                    </button>
                                })}
                                {[MentionKind::Plugin, MentionKind::Skill, MentionKind::Mcp].into_iter().map(|k| view! {
                                    <button type="button" class="menu-item" role="menuitem" data-testid=format!("plus-{}", kind_key(k))
                                        on:click=move |_| { search.set(String::new()); view_of.set(PlusView::List(k)); }>
                                        {icon(kind_glyph(k))}{kind_label(k)}
                                        <span class="menu-hint">{icon(Glyph::ChevronRight)}</span>
                                    </button>
                                }).collect_view()}
                                {toolbox.then(|| view! {
                                    <button type="button" class="menu-item" role="menuitem" data-testid="plus-toolbox"
                                        on:click=move |_| view_of.set(PlusView::Toolbox)>
                                        {icon(Glyph::Sparkle)}{t(T::DesignToolbox)}
                                        <span class="menu-hint">{icon(Glyph::ChevronRight)}</span>
                                    </button>
                                })}
                                {children.with_value(|c| c.as_ref().map(|c| view! { <div class="menu-sep" role="presentation"></div>{c()} }))}
                            }.into_any()
                        }
                        PlusView::Toolbox => view! {
                            <button type="button" class="menu-item" on:click=move |_| view_of.set(PlusView::Main)>
                                {icon(Glyph::ChevronLeft)}<strong>{t(T::DesignToolbox)}</strong>
                            </button>
                            {crate::toolbox::ACTIONS.iter().map(|a| view! {
                                <button type="button" class="menu-item" role="menuitem" data-testid="toolbox-action" data-action=a.id
                                    title=t(a.prompt)
                                    on:click=move |_| {
                                        open.set(false);
                                        apply_toolbox(a, sources, context, draft);
                                    }>
                                    {icon(Glyph::Sparkle)}{t(a.title)}
                                </button>
                            }).collect_view()}
                        }.into_any(),
                        PlusView::List(kind) => {
                            let items = move || {
                                let q = search.get();
                                let all = sources.of(kind);
                                let mut rows = mention::rank(&q, &all);
                                rows.truncate(40);
                                rows
                            };
                            let empty = match kind {
                                MentionKind::Plugin => t(T::NoPluginsYet),
                                MentionKind::Mcp => t(T::NoMcpYet),
                                _ => t(T::NoSkillsYet),
                            };
                            view! {
                                <button type="button" class="menu-item" on:click=move |_| view_of.set(PlusView::Main)>
                                    {icon(Glyph::ChevronLeft)}<strong>{kind_label(kind)}</strong>
                                </button>
                                <input class="input small plus-search" type="search" data-testid="plus-search"
                                    placeholder=t(T::SearchList) aria-label=t(T::SearchList)
                                    prop:value=move || search.get()
                                    on:input=move |ev| search.set(event_target_value(&ev))/>
                                <div class="plus-list">
                                    {move || {
                                        let rows = items();
                                        if rows.is_empty() {
                                            return view! { <p class="mention-empty">{empty}</p> }.into_any();
                                        }
                                        rows.into_iter().map(|c| {
                                            let label = c.label.clone();
                                            let detail = c.detail.clone();
                                            view! {
                                                <button type="button" class="menu-item" role="menuitem" data-testid="plus-item"
                                                    on:click=move |_| add(c.clone())>
                                                    {icon(kind_glyph(kind))}
                                                    <span class="mention-label">{label}</span>
                                                    <span class="menu-hint">{detail}</span>
                                                </button>
                                            }
                                        }).collect_view().into_any()
                                    }}
                                </div>
                                {(kind == MentionKind::Mcp).then(|| view! {
                                    <button type="button" class="menu-item" role="menuitem"
                                        on:click=move |_| { open.set(false); to_mcp.run(()); }>
                                        {icon(Glyph::Add)}{t(T::AddMcpServer)}
                                    </button>
                                })}
                            }.into_any()
                        }
                    }}
                </div>
            </Show>
            {move || (picker.get() && on_library.is_some()).then(|| view! {
                <super::library_picker::LibraryPicker
                    on_close=Callback::new(move |()| picker.set(false))
                    on_pick=Callback::new(move |assets| {
                        picker.set(false);
                        if let Some(cb) = on_library {
                            cb.run(assets);
                        }
                    })
                />
            })}
        </div>
    }
}

/// The agent, model and reasoning effort runs use, switched in place. Saved
/// to settings like the Settings page does (`agentModels`).
#[component]
pub fn ModelPicker(#[prop(optional)] up: bool) -> impl IntoView {
    let shell = Shell::get();
    let open = RwSignal::new(false);
    let wrap = NodeRef::<Div>::new();
    close_outside(open, wrap);
    let custom = RwSignal::new(false);
    let agent = move || shell.config.with(|_| shell.agent_id()).unwrap_or_default();
    let current = move || {
        let id = agent();
        shell
            .config
            .with(|c| c.as_ref().map(|c| models::selection(c, &id)))
            .unwrap_or_default()
    };
    let agent_name = move || {
        let id = agent();
        shell
            .agents
            .with(|a| match a {
                Some(Ok(list)) => list.iter().find(|x| x.id == id).map(|x| x.name.clone()),
                _ => None,
            })
            .unwrap_or(id)
    };
    let save = move |agent_id: String, sel: Selection| {
        let Some(base) = shell.config.get_untracked() else {
            return;
        };
        let next = models::with_selection(&base, &agent_id, &sel);
        leptos::task::spawn_local(async move {
            let _ = shell.save_config(next).await;
        });
    };
    view! {
        <div class="menu-wrap" node_ref=wrap>
            <button
                type="button"
                class="btn ghost small model-btn"
                aria-label=t(T::SwitchModel)
                data-tip=t(T::SwitchModel)
                data-tip-pos="top"
                aria-expanded=move || open.get().to_string()
                data-testid="model-picker"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {icon(Glyph::Agent)}
                <span class="model-label">
                    {move || {
                        let sel = current();
                        let model = sel.model.unwrap_or_else(|| t(T::DefaultChoice).to_owned());
                        match sel.reasoning {
                            Some(r) => format!("{} · {model} · {r}", agent_name()),
                            None => format!("{} · {model}", agent_name()),
                        }
                    }}
                </span>
                {icon(Glyph::ChevronDown)}
            </button>
            <Show when=move || open.get()>
                <div class=if up { "popover left up model-pop" } else { "popover left model-pop" } data-testid="model-popover">
                    {move || {
                        let agents: Vec<(String, String)> = shell.agents.with(|a| match a {
                            Some(Ok(list)) => list.iter().filter(|x| x.available).map(|x| (x.id.clone(), x.name.clone())).collect(),
                            _ => Vec::new(),
                        });
                        if agents.is_empty() {
                            return view! { <p class="mention-empty">{t(T::NoAgentOnPath)}</p> }.into_any();
                        }
                        let id = agent();
                        let sel = current();
                        let models_list = models::models_for(&id);
                        let efforts = models::reasoning_for(&id);
                        let model_value = sel.model.clone().unwrap_or_else(|| "default".to_owned());
                        let is_custom = custom.get() || !models_list.contains(&model_value.as_str());
                        let (id_m, id_c, id_r) = (id.clone(), id.clone(), id.clone());
                        let (sel_m, sel_c, sel_r) = (sel.clone(), sel.clone(), sel.clone());
                        view! {
                            <label class="field">
                                <span>{t(T::Agent)}</span>
                                <select data-testid="model-agent" on:change=move |ev| {
                                    let next = event_target_value(&ev);
                                    custom.set(false);
                                    let sel = shell.config.with_untracked(|c| c.as_ref().map(|c| models::selection(c, &next))).unwrap_or_default();
                                    save(next, sel);
                                }>
                                    {agents.into_iter().map(|(aid, name)| {
                                        let selected = aid == id;
                                        view! { <option value=aid selected=selected>{name}</option> }
                                    }).collect_view()}
                                </select>
                            </label>
                            <label class="field">
                                <span>{t(T::Model)}</span>
                                <select data-testid="model-select" on:change=move |ev| {
                                    let v = event_target_value(&ev);
                                    if v == "__custom" {
                                        custom.set(true);
                                        return;
                                    }
                                    custom.set(false);
                                    save(id_m.clone(), Selection { model: Some(v).filter(|v| v != "default"), ..sel_m.clone() });
                                }>
                                    {models_list.iter().map(|m| {
                                        let label = if *m == "default" { t(T::DefaultChoice).to_owned() } else { (*m).to_owned() };
                                        let selected = !is_custom && *m == model_value;
                                        view! { <option value=*m selected=selected>{label}</option> }
                                    }).collect_view()}
                                    <option value="__custom" selected=is_custom>{t(T::CustomModel)}</option>
                                </select>
                            </label>
                            {is_custom.then(|| view! {
                                <input class="input small" data-testid="model-custom" placeholder=t(T::CustomModel)
                                    prop:value=sel_c.model.clone().unwrap_or_default()
                                    on:change=move |ev| {
                                        let v = event_target_value(&ev).trim().to_owned();
                                        save(id_c.clone(), Selection { model: Some(v).filter(|v| !v.is_empty()), ..sel_c.clone() });
                                    }/>
                            })}
                            {(!efforts.is_empty()).then(|| {
                                let effort = sel_r.reasoning.clone().unwrap_or_else(|| "default".to_owned());
                                view! {
                                    <label class="field">
                                        <span>{t(T::ReasoningEffort)}</span>
                                        <select data-testid="model-reasoning" on:change=move |ev| {
                                            let v = event_target_value(&ev);
                                            save(id_r.clone(), Selection { reasoning: Some(v).filter(|v| v != "default"), ..sel_r.clone() });
                                        }>
                                            {efforts.iter().map(|e| {
                                                let label = if *e == "default" { t(T::DefaultChoice).to_owned() } else { (*e).to_owned() };
                                                view! { <option value=*e selected=*e == effort>{label}</option> }
                                            }).collect_view()}
                                        </select>
                                    </label>
                                }
                            })}
                        }.into_any()
                    }}
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn take_pending_removes_what_home_staged() {
        let mut meta: od_contracts::ProjectMetadata = serde_json::from_value(json!({
            "kind": "video",
            "videoLength": 8,
            "pendingAttachments": ["ref.png"],
            "pendingContext": [{ "kind": "skill", "id": "web-clone", "label": "web-clone" }]
        }))
        .unwrap_or_default();
        let (files, context) = take_pending(&mut meta).unwrap_or_default();
        assert_eq!(files, ["ref.png"]);
        assert_eq!(context.len(), 1);
        assert_eq!(context[0].kind, MentionKind::Skill);
        assert_eq!(
            meta.extra.get("videoLength"),
            Some(&json!(8)),
            "other keys kept"
        );
        assert!(!meta.extra.contains_key(PENDING_ATTACHMENTS));
        assert_eq!(take_pending(&mut meta), None);
    }
}
