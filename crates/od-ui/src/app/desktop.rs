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
        super::tasks::detached(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Opens a web page in the system browser: a `_blank` link clicked in a
/// design, which the shell's navigation guard refuses in the window
/// (QA-054), or one of the app's own (see [`open_new_window`]). Does
/// nothing outside the desktop app.
pub fn open_link(url: &str) {
    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&args, &JsValue::from_str("url"), &JsValue::from_str(url));
    if let Some(promise) = invoke("open_link", &args) {
        super::tasks::detached(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Opens `url` (absolute, or relative to the page) in a new tab or window.
/// In the desktop app the system browser opens it (`open_link`): the
/// window opens no new window itself, since it cannot tell the app's
/// `window.open` from a design's (QA-061). Elsewhere
/// `window.open(url, target, features)`.
pub fn open_new_window(url: &str, target: &str, features: &str) {
    let Some(w) = web_sys::window() else {
        return;
    };
    if is_desktop() {
        let base = w.location().href().unwrap_or_default();
        if let Ok(u) = web_sys::Url::new_with_base(url, &base) {
            open_link(&u.href());
        }
    } else if features.is_empty() {
        let _ = w.open_with_url_and_target(url, target);
    } else {
        let _ = w.open_with_url_and_target_and_features(url, target, features);
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
        super::tasks::detached(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Restarts the app into the update just installed.
pub fn restart_app() {
    if let Some(promise) = invoke("restart_app", &JsValue::UNDEFINED) {
        super::tasks::detached(async move {
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
        super::tasks::detached(async move {
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

/// The user opened another design or the preview reloaded (QA-068): the
/// shell shows the page's `alert`,
/// `confirm` and `prompt` again though the last design's were blocked
/// (QA-064). Does nothing outside the desktop app.
pub fn reset_dialog_gate() {
    if let Some(promise) = invoke("reset_dialog_gate", &JsValue::UNDEFINED) {
        super::tasks::detached(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// The page took View ▸ Reload (`od:menu` `reload`): the shell then
/// leaves the reload to it instead of restarting a page it takes for hung
/// (QA-067). Does nothing outside the desktop app.
pub fn reload_ack() {
    if let Some(promise) = invoke("reload_ack", &JsValue::UNDEFINED) {
        super::tasks::detached(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

/// Whether the page's query (`location.search`) says the shell stopped
/// the design's preview: it hung or crashed the window, so the shell
/// loaded the app again with `preview=stopped` (QA-067).
#[must_use]
pub fn preview_was_stopped(search: &str) -> bool {
    search
        .trim_start_matches('?')
        .split('&')
        .any(|pair| pair == "preview=stopped")
}

/// Why the shell stopped the design's preview (QA-071): the `why` it adds
/// next to `preview=stopped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The design kept opening dialogs (`why=dialogs`).
    Dialogs,
    /// The page did not answer View ▸ Reload and the user chose Reload
    /// (`why=hung`).
    Hung,
    /// The page crashed twice in a row (`why=crashed`).
    Crashed,
}

/// The reason in the page's query ([`StopReason`]). No `why` (or one this
/// version does not know) reads as dialogs, the first reason the shell
/// had.
#[must_use]
pub fn stop_reason(search: &str) -> StopReason {
    let why = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("why="));
    match why {
        Some("hung") => StopReason::Hung,
        Some("crashed") => StopReason::Crashed,
        _ => StopReason::Dialogs,
    }
}

/// Whether `pair` of the query is the shell's mark or its reason.
fn is_stop_mark(pair: &str) -> bool {
    pair == "preview=stopped" || pair.starts_with("why=")
}

/// The page's query without `preview=stopped` and its `why`, for the
/// address once the preview runs again.
#[must_use]
pub fn without_preview_stopped(search: &str) -> String {
    let kept: Vec<&str> = search
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty() && !is_stop_mark(pair))
        .collect();
    if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    }
}

/// The design a route shows (`/projects/:id`, `/present/:id`), if any.
#[must_use]
pub fn design_in_path(path: &str) -> Option<&str> {
    path.strip_prefix("/projects/")
        .or_else(|| path.strip_prefix("/present/"))
        .and_then(|rest| rest.split('/').next())
        .filter(|id| !id.is_empty())
}

/// Whether the route at `path` opens another design than `last`, the one
/// opened before (leaving it for Home and back is the same design).
#[must_use]
pub fn opens_another_design(last: Option<&str>, path: &str) -> bool {
    design_in_path(path).is_some_and(|id| last != Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_design_route_names_its_design() {
        assert_eq!(design_in_path("/projects/p1"), Some("p1"));
        assert_eq!(design_in_path("/projects/p1/files"), Some("p1"));
        assert_eq!(design_in_path("/present/p1"), Some("p1"));
        assert_eq!(design_in_path("/projects"), None);
        assert_eq!(design_in_path("/projects/"), None);
        assert_eq!(design_in_path("/"), None);
        assert_eq!(design_in_path("/settings"), None);
    }

    #[test]
    fn opening_design_b_after_a_resets_the_gate() {
        assert!(opens_another_design(Some("a"), "/projects/b"));
        assert!(opens_another_design(None, "/projects/a"));
        assert!(opens_another_design(Some("a"), "/present/b"));
    }

    #[test]
    fn the_same_design_or_no_design_keeps_the_gate() {
        assert!(!opens_another_design(Some("a"), "/projects/a"));
        assert!(!opens_another_design(Some("a"), "/present/a"));
        assert!(!opens_another_design(Some("a"), "/"));
        assert!(!opens_another_design(None, "/settings"));
    }

    #[test]
    fn the_shell_marks_a_stopped_preview_in_the_query() {
        assert!(preview_was_stopped("?preview=stopped"));
        assert!(preview_was_stopped("?a=1&preview=stopped"));
        assert!(!preview_was_stopped(""));
        assert!(!preview_was_stopped("?preview=running"));
        assert!(!preview_was_stopped("?xpreview=stopped"));
    }

    #[test]
    fn the_shell_says_why_it_stopped_the_preview() {
        // QA-071: a busy loop with no dialog is not "kept opening dialogs".
        assert_eq!(
            stop_reason("?preview=stopped&why=dialogs"),
            StopReason::Dialogs
        );
        assert_eq!(stop_reason("?preview=stopped&why=hung"), StopReason::Hung);
        assert_eq!(
            stop_reason("?a=1&preview=stopped&why=crashed"),
            StopReason::Crashed
        );
        assert_eq!(stop_reason("?preview=stopped"), StopReason::Dialogs);
        assert_eq!(stop_reason("?preview=stopped&why=x"), StopReason::Dialogs);
        assert_eq!(stop_reason("?nowhy=hung"), StopReason::Dialogs);
    }

    #[test]
    fn running_again_drops_only_the_mark() {
        assert_eq!(without_preview_stopped("?preview=stopped"), "");
        assert_eq!(without_preview_stopped("?preview=stopped&why=hung"), "");
        assert_eq!(
            without_preview_stopped("?a=1&preview=stopped&b=2"),
            "?a=1&b=2"
        );
        assert_eq!(
            without_preview_stopped("?a=1&preview=stopped&why=crashed&b=2"),
            "?a=1&b=2"
        );
        assert_eq!(without_preview_stopped(""), "");
    }
}
