//! Same-origin JSON calls to the daemon.

use crate::i18n::{T as Msg, t, tf};
use gloo_net::http::{Request, Response};
use od_contracts::{ApiErrorResponse, ErrorCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;

/// A failed call: HTTP status (0 for network errors), the daemon's error
/// code when it sent one, and a readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status: u16,
    pub code: Option<ErrorCode>,
    pub message: String,
}

impl HttpError {
    /// The daemon said the project the call was made on does not exist
    /// (deleted in another tab, QA-079); a missing file or version in it
    /// has its own code.
    #[must_use]
    pub fn project_missing(&self) -> bool {
        self.status == 404 && self.code == Some(ErrorCode::ProjectNotFound)
    }

    /// The daemon failed on its own side ("database is locked", an I/O
    /// error): its words give the user nothing to act on (QA-081/082), so
    /// they go to the console and the user reads "try again".
    #[must_use]
    pub fn internal(&self) -> bool {
        match self.code {
            Some(code) => code == ErrorCode::InternalError,
            None => self.status >= 500,
        }
    }
}

/// `template` ("Could not save the comment: {0}") filled with what went
/// wrong. An internal failure has no cause worth showing: the sentence
/// stops before it and says to try again ("Could not save the comment.
/// Try again."). Never an HTTP status, never "daemon" (QA-081).
#[must_use]
pub fn failure_text(template: &str, e: &HttpError) -> String {
    if e.internal() {
        try_again_text(template)
    } else {
        template.replace("{0}", &e.to_string())
    }
}

/// `template` with no cause, saying to try again: "Could not save the
/// comment: {0}" reads "Could not save the comment. Try again."
#[must_use]
pub fn try_again_text(template: &str) -> String {
    let head = template.replace("{0}", "");
    let head = head.trim_end_matches([':', '：', ' ', '.']);
    format!("{head}. {}.", t(Msg::TryAgain))
}

/// [`failure_text`] for an i18n template.
#[must_use]
pub fn failure(key: Msg, e: &HttpError) -> String {
    failure_text(t(key), e)
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A cause the user can act on: their words, no status, no "daemon".
        if let Some(hint) = self.code.and_then(code_hint) {
            // The message may already say it ("Contour has no permission
            // to write…"): once is enough (QA-087).
            let hint = t(hint);
            if self.message.to_lowercase().contains(&hint.to_lowercase()) {
                return write!(f, "{}", self.message);
            }
            return write!(f, "{hint}: {}", self.message);
        }
        // The status and the daemon's own words for an internal failure
        // are in the console (`error_of`), not in front of the user.
        if self.status == 0 {
            return write!(f, "{}", self.message);
        }
        if self.internal() {
            return write!(f, "{}", t(Msg::ErrServer));
        }
        match status_hint(self.status) {
            Some(hint) => write!(f, "{}: {}", t(hint), self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// What a failed status means, in the user's words; the daemon's own
/// message (English, precise) follows it.
/// A non-JSON error body worth showing: one short line of words, not a
/// proxy's HTML page or a stack trace (those go to the console only).
fn plain_reason(body: &str) -> bool {
    let b = body.trim();
    !b.is_empty()
        && b.len() <= 200
        && !b.contains('\n')
        && !b.starts_with('<')
        && !b.starts_with('{')
}

fn status_hint(status: u16) -> Option<Msg> {
    Some(match status {
        400 | 422 => Msg::ErrBadRequest,
        401 | 403 => Msg::ErrForbidden,
        404 => Msg::ErrNotFound,
        409 => Msg::ErrConflict,
        413 => Msg::ErrTooLarge,
        _ => return None,
    })
}

/// Codes whose message already says what to do (QA-029).
fn code_hint(code: ErrorCode) -> Option<Msg> {
    match code {
        ErrorCode::NoWritePermission => Some(Msg::ErrNoWritePermission),
        ErrorCode::NoReadPermission => Some(Msg::ErrNoReadPermission),
        ErrorCode::StorageFull => Some(Msg::ErrStorageFull),
        _ => None,
    }
}

impl std::error::Error for HttpError {}

pub type ApiResult<T> = Result<T, HttpError>;

fn network(e: gloo_net::Error) -> HttpError {
    HttpError {
        status: 0,
        code: None,
        // The browser's own words ("TypeError: Failed to fetch") help
        // nobody in the chat: they go to the console (UI-008).
        message: {
            leptos::logging::warn!("request failed: {e}");
            t(Msg::DaemonUnreachable).to_owned()
        },
    }
}

pub(crate) async fn error_of(res: Response) -> HttpError {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let (code, message) = serde_json::from_str::<ApiErrorResponse>(&body)
        .map(|e| (Some(e.error.code), e.error.message))
        .unwrap_or_else(|_| {
            let text = if plain_reason(&body) {
                body.trim().to_owned()
            } else {
                res.status_text()
            };
            (None, text)
        });
    // The status and the daemon's own words, for whoever debugs it; the
    // user reads `Display` (QA-081). The raw body too, when a proxy's page
    // was kept out of `message`.
    leptos::logging::warn!("request failed: HTTP {status} {code:?}: {message}");
    if code.is_none() && message != body.trim() && !body.trim().is_empty() {
        leptos::logging::warn!("response body: {body}");
    }
    HttpError {
        status,
        code,
        message,
    }
}

/// `POST` returning raw bytes (exports).
pub async fn post_bytes<B: Serialize>(path: &str, body: &B) -> ApiResult<Vec<u8>> {
    let req = Request::post(path).json(body).map_err(network)?;
    let res = req.send().await.map_err(network)?;
    if !res.ok() {
        return Err(error_of(res).await);
    }
    res.binary().await.map_err(network)
}

/// [`post_bytes`] plus the value of response header `header`.
pub async fn post_bytes_with_header<B: Serialize>(
    path: &str,
    body: &B,
    header: &str,
) -> ApiResult<(Vec<u8>, Option<String>)> {
    let req = Request::post(path).json(body).map_err(network)?;
    let res = req.send().await.map_err(network)?;
    if !res.ok() {
        return Err(error_of(res).await);
    }
    let value = res.headers().get(header);
    Ok((res.binary().await.map_err(network)?, value))
}

/// `GET` returning raw bytes (archive download).
pub async fn get_bytes(path: &str) -> ApiResult<Vec<u8>> {
    let res = Request::get(path).send().await.map_err(network)?;
    if !res.ok() {
        return Err(error_of(res).await);
    }
    res.binary().await.map_err(network)
}

/// `GET` returning the body as text (deck source for speaker notes).
pub async fn get_text(path: &str) -> ApiResult<String> {
    let res = Request::get(path).send().await.map_err(network)?;
    if !res.ok() {
        return Err(error_of(res).await);
    }
    res.text().await.map_err(network)
}

async fn decode<T: DeserializeOwned>(res: Response) -> ApiResult<T> {
    let status = res.status();
    if !res.ok() {
        return Err(error_of(res).await);
    }
    res.json::<T>().await.map_err(|e| HttpError {
        status,
        code: None,
        message: tf(Msg::UnexpectedResponse, &[&e.to_string()]),
    })
}

/// `PUT` of a text body (file content).
pub async fn put_text<T: DeserializeOwned>(path: &str, body: &str) -> ApiResult<T> {
    let req = Request::put(path)
        .header("content-type", "text/plain; charset=utf-8")
        .body(body)
        .map_err(network)?;
    decode(req.send().await.map_err(network)?).await
}

/// `POST` of a multipart form (uploads).
pub async fn post_form<T: DeserializeOwned>(path: &str, form: web_sys::FormData) -> ApiResult<T> {
    let req = Request::post(path).body(form).map_err(network)?;
    decode(req.send().await.map_err(network)?).await
}

pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    decode(Request::get(path).send().await.map_err(network)?).await
}

pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    let req = Request::post(path).json(body).map_err(network)?;
    decode(req.send().await.map_err(network)?).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    let req = Request::put(path).json(body).map_err(network)?;
    decode(req.send().await.map_err(network)?).await
}

pub async fn patch<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    let req = Request::patch(path).json(body).map_err(network)?;
    decode(req.send().await.map_err(network)?).await
}

pub async fn delete<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    decode(Request::delete(path).send().await.map_err(network)?).await
}

/// Percent-encodes one path segment.
#[must_use]
pub fn seg(s: &str) -> String {
    js_sys::encode_uri_component(s).into()
}

/// Encodes a project-relative file path, keeping `/` separators.
#[must_use]
pub fn file_path(path: &str) -> String {
    path.split('/').map(seg).collect::<Vec<_>>().join("/")
}

/// A random id for client-chosen ids (`clientRequestId`).
#[must_use]
pub fn random_id() -> String {
    web_sys::window()
        .and_then(|w| w.crypto().ok())
        .map(|c| c.random_uuid())
        .unwrap_or_else(|| format!("{:x}", (js_sys::Math::random() * 1e16) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_lead_with_what_went_wrong() {
        let e = |status| HttpError {
            status,
            code: None,
            message: "project p1 not found".to_owned(),
        };
        assert_eq!(e(404).to_string(), "Not found: project p1 not found");
        assert_eq!(e(418).to_string(), "project p1 not found");
        assert_eq!(e(0).to_string(), "project p1 not found");
    }

    #[test]
    fn only_a_short_plain_body_is_shown() {
        assert!(plain_reason("project p1 not found"));
        assert!(plain_reason("  Bad Gateway \n"));
        for raw in [
            "",
            "<html><head><title>502 Bad Gateway</title></head></html>",
            "<!DOCTYPE html>",
            "line one\nline two",
            "{\"broken\": ",
            &"x".repeat(201),
        ] {
            assert!(!plain_reason(raw), "{raw:?} would be shown");
        }
    }

    /// The daemon's 500 as the e2e plays it (`failures.qa.mjs`).
    fn locked() -> HttpError {
        HttpError {
            status: 500,
            code: Some(ErrorCode::InternalError),
            message: "database is locked".to_owned(),
        }
    }

    #[test]
    fn an_internal_failure_hides_the_status_and_the_daemons_words() {
        let shown = locked().to_string();
        assert_eq!(shown, t(Msg::ErrServer));
        for raw in ["HTTP", "500", "daemon", "database is locked"] {
            assert!(!shown.contains(raw), "{raw} in {shown}");
        }
        // A 5xx with no JSON body (a proxy, a crash) reads the same.
        let bare = HttpError {
            status: 502,
            code: None,
            message: "<html>Bad Gateway</html>".to_owned(),
        };
        assert!(bare.internal());
        assert_eq!(bare.to_string(), shown);
        // A 5xx whose code says what to do keeps its message, no status.
        let chrome = HttpError {
            status: 503,
            code: Some(ErrorCode::UpstreamUnavailable),
            message: "Chrome is not installed.".to_owned(),
        };
        assert!(!chrome.internal());
        assert_eq!(chrome.to_string(), "Chrome is not installed.");
    }

    #[test]
    fn a_failure_says_what_was_not_done_then_why_or_try_again() {
        let template = "Could not save the comment: {0}";
        assert_eq!(
            failure_text(template, &locked()),
            "Could not save the comment. Try again."
        );
        let missing = HttpError {
            status: 404,
            code: Some(ErrorCode::FileNotFound),
            message: "index.html".to_owned(),
        };
        assert_eq!(
            failure_text(template, &missing),
            "Could not save the comment: Not found: index.html"
        );
        let offline = HttpError {
            status: 0,
            code: None,
            message: t(Msg::DaemonUnreachable).to_owned(),
        };
        assert_eq!(
            failure_text(template, &offline),
            format!("Could not save the comment: {}", t(Msg::DaemonUnreachable))
        );
        // A template whose reason is mid-sentence or ends with a stop.
        assert_eq!(
            failure_text("The edit was not applied: {0}.", &locked()),
            "The edit was not applied. Try again."
        );
        assert_eq!(
            failure(Msg::CommentFailed, &locked()),
            "Could not save the comment. Try again."
        );
    }

    #[test]
    fn write_refusals_and_full_disks_read_as_plain_words() {
        let e = |status, code, message: &str| HttpError {
            status,
            code: Some(code),
            message: message.to_owned(),
        };
        let denied = e(
            403,
            ErrorCode::NoWritePermission,
            "Contour has no permission to write to the folder /a.",
        )
        .to_string();
        // Said once, not "No permission to write: Contour has no
        // permission to write…" (QA-087).
        assert_eq!(
            denied,
            "Contour has no permission to write to the folder /a."
        );
        assert_eq!(
            e(403, ErrorCode::NoWritePermission, "read-only volume").to_string(),
            "No permission to write: read-only volume"
        );
        // QA-085: a refused read says "read", not "write".
        let unreadable = e(
            403,
            ErrorCode::NoReadPermission,
            "Contour has no permission to read this file. Check its permissions, then try again.",
        )
        .to_string();
        assert!(
            unreadable.starts_with("No permission to read: ") && !unreadable.contains("write"),
            "{unreadable}"
        );
        let full = e(
            507,
            ErrorCode::StorageFull,
            "The disk is full. Free some space, then try again.",
        )
        .to_string();
        assert!(!full.contains("HTTP") && !full.contains("daemon"), "{full}");
        // Other codes keep the status hint, without the status.
        assert_eq!(
            e(404, ErrorCode::NotFound, "gone").to_string(),
            "Not found: gone"
        );
    }

    #[test]
    fn only_the_project_code_says_the_project_is_missing() {
        let e = |code| HttpError {
            status: 404,
            code,
            message: "x".to_owned(),
        };
        assert!(e(Some(ErrorCode::ProjectNotFound)).project_missing());
        for other in [
            Some(ErrorCode::FileNotFound),
            Some(ErrorCode::VersionNotFound),
            Some(ErrorCode::ConversationNotFound),
            Some(ErrorCode::NotFound),
            None,
        ] {
            assert!(!e(other).project_missing(), "{other:?}");
        }
    }
}
