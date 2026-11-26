//! Compare-and-set for the forms that edit a user's file (the skill
//! editor, DESIGN.md, the memory index): the `GET` the form opens on
//! answers an `ETag` (a hash of the content it shows), the form's `PUT`
//! sends it back as `If-Match`, and a file changed since (an agent, a
//! replace, another window) is refused with 412 and left as it is. The
//! check runs under the lock the other writers of that file take, so
//! nothing lands between the compare and the write.
//!
//! A `PUT` with no `If-Match` is refused with 428 and writes nothing: a
//! form that has no tag yet (its read is late or failed) must not write
//! blind. A write over whatever is there says so: `If-Match: *` ("Save
//! anyway", an agent that means it). `tests/put_reads_if_match.rs` holds
//! every `PUT` route to taking [`IfMatch`] (never `Option<IfMatch>`), or
//! to a listed reason it does not.

use crate::error::ApiErr;
use axum::extract::FromRequestParts;
use axum::http::header::{ETAG, IF_MATCH};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use od_contracts::ErrorCode;

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
/// tag is read as its tag), `None` for `*` (whatever is there). A request
/// without one is refused before the handler runs ([`required`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IfMatch(pub Option<String>);

impl IfMatch {
    /// Ok for `*` or when the content is still `current` (its tag); else
    /// the 412 that tells the form it changed.
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

/// 428: no `If-Match`; nothing written. A form sends the tag it opened
/// on, a write over whatever is there sends `*`.
#[must_use]
pub fn required() -> ApiErr {
    ApiErr::new(
        StatusCode::PRECONDITION_REQUIRED,
        ErrorCode::BadRequest,
        "If-Match is required: the ETag the form opened on, or * to write over whatever is there",
    )
}

/// `Some(None)` for `*`, `Some(Some(tag))` for a tag, `None` when there is
/// no usable value (empty, not text).
fn parse(value: Option<&str>) -> Option<Option<String>> {
    let v = value?.trim();
    let v = v.strip_prefix("W/").unwrap_or(v).trim_matches('"');
    match v {
        "" => None,
        "*" => Some(None),
        tag => Some(Some(tag.to_owned())),
    }
}

impl<S: Send + Sync> FromRequestParts<S> for IfMatch {
    type Rejection = ApiErr;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let sent = parts.headers.get(IF_MATCH).and_then(|v| v.to_str().ok());
        parse(sent).map(Self).ok_or_else(required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_match_reads_the_tag_inside_its_quotes() {
        let tag = |t: &str| Some(Some(t.to_owned()));
        assert_eq!(parse(Some("\"abc\"")), tag("abc"));
        assert_eq!(parse(Some("W/\"abc\"")), tag("abc"));
        assert_eq!(parse(Some(" abc ")), tag("abc"));
        assert_eq!(parse(Some("*")), Some(None));
        assert_eq!(parse(Some("\"*\"")), Some(None));
        assert_eq!(parse(Some("")), None);
        assert_eq!(parse(Some("\"\"")), None);
        assert_eq!(parse(None), None);
    }

    #[test]
    fn only_another_tag_is_refused() {
        assert!(IfMatch(None).check("a").is_ok());
        assert!(IfMatch(Some("a".into())).check("a").is_ok());
        let err = IfMatch(Some("a".into())).check("b").unwrap_err();
        assert_eq!(err.status, StatusCode::PRECONDITION_FAILED);
    }
}
