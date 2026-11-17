//! Requests that must reach the daemon one at a time, in the order they
//! were made, such as the saves and reloads of the shared settings copy
//! (QA-108). A caller takes a [`Turn`] when it is called, sends once
//! [`Turn::ready`] resolves, and keeps the answer only if [`Turn::is_last`]:
//! a request made later already holds this one's effect, and its answer is
//! newer. Dropping the turn hands over to the next one, also when the task
//! holding it stops before its turn came.

use std::collections::BTreeSet;
use std::future::poll_fn;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};

/// The queue of turns.
#[derive(Debug, Default)]
pub struct Turns(Mutex<State>);

#[derive(Debug, Default)]
struct State {
    /// Turns handed out so far; the next turn gets this number.
    taken: u64,
    /// The turn allowed to send now.
    serving: u64,
    /// Turns dropped before `serving` reached them.
    done: BTreeSet<u64>,
    /// Tasks waiting for their turn.
    waiting: Vec<Waker>,
}

impl Turns {
    /// The next turn, behind every turn taken before.
    #[must_use]
    pub fn take(self: &Arc<Self>) -> Turn {
        let mut state = self.lock();
        let n = state.taken;
        state.taken += 1;
        Turn {
            turns: Arc::clone(self),
            n,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A place in [`Turns`]; dropping it ends the turn.
#[derive(Debug)]
pub struct Turn {
    turns: Arc<Turns>,
    n: u64,
}

impl Turn {
    /// Resolves once every turn taken before this one was dropped.
    pub async fn ready(&self) {
        poll_fn(|cx| {
            let mut state = self.turns.lock();
            if state.serving == self.n {
                Poll::Ready(())
            } else {
                state.waiting.push(cx.waker().clone());
                Poll::Pending
            }
        })
        .await;
    }

    /// No turn was taken after this one.
    #[must_use]
    pub fn is_last(&self) -> bool {
        self.turns.lock().taken == self.n + 1
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut state = self.turns.lock();
        state.done.insert(self.n);
        loop {
            let serving = state.serving;
            if !state.done.remove(&serving) {
                break;
            }
            state.serving += 1;
        }
        let waiting = std::mem::take(&mut state.waiting);
        drop(state);
        waiting.into_iter().for_each(Waker::wake);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Wake};

    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn ready_now(turn: &Turn, waker: &Waker) -> bool {
        let fut = pin!(turn.ready());
        fut.poll(&mut Context::from_waker(waker)).is_ready()
    }

    /// A save is waiting for its answer when the same window reloads. The
    /// reload's GET must not go out before the save's answer came back,
    /// or it may be served first and carry the settings without the save;
    /// and the save's answer, older than the reload's, is not kept.
    #[test]
    fn a_reload_waits_for_the_save_before_it() {
        let turns = Arc::new(Turns::default());
        let count = Arc::new(Count::default());
        let waker = Waker::from(Arc::clone(&count));
        let save = turns.take();
        assert!(ready_now(&save, &waker));
        assert!(save.is_last());
        let reload = turns.take();
        assert!(!ready_now(&reload, &waker), "the GET went out first");
        assert!(
            !save.is_last(),
            "the save's answer would replace the reload's"
        );
        drop(save);
        assert!(count.0.load(Ordering::SeqCst) > 0, "the reload is woken");
        assert!(ready_now(&reload, &waker));
        assert!(reload.is_last());
    }

    /// A task stopped while waiting (its screen was left) does not hold
    /// the queue: the turns after it still come, in order.
    #[test]
    fn a_turn_dropped_before_it_came_is_skipped() {
        let turns = Arc::new(Turns::default());
        let waker = Waker::from(Arc::new(Count::default()));
        let first = turns.take();
        let stopped = turns.take();
        let third = turns.take();
        drop(stopped);
        assert!(!ready_now(&third, &waker));
        drop(first);
        assert!(ready_now(&third, &waker));
        let fourth = turns.take();
        assert!(!third.is_last());
        drop(third);
        assert!(ready_now(&fourth, &waker));
    }
}
