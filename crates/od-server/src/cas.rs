//! Compare-and-set for the forms that edit a user's file (the skill
//! editor, DESIGN.md, the memory index): the `GET` the form opens on
//! answers an `ETag` (a hash of the content it shows), the form's `PUT`
//! sends it back as `If-Match`, and a file changed since (an agent, a
//! replace, another window) is refused with 412 and left as it is. The
//! check runs under the lock the other writers of that file take, so
//! nothing lands between the compare and the write.
//!
//! A `PUT` with no `If-Match` writes as before: agents and `od` write
//! what they mean to write. `tests/put_reads_if_match.rs` holds every
//! `PUT` route to reading it, or to a reason it does not.

use crate::error::ApiErr;
use axum::extract::FromRequestParts;
use axum::http::header::{ETAG, IF_MATCH};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use od_contracts::ErrorCode;
use std::convert::Infallible;

/// The tag of content `bytes`: sha256, hex.
#[must_use]
pub fn tag_of(bytes: &[u8]) -> String {
    od_content::user_content::tag_of(bytes)
}

/// The `ETag` header for `tag` (a strong one: quoted).
#[must_use]
pub fn etag(tag: &str) -> [(axum::http::HeaderName, HeaderValue); 1] {
    let value = HeaderValue::from_str(&format!("\"{tag}\""))
        .unwrap_or_else(|_| HeaderValue::from_static("\"\""));
    [(ETAG, value)]
}

/// The `If-Match` a request sent: the tag inside its quotes (a weak `W/`
/// tag is read as its tag), `None` when there is none or it is `*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IfMatch(pub Option<String>);

impl IfMatch {
    /// Ok when nothing was sent or the content is still `current` (its
    /// tag); else the 412 that tells the form it changed.
    ///
    /// # Errors
    /// [`changed`] when the tags differ.
    pub fn check(&self, current: &str) -> Result<(), ApiErr> {
        match &self.0 {
            Some(sent) if sent != current => Err(changed()),
            _ => Ok(()),
        }
    }

    /// Whether a write may go over content tagged `current`.
    #[must_use]
    pub fn allows(&self, current: &str) -> bool {
        self.check(current).is_ok()
    }
}

/// 412: changed since the form opened on it; nothing written.
#[must_use]
pub fn changed() -> ApiErr {
    ApiErr::new(
        StatusCode::PRECONDITION_FAILED,
        ErrorCode::Conflict,
        "changed since it was opened",
    )
}

fn parse(value: &str) -> Option<String> {
    let v = value.trim();
    let v = v.strip_prefix("W/").unwrap_or(v);
    if v.is_empty() || v == "*" {
        return None;
    }
    Some(v.trim_matches('"').to_owned())
}

impl<S: Send + Sync> FromRequestParts<S> for IfMatch {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .headers
                .get(IF_MATCH)
                .and_then(|v| v.to_str().ok())
                .and_then(parse),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_match_reads_the_tag_inside_its_quotes() {
        assert_eq!(parse("\"abc\""), Some("abc".to_owned()));
        assert_eq!(parse("W/\"abc\""), Some("abc".to_owned()));
        assert_eq!(parse(" abc "), Some("abc".to_owned()));
        assert_eq!(parse("*"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn only_another_tag_is_refused() {
        assert!(IfMatch(None).check("a").is_ok());
        assert!(IfMatch(Some("a".into())).check("a").is_ok());
        let err = IfMatch(Some("a".into())).check("b").unwrap_err();
        assert_eq!(err.status, StatusCode::PRECONDITION_FAILED);
    }
}
