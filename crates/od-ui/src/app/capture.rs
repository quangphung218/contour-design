//! Screenshot of the preview (Studio preview toolbar): to the clipboard,
//! to the next chat message, or as a PNG download. The page is captured
//! by the bridge (`od:snapshot`), so it works without Chrome.

use super::export_menu::download;
use super::files_panel::FilesUi;
use super::icons::{Glyph, icon};
use super::menu_scrim::MenuScrim;
use super::preview_tools::{page_png, save_png};
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

/// Folder (in the project) for screenshots sent to the chat.
const SCREENSHOTS_DIR: &str = "screenshots";

/// Bytes of a `data:…;base64,` URL.
fn data_url_bytes(url: &str) -> Option<Vec<u8>> {
    let b64 = url.split_once(',')?.1;
    let bin = web_sys::window()?.atob(b64).ok()?;
    Some(bin.chars().map(|c| c as u8).collect())
}

fn png_blob(bytes: &[u8]) -> Result<web_sys::Blob, JsValue> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("image/png");
    web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
}

async fn shot(ctx: StudioCtx) -> Result<String, String> {
    page_png(ctx.tools)
        .await
        .ok_or_else(|| t(T::ScreenshotFailed).to_owned())
}

/// Copies the page as an image. The clipboard item is made inside the
/// click with a promise of the image (WebKit needs the user gesture).
fn copy_to_clipboard(ctx: StudioCtx, ui: FilesUi) {
    let blob = wasm_bindgen_futures::future_to_promise(async move {
        let png = shot(ctx).await.map_err(|e| JsValue::from_str(&e))?;
        let bytes = data_url_bytes(&png).ok_or_else(|| JsValue::from_str("bad image data"))?;
        Ok(png_blob(&bytes)?.into())
    });
    let write = (|| -> Result<js_sys::Promise, JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let ctor: js_sys::Function =
            js_sys::Reflect::get(&window, &JsValue::from_str("ClipboardItem"))?
                .dyn_into()
                .map_err(|_| JsValue::from_str("no ClipboardItem"))?;
        let record = js_sys::Object::new();
        js_sys::Reflect::set(&record, &JsValue::from_str("image/png"), &blob)?;
        let item = js_sys::Reflect::construct(&ctor, &js_sys::Array::of1(&record))?;
        Ok(window
            .navigator()
            .clipboard()
            .write(&js_sys::Array::of1(&item)))
    })();
    leptos::task::spawn_local(async move {
        let res = match write {
            Ok(p) => wasm_bindgen_futures::JsFuture::from(p).await.map(|_| ()),
            Err(e) => Err(e),
        };
        match res {
            Ok(()) => ui.say(t(T::ScreenshotCopied).to_owned()),
            Err(e) => ctx.notice.set(Some(tf(
                T::ScreenshotCopyFailed,
                &[&e.as_string().unwrap_or_else(|| format!("{e:?}"))],
            ))),
        }
    });
}

/// Saves the page under `screenshots/` and attaches it to the next message.
fn to_chat(ctx: StudioCtx, ui: FilesUi, busy: RwSignal<bool>) {
    busy.set(true);
    leptos::task::spawn_local(async move {
        let res = match shot(ctx).await {
            Ok(png) => {
                let name = format!(
                    "{SCREENSHOTS_DIR}/screenshot-{}.png",
                    js_sys::Date::now() as u64
                );
                save_png(ctx, name, &png).await
            }
            Err(e) => Err(e),
        };
        busy.set(false);
        match res {
            Ok(path) => {
                ctx.tools.attachments.update(|a| a.push(path));
                ctx.refresh_files();
                ui.say(t(T::ScreenshotAttached).to_owned());
            }
            Err(e) => ctx.notice.set(Some(e)),
        }
    });
}

fn save_file(ctx: StudioCtx, busy: RwSignal<bool>) {
    busy.set(true);
    let stem = ctx
        .active_untracked()
        .map(|p| {
            let name = p.rsplit('/').next().unwrap_or(&p).to_owned();
            name.rsplit_once('.')
                .map_or(name.clone(), |(s, _)| s.to_owned())
        })
        .unwrap_or_else(|| "preview".to_owned());
    leptos::task::spawn_local(async move {
        let res = shot(ctx).await;
        busy.set(false);
        match res.map(|png| data_url_bytes(&png)) {
            Ok(Some(bytes)) => {
                download(&bytes, &format!("{stem}.png"), "image/png");
            }
            Ok(None) => ctx.notice.set(Some(t(T::ScreenshotFailed).to_owned())),
            Err(e) => ctx.notice.set(Some(e)),
        }
    });
}

#[component]
pub fn ScreenshotMenu(ctx: StudioCtx) -> impl IntoView {
    let ui = FilesUi::get();
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    view! {
        <div class="menu-wrap">
            <button class="icon-btn" data-testid="screenshot" aria-haspopup="menu"
                aria-label=t(T::Screenshot) data-tip=t(T::Screenshot)
                aria-expanded=move || open.get().to_string() class:active=move || open.get()
                disabled=move || busy.get()
                on:click=move |_| open.update(|o| *o = !*o)>
                {move || if busy.get() { view! { <span class="spinner"></span> }.into_any() } else { icon(Glyph::Screenshot).into_any() }}
            </button>
            <Show when=move || open.get()>
                <MenuScrim on_close=move || open.set(false)/>
                <div class="menu" role="menu" data-testid="screenshot-menu">
                    <button class="menu-item" role="menuitem" data-testid="screenshot-copy" on:click=move |_| { open.set(false); copy_to_clipboard(ctx, ui); }>
                        {icon(Glyph::Clipboard)}{t(T::ScreenshotCopy)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="screenshot-chat" on:click=move |_| { open.set(false); to_chat(ctx, ui, busy); }>
                        {icon(Glyph::Attachment)}{t(T::ScreenshotToChat)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="screenshot-save" on:click=move |_| { open.set(false); save_file(ctx, busy); }>
                        {icon(Glyph::Download)}{t(T::ScreenshotSave)}
                    </button>
                </div>
            </Show>
        </div>
    }
}
