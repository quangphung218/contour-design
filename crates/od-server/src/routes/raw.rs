//! `GET /api/projects/:id/raw/<path>`: streams a project file with
//! `ETag`/`Last-Modified` revalidation (304) and single byte ranges (206).

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::io::SeekFrom;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

/// Result of parsing a `Range` header against a file size.
#[derive(Debug, PartialEq, Eq)]
enum ByteRange {
    /// Inclusive `start..=end`.
    Satisfiable(u64, u64),
    Unsatisfiable,
}

/// Single `bytes=` range (RFC 7233). `None` means "serve the whole file":
/// absent, malformed or multi-range headers.
fn parse_byte_range(header: &str, size: u64) -> Option<ByteRange> {
    let spec = header.strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (raw_start, raw_end) = spec.split_once('-')?;
    if raw_start.is_empty() {
        let suffix: u64 = raw_end.parse().unwrap_or(0);
        if suffix == 0 || size == 0 {
            return Some(ByteRange::Unsatisfiable);
        }
        return Some(ByteRange::Satisfiable(
            size.saturating_sub(suffix),
            size - 1,
        ));
    }
    let start: u64 = raw_start.parse().ok()?;
    if start >= size {
        return Some(ByteRange::Unsatisfiable);
    }
    let end = if raw_end.is_empty() {
        size - 1
    } else {
        let end: u64 = raw_end.parse().ok()?;
        if end < start {
            return None;
        }
        end.min(size - 1)
    };
    Some(ByteRange::Satisfiable(start, end))
}

/// Weak validator from size and mtime, like Express `send`.
fn etag_for(size: u64, mtime_ms: f64) -> String {
    // Millisecond mtimes fit in u64 for any realistic date.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ms = mtime_ms.max(0.0) as u64;
    format!("W/\"{size:x}-{ms:x}\"")
}

fn opaque(tag: &str) -> &str {
    tag.trim().trim_start_matches("W/")
}

fn none_match(headers: &HeaderMap, etag: &str) -> Option<bool> {
    let raw = headers.get(header::IF_NONE_MATCH)?.to_str().ok()?;
    Some(
        raw.split(',')
            .any(|t| t.trim() == "*" || opaque(t) == opaque(etag)),
    )
}

fn not_modified_since(headers: &HeaderMap, mtime: SystemTime) -> bool {
    let Some(since) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| httpdate::parse_http_date(v).ok())
    else {
        return false;
    };
    // HTTP dates have second precision.
    let secs = mtime.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    UNIX_EPOCH + Duration::from_secs(secs) <= since
}

/// Types a browser renders as a scriptable document.
fn is_document(mime: &str) -> bool {
    let essence = mime.split(';').next().unwrap_or_default().trim();
    matches!(
        essence,
        "text/html" | "application/xhtml+xml" | "image/svg+xml" | "text/xml" | "application/xml"
    )
}

pub async fn raw(
    State(state): State<AppState>,
    Path((id, rest)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let (file, full) = state.blocking(move |s| s.stat_file(&id, &rest)).await?;
    let size = file.size;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mtime = UNIX_EPOCH + Duration::from_millis(file.mtime.max(0.0) as u64);
    let etag = etag_for(size, file.mtime);

    let mut builder = Response::builder()
        .header(header::ETAG, &etag)
        .header(header::LAST_MODIFIED, httpdate::fmt_http_date(mtime))
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    // Project HTML renders in the Studio iframe from here: no network.
    // Other types (PDF, images) keep their native viewers.
    if is_document(&file.mime) {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, od_preview::RAW_CSP);
    }
    // Sandboxed preview iframes send `Origin: null`; let them read assets.
    if headers.get(header::ORIGIN).map(HeaderValue::as_bytes) == Some(b"null") {
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    }

    let fresh = none_match(&headers, &etag).unwrap_or_else(|| not_modified_since(&headers, mtime));
    if fresh {
        return builder
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .map_err(|e| ApiErr::internal(e.to_string()));
    }

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_byte_range(v, size));
    let (status, start, len) = match range {
        Some(ByteRange::Unsatisfiable) => {
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{size}"))],
            )
                .into_response());
        }
        Some(ByteRange::Satisfiable(start, end)) => {
            builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
            (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        None => (StatusCode::OK, 0, size),
    };

    let mut handle = tokio::fs::File::open(&full)
        .await
        .map_err(|e| ApiErr::from(od_store::StoreError::Io(e)))?;
    if start > 0 {
        handle
            .seek(SeekFrom::Start(start))
            .await
            .map_err(|e| ApiErr::internal(e.to_string()))?;
    }
    let body = Body::from_stream(ReaderStream::new(handle.take(len)));
    builder
        .status(status)
        .header(header::CONTENT_TYPE, file.mime)
        .header(header::CONTENT_LENGTH, len)
        .body(body)
        .map_err(|e| ApiErr::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing() {
        use ByteRange::{Satisfiable as S, Unsatisfiable as U};
        assert_eq!(parse_byte_range("bytes=0-3", 10), Some(S(0, 3)));
        assert_eq!(parse_byte_range("bytes=5-", 10), Some(S(5, 9)));
        assert_eq!(parse_byte_range("bytes=-4", 10), Some(S(6, 9)));
        assert_eq!(parse_byte_range("bytes=8-100", 10), Some(S(8, 9)));
        assert_eq!(parse_byte_range("bytes=10-", 10), Some(U));
        assert_eq!(parse_byte_range("bytes=-0", 10), Some(U));
        assert_eq!(parse_byte_range("bytes=0-1,3-4", 10), None);
        assert_eq!(parse_byte_range("bytes=5-2", 10), None);
        assert_eq!(parse_byte_range("items=0-1", 10), None);
    }

    #[test]
    fn etag_matching_is_weak() {
        let tag = etag_for(10, 1000.0);
        let mut h = HeaderMap::new();
        h.insert(
            header::IF_NONE_MATCH,
            tag.trim_start_matches("W/").parse().unwrap(),
        );
        assert_eq!(none_match(&h, &tag), Some(true));
        h.insert(header::IF_NONE_MATCH, "\"other\"".parse().unwrap());
        assert_eq!(none_match(&h, &tag), Some(false));
    }
}
