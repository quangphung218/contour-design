//! Export menu (spec 1.3) over the M5 export API, plus handoff: open the
//! project folder, copy a prompt for a coding agent.

use super::api;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::{HandoffResponse, OkResponse};
use serde_json::json;
use wasm_bindgen::JsCast;

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

    /// Needs an HTML file to render (all but ZIP).
    fn needs_file(self) -> bool {
        self != Self::Zip
    }
}

/// Saves `bytes` as `name` through a temporary object URL.
fn download(bytes: &[u8], name: &str, mime: &str) -> Option<()> {
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

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

#[component]
pub fn ExportMenu(ctx: StudioCtx) -> impl IntoView {
    let open = RwSignal::new(false);
    let status = RwSignal::new(None::<Result<String, String>>);

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
                data-testid="export"
                aria-haspopup="menu"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {t(T::Export)}
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
                                    {f.label()}
                                </button>
                            }
                        })
                        .collect_view()}
                    <div class="menu-sep">{t(T::Handoff)}</div>
                    <button class="menu-item" role="menuitem" data-testid="handoff-open" on:click=open_folder>
                        {t(T::HandoffOpenFolder)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="handoff-copy" on:click=copy_prompt>
                        {t(T::HandoffCopyPrompt)}
                    </button>
                </div>
            </Show>
            {move || status.get().map(|s| match s {
                Ok(m) => view! { <span class="menu-status muted" data-testid="export-status">{m}</span> }.into_any(),
                Err(e) => view! { <span class="menu-status error-text" data-testid="export-status">{e}</span> }.into_any(),
            })}
        </div>
    }
}

/// Copies `text` to the clipboard.
async fn copy_text(text: &str) -> Result<(), String> {
    let clipboard = web_sys::window()
        .ok_or("no window")?
        .navigator()
        .clipboard();
    wasm_bindgen_futures::JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}
