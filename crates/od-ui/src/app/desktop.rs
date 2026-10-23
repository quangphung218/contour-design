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

/// Calls Tauri command `cmd` with `args` (an object, or undefined).
fn invoke(cmd: &str, args: &JsValue) -> Option<js_sys::Promise> {
    let tauri = internals()?;
    let invoke = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke"))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    invoke
        .call2(&tauri, &JsValue::from_str(cmd), args)
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()
}

/// Opens the native folder picker. `None` when cancelled or not on desktop.
pub async fn pick_folder() -> Option<String> {
    let promise = invoke("pick_folder", &JsValue::UNDEFINED)?;
    JsFuture::from(promise).await.ok()?.as_string()
}

/// Gives the native window the app's theme (`light`, `dark`, `system`),
/// so its title bar matches. Does nothing outside the desktop app.
pub fn set_theme(theme: &str) {
    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &args,
        &JsValue::from_str("theme"),
        &JsValue::from_str(theme),
    );
    if let Some(promise) = invoke("set_theme", &args) {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}
