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

use std::time::{Duration, Instant};

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

/// How soon after a dialog closes the next one of the same webview
/// offers to block the page's dialogs: a page calling `alert` in a loop
/// would otherwise hold the app (an app-modal alert blocks every window
/// and ⌘Q) until it is force-quit.
const LOOP_WINDOW: Duration = Duration::from_secs(3);

/// The checkbox under a dialog shown within [`LOOP_WINDOW`] of the last.
#[cfg(target_os = "macos")]
const BLOCK_LABEL: &str = "Don't allow this page to show more dialogs";

/// What to do with a page's next dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Show it; with the [`BLOCK_LABEL`] checkbox when `offer_block`.
    Show { offer_block: bool },
    /// Do not show it: `alert` returns, `confirm` gives `false`, `prompt`
    /// `null`, as if cancelled.
    Block,
}

/// One webview's dialogs since its page loaded (a new page starts afresh).
#[derive(Debug, Default)]
pub(crate) struct DialogGate {
    last_closed: Option<Instant>,
    blocked: bool,
}

impl DialogGate {
    /// The next dialog, asked for at `now`.
    pub(crate) fn open(&self, now: Instant) -> Gate {
        if self.blocked {
            return Gate::Block;
        }
        let offer_block = self
            .last_closed
            .is_some_and(|closed| now.saturating_duration_since(closed) < LOOP_WINDOW);
        Gate::Show { offer_block }
    }

    /// A dialog closed at `now`; `block` when its checkbox was ticked.
    pub(crate) fn closed(&mut self, now: Instant, block: bool) {
        self.last_closed = Some(now);
        self.blocked |= block;
    }
}

#[cfg(target_os = "macos")]
pub(crate) use mac::{install, page_loaded};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use super::{BLOCK_LABEL, DialogGate, Gate, dialog_message, dialog_title};
    use block2::Block;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, ProtocolObject, Sel};
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSControlStateValueOn, NSTextField};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use objc2_web_kit::{WKFrameInfo, WKUIDelegate, WKWebView};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::{CStr, c_void};
    use std::time::Instant;

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

    thread_local! {
        /// Each webview's [`DialogGate`], by its `WKWebView *`. The panel
        /// methods and [`page_loaded`] all run on the main thread.
        static GATES: RefCell<HashMap<usize, DialogGate>> = RefCell::new(HashMap::new());
    }

    /// `webview` (a `WKWebView *`) started loading a page: dialogs it had
    /// blocked may show again.
    pub(crate) fn page_loaded(webview: *mut c_void) {
        GATES.with(|gates| gates.borrow_mut().remove(&(webview as usize)));
    }

    /// Runs a panel from `frame` of `webview`: its origin as title,
    /// `message` below, OK first (Return), Cancel too when `cancel`
    /// (Escape), `field` under the message. `true` when OK closed it;
    /// `false` at once, nothing shown, when the user blocked this page's
    /// dialogs ([`DialogGate`]).
    fn run_panel(
        mtm: MainThreadMarker,
        webview: &WKWebView,
        frame: &WKFrameInfo,
        message: &NSString,
        cancel: bool,
        field: Option<&NSTextField>,
    ) -> bool {
        let key = std::ptr::from_ref(webview) as usize;
        let now = Instant::now();
        let offer_block =
            match GATES.with(|gates| gates.borrow().get(&key).map(|gate| gate.open(now))) {
                Some(Gate::Block) => return false,
                Some(Gate::Show { offer_block }) => offer_block,
                None => false,
            };
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
        if let Some(field) = field {
            alert.setAccessoryView(Some(field));
            alert.window().setInitialFirstResponder(Some(field));
        }
        if offer_block {
            alert.setShowsSuppressionButton(true);
            if let Some(button) = alert.suppressionButton() {
                button.setTitle(&NSString::from_str(BLOCK_LABEL));
            }
        }
        // Not borrowing `GATES` here: `runModal` spins the run loop.
        let ok = alert.runModal() == NSAlertFirstButtonReturn;
        let block = offer_block
            && alert
                .suppressionButton()
                .is_some_and(|button| button.state() == NSControlStateValueOn);
        GATES.with(|gates| {
            gates
                .borrow_mut()
                .entry(key)
                .or_default()
                .closed(Instant::now(), block);
        });
        ok
    }

    extern "C-unwind" fn run_alert(
        _this: &AnyObject,
        _cmd: Sel,
        webview: &WKWebView,
        message: &NSString,
        frame: &WKFrameInfo,
        done: &Block<dyn Fn()>,
    ) {
        if let Some(mtm) = MainThreadMarker::new() {
            run_panel(mtm, webview, frame, message, false, None);
        }
        done.call(());
    }

    extern "C-unwind" fn run_confirm(
        _this: &AnyObject,
        _cmd: Sel,
        webview: &WKWebView,
        message: &NSString,
        frame: &WKFrameInfo,
        done: &Block<dyn Fn(Bool)>,
    ) {
        let ok = MainThreadMarker::new()
            .is_some_and(|mtm| run_panel(mtm, webview, frame, message, true, None));
        done.call((Bool::new(ok),));
    }

    extern "C-unwind" fn run_prompt(
        _this: &AnyObject,
        _cmd: Sel,
        webview: &WKWebView,
        prompt: &NSString,
        default_text: Option<&NSString>,
        frame: &WKFrameInfo,
        done: &Block<dyn Fn(*mut NSString)>,
    ) {
        let answer: Option<Retained<NSString>> = MainThreadMarker::new().and_then(|mtm| {
            let empty = NSString::new();
            let field = NSTextField::textFieldWithString(default_text.unwrap_or(&empty), mtm);
            field.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(300.0, 24.0),
            ));
            run_panel(mtm, webview, frame, prompt, true, Some(&field)).then(|| field.stringValue())
        });
        // `null` on Cancel. `answer` owns the string and lives until after
        // the call, which copies it.
        let ptr = answer
            .as_deref()
            .map_or(std::ptr::null_mut(), |s| std::ptr::from_ref(s).cast_mut());
        done.call((ptr,));
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

    #[test]
    fn the_first_dialog_shows_without_the_checkbox() {
        let gate = DialogGate::default();
        assert_eq!(gate.open(Instant::now()), Gate::Show { offer_block: false });
    }

    #[test]
    fn a_dialog_soon_after_the_last_offers_to_block() {
        let t0 = Instant::now();
        let mut gate = DialogGate::default();
        gate.closed(t0, false);
        assert_eq!(gate.open(t0), Gate::Show { offer_block: true });
        let almost = t0 + LOOP_WINDOW - Duration::from_millis(1);
        assert_eq!(gate.open(almost), Gate::Show { offer_block: true });
    }

    #[test]
    fn a_dialog_long_after_the_last_does_not_offer_to_block() {
        let t0 = Instant::now();
        let mut gate = DialogGate::default();
        gate.closed(t0, false);
        assert_eq!(
            gate.open(t0 + LOOP_WINDOW),
            Gate::Show { offer_block: false }
        );
    }

    #[test]
    fn ticking_the_checkbox_blocks_every_later_dialog() {
        let t0 = Instant::now();
        let mut gate = DialogGate::default();
        gate.closed(t0, true);
        assert_eq!(gate.open(t0), Gate::Block);
        assert_eq!(gate.open(t0 + Duration::from_secs(3600)), Gate::Block);
        // A later close without the box (none is shown) keeps the block.
        gate.closed(t0, false);
        assert_eq!(gate.open(t0), Gate::Block);
    }

    #[test]
    fn a_new_page_starts_with_a_fresh_gate() {
        // `page_loaded` drops the webview's gate; the next one is default.
        let mut gate = DialogGate::default();
        gate.closed(Instant::now(), true);
        gate = DialogGate::default();
        assert_eq!(gate.open(Instant::now()), Gate::Show { offer_block: false });
    }
}
