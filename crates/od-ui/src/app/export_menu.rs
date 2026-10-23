//! Export menu (spec 1.3) over the M5 export API, plus handoff: open the
//! project folder, copy a prompt for a coding agent.

use super::api;
use super::icons::{Glyph, icon};
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
    Pptx,
}

impl Format {
    const ALL: [Self; 5] = [Self::Html, Self::Zip, Self::Pdf, Self::Png, Self::Pptx];

    fn label(self) -> &'static str {
        match self {
            Self::Html => t(T::ExportHtml),
            Self::Zip => t(T::ExportZip),
            Self::Pdf => t(T::ExportPdf),
            Self::Png => t(T::ExportPng),
            Self::Pptx => t(T::ExportPptx),
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Zip => "zip",
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Pptx => "pptx",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Html => "text/html",
            Self::Zip => "application/zip",
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Self::Html => Glyph::FileCode,
            Self::Zip => Glyph::Download,
            Self::Pdf => Glyph::FileText,
            Self::Png => Glyph::FileImage,
            Self::Pptx => Glyph::Deck,
        }
    }

    /// Needs an HTML file to render (all but ZIP).
    fn needs_file(self) -> bool {
        self != Self::Zip
    }
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
            Some(f) if format.needs_file() => format!("{}.{}", stem(f), format.ext()),
            _ => format!("{project}.{}", format.ext()),
        };
        busy.set(true);
        status.set(Some(Ok(tf(T::Exporting, &[format.ext()]))));
        leptos::task::spawn_local(async move {
            let base = ctx.base();
            let res = match format {
                Format::Zip => api::get_bytes(&format!("{base}/archive")).await,
                _ => {
                    let kind = match format {
                        Format::Html => "html",
                        Format::Pdf => "pdf",
                        Format::Png => "image",
                        _ => "pptx",
                    };
                    let mut body = json!({ "fileName": file, "title": project });
                    if format == Format::Png {
                        body["imageFormat"] = json!("png");
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
            <button
                class="btn small"
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
                <div class="menu" role="menu" data-testid="export-menu">
                    {Format::ALL
                        .into_iter()
                        .map(|f| {
                            view! {
                                <button
                                    class="menu-item"
                                    role="menuitem"
                                    data-testid=format!("export-{}", f.ext())
                                    disabled=move || f.needs_file() && ctx.active().is_none()
                                    on:click=move |_| export(f)
                                >
                                    {icon(f.glyph())}
                                    {f.label()}
                                </button>
                            }
                        })
                        .collect_view()}
                    <div class="menu-sep">{t(T::Handoff)}</div>
                    <button class="menu-item" role="menuitem" data-testid="handoff-open" on:click=open_folder>
                        {icon(Glyph::FolderOpen)}
                        {t(T::HandoffOpenFolder)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="handoff-copy" on:click=copy_prompt>
                        {icon(Glyph::Handoff)}
                        {t(T::HandoffCopyPrompt)}
                    </button>
                </div>
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
    use super::toast_expired;

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
