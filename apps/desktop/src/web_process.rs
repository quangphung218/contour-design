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

#[cfg(target_os = "macos")]
pub(crate) use mac::{kill, menu_keys_first};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use super::menu_first;
    use block2::RcBlock;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{MainThreadMarker, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags};
    use objc2_web_kit::WKWebView;
    use std::ffi::c_void;
    use std::ptr::NonNull;

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
    fn other_keys_reach_the_page() {
        // ⌘W closes a Studio tab (QA-042), ⌘K the palette.
        assert!(!menu_first(true, "w"));
        assert!(!menu_first(true, "k"));
        // ⇧⌘R, ⌥⌘Q or a bare R are the page's.
        assert!(!menu_first(false, "r"));
        assert!(!menu_first(false, "q"));
    }
}
