//! What the Studio's file views (Markdown, sketch, Office text, History)
//! share: the state of a read with its error and Retry ([`ReadState`]), and
//! a save made a moment after the last change, at once when the view goes
//! and as the page unloads ([`Autosave`]).

use super::icons::{Glyph, icon};
use super::tasks::Tasks;
use crate::i18n::{T, t};
use leptos::prelude::*;
use std::cell::{Cell, RefCell};

/// A read for a view: what it got, and why the last one failed while
/// nothing was shown yet.
///
/// - A first read that failed says so (with Retry), not a skeleton for
///   good.
/// - A later read that failed keeps what is shown: an error over it would
///   hide it, and a blank would invite a save over the file.
/// - A read that worked, and a Retry, clear the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadState<T> {
    value: Option<T>,
    error: Option<String>,
}

impl<T> Default for ReadState<T> {
    fn default() -> Self {
        Self {
            value: None,
            error: None,
        }
    }
}

impl<T> ReadState<T> {
    /// A read starts (Retry): no error until it fails again.
    pub fn retry(&mut self) {
        self.error = None;
    }

    /// The outcome of a read.
    pub fn read(&mut self, res: Result<T, String>) {
        match res {
            Ok(v) => {
                self.value = Some(v);
                self.error = None;
            }
            Err(why) if self.value.is_none() => self.error = Some(why),
            Err(_) => {}
        }
    }

    /// What the last read that worked got.
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Whether a read worked once.
    pub fn loaded(&self) -> bool {
        self.value.is_some()
    }

    /// Why nothing could be shown, for the banner.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Nothing to show and nothing to say yet: the skeleton.
    pub fn waiting(&self) -> bool {
        self.value.is_none() && self.error.is_none()
    }
}

/// "Could not read" with Retry, shown while `read` has an error.
/// `id` prefixes the test ids (`{id}-read-error`, `{id}-retry`).
pub fn read_error_banner<V: Send + Sync + 'static>(
    read: RwSignal<ReadState<V>>,
    class: &'static str,
    id: &'static str,
    message: T,
    retry: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    move || {
        read.with(|r| r.error().map(str::to_owned)).map(|why| {
            view! {
                <div class=class role="alert" data-testid=format!("{id}-read-error")>
                    <p class="error-text" title=why>{icon(Glyph::Warning)}{t(message)}</p>
                    <button class="btn small" data-testid=format!("{id}-retry") on:click=move |_| retry()>
                        {icon(Glyph::Restore)}{t(T::NextRetry)}
                    </button>
                </div>
            }
        })
    }
}

/// Saves a moment after the last change, and at once with a change not
/// saved yet when its component goes (tab switch, panel closed, Studio
/// left) or the page unloads (reload, tab closed, Back to another page
/// load; see [`flush_all_unloading`]).
///
/// `save` gets a [`Save`]: its result updates the view only while
/// [`Save::is_latest`] holds, and one that failed calls [`Save::failed`], so
/// leaving the view tries it again. Its request is sent before `save`
/// returns, with `keepalive` while [`Save::unloading`].
#[derive(Clone, Copy)]
pub struct Autosave {
    delay_ms: u32,
    changes: StoredValue<u32>,
    saves: StoredValue<u32>,
    pending: StoredValue<bool>,
    save: StoredValue<Box<dyn Fn(Save)>, LocalStorage>,
    tasks: Tasks,
}

impl Autosave {
    /// Autosave for the current component; call it in the component's body.
    pub fn new(delay_ms: u32, save: impl Fn(Save) + 'static) -> Self {
        let this = Self {
            delay_ms,
            changes: StoredValue::new(0),
            saves: StoredValue::new(0),
            pending: StoredValue::new(false),
            save: StoredValue::new_local(Box::new(save)),
            tasks: Tasks::here(),
        };
        let id = OPEN.with(|open| open.add(this));
        // The owner's signals are still there while its cleanups run.
        on_cleanup(move || {
            OPEN.with(|open| open.remove(id));
            this.flush();
        });
        #[cfg(target_arch = "wasm32")]
        listen_for_unload();
        this
    }

    /// Something changed: saved once nothing more changes for the delay.
    pub fn changed(self) {
        let n = self.mark();
        let delay = self.delay_ms;
        self.tasks.spawn(async move {
            gloo_timers::future::TimeoutFuture::new(delay).await;
            if self.changes.get_value() == n {
                self.save_now();
            }
        });
    }

    /// A change not saved yet; the number of the change.
    fn mark(self) -> u32 {
        self.pending.set_value(true);
        let n = self.changes.get_value().wrapping_add(1);
        self.changes.set_value(n);
        n
    }

    /// Saves now (Cmd+S, blur), whatever the timer.
    pub fn save_now(self) {
        self.save_as(false);
    }

    fn save_as(self, unloading: bool) {
        self.pending.set_value(false);
        let n = self.saves.get_value().wrapping_add(1);
        self.saves.set_value(n);
        self.save.with_value(|save| {
            save(Save {
                autosave: self,
                n,
                unloading,
            });
        });
    }

    /// Saves now if a change is not saved yet.
    pub fn flush(self) {
        if self.pending.get_value() {
            self.save_now();
        }
    }
}

/// The autosaves of the views on screen, by a number of their own.
#[derive(Default)]
struct Open {
    next: Cell<u64>,
    list: RefCell<Vec<(u64, Autosave)>>,
}

impl Open {
    fn add(&self, autosave: Autosave) -> u64 {
        let id = self.next.get().wrapping_add(1);
        self.next.set(id);
        self.list.borrow_mut().push((id, autosave));
        id
    }

    fn remove(&self, id: u64) {
        self.list.borrow_mut().retain(|(n, _)| *n != id);
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn all(&self) -> Vec<Autosave> {
        self.list.borrow().iter().map(|(_, a)| *a).collect()
    }
}

thread_local! {
    static OPEN: Open = Open::default();
}

/// Saves every change not saved yet of the views on screen, as the page
/// unloads: their timer would never fire, and no cleanup runs (QA-090).
/// Each request is sent with `keepalive`, so the unload does not cut it,
/// as the original app does on `pagehide` (`FileWorkspace.tsx`).
#[cfg(any(target_arch = "wasm32", test))]
fn flush_all_unloading() {
    for autosave in OPEN.with(Open::all) {
        if autosave.pending.try_get_value() == Some(true) {
            autosave.save_as(true);
        }
    }
}

/// Flushes the autosaves on `pagehide` (a reload, a closed tab, Back to
/// another page load, a page put in the back-forward cache) and on
/// `beforeunload`, which comes first; listened to once, for the page's life.
#[cfg(target_arch = "wasm32")]
fn listen_for_unload() {
    thread_local! {
        static LISTENING: Cell<bool> = const { Cell::new(false) };
    }
    if LISTENING.with(|l| l.replace(true)) {
        return;
    }
    // Never removed: the handles are dropped, the listeners stay.
    let _ = window_event_listener(leptos::ev::pagehide, |_| flush_all_unloading());
    let _ = window_event_listener(leptos::ev::beforeunload, |_| flush_all_unloading());
}

/// One save of an [`Autosave`].
#[derive(Clone, Copy)]
pub struct Save {
    autosave: Autosave,
    n: u32,
    unloading: bool,
}

impl Save {
    /// Whether the page is unloading: the request must outlive it
    /// (`keepalive`), and nothing is left to show its result.
    pub fn unloading(self) -> bool {
        self.unloading
    }

    /// Whether no save started after this one: only the last one says how
    /// it went.
    pub fn is_latest(self) -> bool {
        self.autosave.saves.get_value() == self.n
    }

    /// The save failed: the change is still to save.
    pub fn failed(self) {
        self.autosave.pending.set_value(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn a_first_read_that_failed_shows_the_error() {
        let mut r = ReadState::<String>::default();
        assert!(r.waiting());
        r.read(Err("HTTP 500".into()));
        assert_eq!(r.error(), Some("HTTP 500"));
        assert!(!r.loaded());
        assert!(!r.waiting(), "no skeleton for good");
    }

    #[test]
    fn a_later_read_that_failed_keeps_what_is_shown() {
        let mut r = ReadState::default();
        r.read(Ok("# Notes".to_owned()));
        r.read(Err("HTTP 500".into()));
        assert_eq!(r.error(), None, "no error over the document");
        assert_eq!(r.value().map(String::as_str), Some("# Notes"));
    }

    #[test]
    fn a_read_that_worked_clears_the_error() {
        let mut r = ReadState::default();
        r.read(Err("HTTP 500".into()));
        r.read(Ok(1));
        assert_eq!(r.error(), None);
        assert_eq!(r.value(), Some(&1));
    }

    #[test]
    fn retry_clears_the_error_until_the_read_fails_again() {
        let mut r = ReadState::<u8>::default();
        r.read(Err("HTTP 500".into()));
        r.retry();
        assert!(r.waiting(), "the skeleton while the read runs again");
        r.read(Err("HTTP 502".into()));
        assert_eq!(r.error(), Some("HTTP 502"));
    }

    /// An autosave in a fresh owner that records the saves it makes.
    fn autosave() -> (Owner, Autosave, Rc<RefCell<Vec<Save>>>) {
        let owner = Owner::new();
        let saved = Rc::new(RefCell::new(Vec::new()));
        let a = owner.with(|| {
            let saved = saved.clone();
            Autosave::new(700, move |s| saved.borrow_mut().push(s))
        });
        (owner, a, saved)
    }

    #[test]
    fn a_change_is_saved_when_the_view_goes_within_the_delay() {
        let (owner, a, saved) = autosave();
        // Changed; the timer has not fired.
        a.mark();
        drop(owner);
        assert_eq!(saved.borrow().len(), 1, "the change was saved on leaving");
    }

    #[test]
    fn nothing_is_saved_on_leaving_without_a_change() {
        let (owner, a, saved) = autosave();
        a.mark();
        a.save_now();
        drop(owner);
        assert_eq!(saved.borrow().len(), 1, "saved once, not again on leaving");
    }

    #[test]
    fn a_save_that_failed_is_tried_again_on_leaving() {
        let (owner, a, saved) = autosave();
        a.mark();
        a.save_now();
        let first = saved.borrow()[0];
        first.failed();
        drop(owner);
        assert_eq!(saved.borrow().len(), 2);
    }

    #[test]
    fn a_change_is_saved_with_keepalive_when_the_page_unloads() {
        let (owner, a, saved) = autosave();
        a.mark();
        flush_all_unloading();
        {
            let saves = saved.borrow();
            assert_eq!(saves.len(), 1, "the change was saved on unload");
            assert!(saves[0].unloading(), "a request that outlives the page");
        }
        drop(owner);
        assert_eq!(saved.borrow().len(), 1, "not saved again on leaving");
    }

    #[test]
    fn nothing_is_saved_on_unload_without_a_change_or_once_the_view_went() {
        let (owner, a, saved) = autosave();
        flush_all_unloading();
        assert!(saved.borrow().is_empty(), "no change, no save");
        a.mark();
        drop(owner);
        assert_eq!(saved.borrow().len(), 1, "saved as the view went");
        assert!(!saved.borrow()[0].unloading());
        flush_all_unloading();
        assert_eq!(saved.borrow().len(), 1, "a view gone is not saved again");
    }

    #[test]
    fn only_the_last_save_updates_the_view() {
        let (_owner, a, saved) = autosave();
        a.save_now();
        a.save_now();
        let saves = saved.borrow();
        assert!(!saves[0].is_latest());
        assert!(saves[1].is_latest());
    }
}
