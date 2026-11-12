//! Async work tied to the component that spawns it.
//!
//! A raw `spawn_local` does not stop a task when the component that
//! spawned it goes: a fetch that answers after the reader closed the
//! panel, the dialog or the screen resumes and reads signals already
//! disposed, which panics and takes the whole app down. Every task in
//! `app` goes through this module instead (`clippy.toml` forbids the raw
//! spawners elsewhere), and keeps three rules:
//!
//! 1. Code after an `await` only touches signals whose owner is sure to be
//!    alive: the **deepest** owner the code touches, not the screen. Owners
//!    form a tree, so the parent outlives the child: a task of a dialog
//!    checks the dialog, and may then touch the screen's signals too. Take
//!    [`Tasks::here`] in the body of that deepest component.
//! 2. Work the user asked of the server (a send, a delete, a save, an
//!    upload) runs to its end whatever the screen does; only the update of
//!    the view with its result is skipped once its owner went
//!    ([`Tasks::spawn_then`]). Work with no view to update at all goes
//!    through [`detached`].
//! 3. A component's helpers (`StudioCtx` methods and the like) touch its
//!    signals, so they are not called after an `await` once it went: they
//!    go in the `ui` part, or the task's [`Tasks::spawn`].
//!
//! Three ways to spawn:
//! - [`Tasks::spawn`]: the whole task belongs to its owner (loads, polls,
//!   reads for the view). It is dropped at its first wake-up after the
//!   owner went, so nothing after its first `await` runs, a request
//!   included.
//! - [`Tasks::spawn_then`]: `work` runs to its end whatever happens; `ui`
//!   runs with its output only while the owner is still there. `work` must
//!   not read the owner's signals: take what it needs before.
//! - [`detached`]: work with no view to update; runs to its end.

use leptos::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// The tasks of one component; `Copy`, so it goes into closures like a
/// signal.
#[derive(Clone, Copy)]
pub struct Tasks(StoredValue<()>);

impl Tasks {
    /// Tasks tied to the current owner. Call it in the body of the
    /// component whose signals the tasks touch, not in an event handler or
    /// after an `await`: there is no owner there, and the tasks would never
    /// stop.
    pub fn here() -> Self {
        if Owner::current().is_none() {
            leptos::logging::warn!("Tasks::here() outside a component: its tasks never stop");
        }
        Self(StoredValue::new(()))
    }

    /// Whether the owner the tasks were made in is still there.
    pub fn alive(self) -> bool {
        !self.0.is_disposed()
    }

    /// Spawns `fut`; it stops at the first wake-up after the owner went.
    /// Its first step (up to its first `await`) always runs; nothing after
    /// it does, so a task that sends more than one request to the server
    /// goes through [`Self::spawn_then`] instead.
    pub fn spawn(self, fut: impl Future<Output = ()> + 'static) {
        #[allow(clippy::disallowed_methods)] // the one place that spawns
        leptos::task::spawn_local(Guarded::new(fut, move || self.alive()));
    }

    /// Spawns `work`, which runs to its end even after the owner went (it
    /// must not read the owner's signals), then `ui` with its output while
    /// the owner is still there.
    pub fn spawn_then<T: 'static>(
        self,
        work: impl Future<Output = T> + 'static,
        ui: impl FnOnce(T) + 'static,
    ) {
        #[allow(clippy::disallowed_methods)] // the one place that spawns
        leptos::task::spawn_local(then_if(work, move || self.alive(), ui));
    }
}

/// Spawns `work`, which touches no signal of a component that may go (a
/// record kept for the history, a write to storage, a call into the
/// desktop shell, a store that lives as long as the app); it runs to its
/// end.
pub fn detached(work: impl Future<Output = ()> + 'static) {
    #[allow(clippy::disallowed_methods)] // the one place that spawns
    leptos::task::spawn_local(work);
}

/// `work` to its end, then `ui` with its output if `alive` holds.
async fn then_if<T>(
    work: impl Future<Output = T>,
    alive: impl FnOnce() -> bool,
    ui: impl FnOnce(T),
) {
    let out = work.await;
    if alive() {
        ui(out);
    }
}

/// `inner`, polled while `alive` holds after its first poll; done as soon
/// as it does not.
struct Guarded<F, A> {
    inner: Pin<Box<F>>,
    alive: A,
    started: bool,
}

impl<F: Future<Output = ()>, A: Fn() -> bool> Guarded<F, A> {
    fn new(inner: F, alive: A) -> Self {
        Self {
            inner: Box::pin(inner),
            alive,
            started: false,
        }
    }
}

impl<F: Future<Output = ()>, A: Fn() -> bool + Unpin> Future for Guarded<F, A> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.started && !(this.alive)() {
            return Poll::Ready(());
        }
        this.started = true;
        this.inner.as_mut().poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::task::Waker;

    /// Pending once, then ready; counts the steps it ran.
    struct TwoSteps(Rc<Cell<u32>>);

    impl Future for TwoSteps {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            let n = self.0.get() + 1;
            self.0.set(n);
            if n == 1 {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }

    fn poll<F: Future<Output = ()>>(f: &mut Pin<Box<F>>) -> Poll<()> {
        f.as_mut().poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn a_task_stops_once_its_screen_is_gone() {
        let owner = Owner::new();
        let tasks = owner.with(Tasks::here);
        let steps = Rc::new(Cell::new(0));
        let mut f = Box::pin(Guarded::new(TwoSteps(steps.clone()), move || tasks.alive()));
        assert!(poll(&mut f).is_pending());
        assert_eq!(steps.get(), 1);
        drop(owner);
        assert!(!tasks.alive());
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 1, "nothing after the first await ran");
    }

    #[test]
    fn a_task_runs_to_its_end_while_its_screen_stays() {
        let owner = Owner::new();
        let tasks = owner.with(Tasks::here);
        let steps = Rc::new(Cell::new(0));
        let mut f = Box::pin(Guarded::new(TwoSteps(steps.clone()), move || tasks.alive()));
        assert!(poll(&mut f).is_pending());
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 2);
        assert!(tasks.alive());
    }

    #[test]
    fn work_runs_to_its_end_after_the_screen_went_and_ui_does_not() {
        let owner = Owner::new();
        let tasks = owner.with(Tasks::here);
        let steps = Rc::new(Cell::new(0));
        let ui_ran = Rc::new(Cell::new(false));
        let work = {
            let steps = steps.clone();
            async move {
                TwoSteps(steps.clone()).await;
                steps.get()
            }
        };
        let ui = {
            let ui_ran = ui_ran.clone();
            move |_: u32| ui_ran.set(true)
        };
        let mut f = Box::pin(then_if(work, move || tasks.alive(), ui));
        assert!(poll(&mut f).is_pending());
        drop(owner);
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 2, "the work went past its first await");
        assert!(!ui_ran.get(), "no screen update once the screen went");
    }

    #[test]
    fn ui_gets_the_work_output_while_the_screen_stays() {
        let owner = Owner::new();
        let tasks = owner.with(Tasks::here);
        let got = Rc::new(Cell::new(0));
        let ui = {
            let got = got.clone();
            move |n: u32| got.set(n)
        };
        let steps = Rc::new(Cell::new(0));
        let work = {
            let steps = steps.clone();
            async move {
                TwoSteps(steps.clone()).await;
                7
            }
        };
        let mut f = Box::pin(then_if(work, move || tasks.alive(), ui));
        assert!(poll(&mut f).is_pending());
        assert!(poll(&mut f).is_ready());
        assert_eq!(got.get(), 7);
    }

    #[test]
    fn ui_waits_on_the_deepest_owner_not_the_screen() {
        let screen = Owner::new();
        let panel = screen.child();
        let screen_tasks = screen.with(Tasks::here);
        let panel_tasks = panel.with(Tasks::here);
        let steps = Rc::new(Cell::new(0));
        let ui_ran = Rc::new(Cell::new(false));
        let work = {
            let steps = steps.clone();
            async move {
                TwoSteps(steps.clone()).await;
                steps.get()
            }
        };
        let ui = {
            let ui_ran = ui_ran.clone();
            move |_: u32| ui_ran.set(true)
        };
        let mut f = Box::pin(then_if(work, move || panel_tasks.alive(), ui));
        assert!(poll(&mut f).is_pending());
        drop(panel);
        assert!(screen_tasks.alive(), "the screen stays");
        assert!(!panel_tasks.alive(), "the panel went");
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 2, "the work ran to its end");
        assert!(!ui_ran.get(), "no update of the panel that went");
    }

    #[test]
    fn a_task_of_a_panel_stops_when_the_panel_goes_and_the_screen_stays() {
        let screen = Owner::new();
        let panel = screen.child();
        let screen_tasks = screen.with(Tasks::here);
        let panel_tasks = panel.with(Tasks::here);
        let steps = Rc::new(Cell::new(0));
        let mut f = Box::pin(Guarded::new(TwoSteps(steps.clone()), move || {
            panel_tasks.alive()
        }));
        assert!(poll(&mut f).is_pending());
        drop(panel);
        assert!(screen_tasks.alive());
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 1, "nothing after the first await ran");
    }

    #[test]
    fn the_panel_goes_with_its_screen() {
        let screen = Owner::new();
        let panel = screen.child();
        let panel_tasks = panel.with(Tasks::here);
        drop(screen);
        assert!(!panel_tasks.alive());
        drop(panel);
    }

    #[test]
    fn the_first_step_runs_even_after_the_screen_went() {
        let steps = Rc::new(Cell::new(0));
        let mut f = Box::pin(Guarded::new(TwoSteps(steps.clone()), || false));
        assert!(poll(&mut f).is_pending());
        assert_eq!(steps.get(), 1);
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 1);
    }
}
