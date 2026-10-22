//! Calls into the Tauri shell when the UI runs inside the desktop app.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

fn internals() -> Option<JsValue> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__"))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

/// Whether the native shell is present (folder dialog available).
#[must_use]
pub fn is_desktop() -> bool {
    internals().is_some()
}

/// Opens the native folder picker. `None` when cancelled or not on desktop.
pub async fn pick_folder() -> Option<String> {
    let tauri = internals()?;
    let invoke = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke"))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    let promise = invoke
        .call1(&tauri, &JsValue::from_str("pick_folder"))
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()?;
    JsFuture::from(promise).await.ok()?.as_string()
}
