//! Same-origin JSON calls to the daemon.

use crate::i18n::{T as Msg, t, tf};
use gloo_net::http::{Request, Response};
use od_contracts::ApiErrorResponse;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;

/// A failed call: HTTP status (0 for network errors) and a readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status, status_hint(self.status)) {
            (0, _) => write!(f, "{}", self.message),
            (status, Some(hint)) => write!(f, "{}: {} (HTTP {status})", t(hint), self.message),
            (status, None) => write!(f, "{} (HTTP {status})", self.message),
        }
    }
}

/// What a failed status means, in the user's words; the daemon's own
/// message (English, precise) follows it.
fn status_hint(status: u16) -> Option<Msg> {
    Some(match status {
        400 | 422 => Msg::ErrBadRequest,
        401 | 403 => Msg::ErrForbidden,
        404 => Msg::ErrNotFound,
        409 => Msg::ErrConflict,
        413 => Msg::ErrTooLarge,
        500..=599 => Msg::ErrServer,
        _ => return None,
    })
}

impl std::error::Error for HttpError {}

pub type ApiResult<T> = Result<T, HttpError>;

fn network(e: gloo_net::Error) -> HttpError {
    HttpError {
        status: 0,
        message: tf(Msg::DaemonUnreachable, &[&e.to_string()]),
    }
}

async fn error_of(res: Response) -> HttpError {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let message = serde_json::from_str::<ApiErrorResponse>(&body)
        .map(|e| e.error.message)
        .unwrap_or_else(|_| {
            if body.trim().is_empty() {
                res.status_text()
            } else {
                body
            }
        });
    HttpError { status, message }
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
            message: "project p1 not found".to_owned(),
        };
        assert_eq!(
            e(404).to_string(),
            "Not found: project p1 not found (HTTP 404)"
        );
        assert_eq!(status_hint(503), Some(Msg::ErrServer));
        assert_eq!(e(418).to_string(), "project p1 not found (HTTP 418)");
        assert_eq!(e(0).to_string(), "project p1 not found");
    }
}
