//! Export menu (spec 1.3) over the M5 export API, plus handoff: open the
//! project folder, copy a prompt for a coding agent.

use super::api;
use super::icons::{Glyph, icon};
use super::menu_scrim::MenuScrim;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::{HandoffResponse, OkResponse};
use serde_json::json;
use wasm_bindgen::JsCast;

/// How long an export or handoff result stays on screen.
const TOAST_MS: u32 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Zip,
    Pdf,
    Png,
    Jpeg,
    Webp,
    Pptx,
    PptxEditable,
    Mp4,
    Markdown,
    /// A React component's own source (track C2).
    Jsx,
    /// A React component as a page that renders it, runtime inlined.
    JsxHtml,
}

impl Format {
    fn testid(self) -> String {
        match self {
            Self::PptxEditable => "export-pptx-editable".to_owned(),
            Self::JsxHtml => "export-jsx-html".to_owned(),
            f => format!("export-{}", f.ext()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Html => t(T::ExportHtml),
            Self::Zip => t(T::ExportZip),
            Self::Pdf => t(T::ExportPdf),
            Self::Png => t(T::ExportPng),
            Self::Jpeg => t(T::ExportJpeg),
            Self::Webp => t(T::ExportWebp),
            Self::Pptx => t(T::ExportPptx),
            Self::PptxEditable => t(T::ExportPptxEditable),
            Self::Mp4 => t(T::ExportMp4),
            Self::Markdown => t(T::ExportMarkdown),
            Self::Jsx => t(T::ExportJsx),
            Self::JsxHtml => t(T::ExportJsxHtml),
        }
    }

    /// The name under its group: "PNG" under Image, "Editable" under
    /// PowerPoint.
    fn short_label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Pptx => t(T::ExportPptxSlides),
            Self::PptxEditable => t(T::ExportPptxEditableShort),
            f => f.label(),
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Html | Self::JsxHtml => "html",
            Self::Zip => "zip",
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Pptx | Self::PptxEditable => "pptx",
            Self::Mp4 => "mp4",
            Self::Markdown => "md",
            Self::Jsx => "jsx",
        }
    }

    /// Told apart in the download name: both PowerPoint kinds are `.pptx`.
    fn name_suffix(self) -> &'static str {
        match self {
            Self::Pptx => "-images",
            Self::PptxEditable => "-editable",
            _ => "",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Html | Self::JsxHtml => "text/html",
            Self::Zip => "application/zip",
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Mp4 => "video/mp4",
            Self::Markdown => "text/markdown",
            Self::Jsx => "text/jsx;charset=utf-8",
            Self::Pptx | Self::PptxEditable => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Self::Html => Glyph::FileCode,
            Self::Zip => Glyph::Download,
            Self::Pdf => Glyph::FileText,
            Self::Png | Self::Jpeg | Self::Webp => Glyph::FileImage,
            Self::Pptx | Self::PptxEditable => Glyph::Deck,
            Self::Mp4 => Glyph::Play,
            Self::Markdown => Glyph::Markdown,
            Self::Jsx | Self::JsxHtml => Glyph::FileCode,
        }
    }

    /// Needs a file to export (all but ZIP).
    fn needs_file(self) -> bool {
        self != Self::Zip
    }

    /// Whether the format applies to `path`: Markdown files export as
    /// Markdown, everything else needs an HTML page.
    fn fits(self, path: Option<&str>) -> bool {
        match (self, path) {
            (Self::Zip, _) => true,
            (_, None) => false,
            (Self::Markdown, Some(p)) => crate::file_tree::is_markdown(p),
            (Self::Jsx | Self::JsxHtml, Some(p)) => crate::jsx_export::is_component(p),
            (_, Some(p)) => super::studio::is_html_path(p),
        }
    }
}

/// A row of the export list: a format, or a group folded under one name
/// (the image formats, the two PowerPoint kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    One(Format),
    Group(Group),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Image,
    PowerPoint,
}

impl Group {
    fn formats(self) -> &'static [Format] {
        match self {
            Self::Image => &[Format::Png, Format::Jpeg, Format::Webp],
            Self::PowerPoint => &[Format::Pptx, Format::PptxEditable],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Image => t(T::Image),
            Self::PowerPoint => t(T::ExportPowerPoint),
        }
    }

    fn testid(self) -> &'static str {
        match self {
            Self::Image => "export-image",
            Self::PowerPoint => "export-powerpoint",
        }
    }

    fn glyph(self) -> Glyph {
        self.formats()[0].glyph()
    }
}

/// The export list in menu order, every format once.
const ROWS: [Row; 9] = [
    Row::One(Format::Html),
    Row::One(Format::Zip),
    Row::One(Format::Pdf),
    Row::Group(Group::Image),
    Row::Group(Group::PowerPoint),
    Row::One(Format::Mp4),
    Row::One(Format::Markdown),
    Row::One(Format::Jsx),
    Row::One(Format::JsxHtml),
];

/// The rows that apply to the open file `path`; a format that does not
/// (Markdown next to a page, PDF with nothing open) is left out, not shown
/// greyed without a reason.
fn rows_for(path: Option<&str>) -> Vec<Row> {
    ROWS.into_iter()
        .filter(|r| match r {
            Row::One(f) => f.fits(path),
            Row::Group(g) => g.formats().iter().any(|f| f.fits(path)),
        })
        .collect()
}

/// Saves `bytes` as `name` through a temporary object URL.
pub(super) fn download(bytes: &[u8], name: &str, mime: &str) -> Option<()> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array);
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).ok()?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
    let a: web_sys::HtmlAnchorElement = web_sys::window()?
        .document()?
        .create_element("a")
        .ok()?
        .dyn_into()
        .ok()?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Some(())
}

/// Whether the toast numbered `shown` may close when its time is up:
/// never while an export is still running, never over a newer message
/// (each message gets a new number, even one with the same text).
fn toast_expired(busy: bool, current: u32, shown: u32) -> bool {
    if busy {
        return false;
    }
    current == shown
}

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

#[component]
pub fn ExportMenu(ctx: StudioCtx) -> impl IntoView {
    let open = RwSignal::new(false);
    let files_ui = super::files_panel::FilesUi::get();
    let template_open = RwSignal::new(false);
    let handoff_open = RwSignal::new(false);
    let status = RwSignal::new(None::<Result<String, String>>);
    let busy = RwSignal::new(false);
    // Numbers each message shown, so a timer only closes its own toast.
    let shown_seq = RwSignal::new(0u32);
    // The result shows as a toast for a few seconds, then leaves.
    Effect::new(move |_| {
        if status.with(Option::is_none) {
            return;
        }
        let shown = shown_seq.get_untracked().wrapping_add(1);
        shown_seq.set(shown);
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(TOAST_MS).await;
            // The menu may be gone (the user left the Studio): its signals
            // are disposed then, and reading them would panic.
            let (Some(busy), Some(current)) =
                (busy.try_get_untracked(), shown_seq.try_get_untracked())
            else {
                return;
            };
            if toast_expired(busy, current, shown) {
                status.set(None);
            }
        });
    });

    // The group unfolded in the menu (Image ▸, PowerPoint ▸).
    let expanded = RwSignal::new(None::<Group>);
    Effect::new(move |_| {
        if !open.get() {
            expanded.set(None);
        }
    });
    let export = move |format: Format| {
        open.set(false);
        let file = ctx.active_untracked();
        if format.needs_file() && file.is_none() {
            return;
        }
        let project = ctx
            .project
            .with_untracked(|p| p.as_ref().map(|p| p.name.clone()))
            .unwrap_or_else(|| "project".to_owned());
        let name = match &file {
            Some(f) if format == Format::Jsx => crate::jsx_export::download_name(f),
            Some(f) if format.needs_file() => {
                format!("{}{}.{}", stem(f), format.name_suffix(), format.ext())
            }
            _ => format!("{project}{}.{}", format.name_suffix(), format.ext()),
        };
        busy.set(true);
        status.set(Some(Ok(tf(T::Exporting, &[format.ext()]))));
        leptos::task::spawn_local(async move {
            let base = ctx.base();
            let res = match format {
                Format::Zip => api::get_bytes(&format!("{base}/archive")).await,
                Format::Markdown | Format::Jsx => {
                    let path = file.clone().unwrap_or_default();
                    api::get_bytes(&format!("{base}/raw/{}", api::file_path(&path))).await
                }
                Format::JsxHtml => {
                    let path = file.clone().unwrap_or_default();
                    api::get_bytes(&format!("{base}/react/{}", api::file_path(&path))).await
                }
                _ => {
                    let kind = match format {
                        Format::Html => "html",
                        Format::Pdf => "pdf",
                        Format::Png | Format::Jpeg | Format::Webp => "image",
                        Format::Mp4 => "mp4",
                        Format::PptxEditable => "pptx-editable",
                        _ => "pptx",
                    };
                    let mut body = json!({ "fileName": file, "title": project });
                    if format == Format::Png {
                        body["imageFormat"] = json!("png");
                    } else if format == Format::Jpeg {
                        body["imageFormat"] = json!("jpeg");
                    } else if format == Format::Webp {
                        body["imageFormat"] = json!("webp");
                    }
                    api::post_bytes(&format!("{base}/export/{kind}"), &body).await
                }
            };
            busy.set(false);
            match res {
                Ok(bytes) => {
                    download(&bytes, &name, format.mime());
                    status.set(Some(Ok(tf(T::Exported, &[&name]))));
                }
                Err(e) => status.set(Some(Err(tf(T::ExportFailed, &[&e.to_string()])))),
            }
        });
    };

    let open_folder = move |_| {
        open.set(false);
        leptos::task::spawn_local(async move {
            let path = format!("{}/handoff/open", ctx.base());
            match api::post::<_, OkResponse>(&path, &json!({})).await {
                Ok(_) => status.set(Some(Ok(t(T::FolderOpened).to_owned()))),
                Err(e) => status.set(Some(Err(tf(T::HandoffFailed, &[&e.to_string()])))),
            }
        });
    };
    let copy_prompt = move |_| {
        open.set(false);
        leptos::task::spawn_local(async move {
            let path = format!("{}/handoff", ctx.base());
            let res = match api::get::<HandoffResponse>(&path).await {
                Ok(h) => copy_text(&h.prompt).await,
                Err(e) => Err(e.to_string()),
            };
            match res {
                Ok(()) => status.set(Some(Ok(t(T::PromptCopied).to_owned()))),
                Err(e) => status.set(Some(Err(tf(T::HandoffFailed, &[&e])))),
            }
        });
    };

    view! {
        <div class="menu-wrap">
            // The way out of the main flow: primary once there is a file.
            <button
                class="btn small"
                class:primary=move || ctx.active().is_some()
                class:active=move || open.get()
                data-testid="export"
                aria-haspopup="menu"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {icon(Glyph::Export)}
                {t(T::Export)}
                {icon(Glyph::ChevronDown)}
            </button>
            <Show when=move || open.get()>
                <MenuScrim on_close=move || open.set(false)/>
                <div class="menu" role="menu" data-testid="export-menu">
                    {move || {
                        let path = ctx.active();
                        rows_for(path.as_deref())
                            .into_iter()
                            .map(|row| match row {
                                Row::One(f) => format_item(f, export, false).into_any(),
                                Row::Group(g) => {
                                    let unfolded = move || expanded.get() == Some(g);
                                    view! {
                                        <button
                                            class="menu-item"
                                            role="menuitem"
                                            data-testid=g.testid()
                                            aria-haspopup="true"
                                            aria-expanded=move || unfolded().to_string()
                                            on:click=move |_| expanded.update(|e| *e = if *e == Some(g) { None } else { Some(g) })
                                        >
                                            {icon(g.glyph())}
                                            {g.label()}
                                            <span class="menu-hint">{move || icon(if unfolded() { Glyph::ChevronDown } else { Glyph::ChevronRight })}</span>
                                        </button>
                                        {move || unfolded().then(|| g
                                            .formats()
                                            .iter()
                                            .map(|&f| format_item(f, export, true))
                                            .collect_view())}
                                    }
                                    .into_any()
                                }
                            })
                            .collect_view()
                    }}
                    <div class="menu-sep">{t(T::FileSection)}</div>
                    <button class="menu-item" role="menuitem" data-testid="file-new-tab" disabled=move || ctx.active().is_none()
                        on:click=move |_| { open.set(false); if let Some(p) = ctx.active_untracked() { super::files_panel::open_in_new_tab(ctx, &p); } }>
                        {icon(Glyph::ExternalLink)}{t(T::OpenInNewTab)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="file-copy-path" disabled=move || ctx.active().is_none()
                        on:click=move |_| { open.set(false); if let Some(p) = ctx.active_untracked() { super::files_panel::copy_path(ctx, files_ui, &p, true); } }>
                        {icon(Glyph::Link)}{t(T::CopyFullPath)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="file-download" disabled=move || ctx.active().is_none()
                        on:click=move |_| { open.set(false); if let Some(p) = ctx.active_untracked() { super::files_panel::download_file(ctx, &p); } }>
                        {icon(Glyph::Download)}{t(T::DownloadFile)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="save-template" disabled=move || !ctx.active().is_some_and(|p| super::studio::is_html_path(&p))
                        on:click=move |_| { open.set(false); template_open.set(true); }>
                        {icon(Glyph::Grid)}{t(T::SaveAsTemplate)}
                    </button>
                    <div class="menu-sep">{t(T::Handoff)}</div>
                    <button class="menu-item" role="menuitem" data-testid="handoff-open" on:click=open_folder>
                        {icon(Glyph::FolderOpen)}
                        {t(T::HandoffOpenFolder)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="handoff-copy" on:click=copy_prompt>
                        {icon(Glyph::Handoff)}
                        {t(T::HandoffCopyPrompt)}
                        <span class="menu-hint">{format!("{}⇧K", super::shortcuts::modifier())}</span>
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="finalize" disabled=move || ctx.live_run.with(Option::is_some)
                        on:click=move |_| { open.set(false); finalize(ctx); }>
                        {icon(Glyph::Markdown)}
                        {move || finalize_label(ctx)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="handoff-options" on:click=move |_| { open.set(false); handoff_open.set(true); }>
                        {icon(Glyph::Settings)}
                        {t(T::HandoffChoose)}
                    </button>
                </div>
            </Show>
            <Show when=move || template_open.get()>
                <super::export_extra::SaveTemplateDialog ctx=ctx open=template_open status=status/>
            </Show>
            <Show when=move || handoff_open.get()>
                <super::export_extra::HandoffDialog ctx=ctx open=handoff_open status=status/>
            </Show>
            <div class="toast-region" aria-live="polite">
                {move || status.get().map(|s| {
                    let (error, text) = match s {
                        Ok(m) => (false, m),
                        Err(e) => (true, e),
                    };
                    let glyph = if error {
                        Glyph::Warning
                    } else if busy.get() {
                        Glyph::Loader
                    } else {
                        Glyph::CheckCircle
                    };
                    view! {
                        <div class="toast" class:error=error role="status" data-testid="export-status">
                            <span class="toast-ico" class:spin=move || busy.get()>{icon(glyph)}</span>
                            <span class="toast-text">{text}</span>
                            <button class="chip-x" aria-label=t(T::Close) on:click=move |_| status.set(None)>{icon(Glyph::Close)}</button>
                        </div>
                    }
                })}
            </div>
        </div>
    }
}

/// A menu item that exports `f`; `nested` under its group's name.
fn format_item(f: Format, export: impl Fn(Format) + 'static, nested: bool) -> impl IntoView {
    view! {
        <button
            class="menu-item"
            class:menu-sub=nested
            role="menuitem"
            data-testid=f.testid()
            on:click=move |_| export(f)
        >
            {icon(f.glyph())}
            {if nested { f.short_label() } else { f.label() }}
        </button>
    }
}

/// DESIGN.md state of the project: missing, current, or older than the
/// newest page (the spec is stale).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DesignMd {
    Missing,
    Current,
    Stale,
}

pub(super) fn design_md_state(files: &[od_contracts::ProjectFile]) -> DesignMd {
    let Some(spec) = files.iter().find(|f| f.path == DESIGN_MD) else {
        return DesignMd::Missing;
    };
    let newest_page = files
        .iter()
        .filter(|f| super::studio::is_html_path(&f.path))
        .map(|f| f.mtime)
        .fold(f64::MIN, f64::max);
    if newest_page > spec.mtime {
        DesignMd::Stale
    } else {
        DesignMd::Current
    }
}

/// The hand-off spec file at the project root.
const DESIGN_MD: &str = "DESIGN.md";

fn finalize_label(ctx: StudioCtx) -> &'static str {
    match ctx.files.with(|f| design_md_state(f)) {
        DesignMd::Missing => t(T::Finalize),
        DesignMd::Current => t(T::Refinalize),
        DesignMd::Stale => t(T::RefinalizeStale),
    }
}

/// Asks the agent to write (or refresh) `DESIGN.md`, the design package a
/// coding agent starts from.
fn finalize(ctx: StudioCtx) {
    ctx.send(t(T::FinalizePrompt).to_owned(), ctx.shell);
}

/// Copies `text` to the clipboard.
pub(super) async fn copy_text(text: &str) -> Result<(), String> {
    let clipboard = web_sys::window()
        .ok_or("no window")?
        .navigator()
        .clipboard();
    wasm_bindgen_futures::JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::{DesignMd, design_md_state, toast_expired};
    use od_contracts::{FileKind, ProjectFile};

    fn file(path: &str, mtime: f64) -> ProjectFile {
        ProjectFile {
            name: path.to_owned(),
            path: path.to_owned(),
            entry_type: "file".to_owned(),
            size: 1,
            mtime,
            kind: FileKind::Text,
            mime: "text/plain".to_owned(),
            artifact_kind: None,
            artifact_manifest: None,
        }
    }

    #[test]
    fn design_md_is_stale_once_a_page_is_newer() {
        assert_eq!(
            design_md_state(&[file("index.html", 5.0)]),
            DesignMd::Missing
        );
        let spec = file("DESIGN.md", 10.0);
        assert_eq!(
            design_md_state(&[spec.clone(), file("index.html", 5.0)]),
            DesignMd::Current
        );
        assert_eq!(
            design_md_state(&[spec, file("a/b.html", 11.0)]),
            DesignMd::Stale
        );
    }

    #[test]
    fn the_menu_lists_only_what_the_open_file_exports_to() {
        use super::{Format, Group, Row, rows_for};
        assert_eq!(rows_for(None), [Row::One(Format::Zip)]);
        let page = rows_for(Some("deck.html"));
        assert!(page.contains(&Row::Group(Group::Image)));
        assert!(page.contains(&Row::Group(Group::PowerPoint)));
        assert!(page.contains(&Row::One(Format::Mp4)));
        assert!(!page.contains(&Row::One(Format::Markdown)));
        assert!(!page.contains(&Row::One(Format::Jsx)));
        assert_eq!(
            rows_for(Some("notes.md")),
            [Row::One(Format::Zip), Row::One(Format::Markdown)]
        );
        // Every format is reachable once.
        let mut all: Vec<Format> = super::ROWS
            .iter()
            .flat_map(|r| match r {
                Row::One(f) => vec![*f],
                Row::Group(g) => g.formats().to_vec(),
            })
            .collect();
        all.dedup();
        assert_eq!(all.len(), 12);
    }

    #[test]
    fn a_toast_closes_only_over_itself_and_not_mid_export() {
        assert!(toast_expired(false, 3, 3));
        // A newer message, even with the same text, has its own number and
        // keeps its full time on screen.
        assert!(!toast_expired(false, 4, 3));
        // "Exporting…" stays until the export ends, however long it takes.
        assert!(!toast_expired(true, 3, 3));
    }
}
