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

/// Opens a web page in the system browser: a `_blank` link clicked in a
/// design, which the shell's navigation guard refuses in the window
/// (QA-054). Does nothing outside the desktop app.
pub fn open_link(url: &str) {
    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&args, &JsValue::from_str("url"), &JsValue::from_str(url));
    if let Some(promise) = invoke("open_link", &args) {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Whether this copy of the desktop app can update itself (the shell's
/// `update_support`): `ready`, `devBuild` or `package`. `None` outside the
/// desktop app.
pub async fn update_support() -> Option<String> {
    let promise = invoke("update_support", &JsValue::UNDEFINED)?;
    JsFuture::from(promise).await.ok()?.as_string()
}

/// Update now: the shell downloads release `version`, checks its signature
/// and installs it, sending `od:update-progress` window events meanwhile.
///
/// # Errors
/// The failure's `kind` (`network`, `signature`, `noBuild`, `unsupported`,
/// `busy`, `cancelled`, `failed`); `unsupported` outside the desktop app.
pub async fn install_update(version: &str) -> Result<(), String> {
    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &args,
        &JsValue::from_str("version"),
        &JsValue::from_str(version),
    );
    let Some(promise) = invoke("install_update", &args) else {
        return Err("unsupported".to_owned());
    };
    JsFuture::from(promise).await.map(|_| ()).map_err(|e| {
        js_sys::Reflect::get(&e, &JsValue::from_str("kind"))
            .ok()
            .and_then(|k| k.as_string())
            .unwrap_or_else(|| "failed".to_owned())
    })
}

/// Stops a running Update now.
pub fn cancel_update() {
    if let Some(promise) = invoke("cancel_update", &JsValue::UNDEFINED) {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Restarts the app into the update just installed.
pub fn restart_app() {
    if let Some(promise) = invoke("restart_app", &JsValue::UNDEFINED) {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Gives the native menu the UI's words (`labels`, an object of
/// `MenuLabels` fields; empty for English). Does nothing outside the app.
pub fn set_menu_labels(labels: &serde_json::Value) {
    let Ok(args) = js_sys::JSON::parse(&serde_json::json!({ "labels": labels }).to_string()) else {
        return;
    };
    if let Some(promise) = invoke("set_menu_labels", &args) {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// [`save_file`]'s error outside the app: no native shell to call.
pub const UNSUPPORTED: &str = "unsupported";

/// Saves `bytes` through the native Save dialog, `name` suggested: the
/// path written, `None` when cancelled.
///
/// # Errors
/// Not in the app (`unsupported`), or the file could not be written.
pub async fn save_file(bytes: &[u8], name: &str) -> Result<Option<String>, String> {
    let tauri = internals().ok_or_else(|| UNSUPPORTED.to_owned())?;
    let invoke = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| UNSUPPORTED.to_owned())?;
    // The body goes raw (an MP4 can be large); the name, URI-encoded, in a
    // header.
    let headers = js_sys::Object::new();
    let encoded: String = js_sys::encode_uri_component(name).into();
    let _ = js_sys::Reflect::set(
        &headers,
        &JsValue::from_str("x-file-name"),
        &JsValue::from_str(&encoded),
    );
    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&options, &JsValue::from_str("headers"), &headers);
    let body = js_sys::Uint8Array::from(bytes);
    let promise = invoke
        .call3(&tauri, &JsValue::from_str("save_file"), &body, &options)
        .ok()
        .and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
        .ok_or_else(|| UNSUPPORTED.to_owned())?;
    match JsFuture::from(promise).await {
        Ok(v) => Ok(v.as_string()),
        Err(e) => Err(e.as_string().unwrap_or_else(|| format!("{e:?}"))),
    }
}
