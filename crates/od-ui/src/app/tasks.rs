//! Async work that belongs to one screen.
//!
//! `spawn_local` does not stop a task when the component that spawned it
//! goes: a fetch that answers after the reader left the Studio (or Media,
//! Automations) would resume and read the screen's signals, already
//! disposed, which panics. A [`ScreenTasks`] is made in the screen's body,
//! so it is disposed with the screen's signals; a task spawned through it
//! is dropped at its next wake-up once that happened, before any of its
//! code runs again.

use leptos::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// The tasks of one screen; `Copy`, so it goes into closures and contexts
/// like a signal.
#[derive(Clone, Copy)]
pub struct ScreenTasks(StoredValue<()>);

impl ScreenTasks {
    /// Tasks tied to the current owner: call it in the screen's body.
    pub fn new() -> Self {
        Self(StoredValue::new(()))
    }

    /// New tasks for the screen whose body calls it, handed to the
    /// components inside it through context (see [`Self::current`]).
    pub fn provide() -> Self {
        let tasks = Self::new();
        provide_context(tasks);
        tasks
    }

    /// The tasks of the screen around the caller; the caller's own when
    /// no screen provided any.
    pub fn current() -> Self {
        use_context().unwrap_or_else(Self::new)
    }

    /// Whether the screen is still there.
    pub fn alive(self) -> bool {
        self.0.try_with_value(|()| ()).is_some()
    }

    /// Spawns `fut`; it stops at the first wake-up after the screen went.
    /// Its first step always runs, so a request sent as the screen closes
    /// (a save on leave) still goes out; what follows its first `await`
    /// does not.
    pub fn spawn(self, fut: impl Future<Output = ()> + 'static) {
        leptos::task::spawn_local(Guarded::new(fut, move || self.alive()));
    }
}

impl Default for ScreenTasks {
    fn default() -> Self {
        Self::new()
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
        let tasks = owner.with(ScreenTasks::new);
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
        let tasks = owner.with(ScreenTasks::new);
        let steps = Rc::new(Cell::new(0));
        let mut f = Box::pin(Guarded::new(TwoSteps(steps.clone()), move || tasks.alive()));
        assert!(poll(&mut f).is_pending());
        assert!(poll(&mut f).is_ready());
        assert_eq!(steps.get(), 2);
        assert!(tasks.alive());
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
