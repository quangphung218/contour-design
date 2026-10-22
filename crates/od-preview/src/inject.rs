//! Streams HTML through `lol_html` to put the bridge script (and optionally a
//! `<base href>`) ahead of every author script.

use lol_html::html_content::ContentType;
use lol_html::{HtmlRewriter, Settings, element};
use std::cell::Cell;

/// The bridge, inlined into previews (`bridges/od-bridge.js`).
pub const BRIDGE_JS: &str = include_str!("../../../bridges/od-bridge.js");

/// HTML above this size is served untouched (streamed, Range-capable).
pub const MAX_INJECT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("HTML is {0} bytes; bridge injection is limited to {MAX_INJECT_BYTES}")]
    TooLarge(usize),
    #[error("HTML rewrite failed: {0}")]
    Rewrite(#[from] lol_html::errors::RewritingError),
}

fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Whether the author already set a `<base href>`; theirs must win.
fn has_authored_base(html: &[u8]) -> Result<bool, InjectError> {
    let found = Cell::new(false);
    let mut scan = HtmlRewriter::new(
        Settings::new().append_element_content_handler(element!("base[href]", |_el| {
            found.set(true);
            Ok(())
        })),
        |_: &[u8]| {},
    );
    scan.write(html)?;
    scan.end()?;
    Ok(found.get())
}

fn payload(base_href: Option<&str>) -> String {
    let mut out = String::with_capacity(BRIDGE_JS.len() + 128);
    if let Some(href) = base_href {
        out.push_str("<base href=\"");
        out.push_str(&escape_attr(href));
        out.push_str("\" data-od-preview-base>");
    }
    out.push_str("<script data-od-bridge>");
    out.push_str(BRIDGE_JS);
    out.push_str("</script>");
    out
}

/// Byte offset just past a leading `<!doctype …>`, if any.
fn after_doctype(html: &[u8]) -> usize {
    let start = html
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(html.len());
    let rest = &html[start..];
    let is_doctype = rest.len() >= 9 && rest[..9].eq_ignore_ascii_case(b"<!doctype");
    if !is_doctype {
        return 0;
    }
    rest.iter()
        .position(|&b| b == b'>')
        .map_or(0, |i| start + i + 1)
}

/// Inserts the bridge (and `<base href>` when given and the author has none)
/// before the first author script: at the start of `<head>`, else `<body>`,
/// else right before the first `<script>`, else after the doctype.
///
/// # Errors
/// [`InjectError::TooLarge`] above [`MAX_INJECT_BYTES`]; rewrite errors.
pub fn inject_bridge(html: &[u8], base_href: Option<&str>) -> Result<Vec<u8>, InjectError> {
    if html.len() > MAX_INJECT_BYTES {
        return Err(InjectError::TooLarge(html.len()));
    }
    let base_href = match base_href {
        Some(href) if !has_authored_base(html)? => Some(href),
        _ => None,
    };
    let payload = payload(base_href);
    let injected = Cell::new(false);
    let mut out = Vec::with_capacity(html.len() + payload.len());
    {
        let settings = Settings::new()
            .append_element_content_handler(element!("head, body", |el| {
                if !injected.replace(true) {
                    el.prepend(&payload, ContentType::Html);
                }
                Ok(())
            }))
            .append_element_content_handler(element!("script", |el| {
                if !injected.replace(true) {
                    el.before(&payload, ContentType::Html);
                }
                Ok(())
            }));
        let mut rewriter = HtmlRewriter::new(settings, |chunk: &[u8]| out.extend_from_slice(chunk));
        rewriter.write(html)?;
        rewriter.end()?;
    }
    if !injected.get() {
        let at = after_doctype(&out);
        out.splice(at..at, payload.bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn injected(html: &str, base: Option<&str>) -> String {
        String::from_utf8(inject_bridge(html.as_bytes(), base).unwrap()).unwrap()
    }

    fn bridge_at(out: &str) -> usize {
        out.find("<script data-od-bridge>").expect("bridge present")
    }

    #[test]
    fn bridge_has_no_script_terminator() {
        assert!(!BRIDGE_JS.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn goes_first_in_head_before_author_scripts() {
        let out = injected(
            "<!doctype html><html><head><meta charset=utf-8><script>window.a=1</script></head><body></body></html>",
            None,
        );
        let bridge = bridge_at(&out);
        assert!(bridge < out.find("<meta").unwrap());
        assert!(bridge < out.find("<script>window.a").unwrap());
        assert_eq!(out.matches("<script data-od-bridge>").count(), 1);
    }

    #[test]
    fn script_before_head_still_runs_after_bridge() {
        let out = injected("<html><script>first()</script><head></head></html>", None);
        assert!(bridge_at(&out) < out.find("first()").unwrap());
    }

    #[test]
    fn body_only_and_fragment_documents() {
        let out = injected("<body><p>x</p><script>s()</script></body>", None);
        assert!(bridge_at(&out) < out.find("<p>").unwrap());

        let out = injected("<!DOCTYPE html>\n<p>plain</p>", None);
        assert!(out.starts_with("<!DOCTYPE html><script data-od-bridge>"));

        let out = injected("<div>frag</div><script>s()</script>", None);
        assert!(bridge_at(&out) < out.find("s()").unwrap());
        assert!(out.starts_with("<div>frag</div>"));
    }

    #[test]
    fn script_text_mentioning_head_is_not_a_head() {
        let out = injected(
            "<!doctype html><script>var s = '<head>';</script><head></head>",
            None,
        );
        assert!(bridge_at(&out) < out.find("var s").unwrap());
        assert_eq!(out.matches("<script data-od-bridge>").count(), 1);
    }

    #[test]
    fn base_added_unless_authored() {
        let out = injected(
            "<head><title>t</title></head>",
            Some("/api/projects/p/preview/s/a&b/"),
        );
        assert!(
            out.contains("<base href=\"/api/projects/p/preview/s/a&amp;b/\" data-od-preview-base>")
        );
        assert!(out.find("<base").unwrap() < bridge_at(&out));

        let out = injected("<head><base href=\"https://x/\"></head>", Some("/p/"));
        assert!(!out.contains("data-od-preview-base"));
        assert!(out.contains("data-od-bridge"));
    }

    #[test]
    fn rejects_oversized_html() {
        let big = vec![b' '; MAX_INJECT_BYTES + 1];
        assert!(matches!(
            inject_bridge(&big, None),
            Err(InjectError::TooLarge(n)) if n == MAX_INJECT_BYTES + 1
        ));
        let edge = vec![b' '; MAX_INJECT_BYTES];
        assert!(inject_bridge(&edge, None).is_ok());
    }
}
