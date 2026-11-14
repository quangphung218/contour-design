//! Viewers for files that are not HTML pages (Studio preview): Markdown
//! with Editor / Split / Preview and auto-save, and PDF, media and Office
//! files with Download and Open in new tab (Office files have no preview).

use super::api;
use super::file_state::{Autosave, ReadState, Save, read_error_banner};
use super::files_panel::{
    FilesUi, download_file, open_in_new_tab, raw_url, replace_content, upload,
};
use super::icons::{Glyph, icon};
use super::studio::StudioCtx;
use super::tasks::{Tasks, live};
use crate::file_tree as ft;
use crate::i18n::{T, t, tf};
use crate::markdown;
use leptos::prelude::*;
use od_contracts::{FileKind, OfficeTextResponse, ProjectFile};

/// Quiet time before the editor saves, ms.
const SAVE_DELAY_MS: u32 = 800;
/// `localStorage` key of the Markdown view mode.
const MD_MODE_KEY: &str = "od.markdown.mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Editor,
    Split,
    Preview,
}

impl Mode {
    const ALL: [Self; 3] = [Self::Editor, Self::Split, Self::Preview];

    fn key(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::Split => "split",
            Self::Preview => "preview",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Editor => t(T::MdEditor),
            Self::Split => t(T::MdSplit),
            Self::Preview => t(T::MdPreview),
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Self::Editor => Glyph::Code,
            Self::Split => Glyph::Split,
            Self::Preview => Glyph::Eye,
        }
    }

    fn stored() -> Self {
        let v = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(MD_MODE_KEY).ok().flatten());
        Self::ALL
            .into_iter()
            .find(|m| v.as_deref() == Some(m.key()))
            .unwrap_or(Self::Split)
    }

    fn store(self) {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(MD_MODE_KEY, self.key());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Clean,
    Saving,
    Saved(String),
    Failed(String),
}

fn clock() -> String {
    let d = js_sys::Date::new_0();
    format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
}

/// `![name](link)` for an image uploaded next to the document.
fn image_markdown(doc: &str, stored: &str) -> String {
    let dir = ft::parent(doc);
    let link = stored
        .strip_prefix(dir)
        .map_or(stored, |r| r.trim_start_matches('/'));
    format!("![{}]({})", ft::base(stored), link.replace(' ', "%20"))
}

/// Markdown editor and preview of `path`, saved as you type.
#[component]
pub fn MarkdownView(ctx: StudioCtx, path: String) -> impl IntoView {
    let ui = FilesUi::get();
    let mode = RwSignal::new(Mode::stored());
    let text = RwSignal::new(String::new());
    let read = RwSignal::new(ReadState::<()>::default());
    let status = RwSignal::new(Status::Clean);
    // What the file holds as far as this editor knows.
    let on_disk = StoredValue::new(String::new());
    let path = StoredValue::new(path);
    let area = NodeRef::<leptos::html::Textarea>::new();
    let tasks = Tasks::here();
    let load = move || {
        read.update(ReadState::retry);
        // Read before the task: its first step runs even after the view went.
        let url = raw_url(ctx, &path.get_value());
        tasks.spawn(async move {
            let res = api::get_text(&url).await;
            let was_loaded = read.with_untracked(ReadState::loaded);
            let res = res.map(|body| {
                let disk = on_disk.get_value();
                let dirty = text.with_untracked(|t| *t != disk);
                // Unsaved edits win over what the agent wrote.
                if !was_loaded || (!dirty && body != disk) {
                    on_disk.set_value(body.clone());
                    text.set(body);
                }
            });
            read.update(|r| r.read(res.map_err(|e| ctx.failed(&e))));
        });
    };
    // Load, and reload when the file changes on disk (the agent wrote it)
    // unless there are unsaved edits.
    Effect::new(move |_| {
        ctx.preview_rev.track();
        load();
    });
    // Saved a moment after typing stops, on blur and Cmd+S, and when the
    // view goes (tab switch) with a change not saved yet.
    let autosave = Autosave::new(SAVE_DELAY_MS, move |save: Save| {
        let content = text.get_untracked();
        if content == on_disk.get_value() {
            return;
        }
        status.set(Status::Saving);
        // Started here: on leaving, the save runs after the view is disposed.
        let work = replace_content(
            ctx,
            &path.get_value(),
            &content,
            "Markdown",
            T::MdSaveFailed,
            save,
        );
        tasks.spawn_then(work, move |res| {
            if !save.is_latest() {
                return;
            }
            match res {
                Ok(_) => {
                    on_disk.set_value(content);
                    status.set(Status::Saved(clock()));
                }
                Err(e) => {
                    save.failed();
                    status.set(Status::Failed(e));
                }
            }
        });
    });
    // Images pasted or dropped on the editor are uploaded next to the
    // document and linked at the cursor.
    let insert_images = move |files: Vec<web_sys::File>| {
        let images: Vec<web_sys::File> = files
            .into_iter()
            .filter(|f| f.type_().starts_with("image/"))
            .collect();
        if images.is_empty() {
            return false;
        }
        // The upload runs to its end; the link goes in while the editor is
        // there.
        let doc = path.get_value();
        let dir = ft::parent(&doc).to_owned();
        let work = async move { upload(ctx, T::UploadFailed, images, &dir, None).await };
        tasks.spawn_then(work, move |res| {
            match res {
                Ok(stored) => {
                    let snippet = stored
                        .iter()
                        .map(|s| image_markdown(&doc, s))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let at = area
                        .get_untracked()
                        .and_then(|a| a.selection_start().ok().flatten())
                        .map_or(usize::MAX, |i| i as usize);
                    text.update(|t| {
                        // `selectionStart` counts UTF-16 units.
                        let byte = t
                            .char_indices()
                            .scan(0usize, |units, (i, c)| {
                                let here = *units;
                                *units += c.len_utf16();
                                Some((here, i))
                            })
                            .find(|(u, _)| *u >= at)
                            .map_or(t.len(), |(_, i)| i);
                        t.insert_str(byte, &format!("{snippet}\n"));
                    });
                    ctx.refresh_files();
                    ui.refresh(ctx);
                    autosave.changed();
                }
                Err(e) => ctx.notice.set(Some(e)),
            }
        });
        true
    };
    let preview = move || {
        view! { <div class="md md-preview" data-testid="md-preview" inner_html=move || markdown::rebase_images(&markdown::to_html(&text.get()), &format!("{}/", raw_url(ctx, ft::parent(&path.get_value())).trim_end_matches('/')))></div> }
    };
    let editor = move || {
        view! {
            <textarea
                class="md-editor mono"
                data-testid="md-editor"
                node_ref=area
                aria-label=t(T::MdEditor)
                placeholder=t(T::MdPlaceholder)
                spellcheck="true"
                prop:value=move || text.get()
                on:input=live(move |ev| {
                    text.set(event_target_value(&ev));
                    autosave.changed();
                })
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if (ev.meta_key() || ev.ctrl_key()) && ev.key().eq_ignore_ascii_case("s") {
                        ev.prevent_default();
                        autosave.save_now();
                    }
                }
                on:blur=live(move |_| autosave.save_now())
                on:paste=move |ev: web_sys::ClipboardEvent| {
                    let files = ev
                        .clipboard_data()
                        .and_then(|d| d.files())
                        .map(|l| (0..l.length()).filter_map(|i| l.get(i)).collect())
                        .unwrap_or_default();
                    if insert_images(files) {
                        ev.prevent_default();
                    }
                }
                on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                on:drop=move |ev: web_sys::DragEvent| {
                    let files = ev
                        .data_transfer()
                        .and_then(|d| d.files())
                        .map(|l| (0..l.length()).filter_map(|i| l.get(i)).collect())
                        .unwrap_or_default();
                    if insert_images(files) {
                        ev.prevent_default();
                        ev.stop_propagation();
                    }
                }
            ></textarea>
        }
    };
    view! {
        <div class="md-view" data-testid="markdown-view" data-mode=move || mode.get().key()>
            <div class="md-bar">
                <div class="segmented small" role="radiogroup" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::MdViewMode)>
                    {Mode::ALL.into_iter().map(|m| view! {
                        <button class="seg" role="radio" data-testid=format!("md-mode-{}", m.key())
                            aria-checked=move || (mode.get() == m).to_string() tabindex=move || super::focus::roving(mode.get() == m) class:active=move || mode.get() == m
                            on:click=move |_| { mode.set(m); m.store(); }>
                            {icon(m.glyph())}<span class="btn-label">{m.label()}</span>
                        </button>
                    }).collect_view()}
                </div>
                <span class="spacer"></span>
                <span class="subtle md-status" data-testid="md-status" role="status">
                    {move || match status.get() {
                        Status::Clean => t(T::MdAutoSaveOn).to_owned(),
                        Status::Saving => t(T::Saving).to_owned(),
                        Status::Saved(at) => tf(T::MdAutoSaved, &[&at]),
                        Status::Failed(e) => e,
                    }}
                </span>
            </div>
            {read_error_banner(read, "empty-state", "md", T::MdReadFailed, load)}
            <Show when=move || read.with(ReadState::loaded) fallback=move || read.with(ReadState::waiting).then(|| view! { <div class="skeleton code-skeleton"></div> })>
                <div class="md-panes">
                    {move || match mode.get() {
                        Mode::Editor => editor().into_any(),
                        Mode::Preview => preview().into_any(),
                        Mode::Split => view! { {editor()}{preview()} }.into_any(),
                    }}
                </div>
            </Show>
        </div>
    }
}

/// Whether the Studio shows `file` with [`DocView`].
#[must_use]
pub fn is_document(file: &ProjectFile) -> bool {
    matches!(
        file.kind,
        FileKind::Pdf
            | FileKind::Video
            | FileKind::Audio
            | FileKind::Document
            | FileKind::Presentation
            | FileKind::Spreadsheet
            | FileKind::Binary
    )
}

/// `1.2 MB` style size.
#[must_use]
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// PDF, video, audio: the browser's own player; Office and other binary
/// files: a card. Every kind gets Download and Open in new tab.
#[component]
pub fn DocView(ctx: StudioCtx, file: ProjectFile) -> impl IntoView {
    let url = format!(
        "{}?v={}",
        raw_url(ctx, &file.path),
        ctx.preview_rev.get_untracked()
    );
    let kind_label = t(match file.kind {
        FileKind::Pdf => T::KindPdf,
        FileKind::Video => T::KindVideo,
        FileKind::Audio => T::KindAudio,
        FileKind::Document => T::KindWordDoc,
        FileKind::Presentation => T::KindSlidesDoc,
        FileKind::Spreadsheet => T::KindSheet,
        _ => T::KindBinary,
    });
    let (p1, p2) = (file.path.clone(), file.path.clone());
    let body = match file.kind {
        FileKind::Pdf => view! { <iframe class="doc-frame" data-testid="doc-frame" title=file.name.clone() src=url></iframe> }.into_any(),
        FileKind::Video => view! { <video class="doc-media" data-testid="doc-video" controls src=url></video> }.into_any(),
        FileKind::Audio => view! { <audio class="doc-audio" data-testid="doc-audio" controls src=url></audio> }.into_any(),
        FileKind::Document | FileKind::Presentation | FileKind::Spreadsheet => view! {
            <OfficeText ctx=ctx path=file.path.clone()/>
        }
        .into_any(),
        _ => view! {
            <div class="empty-state" data-testid="doc-unavailable">
                <span class="empty-icon">{icon(Glyph::File)}</span>
                <strong>{t(T::PreviewUnavailable)}</strong>
                <p>{t(T::PreviewUnavailableHint)}</p>
            </div>
        }
        .into_any(),
    };
    view! {
        <div class="doc-view" data-testid="doc-view">
            <div class="md-bar">
                <span class="subtle">{format!("{kind_label} · {}", size_label(file.size))}</span>
                <span class="spacer"></span>
                <button class="btn small ghost" data-testid="doc-download" on:click=move |_| download_file(ctx, &p1)>{icon(Glyph::Download)}{t(T::DownloadFile)}</button>
                <button class="btn small ghost" data-testid="doc-open" on:click=move |_| open_in_new_tab(ctx, &p2)>{icon(Glyph::ExternalLink)}{t(T::OpenInNewTab)}</button>
            </div>
            <div class="doc-body">{body}</div>
        </div>
    }
}

/// Text of a Word, PowerPoint or Excel file, per slide or sheet; files
/// that cannot be read say so and keep Download and Open.
#[component]
fn OfficeText(ctx: StudioCtx, path: String) -> impl IntoView {
    let url = StoredValue::new(format!(
        "{}/office-text/{}",
        ctx.base(),
        api::file_path(&path)
    ));
    let read = RwSignal::new(ReadState::<OfficeTextResponse>::default());
    let tasks = Tasks::here();
    let load = move || {
        read.update(ReadState::retry);
        let url = url.get_value();
        tasks.spawn(async move {
            let res = api::get::<OfficeTextResponse>(&url).await;
            read.update(|r| r.read(res.map_err(|e| ctx.failed(&e))));
        });
    };
    load();
    move || match read.with(|r| (r.value().cloned(), r.error().map(str::to_owned))) {
        (None, None) => view! { <div class="skeleton code-skeleton"></div> }.into_any(),
        (None, Some(e)) => view! {
            <div class="empty-state" data-testid="doc-unavailable">
                <span class="empty-icon">{icon(Glyph::File)}</span>
                <strong>{t(T::PreviewUnavailable)}</strong>
                <p>{t(T::PreviewUnavailableHint)}</p>
                <p class="subtle">{e}</p>
                <button class="btn small" data-testid="office-retry" on:click=move |_| load()>
                    {icon(Glyph::Restore)}{t(T::NextRetry)}
                </button>
            </div>
        }
        .into_any(),
        (Some(doc), _) => {
            let label = match doc.kind.as_str() {
                "presentation" => T::SlideN,
                "spreadsheet" => T::SheetN,
                _ => T::KindWordDoc,
            };
            let empty = doc.sections.iter().all(|s| s.text.trim().is_empty());
            view! {
                <div class="office-text" data-testid="office-text">
                    {empty.then(|| view! { <p class="muted">{t(T::OfficeNoText)}</p> })}
                    {doc.sections.into_iter().map(|s| view! {
                        <section class="office-section">
                            {(!s.title.is_empty()).then(|| view! { <h3 class="eyebrow">{tf(label, &[&s.title])}</h3> })}
                            <pre class="office-body">{s.text}</pre>
                        </section>
                    }).collect_view()}
                </div>
            }
            .into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_image_links() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(1536), "1.5 KB");
        assert_eq!(size_label(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(
            image_markdown("docs/brief.md", "docs/hero shot.png"),
            "![hero shot.png](hero%20shot.png)"
        );
        assert_eq!(image_markdown("brief.md", "a.png"), "![a.png](a.png)");
    }
}
