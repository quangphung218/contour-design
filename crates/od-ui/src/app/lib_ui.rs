//! Pieces the library pages share (Design systems, Plugins, Media,
//! Templates): a modal, an initials tile, uploads, clipboard, file reads.

use super::icons::{Glyph, icon};
use crate::i18n::{T, t};
use gloo_net::http::Request;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// A modal dialog: Esc and a click on the backdrop close it.
///
/// While it is open the rest of the page is `inert` ([`ModalStack`]) and Tab
/// goes round the dialog's own controls ([`tab_round`]), so focus never
/// leaves it and Esc always reaches it; once it closes, focus goes back to
/// the control that opened it.
///
/// The backdrop is a sibling of the dialog, not its parent, and only the
/// dialog listens for keys: a button inside that closes the modal (or
/// navigates away) removes it while its click is still bubbling, and no
/// listener of the removed tree may run after that.
#[component]
pub fn Modal(
    title: String,
    #[prop(optional)] subtitle: Option<String>,
    glyph: Glyph,
    on_close: Callback<()>,
    #[prop(optional)] wide: bool,
    #[prop(optional, into)] testid: Option<&'static str>,
    /// A selector inside the dialog to focus on open (the safe answer)
    /// instead of the dialog itself.
    #[prop(optional, into)]
    initial_focus: Option<&'static str>,
    children: Children,
) -> impl IntoView {
    let label = title.clone();
    // Taken before the dialog takes the focus: the opener gets it back.
    super::focus::return_focus_on_cleanup();
    // Focus the dialog so Esc reaches it right away.
    let dialog = NodeRef::<leptos::html::Div>::new();
    let shown = StoredValue::new(None::<SendWrapper<web_sys::Element>>);
    on_cleanup(move || {
        if let Some(overlay) = shown.try_get_value().flatten() {
            ModalStack::close(&overlay);
        }
    });
    Effect::new(move |_| {
        if let Some(el) = dialog.get() {
            if let Some(overlay) = el.parent_element()
                && shown.with_value(Option::is_none)
            {
                ModalStack::open(&overlay);
                shown.set_value(Some(SendWrapper::new(overlay)));
            }
            let inner = initial_focus
                .and_then(|sel| el.query_selector(sel).ok().flatten())
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
            let _ = match inner {
                Some(inner) => inner.focus(),
                None => el.focus(),
            };
        }
    });
    view! {
        <div class="overlay center">
            <div class="modal-backdrop" aria-hidden="true" on:click=move |_| on_close.run(())></div>
            <div
                class="dialog"
                class:lib-wide=wide
                role="dialog"
                aria-modal="true"
                aria-label=label
                data-testid=testid
                tabindex="-1"
                node_ref=dialog
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        ev.stop_propagation();
                        on_close.run(());
                    } else if ev.key() == "Tab"
                        && let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                        && let Some(to) = tab_round(&el, ev.shift_key())
                    {
                        ev.prevent_default();
                        let _ = to.focus();
                    }
                }
            >
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(glyph)}</span>
                    <div class="grow">
                        <h2>{title}</h2>
                        {subtitle.map(|s| view! { <p>{s}</p> })}
                    </div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| on_close.run(())>
                        {icon(Glyph::Close)}
                    </button>
                </div>
                {children()}
            </div>
        </div>
    }
}

/// What Tab can reach inside a dialog, in order.
const TABBABLE: &str = "a[href], button:not(:disabled), input:not([type=hidden]):not(:disabled), select:not(:disabled), textarea:not(:disabled), iframe, summary, [contenteditable=true], [tabindex]:not([tabindex='-1'])";

/// Where Tab (or Shift+Tab with `back`) goes from the focused element in
/// `dialog` when the browser's own order would leave it: from the last
/// control to the first, from the first back to the last, and from the
/// dialog itself (or anything outside it) to the first or last. `None`
/// when the browser's own step stays inside.
fn tab_round(dialog: &web_sys::Element, back: bool) -> Option<web_sys::HtmlElement> {
    let list = dialog.query_selector_all(TABBABLE).ok()?;
    let items: Vec<web_sys::HtmlElement> = (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .filter(|e| {
            // Tab reaches it (a roving chip's `tabindex="-1"` is skipped:
            // as the last one, Tab from the real last control would leave),
            // shown, and not tucked away in a closed `<details>`.
            let r = e.get_bounding_client_rect();
            e.tab_index() >= 0 && (r.width() > 0.0 || r.height() > 0.0)
        })
        .collect();
    let (first, last) = match (items.first(), items.last()) {
        (Some(f), Some(l)) => (f.clone(), l.clone()),
        _ => return dialog.clone().dyn_into::<web_sys::HtmlElement>().ok(),
    };
    let active = document().active_element();
    let inside = active
        .as_ref()
        .is_some_and(|a| a != dialog && dialog.contains(Some(a)));
    let at = |e: &web_sys::HtmlElement| {
        active
            .as_ref()
            .is_some_and(|a| a == e.unchecked_ref::<web_sys::Element>())
    };
    match (back, inside) {
        (false, false) => Some(first),
        (true, false) => Some(last),
        (false, true) if at(&last) => Some(first),
        (true, true) if at(&first) => Some(last),
        _ => None,
    }
}

thread_local! {
    static OPEN_MODALS: std::cell::RefCell<Vec<web_sys::Element>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The open modals' overlays, last opened on top. Everything beside the
/// top one's path to `<body>` is `inert` (out of Tab, clicks and screen
/// readers); a toast region stays live. A modal opened over another (an
/// update or leave prompt over a library dialog) is on top whatever part
/// of the page it sits in: the page is made inert again for it, and for
/// the one under it when it closes.
struct ModalStack;

/// Marks the `inert` a modal set, so only those are taken off.
const INERT_MARK: &str = "data-modal-inert";

impl ModalStack {
    fn open(overlay: &web_sys::Element) {
        OPEN_MODALS.with(|m| m.borrow_mut().push(overlay.clone()));
        Self::apply();
    }

    fn close(overlay: &web_sys::Element) {
        OPEN_MODALS.with(|m| m.borrow_mut().retain(|o| o != overlay));
        Self::apply();
    }

    fn apply() {
        let doc = document();
        if let Ok(marked) = doc.query_selector_all(&format!("[{INERT_MARK}]")) {
            for el in (0..marked.length())
                .filter_map(|i| marked.item(i))
                .filter_map(|n| n.dyn_into::<web_sys::Element>().ok())
            {
                let _ = el.remove_attribute("inert");
                let _ = el.remove_attribute(INERT_MARK);
            }
        }
        let top = OPEN_MODALS.with(|m| m.borrow().iter().rev().find(|o| o.is_connected()).cloned());
        let Some(mut node) = top else {
            return;
        };
        while let Some(parent) = node.parent_element() {
            let mut child = parent.first_element_child();
            while let Some(c) = child {
                let spared = c == node
                    || c.has_attribute("inert")
                    || c.matches(".toast-region, script, style, link")
                        .unwrap_or(false);
                if !spared {
                    let _ = c.set_attribute("inert", "");
                    let _ = c.set_attribute(INERT_MARK, "");
                }
                child = c.next_element_sibling();
            }
            if parent.tag_name().eq_ignore_ascii_case("body") {
                break;
            }
            node = parent;
        }
    }
}

/// Whether a key event comes from inside a modal dialog (page shortcuts
/// ignore those).
#[must_use]
pub fn from_dialog(ev: &web_sys::Event) -> bool {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.closest(".dialog").ok().flatten())
        .is_some()
}

/// An inline "are you sure" line (the Projects page pattern; a blocking
/// `window.confirm` would re-enter the app's event handlers).
#[component]
pub fn ConfirmBar(
    message: String,
    on_confirm: Callback<()>,
    on_cancel: Callback<()>,
    #[prop(into)] testid: &'static str,
) -> impl IntoView {
    let label = message.clone();
    view! {
        <div class="card padded confirm-bar" role="alertdialog" aria-label=label data-testid=testid>
            <span class="confirm grow">{message}</span>
            <button class="btn small" on:click=move |_| on_cancel.run(())>{t(T::Cancel)}</button>
            <button class="btn small danger" data-testid=format!("{testid}-yes") on:click=move |_| on_confirm.run(())>{t(T::Delete)}</button>
        </div>
    }
}

/// Up to two initials of `name`, for a tile standing in for a logo.
#[must_use]
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let s: String = match words.as_slice() {
        [] => "?".to_owned(),
        [one] => one.chars().take(2).collect(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    s.to_uppercase()
}

/// One of four quiet tints, stable per id (the template covers' tints).
#[must_use]
pub fn tint(id: &str) -> u32 {
    id.bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        % 4
}

/// Copies `text` to the clipboard.
pub async fn copy_text(text: &str) -> Result<(), String> {
    let clipboard = web_sys::window()
        .ok_or("no window")?
        .navigator()
        .clipboard();
    JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The files picked in an `<input type="file">`.
#[must_use]
pub fn picked_files(ev: &web_sys::Event) -> Vec<web_sys::File> {
    let Some(input) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        return Vec::new();
    };
    let files = input.files();
    let out = files
        .map(|list| (0..list.length()).filter_map(|i| list.get(i)).collect())
        .unwrap_or_default();
    // Picking the same file again must fire `change` again.
    input.set_value("");
    out
}

/// `POST`s `files` as multipart `file` parts.
pub async fn upload<T: DeserializeOwned>(path: &str, files: &[web_sys::File]) -> Result<T, String> {
    let form = web_sys::FormData::new().map_err(|e| format!("{e:?}"))?;
    for f in files {
        form.append_with_blob_and_filename("file", f, &f.name())
            .map_err(|e| format!("{e:?}"))?;
    }
    let res = Request::post(path)
        .body(form)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.ok() {
        // The same words as every other call: no status (QA-081).
        return Err(super::api::error_of(path, res).await.to_string());
    }
    res.json::<T>().await.map_err(|e| e.to_string())
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A picked file's bytes.
pub async fn file_bytes(file: &web_sys::File) -> Result<Vec<u8>, String> {
    let buf = JsFuture::from(file.array_buffer())
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// A file size as `12 KB` / `3.4 MB`.
#[must_use]
pub fn size_label(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    }
}

/// The value of query parameter `key` in the router's address. Not
/// `window.location`: a navigation to another path renders the new page
/// before the browser's address changes, so a page reading that while it
/// mounts got the query of the page it came from (Home's `?skill=` sent
/// the Plugins on to a skill).
#[must_use]
pub fn query_param(key: &str) -> Option<String> {
    leptos_router::hooks::use_query_map()
        .with_untracked(|q| q.get(key))
        .filter(|v| !v.is_empty())
}

/// Text in the UI language from an i18n map (`{ "en": …, "vi": … }`) or a
/// plain string.
#[must_use]
pub fn localized(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(m) => m
            .get(crate::i18n::locale().tag())
            .or_else(|| m.get("en"))
            .and_then(|s| s.as_str())
            .map(str::to_owned),
        _ => None,
    }
    .filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_of_names() {
        assert_eq!(initials("Recipe cards"), "RC");
        assert_eq!(initials("airbnb"), "AI");
        assert_eq!(initials("  "), "?");
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe]), "//4=");
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(4096), "4 KB");
        assert_eq!(size_label(3 * 1024 * 1024 + 400_000), "3.4 MB");
    }
}
