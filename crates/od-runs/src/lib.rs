//! Run registry (spec 2.2, 5.4).
//!
//! Every run has a strictly increasing event id. The last [`RING_CAPACITY`]
//! events stay in RAM; every event is also appended to
//! `runs/<id>/events.jsonl`, and lifecycle events (`start`, `error`, `end`)
//! rewrite `runs/<id>/state.json` synchronously (temp file + rename).
//!
//! Subscribers get a gap-free, duplicate-free stream: the backlog after the
//! requested id (from disk when older than the ring, then the ring) followed
//! by live events, cut at one point under the run's lock.

mod run;
mod store;

pub use run::{Orphan, Run, RunEvent, RunMeta, RunState, Subscription};
pub use store::{LoadReport, Runs};

/// Events kept in memory per run.
pub const RING_CAPACITY: usize = 2000;

/// Buffered `events.jsonl` writes reach the disk at least this often.
pub const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunsError {
    #[error("run not found")]
    NotFound,
    #[error("clientRequestId is already associated with a different request")]
    IdempotencyConflict,
    #[error("invalid run id")]
    InvalidId,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = RunsError> = std::result::Result<T, E>;

/// Current time as Unix epoch milliseconds.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Run ids become folder names: one safe segment.
#[must_use]
pub fn is_safe_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
