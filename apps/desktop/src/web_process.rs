//! The window's web content process when a page hangs it (QA-067).
//!
//! A design's sandboxed frame runs on the same WebKit main thread as the
//! app's UI (WKWebView has no out-of-process iframes), so a page that loops
//! without yielding holds both. The app's own process stays free: from
//! here it can end the web content process, and WebKit's public
//! `webViewWebContentProcessDidTerminate:` (Tauri's
//! `on_web_content_process_terminate`) then loads the app again.
//!
//! WKWebView also sends key equivalents to the web content process before
//! the menu, so ⌘Q and ⌘R waited on a hung page for ever: [`menu_keys_first`]
//! gives them to the menu first.

/// Whether a key press with `key` (characters ignoring modifiers) and
/// only ⌘ held (`command_only`) goes to the menu before the page: Quit and
/// View ▸ Reload, the ways out of a hung page.
pub(crate) fn menu_first(command_only: bool, key: &str) -> bool {
    command_only && matches!(key, "q" | "r")
}

// The user's answer to "This page isn't responding" (`ask_not_responding`).
pub(crate) use crate::shell_state::NotRespondingChoice;

/// `NSAlertFirstButtonReturn`: Wait.
const WAIT_RESPONSE: isize = 1000;

/// `NSAlertSecondButtonReturn`: Reload.
const RELOAD_RESPONSE: isize = 1001;

/// The choice `runModal` gave as `response`. Anything but the two buttons
/// (`NSModalResponseAbort`, when [`closes_ask`] closed it) is no choice:
/// a Reload the user did not press never ends the page.
fn choice_of(response: isize) -> NotRespondingChoice {
    match response {
        WAIT_RESPONSE => NotRespondingChoice::Wait,
        RELOAD_RESPONSE => NotRespondingChoice::Reload,
        _ => NotRespondingChoice::Closed,
    }
}

/// How often, while "This page isn't responding" is up, the shell looks
/// whether the page answered the question meanwhile.
#[cfg(target_os = "macos")]
const ASK_POLL_SECS: f64 = 0.25;

/// Whether the shell closes "This page isn't responding" now: the question
/// it asks is no longer open (`still_open`; the page answered, loaded
/// again or a newer Reload replaced it), and the alert is the modal window
/// on top (`ours_on_top`), so a page's own dialog above it is left alone.
fn closes_ask(still_open: bool, ours_on_top: bool) -> bool {
    !still_open && ours_on_top
}

#[cfg(target_os = "macos")]
pub(crate) use mac::{ask_not_responding, bundle_id, kill, menu_keys_first, page_url};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use super::{
        ASK_POLL_SECS, NotRespondingChoice, RELOAD_RESPONSE, WAIT_RESPONSE, choice_of, closes_ask,
        menu_first,
    };
    use block2::RcBlock;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{MainThreadMarker, msg_send, sel};
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication, NSEvent,
        NSEventMask, NSEventModifierFlags, NSModalPanelRunLoopMode,
    };
    use objc2_foundation::{NSBundle, NSRunLoop, NSString, NSTimer};
    use objc2_web_kit::WKWebView;
    use std::ffi::c_void;
    use std::ptr::NonNull;

    /// The running app's bundle id (`CFBundleIdentifier`): a QA copy's own
    /// (`….qa`), not the one built in.
    pub(crate) fn bundle_id() -> Option<String> {
        NSBundle::mainBundle()
            .bundleIdentifier()
            .map(|id| id.to_string())
    }

    /// Ends the web content process of `webview` (a `WKWebView *`, from
    /// Tauri's `with_webview`), on the main thread. `false` when this
    /// WebKit has no way to: WebKit only offers it as SPI, checked first.
    pub(crate) fn kill(webview: *mut c_void) -> bool {
        if webview.is_null() {
            return false;
        }
        // SAFETY: on macOS Tauri's `PlatformWebview::inner` is the window's
        // live `WKWebView`, and `with_webview` runs this on the main thread.
        let webview: &WKWebView = unsafe { &*webview.cast::<WKWebView>() };
        // SAFETY (both calls): the object answers the selector, a method
        // with no argument and no result in every WebKit that has it
        // (`WKWebViewPrivate.h`, `WKWebViewPrivateForTesting.h`).
        if webview.respondsToSelector(sel!(_killWebContentProcessAndResetState)) {
            unsafe {
                let _: () = msg_send![webview, _killWebContentProcessAndResetState];
            }
        } else if webview.respondsToSelector(sel!(_killWebContentProcess)) {
            unsafe {
                let _: () = msg_send![webview, _killWebContentProcess];
            }
        } else {
            tracing::warn!("this WebKit cannot end the web content process");
            return false;
        }
        true
    }

    /// The address `webview` (a `WKWebView *`, from Tauri's
    /// `with_webview`) shows, on the main thread. `None` once its web
    /// content process ended, when WebKit's `URL` is nil: wry's own `url()`
    /// unwraps that and panics the app (QA-073).
    pub(crate) fn page_url(webview: *mut c_void) -> Option<String> {
        if webview.is_null() {
            return None;
        }
        // SAFETY: on macOS Tauri's `PlatformWebview::inner` is the window's
        // live `WKWebView`, and `with_webview` runs this on the main thread.
        let webview: &WKWebView = unsafe { &*webview.cast::<WKWebView>() };
        // SAFETY: `URL` is a property of every `WKWebView`, nil allowed.
        let url = unsafe { webview.URL() }?;
        url.absoluteString().map(|s| s.to_string())
    }

    const _: () = assert!(NSAlertFirstButtonReturn == WAIT_RESPONSE);
    const _: () = assert!(NSAlertSecondButtonReturn == RELOAD_RESPONSE);

    /// Asks the user about a page that did not answer View ▸ Reload: Reload
    /// (its web content process ended) or Wait. `None`, nothing shown,
    /// while a modal panel (a page's dialog) is up. Wait is the default
    /// (Return): a page only busy keeps its unsaved work. While the alert
    /// is up, once `still_open` says the question was answered, the alert
    /// closes itself ([`NotRespondingChoice::Closed`], QA-072): a page that
    /// answers is not left under "isn't responding". On the main thread.
    pub(crate) fn ask_not_responding(
        still_open: impl Fn() -> bool + 'static,
    ) -> Option<NotRespondingChoice> {
        let mtm = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(mtm);
        if app.modalWindow().is_some() {
            return None;
        }
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str("This page isn't responding"));
        alert.setInformativeText(&NSString::from_str(
            "Wait for it, or reload it with the design's preview stopped. \
             Changes not applied are lost.",
        ));
        alert.addButtonWithTitle(&NSString::from_str("Wait"));
        alert.addButtonWithTitle(&NSString::from_str("Reload"));
        let window = alert.window();
        let watch = RcBlock::new(move |_timer| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let app = NSApplication::sharedApplication(mtm);
            let ours_on_top = app.modalWindow().is_some_and(|top| top == window);
            if closes_ask(still_open(), ours_on_top) {
                // From a timer in the modal run loop mode: `abortModal`,
                // not `stopModal`, which waits for the next event.
                app.abortModal();
            }
        });
        // SAFETY: the timer is added to this (the main) thread's run loop
        // only, so the block never runs on another thread; invalidated
        // below, before the alert and what the block holds go.
        let timer = unsafe {
            let timer = NSTimer::timerWithTimeInterval_repeats_block(ASK_POLL_SECS, true, &watch);
            NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSModalPanelRunLoopMode);
            timer
        };
        let response = alert.runModal();
        timer.invalidate();
        Some(choice_of(response))
    }

    /// From now on ⌘Q and ⌘R ([`menu_first`]) go to the main menu before
    /// the window, unless a modal panel (a dialog) is up. Call it once, on
    /// the main thread.
    pub(crate) fn menu_keys_first() {
        let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands a live event for the call.
            let ev: &NSEvent = unsafe { event.as_ref() };
            if take(ev) {
                std::ptr::null_mut()
            } else {
                event.as_ptr()
            }
        });
        // SAFETY: the handler returns the event it got or null, as the
        // method asks.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
        };
        // Kept for the app's life: nothing removes it.
        std::mem::forget(monitor);
    }

    /// Whether the menu took `ev`.
    fn take(ev: &NSEvent) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let held = ev.modifierFlags()
            & NSEventModifierFlags::DeviceIndependentFlagsMask
            & !NSEventModifierFlags::CapsLock;
        let key = ev
            .charactersIgnoringModifiers()
            .map(|s| s.to_string().to_lowercase())
            .unwrap_or_default();
        if !menu_first(held == NSEventModifierFlags::Command, &key) {
            return false;
        }
        let app = NSApplication::sharedApplication(mtm);
        if app.modalWindow().is_some() {
            return false;
        }
        app.mainMenu()
            .is_some_and(|menu| menu.performKeyEquivalent(ev))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_and_reload_go_to_the_menu_first() {
        assert!(menu_first(true, "q"));
        assert!(menu_first(true, "r"));
    }

    #[test]
    fn the_two_buttons_are_the_only_choices() {
        assert_eq!(choice_of(WAIT_RESPONSE), NotRespondingChoice::Wait);
        assert_eq!(choice_of(RELOAD_RESPONSE), NotRespondingChoice::Reload);
        // `NSModalResponseAbort`, `NSModalResponseStop`: closed by the
        // shell, never a Reload (QA-072).
        assert_eq!(choice_of(-1001), NotRespondingChoice::Closed);
        assert_eq!(choice_of(-1000), NotRespondingChoice::Closed);
    }

    #[test]
    fn the_ask_closes_once_answered_and_only_itself() {
        // The page answered: closes, unless a page's dialog is on top.
        assert!(closes_ask(false, true));
        assert!(!closes_ask(false, false));
        // Still not answered: stays up for the user.
        assert!(!closes_ask(true, true));
        assert!(!closes_ask(true, false));
    }

    #[test]
    fn other_keys_reach_the_page() {
        // ⌘W closes a Studio tab (QA-042), ⌘K the palette.
        assert!(!menu_first(true, "w"));
        assert!(!menu_first(true, "k"));
        // ⇧⌘R, ⌥⌘Q or a bare R are the page's.
        assert!(!menu_first(false, "r"));
        assert!(!menu_first(false, "q"));
    }
}
