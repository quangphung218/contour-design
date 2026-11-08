//! A page's `alert`, `confirm` and `prompt` in the macOS window (QA-064).
//!
//! WKWebView shows no JavaScript panel by itself: it asks its UI delegate,
//! and wry's delegate (`WryWebViewUIDelegate`, wry 0.55) has none of the
//! three methods, so `alert` returned at once, `confirm` gave `false` and
//! `prompt` `null`. [`install`] adds them to that delegate's class at
//! startup and shows an `NSAlert`, titled with the asking frame's origin.
//!
//! WebView2 (Windows) and WebKitGTK (Linux) draw these dialogs themselves
//! unless the app handles `ScriptDialogOpening` / `script-dialog`, which
//! neither wry nor Tauri does: nothing to add there.

/// The dialog's title: who is asking, as browsers put it
/// (`127.0.0.1:5173 says`). `port` 0 or the scheme's default is left out.
/// A sandboxed frame (a design in the Studio) has an opaque origin, no
/// host: "This page says".
pub(crate) fn dialog_title(scheme: &str, host: &str, port: i64) -> String {
    if host.is_empty() {
        return "This page says".to_owned();
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let default_port = match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        _ => 0,
    };
    if port <= 0 || port == default_port {
        format!("{host} says")
    } else {
        format!("{host}:{port} says")
    }
}

/// Longest message shown, in characters: a page's huge string would grow
/// the alert past the screen, its buttons out of reach.
const MAX_MESSAGE_CHARS: usize = 2000;

/// The page's message as shown: cut at [`MAX_MESSAGE_CHARS`], with `…`.
pub(crate) fn dialog_message(message: &str) -> String {
    match message.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((cut, _)) => format!("{}…", &message[..cut]),
        None => message.to_owned(),
    }
}

#[cfg(target_os = "macos")]
pub(crate) use mac::install;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use super::{dialog_message, dialog_title};
    use block2::Block;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, ProtocolObject, Sel};
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSTextField};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use objc2_web_kit::{WKFrameInfo, WKUIDelegate, WKWebView};
    use std::ffi::{CStr, c_void};

    /// `webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:`
    type AlertImp = extern "C-unwind" fn(
        &AnyObject,
        Sel,
        &WKWebView,
        &NSString,
        &WKFrameInfo,
        &Block<dyn Fn()>,
    );
    /// `webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:`
    type ConfirmImp = extern "C-unwind" fn(
        &AnyObject,
        Sel,
        &WKWebView,
        &NSString,
        &WKFrameInfo,
        &Block<dyn Fn(Bool)>,
    );
    /// `webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:`
    type PromptImp = extern "C-unwind" fn(
        &AnyObject,
        Sel,
        &WKWebView,
        &NSString,
        Option<&NSString>,
        &WKFrameInfo,
        &Block<dyn Fn(*mut NSString)>,
    );

    /// Gives the UI delegate of `webview` (a `WKWebView *`, from Tauri's
    /// `with_webview`) the three JavaScript panel methods, then sets it
    /// again: WebKit reads which methods a delegate has when it is set.
    /// Call it on the main thread.
    pub(crate) fn install(webview: *mut c_void) {
        if webview.is_null() {
            return;
        }
        // SAFETY: on macOS Tauri's `PlatformWebview::inner` is the window's
        // live `WKWebView`, and `with_webview` runs this on the main thread
        // while the webview exists.
        let webview: &WKWebView = unsafe { &*webview.cast::<WKWebView>() };
        // SAFETY: a plain getter, on the main thread.
        let Some(delegate) = (unsafe { webview.UIDelegate() }) else {
            tracing::warn!("the webview has no UI delegate: no JavaScript dialogs");
            return;
        };
        let object: &AnyObject = (*delegate).as_ref();
        let class = object.class();
        let alert: AlertImp = run_alert;
        let confirm: ConfirmImp = run_confirm;
        let prompt: PromptImp = run_prompt;
        // SAFETY: each `Imp` has the arguments its selector's encoding
        // names (`@` objects, `?` the block, `v` no result), the shape
        // WebKit calls (`WKUIDelegate.h`).
        unsafe {
            add_method(
                class,
                sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:),
                std::mem::transmute::<AlertImp, Imp>(alert),
                c"v@:@@@@?",
            );
            add_method(
                class,
                sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:),
                std::mem::transmute::<ConfirmImp, Imp>(confirm),
                c"v@:@@@@?",
            );
            add_method(
                class,
                sel!(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:),
                std::mem::transmute::<PromptImp, Imp>(prompt),
                c"v@:@@@@@?",
            );
        }
        let delegate: &ProtocolObject<dyn WKUIDelegate> = &delegate;
        // SAFETY: the same delegate back; wry keeps it alive with the
        // webview (the property is weak).
        unsafe { webview.setUIDelegate(Some(delegate)) };
    }

    /// Adds `imp` as `sel` to `class`. Already there (a wry that has its
    /// own) it stays: `class_addMethod` never replaces.
    ///
    /// # Safety
    /// `imp` must take the arguments `types` describes.
    unsafe fn add_method(class: &AnyClass, sel: Sel, imp: Imp, types: &CStr) {
        let class = std::ptr::from_ref(class).cast_mut();
        // SAFETY: a registered class, a valid selector; the caller vouches
        // for `imp` and `types`.
        let added = unsafe { objc2::ffi::class_addMethod(class, sel, imp, types.as_ptr()) };
        if !added.as_bool() {
            tracing::info!(selector = %sel, "the UI delegate already has this method");
        }
    }

    /// The alert for a panel from `frame`: its origin as title, `message`
    /// below, OK first (Return); Cancel too when `cancel` (Escape).
    fn alert(
        mtm: MainThreadMarker,
        frame: &WKFrameInfo,
        message: &NSString,
        cancel: bool,
    ) -> Retained<NSAlert> {
        // SAFETY: getters of a live `WKFrameInfo`, on the main thread.
        let title = unsafe {
            let origin = frame.securityOrigin();
            dialog_title(
                &origin.protocol().to_string(),
                &origin.host().to_string(),
                i64::try_from(origin.port()).unwrap_or(0),
            )
        };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&dialog_message(&message.to_string())));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        if cancel {
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        }
        alert
    }

    extern "C-unwind" fn run_alert(
        _this: &AnyObject,
        _cmd: Sel,
        _webview: &WKWebView,
        message: &NSString,
        frame: &WKFrameInfo,
        done: &Block<dyn Fn()>,
    ) {
        if let Some(mtm) = MainThreadMarker::new() {
            alert(mtm, frame, message, false).runModal();
        }
        done.call(());
    }

    extern "C-unwind" fn run_confirm(
        _this: &AnyObject,
        _cmd: Sel,
        _webview: &WKWebView,
        message: &NSString,
        frame: &WKFrameInfo,
        done: &Block<dyn Fn(Bool)>,
    ) {
        let ok = MainThreadMarker::new().is_some_and(|mtm| {
            alert(mtm, frame, message, true).runModal() == NSAlertFirstButtonReturn
        });
        done.call((Bool::new(ok),));
    }

    extern "C-unwind" fn run_prompt(
        _this: &AnyObject,
        _cmd: Sel,
        _webview: &WKWebView,
        prompt: &NSString,
        default_text: Option<&NSString>,
        frame: &WKFrameInfo,
        done: &Block<dyn Fn(*mut NSString)>,
    ) {
        let answer = MainThreadMarker::new().and_then(|mtm| {
            let alert = alert(mtm, frame, prompt, true);
            let empty = NSString::new();
            let field = NSTextField::textFieldWithString(default_text.unwrap_or(&empty), mtm);
            field.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(300.0, 24.0),
            ));
            alert.setAccessoryView(Some(&field));
            alert.window().setInitialFirstResponder(Some(&field));
            (alert.runModal() == NSAlertFirstButtonReturn).then(|| field.stringValue())
        });
        // `prompt` returns `null` on Cancel; WebKit copies the string
        // before `answer` drops.
        let answer = answer.map_or(std::ptr::null_mut(), |s| Retained::as_ptr(&s).cast_mut());
        done.call((answer,));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_names_host_and_a_non_default_port() {
        assert_eq!(
            dialog_title("http", "127.0.0.1", 5173),
            "127.0.0.1:5173 says"
        );
        assert_eq!(dialog_title("http", "example.com", 80), "example.com says");
        assert_eq!(
            dialog_title("https", "example.com", 443),
            "example.com says"
        );
        assert_eq!(
            dialog_title("https", "example.com", 8443),
            "example.com:8443 says"
        );
        assert_eq!(dialog_title("tauri", "localhost", 0), "localhost says");
    }

    #[test]
    fn an_ipv6_host_is_bracketed() {
        assert_eq!(dialog_title("http", "::1", 8080), "[::1]:8080 says");
        assert_eq!(dialog_title("http", "[::1]", 8080), "[::1]:8080 says");
    }

    #[test]
    fn an_opaque_origin_is_this_page() {
        // A sandboxed `srcdoc` frame (the Studio's design) has no host.
        assert_eq!(dialog_title("", "", 0), "This page says");
        assert_eq!(dialog_title("file", "", 0), "This page says");
    }

    #[test]
    fn a_short_message_is_shown_whole() {
        assert_eq!(dialog_message("Open?"), "Open?");
        assert_eq!(dialog_message(""), "");
        let exact = "é".repeat(MAX_MESSAGE_CHARS);
        assert_eq!(dialog_message(&exact), exact);
    }

    #[test]
    fn a_long_message_is_cut_on_a_char_boundary() {
        let long = "é".repeat(MAX_MESSAGE_CHARS + 5);
        let shown = dialog_message(&long);
        assert_eq!(shown.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(shown.ends_with('…'));
    }
}
