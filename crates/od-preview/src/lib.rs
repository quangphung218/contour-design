//! Preview serving for the daemon (spec 4.1): bridge injection, the preview
//! CSP, short-lived preview scopes and per-project file events.
//!
//! HTTP wiring lives in `od-server` (`routes/preview.rs`); this crate stays
//! framework-free so it can be tested on its own.

pub mod events;
pub mod inject;
pub mod lint;
pub mod scope;
pub mod source_patch;

pub use events::{ChangeKind, ProjectEvent, ProjectEvents, Subscription, WatchOptions};
pub use inject::{BRIDGE_JS, InjectError, MAX_INJECT_BYTES, inject_bridge};
pub use scope::{PreviewScope, PreviewScopes, SCOPE_TTL, is_scope_token};

/// `sandbox` attribute the host puts on the preview iframe (opaque origin).
pub const PREVIEW_IFRAME_SANDBOX: &str = "allow-scripts allow-forms";

/// Fetch directives shared by every preview CSP: only `'self'`, `data:` and
/// `blob:`, no network calls, no form posts.
macro_rules! preview_directives {
    () => {
        "default-src 'self' data: blob:; \
img-src 'self' data: blob:; \
media-src 'self' data: blob:; \
font-src 'self' data:; \
style-src 'self' 'unsafe-inline'; \
script-src 'self' 'unsafe-inline' 'unsafe-eval'; \
connect-src 'none'; \
form-action 'none'; \
object-src 'none'"
    };
}

/// CSP for `GET .../preview/<scope>/...`: no `<base>` rewrites either.
pub const PREVIEW_CSP: &str = concat!(
    "sandbox allow-scripts allow-forms; ",
    preview_directives!(),
    "; base-uri 'none'"
);

/// CSP for `GET .../raw/...`: the Studio preview iframe loads project HTML
/// from `raw`, so it gets the same no-network policy. Its sandbox matches
/// that iframe's `sandbox` attribute (the stricter of the two applies).
/// `?bridge=1` adds a `<base>` into the daemon's preview scope, so `<base>`
/// may point at the daemon (`'self'`), nowhere else.
pub const RAW_CSP: &str = concat!(
    "sandbox allow-scripts allow-forms allow-popups allow-modals; ",
    preview_directives!(),
    "; base-uri 'self'"
);

/// Body of `GET /api/projects/:id/preview-url`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewUrlResponse {
    pub url: String,
    pub file: String,
    pub csp: String,
    pub iframe_sandbox: String,
    pub opaque_origin: bool,
    /// Unix milliseconds.
    pub expires_at: i64,
}

/// Percent-encodes each `/`-separated segment of a project path for a URL.
#[must_use]
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encodes one URL path segment (RFC 3986 unreserved kept).
#[must_use]
pub fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_blocks_network_forms_and_base() {
        for directive in [
            "connect-src 'none'",
            "form-action 'none'",
            "base-uri 'none'",
            "default-src 'self' data: blob:",
        ] {
            assert!(PREVIEW_CSP.contains(directive), "missing {directive}");
        }
        assert!(!PREVIEW_CSP.contains("http"));
    }

    #[test]
    fn path_encoding_keeps_slashes() {
        assert_eq!(
            encode_path("deck/my slide#1.html"),
            "deck/my%20slide%231.html"
        );
        assert_eq!(encode_segment("a/b"), "a%2Fb");
        assert_eq!(encode_path("Tiếng.html"), "Ti%E1%BA%BFng.html");
    }
}
