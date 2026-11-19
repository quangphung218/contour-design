//! macOS: Quit (⌘Q, the app menu's Quit, the Dock's Quit, logging out) is
//! AppKit's `terminate:`. tao's app delegate has no
//! `applicationShouldTerminate:`, so the app ended at once and Tauri saw
//! only `RunEvent::Exit`, too late to ask about work not saved. [`install`]
//! gives the delegate one: the shell's [`crate::close_guard`] says whether
//! the app quits now or holds the quit while the page asks. Logging out,
//! restarting or shutting down says so in the quit Apple Event
//! (`keyAEQuitReason`): running agents then hold nothing
//! ([`crate::close_guard::system_quit`]).

#![allow(unsafe_code)]

use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, class, msg_send, sel};
use objc2_app_kit::NSApplication;
use std::sync::OnceLock;

/// `NSTerminateCancel`.
const TERMINATE_CANCEL: usize = 0;
/// `NSTerminateNow`.
const TERMINATE_NOW: usize = 1;

/// Takes whether macOS logs out, restarts or shuts down.
type MayQuit = Box<dyn Fn(bool) -> bool + Send + Sync>;

/// `keyAEQuitReason` (`'why?'`).
const KEY_QUIT_REASON: u32 = u32::from_be_bytes(*b"why?");

/// Whether the app may quit now; set once by [`install`].
static MAY_QUIT: OnceLock<MayQuit> = OnceLock::new();

/// `-[delegate applicationShouldTerminate:]`.
extern "C-unwind" fn should_terminate(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> usize {
    let system = crate::close_guard::system_quit(quit_reason());
    if MAY_QUIT.get().is_none_or(|may_quit| may_quit(system)) {
        TERMINATE_NOW
    } else {
        TERMINATE_CANCEL
    }
}

/// The quit Apple Event's `keyAEQuitReason`, as macOS sets it when it
/// logs out, restarts or shuts down; `None` for ⌘Q, the menu or the Dock
/// (no Apple Event, or no reason in it).
fn quit_reason() -> Option<u32> {
    let manager = class!(NSAppleEventManager);
    // SAFETY: `+sharedAppleEventManager` and `-currentAppleEvent` take no
    // arguments and return an object or nil; `terminate:` runs on the main
    // thread, where the current event is the one being handled.
    let event: *mut AnyObject = unsafe {
        let shared: *mut AnyObject = msg_send![manager, sharedAppleEventManager];
        let shared = shared.as_ref()?;
        msg_send![shared, currentAppleEvent]
    };
    // SAFETY: a non-nil event is a live `NSAppleEventDescriptor`.
    let event = unsafe { event.as_ref() }?;
    // SAFETY: `-attributeDescriptorForKeyword:` and
    // `-paramDescriptorForKeyword:` take an `AEKeyword` (`unsigned int`)
    // and return a descriptor or nil; `-enumCodeValue` returns an
    // `OSType` (`unsigned int`).
    unsafe {
        let mut reason: *mut AnyObject =
            msg_send![event, attributeDescriptorForKeyword: KEY_QUIT_REASON];
        if reason.is_null() {
            reason = msg_send![event, paramDescriptorForKeyword: KEY_QUIT_REASON];
        }
        let reason = reason.as_ref()?;
        let code: u32 = msg_send![reason, enumCodeValue];
        Some(code)
    }
}

/// From now on `terminate:` asks `may_quit` first: `false` cancels it (the
/// shell quits later itself, through Tauri's exit, which does not come
/// here again). Call it once, on the main thread, once tao set its app
/// delegate (in Tauri's `setup`).
pub(crate) fn install(may_quit: impl Fn(bool) -> bool + Send + Sync + 'static) {
    if MAY_QUIT.set(Box::new(may_quit)).is_err() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("Quit cannot ask first: not on the main thread");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `delegate` is a property of `NSApplication`, nil allowed.
    let delegate: *mut AnyObject = unsafe { msg_send![&app, delegate] };
    // SAFETY: a non-nil delegate is a live object, kept by tao for the
    // app's life.
    let Some(object) = (unsafe { delegate.as_ref() }) else {
        tracing::warn!("Quit cannot ask first: no app delegate");
        return;
    };
    let class = object.class();
    // SAFETY: `should_terminate` has the method's signature (`self`,
    // `_cmd`, an `id`, returning `NSApplicationTerminateReply`, an
    // `NSUInteger`: `Q@:@`); the runtime calls it through the type given.
    // `class_addMethod` adds it only where the class has none of its own.
    let added = unsafe {
        let imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
            Imp,
        >(should_terminate);
        objc2::ffi::class_addMethod(
            std::ptr::from_ref(class).cast_mut(),
            sel!(applicationShouldTerminate:),
            imp,
            c"Q@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        tracing::warn!("Quit cannot ask first: the app delegate answers it already");
        return;
    }
    // AppKit may note what its delegate answers when it is set: set again.
    // SAFETY: the same delegate, still alive.
    unsafe {
        let _: () = msg_send![&app, setDelegate: delegate];
    }
}
