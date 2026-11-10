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

use std::collections::{HashMap, VecDeque};
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

/// A message longer than this many characters, or of more lines than
/// [`SCROLL_LINES`], is shown in a scrolling box of fixed height
/// ([`SCROLL_HEIGHT`]): `NSAlert` grows with its text, and 2000 characters
/// made it taller than a 900-point screen, OK under the Dock (QA-066).
const SCROLL_CHARS: usize = 400;

/// See [`SCROLL_CHARS`].
const SCROLL_LINES: usize = 8;

/// Height of the scrolling message box, points: with the title and the
/// buttons the alert stays under 500 points.
#[cfg(target_os = "macos")]
const SCROLL_HEIGHT: f64 = 240.0;

/// Whether `message` (as shown, [`dialog_message`]) goes in the scrolling
/// box rather than the alert's own text.
pub(crate) fn scrolls(message: &str) -> bool {
    message.chars().count() > SCROLL_CHARS || message.lines().count() > SCROLL_LINES
}

/// How soon after a dialog closes the next one of the same webview
/// offers to block the page's dialogs: a page calling `alert` in a loop
/// would otherwise hold the app (an app-modal alert blocks every window
/// and ⌘Q) until it is force-quit.
const LOOP_WINDOW: Duration = Duration::from_secs(3);

/// The checkbox under a dialog shown within [`LOOP_WINDOW`] of the last.
#[cfg(target_os = "macos")]
const BLOCK_LABEL: &str = "Don't allow this design to show more dialogs";

/// How many refused dialogs within [`HANG_WINDOW`] mark a page as hung:
/// once blocked, `alert` returns at once, so a page calling it in a loop
/// never yields and holds the web content process the app's own UI runs
/// in (QA-067). A page that asks a few more times after the block stays
/// well under this.
const HANG_REFUSALS: usize = 50;

/// See [`HANG_REFUSALS`].
const HANG_WINDOW: Duration = Duration::from_secs(1);

/// What to do with a page's next dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Show it; with the [`BLOCK_LABEL`] checkbox when `offer_block`.
    Show { offer_block: bool },
    /// Do not show it: `alert` returns, `confirm` gives `false`, `prompt`
    /// `null`, as if cancelled.
    Block,
    /// As [`Gate::Block`], and the page is hung in a dialog loop
    /// ([`HANG_REFUSALS`]): stop it. Given once per page.
    Hung,
}

/// One webview's dialogs since its page loaded or the user opened another
/// design (either starts afresh, see [`Gates`]).
#[derive(Debug, Default)]
pub(crate) struct DialogGate {
    last_closed: Option<Instant>,
    blocked: bool,
    /// When the latest refusals happened, at most [`HANG_REFUSALS`].
    refused: VecDeque<Instant>,
    /// [`Gate::Hung`] was given.
    hung: bool,
}

impl DialogGate {
    /// The next dialog, asked for at `now`.
    pub(crate) fn open(&mut self, now: Instant) -> Gate {
        if self.blocked {
            return self.refuse(now);
        }
        let offer_block = self
            .last_closed
            .is_some_and(|closed| now.saturating_duration_since(closed) < LOOP_WINDOW);
        Gate::Show { offer_block }
    }

    /// A blocked dialog at `now`: [`Gate::Hung`] the first time the last
    /// [`HANG_REFUSALS`] fall within [`HANG_WINDOW`].
    fn refuse(&mut self, now: Instant) -> Gate {
        if self.hung {
            return Gate::Block;
        }
        if self.refused.len() == HANG_REFUSALS {
            self.refused.pop_front();
        }
        self.refused.push_back(now);
        let looping = self.refused.len() == HANG_REFUSALS
            && self
                .refused
                .front()
                .is_some_and(|first| now.saturating_duration_since(*first) < HANG_WINDOW);
        if looping {
            self.hung = true;
            return Gate::Hung;
        }
        Gate::Block
    }

    /// A dialog closed at `now`; `block` when its checkbox was ticked.
    pub(crate) fn closed(&mut self, now: Instant, block: bool) {
        self.last_closed = Some(now);
        self.blocked |= block;
    }
}

/// Every webview's [`DialogGate`], by a key naming the webview (its
/// `WKWebView *`). No entry is a fresh gate.
#[derive(Debug, Default)]
pub(crate) struct Gates(HashMap<usize, DialogGate>);

impl Gates {
    /// The next dialog of webview `key`, asked for at `now`.
    pub(crate) fn open(&mut self, key: usize, now: Instant) -> Gate {
        self.0
            .get_mut(&key)
            .map_or(Gate::Show { offer_block: false }, |gate| gate.open(now))
    }

    /// A dialog of webview `key` closed at `now`; `block` when its
    /// checkbox was ticked.
    pub(crate) fn closed(&mut self, key: usize, now: Instant, block: bool) {
        self.0.entry(key).or_default().closed(now, block);
    }

    /// Webview `key` started loading a page: its dialogs show again.
    pub(crate) fn page_loaded(&mut self, key: usize) {
        self.0.remove(&key);
    }

    /// The user opened another design in the Studio, or its preview
    /// reloaded (QA-068), which changes the route or a frame, not the page:
    /// every webview's dialogs show again.
    pub(crate) fn reset_all(&mut self) {
        self.0.clear();
    }
}

/// How many times each webview's web content process ended, by a key
/// naming the webview (its `WKWebView *`). Unlike [`Gates`] a new page
/// keeps the count: the shell loads the page again right after a crash,
/// before an open panel has seen it.
#[derive(Debug, Default)]
pub(crate) struct ProcessEnds(HashMap<usize, u64>);

impl ProcessEnds {
    /// The web content process of webview `key` ended.
    pub(crate) fn ended(&mut self, key: usize) {
        let count = self.0.entry(key).or_default();
        *count = count.wrapping_add(1);
    }

    /// How many times the web content process of webview `key` ended.
    pub(crate) fn count(&self, key: usize) -> u64 {
        self.0.get(&key).copied().unwrap_or(0)
    }
}

/// How often, while a page's panel is up, the shell looks whether the
/// page's web content process ended meanwhile.
#[cfg(target_os = "macos")]
const PANEL_POLL_SECS: f64 = 0.25;

/// Whether the shell closes a page's panel now: the web content process
/// that asked ended since it opened (`opened`, `now`: [`ProcessEnds::count`]
/// then and now), so nobody waits for the answer; and the panel is the
/// modal window on top (`ours_on_top`), so another dialog above it is
/// left alone and the panel closes once that one has.
pub(crate) fn closes_panel(opened: u64, now: u64, ours_on_top: bool) -> bool {
    opened != now && ours_on_top
}

#[cfg(target_os = "macos")]
pub(crate) use mac::{install, page_loaded, process_ended, reset_all};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use super::{
        BLOCK_LABEL, Gate, Gates, PANEL_POLL_SECS, ProcessEnds, SCROLL_HEIGHT, closes_panel,
        dialog_message, dialog_title, scrolls,
    };
    use block2::{Block, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, ProtocolObject, Sel};
    use objc2::{MainThreadMarker, MainThreadOnly, sel};
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSApplication, NSControlStateValueOn, NSFont,
        NSModalPanelRunLoopMode, NSTextField, NSTextView, NSView,
    };
    use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSTimer};
    use objc2_web_kit::{WKFrameInfo, WKUIDelegate, WKWebView};
    use std::cell::RefCell;
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
    /// `on_hung` runs when a page is hung in a dialog loop ([`Gate::Hung`]),
    /// inside WebKit's call: it should only schedule the work. Call it on
    /// the main thread.
    pub(crate) fn install(webview: *mut c_void, on_hung: impl Fn() + 'static) {
        if webview.is_null() {
            return;
        }
        ON_HUNG.with(|slot| *slot.borrow_mut() = Some(Box::new(on_hung)));
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
        /// Each webview's gate, by its `WKWebView *`. The panel methods,
        /// [`page_loaded`] and [`reset_all`] all run on the main thread.
        static GATES: RefCell<Gates> = RefCell::new(Gates::default());
        /// What [`install`] was given to run when a page is hung.
        static ON_HUNG: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
        /// Each webview's ended web content processes, by its
        /// `WKWebView *`: [`process_ended`] counts, an open panel looks.
        static ENDS: RefCell<ProcessEnds> = RefCell::new(ProcessEnds::default());
    }

    /// The web content process of `webview` (a `WKWebView *`) ended: a
    /// panel it opened closes, as if cancelled, nobody being left to read
    /// the answer. Call it on the main thread; WebKit's own call is fine,
    /// it only counts.
    pub(crate) fn process_ended(webview: *mut c_void) {
        ENDS.with(|ends| ends.borrow_mut().ended(webview as usize));
    }

    /// `webview` (a `WKWebView *`) started loading a page: dialogs it had
    /// blocked may show again.
    pub(crate) fn page_loaded(webview: *mut c_void) {
        GATES.with(|gates| gates.borrow_mut().page_loaded(webview as usize));
    }

    /// The user opened another design: dialogs blocked for the last one
    /// may show again. Call it on the main thread.
    pub(crate) fn reset_all() {
        GATES.with(|gates| gates.borrow_mut().reset_all());
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
        let offer_block = match GATES.with(|gates| gates.borrow_mut().open(key, now)) {
            Gate::Block => return false,
            Gate::Hung => {
                tracing::warn!("a page is hung in a dialog loop: stopping it");
                ON_HUNG.with(|slot| {
                    if let Some(on_hung) = slot.borrow().as_ref() {
                        on_hung();
                    }
                });
                return false;
            }
            Gate::Show { offer_block } => offer_block,
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
        let message = dialog_message(&message.to_string());
        let scroll = scrolls(&message).then(|| message_box(mtm, &message));
        if scroll.is_none() {
            alert.setInformativeText(&NSString::from_str(&message));
        }
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        if cancel {
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        }
        match (scroll, field) {
            (None, None) => {}
            (Some(scroll), None) => alert.setAccessoryView(Some(&scroll)),
            (None, Some(field)) => alert.setAccessoryView(Some(field)),
            (Some(scroll), Some(field)) => {
                // The message above the field, both in one view.
                let width = scroll.frame().size.width;
                let field_height = field.frame().size.height;
                let gap = 8.0;
                let both = NSView::initWithFrame(
                    NSView::alloc(mtm),
                    NSRect::new(
                        NSPoint::new(0.0, 0.0),
                        NSSize::new(width, SCROLL_HEIGHT + gap + field_height),
                    ),
                );
                field.setFrame(NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(width, field_height),
                ));
                scroll.setFrameOrigin(NSPoint::new(0.0, field_height + gap));
                both.addSubview(field);
                both.addSubview(&scroll);
                alert.setAccessoryView(Some(&both));
            }
        }
        if let Some(field) = field {
            alert.window().setInitialFirstResponder(Some(field));
        }
        if offer_block {
            alert.setShowsSuppressionButton(true);
            if let Some(button) = alert.suppressionButton() {
                button.setTitle(&NSString::from_str(BLOCK_LABEL));
            }
        }
        // The page's web content process may end while the panel is up (a
        // crash): WebKit says so inside `runModal`, and the panel closes
        // (`NSModalResponseAbort`, not OK) so the window is not held by a
        // question nobody waits for.
        let opened = ENDS.with(|ends| ends.borrow().count(key));
        let window = alert.window();
        let watch = RcBlock::new(move |_timer| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let app = NSApplication::sharedApplication(mtm);
            let ours_on_top = app.modalWindow().is_some_and(|top| top == window);
            let now = ENDS.with(|ends| ends.borrow().count(key));
            if closes_panel(opened, now, ours_on_top) {
                tracing::info!("the page's web content process ended: closing its dialog");
                // From a timer in the modal run loop mode: `abortModal`,
                // not `stopModal`, which waits for the next event.
                app.abortModal();
            }
        });
        // SAFETY: the timer is added to this (the main) thread's run loop
        // only, so the block never runs on another thread; invalidated
        // below, before the alert and what the block holds go.
        let timer = unsafe {
            let timer = NSTimer::timerWithTimeInterval_repeats_block(PANEL_POLL_SECS, true, &watch);
            NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSModalPanelRunLoopMode);
            timer
        };
        // Not borrowing `GATES` or `ENDS` here: `runModal` spins the run
        // loop.
        let ok = alert.runModal() == NSAlertFirstButtonReturn;
        timer.invalidate();
        let block = offer_block
            && alert
                .suppressionButton()
                .is_some_and(|button| button.state() == NSControlStateValueOn);
        GATES.with(|gates| gates.borrow_mut().closed(key, Instant::now(), block));
        ok
    }

    /// Width of the scrolling message box, points.
    const SCROLL_WIDTH: f64 = 360.0;

    /// `message` in a read-only text view that scrolls, [`SCROLL_HEIGHT`]
    /// tall, for the alert's accessory view: the alert keeps its height
    /// and its buttons on screen however long the message (QA-066).
    fn message_box(mtm: MainThreadMarker, message: &str) -> Retained<NSView> {
        let scroll = NSTextView::scrollableTextView(mtm);
        scroll.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(SCROLL_WIDTH, SCROLL_HEIGHT),
        ));
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        if let Some(text) = scroll
            .documentView()
            .and_then(|view| view.downcast::<NSTextView>().ok())
        {
            text.setEditable(false);
            text.setSelectable(true);
            text.setDrawsBackground(false);
            text.setFont(Some(&NSFont::systemFontOfSize(
                NSFont::smallSystemFontSize(),
            )));
            text.setString(&NSString::from_str(message));
        }
        Retained::into_super(scroll)
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
        let mut gate = DialogGate::default();
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

    const A: usize = 0xA0;
    const B: usize = 0xB0;

    /// Webviews `A` and `B` with `A`'s dialogs blocked, `B` mid-loop.
    fn a_blocked_b_looping(t0: Instant) -> Gates {
        let mut gates = Gates::default();
        gates.closed(A, t0, true);
        gates.closed(B, t0, false);
        assert_eq!(gates.open(A, t0), Gate::Block);
        gates
    }

    #[test]
    fn an_unknown_webview_shows_without_the_checkbox() {
        let mut gates = Gates::default();
        assert_eq!(
            gates.open(A, Instant::now()),
            Gate::Show { offer_block: false }
        );
    }

    #[test]
    fn a_new_page_shows_dialogs_again() {
        let t0 = Instant::now();
        let mut gates = a_blocked_b_looping(t0);
        gates.page_loaded(A);
        assert_eq!(gates.open(A, t0), Gate::Show { offer_block: false });
        // The other webview's state stays.
        assert_eq!(gates.open(B, t0), Gate::Show { offer_block: true });
    }

    #[test]
    fn a_new_page_leaves_other_webviews_blocked() {
        let t0 = Instant::now();
        let mut gates = a_blocked_b_looping(t0);
        gates.page_loaded(B);
        assert_eq!(gates.open(A, t0), Gate::Block);
        assert_eq!(gates.open(B, t0), Gate::Show { offer_block: false });
    }

    #[test]
    fn another_design_shows_dialogs_again_everywhere() {
        let t0 = Instant::now();
        let mut gates = a_blocked_b_looping(t0);
        gates.reset_all();
        assert_eq!(gates.open(A, t0), Gate::Show { offer_block: false });
        assert_eq!(gates.open(B, t0), Gate::Show { offer_block: false });
    }

    #[test]
    fn a_short_message_stays_in_the_alert_text() {
        assert!(!scrolls("Delete?"));
        assert!(!scrolls(&"x".repeat(SCROLL_CHARS)));
        assert!(!scrolls(&"line\n".repeat(SCROLL_LINES)));
    }

    #[test]
    fn a_long_or_tall_message_scrolls() {
        // QA-066: 5000 characters cut to 2000 still grew the alert past
        // a 900-point screen.
        assert!(scrolls(&dialog_message(&"x".repeat(5000))));
        assert!(scrolls(&"x".repeat(SCROLL_CHARS + 1)));
        assert!(scrolls(&"line\n".repeat(SCROLL_LINES + 1)));
    }

    /// A gate whose dialogs the user blocked at `t0`.
    fn blocked(t0: Instant) -> DialogGate {
        let mut gate = DialogGate::default();
        gate.closed(t0, true);
        gate
    }

    #[test]
    fn a_page_refused_fifty_times_in_a_second_is_hung() {
        // QA-067: `for (;;) alert('x')`, once blocked, spins the web
        // content process the app's UI shares.
        let t0 = Instant::now();
        let mut gate = blocked(t0);
        let step = Duration::from_millis(10);
        for i in 0..HANG_REFUSALS - 1 {
            let at = t0 + step * u32::try_from(i).unwrap();
            assert_eq!(gate.open(at), Gate::Block, "refusal {i}");
        }
        let last = t0 + step * u32::try_from(HANG_REFUSALS - 1).unwrap();
        assert_eq!(gate.open(last), Gate::Hung);
        // Said once: the next refusals only block.
        assert_eq!(gate.open(last), Gate::Block);
        assert_eq!(gate.open(last + HANG_WINDOW * 5), Gate::Block);
    }

    #[test]
    fn refusals_spread_over_more_than_a_second_are_not_a_hang() {
        let t0 = Instant::now();
        let mut gate = blocked(t0);
        // 50 refusals 21 ms apart span 1.029 s; 200 never bunch up.
        let step = Duration::from_millis(21);
        for i in 0..200 {
            let at = t0 + step * i;
            assert_eq!(gate.open(at), Gate::Block, "refusal {i}");
        }
    }

    #[test]
    fn a_burst_after_slow_refusals_is_a_hang() {
        // The window slides: old refusals drop out, a later burst counts.
        let t0 = Instant::now();
        let mut gate = blocked(t0);
        for i in 0..10 {
            assert_eq!(gate.open(t0 + Duration::from_secs(i)), Gate::Block);
        }
        let burst = t0 + Duration::from_secs(20);
        for _ in 0..HANG_REFUSALS - 1 {
            assert_eq!(gate.open(burst), Gate::Block);
        }
        assert_eq!(gate.open(burst), Gate::Hung);
    }

    #[test]
    fn dialogs_shown_are_not_counted() {
        // Before the block the user answers each dialog: no hang.
        let t0 = Instant::now();
        let mut gate = DialogGate::default();
        for _ in 0..HANG_REFUSALS * 2 {
            assert!(matches!(gate.open(t0), Gate::Show { .. }));
            gate.closed(t0, false);
        }
    }

    #[test]
    fn each_webview_counts_its_own_refusals() {
        let t0 = Instant::now();
        let mut gates = Gates::default();
        gates.closed(A, t0, true);
        gates.closed(B, t0, true);
        for _ in 0..HANG_REFUSALS - 1 {
            assert_eq!(gates.open(A, t0), Gate::Block);
            assert_eq!(gates.open(B, t0), Gate::Block);
        }
        assert_eq!(gates.open(A, t0), Gate::Hung);
        assert_eq!(gates.open(B, t0), Gate::Hung);
    }

    /// Webview `A` blocked and refused until [`Gate::Hung`].
    fn a_hung(t0: Instant) -> Gates {
        let mut gates = Gates::default();
        gates.closed(A, t0, true);
        for _ in 0..HANG_REFUSALS - 1 {
            gates.open(A, t0);
        }
        assert_eq!(gates.open(A, t0), Gate::Hung);
        gates
    }

    #[test]
    fn the_reloaded_page_starts_a_fresh_count() {
        // After the shell loads the app again (`page_loaded`), a page
        // that blocks and loops again is stopped again.
        let t0 = Instant::now();
        let mut gates = a_hung(t0);
        gates.page_loaded(A);
        assert_eq!(gates.open(A, t0), Gate::Show { offer_block: false });
        gates.closed(A, t0, true);
        for _ in 0..HANG_REFUSALS - 1 {
            assert_eq!(gates.open(A, t0), Gate::Block);
        }
        assert_eq!(gates.open(A, t0), Gate::Hung);
    }

    #[test]
    fn a_panel_closes_once_its_process_ended_and_it_is_on_top() {
        // The page's process ended since the panel opened: closes, unless
        // another dialog is on top (then once that one has closed).
        assert!(closes_panel(0, 1, true));
        assert!(!closes_panel(0, 1, false));
        // Still the process that asked: stays up for the user.
        assert!(!closes_panel(0, 0, true));
        assert!(!closes_panel(2, 2, true));
        assert!(!closes_panel(2, 2, false));
    }

    #[test]
    fn each_webview_counts_its_own_process_ends() {
        let mut ends = ProcessEnds::default();
        assert_eq!(ends.count(A), 0);
        let opened_a = ends.count(A);
        let opened_b = ends.count(B);
        ends.ended(A);
        // A's panel closes; B's, another webview's, stays.
        assert!(closes_panel(opened_a, ends.count(A), true));
        assert!(!closes_panel(opened_b, ends.count(B), true));
        ends.ended(A);
        assert_eq!(ends.count(A), 2);
        assert_eq!(ends.count(B), 0);
    }

    #[test]
    fn a_new_page_keeps_the_process_end_seen() {
        // After a crash the shell loads the page again (`page_loaded`)
        // before the panel's timer looks: the end still counts.
        let mut gates = Gates::default();
        let mut ends = ProcessEnds::default();
        let opened = ends.count(A);
        ends.ended(A);
        gates.page_loaded(A);
        gates.reset_all();
        assert!(closes_panel(opened, ends.count(A), true));
    }

    #[test]
    fn a_preview_reload_resets_the_gate() {
        // QA-068: the Studio's reload calls `reset_dialog_gate`, which
        // lifts the block and the count.
        let t0 = Instant::now();
        let mut gates = a_hung(t0);
        gates.reset_all();
        assert_eq!(gates.open(A, t0), Gate::Show { offer_block: false });
        let mut gates = a_blocked_b_looping(t0);
        gates.reset_all();
        assert_eq!(gates.open(A, t0), Gate::Show { offer_block: false });
    }
}
