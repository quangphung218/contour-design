//! One id per HTTP request, to follow one piece of work through the log.
//!
//! [`tag`] gives every request an 8-character id, runs it inside a span
//! `req{request_id=… method=… path=…}` (the path without its query, which
//! may carry a token), and answers with the id in [`HEADER`]. Every line
//! logged while the request runs carries the span, and so does work it
//! hands to its own task when that task is instrumented
//! ([`crate::spawn`], `.in_current_span()`): a run started by `POST /api/runs` logs
//! `req{request_id=…}:run{run_id=…}`. `grep <id> daemon.log` gives the
//! whole chain.
//!
//! The UI shows the id beside an error (`ref 1a2b3c4d`); the daemon also
//! keeps the id of the last request or run that failed ([`last_failed`]) so
//! Export diagnostics and Report Issue put its lines first when the user
//! gives no id.
//!
//! Only ids, the method and the path are logged here: never a body, a
//! prompt, file contents or a token.

use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Mutex;
use std::time::Instant;
use tracing::Instrument as _;

/// Response header carrying the request id.
pub const HEADER: &str = "x-od-request-id";

/// Characters in an id.
pub const ID_LEN: usize = 8;

/// The id of the last request that failed (status ≥ 400 but 404) or of
/// the last run that failed, whichever came later, for the whole process:
/// one daemon per process outside tests.
static LAST_FAILED: Mutex<Option<String>> = Mutex::new(None);

/// A new id: [`ID_LEN`] lowercase hex characters.
#[must_use]
pub fn new_id() -> String {
    let mut id = uuid::Uuid::new_v4().simple().to_string();
    id.truncate(ID_LEN);
    id
}

/// Whether `id` can be looked up in the log: 4 to 64 ASCII letters,
/// digits or `-` (a request id, a run id or its first characters). An
/// empty or short id would match nearly every line.
#[must_use]
pub fn is_lookup_id(id: &str) -> bool {
    (4..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The id of the last request or run that failed, if any.
#[must_use]
pub fn last_failed() -> Option<String> {
    LAST_FAILED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Makes `id` (a request's, or a run's that failed after its request was
/// answered) the last failure: the one diagnostics lead with.
pub fn remember_failed(id: &str) {
    *LAST_FAILED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id.to_owned());
}

/// Whether a request that ended with `status` counts as failed: a 404 is
/// often a normal answer (a file not made yet), the rest of 4xx and 5xx
/// are not.
fn failed(status: StatusCode) -> bool {
    (status.is_client_error() && status != StatusCode::NOT_FOUND) || status.is_server_error()
}

/// The middleware: span, header, and a log line for a request that failed.
pub async fn tag(req: Request, next: Next) -> Response {
    let id = new_id();
    let span = tracing::info_span!(
        "req",
        request_id = %id,
        method = %req.method(),
        path = %req.uri().path(),
    );
    let start = Instant::now();
    let mut res = next.run(req).instrument(span.clone()).await;
    let status = res.status();
    if failed(status) {
        remember_failed(&id);
        let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        span.in_scope(|| {
            if status.is_server_error() {
                tracing::warn!(status = status.as_u16(), elapsed_ms, "request failed");
            } else {
                tracing::info!(status = status.as_u16(), elapsed_ms, "request refused");
            }
        });
    }
    if let Ok(value) = HeaderValue::from_str(&id) {
        res.headers_mut().insert(HEADER, value);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_short_hex_and_differ() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), ID_LEN);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b);
        assert!(is_lookup_id(&a));
    }

    #[test]
    fn only_a_real_id_is_looked_up() {
        assert!(is_lookup_id("1a2b3c4d"));
        assert!(is_lookup_id("0f6c2e1a-9b7d-4c1e-8f00-123456789abc"));
        for bad in [
            "",
            "abc",
            "a b c d",
            "../etc",
            "1a2b3c4d\n",
            &"a".repeat(65),
        ] {
            assert!(!is_lookup_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_404_is_not_a_failure_but_other_errors_are() {
        assert!(!failed(StatusCode::OK));
        assert!(!failed(StatusCode::NOT_FOUND));
        assert!(failed(StatusCode::BAD_REQUEST));
        assert!(failed(StatusCode::BAD_GATEWAY));
    }
}
