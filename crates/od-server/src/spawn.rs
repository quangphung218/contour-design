//! Tasks that keep the caller's span.
//!
//! `tokio::spawn` and `spawn_blocking` start a task outside the span of
//! the code that started it, so a line it logs loses the request's
//! `req{request_id=…}`. Code in this crate spawns through these two
//! instead; `tests/spawn_in_span.rs` refuses a direct call that is not
//! listed there with a reason.
//!
//! Both return tokio's own [`JoinHandle`]: awaiting, aborting or dropping
//! it behaves as before.

use std::future::Future;
use tokio::task::JoinHandle;
use tracing::Instrument as _;

/// [`tokio::spawn`] in the caller's current span.
pub fn spawn_in_span<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(future.in_current_span())
}

/// [`tokio::task::spawn_blocking`] in the caller's current span.
pub fn spawn_blocking_in_span<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || span.in_scope(f))
}
