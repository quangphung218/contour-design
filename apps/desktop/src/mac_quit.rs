//! macOS: Quit (⌘Q, the app menu's Quit, the Dock's Quit, logging out) is
//! AppKit's `terminate:`. tao's app delegate has no
//! `applicationShouldTerminate:`, so the app ended at once and Tauri saw
//! only `RunEvent::Exit`, too late to ask about work not saved. [`install`]
//! gives the delegate one: the shell's [`crate::close_guard`] says whether
//! the app quits now or holds the quit while the page asks.

#![allow(unsafe_code)]

use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::NSApplication;
use std::sync::OnceLock;

/// `NSTerminateCancel`.
const TERMINATE_CANCEL: usize = 0;
/// `NSTerminateNow`.
const TERMINATE_NOW: usize = 1;

type MayQuit = Box<dyn Fn() -> bool + Send + Sync>;

/// Whether the app may quit now; set once by [`install`].
static MAY_QUIT: OnceLock<MayQuit> = OnceLock::new();

/// `-[delegate applicationShouldTerminate:]`.
extern "C-unwind" fn should_terminate(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> usize {
    if MAY_QUIT.get().is_none_or(|may_quit| may_quit()) {
        TERMINATE_NOW
    } else {
        TERMINATE_CANCEL
    }
}

/// From now on `terminate:` asks `may_quit` first: `false` cancels it (the
/// shell quits later itself, through Tauri's exit, which does not come
/// here again). Call it once, on the main thread, once tao set its app
/// delegate (in Tauri's `setup`).
pub(crate) fn install(may_quit: impl Fn() -> bool + Send + Sync + 'static) {
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
