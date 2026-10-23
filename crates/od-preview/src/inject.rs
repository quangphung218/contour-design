//! Puts the bridge script (and optionally a `<base href>`) ahead of all
//! author markup, so it runs before any author code.

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

fn payload(base_href: Option<&str>, edit_key: Option<&str>) -> String {
    let mut out = String::with_capacity(BRIDGE_JS.len() + 128);
    if let Some(href) = base_href {
        out.push_str("<base href=\"");
        out.push_str(&escape_attr(href));
        out.push_str("\" data-od-preview-base>");
    }
    match edit_key {
        // The bridge reads and removes it before any author script runs.
        Some(key) => {
            out.push_str("<script data-od-bridge data-od-key=\"");
            out.push_str(&escape_attr(key));
            out.push_str("\">");
        }
        None => out.push_str("<script data-od-bridge>"),
    }
    out.push_str(BRIDGE_JS);
    out.push_str("</script>");
    out
}

/// Byte offset of the first token that can be author markup: past a BOM and
/// any whitespace, comments (`<!--…-->`, `<!…>`, `<?…>`) and doctype, none
/// of which can run script. Splicing the bridge here puts it ahead of every
/// author element, in whatever context they open (`<svg>`, `<template>`, an
/// `<iframe onload>`), without dropping the page into quirks mode.
fn prolog_end(html: &[u8]) -> usize {
    let mut at = if html.starts_with(b"\xEF\xBB\xBF") {
        3
    } else {
        0
    };
    loop {
        while html.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let rest = &html[at..];
        let len = if rest.starts_with(b"<!--") {
            comment_len(rest)
        } else if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            // Doctype and bogus comments both end at the first `>`.
            rest.iter().position(|&b| b == b'>').map(|i| i + 1)
        } else {
            return at;
        };
        match len {
            Some(len) => at += len,
            // Unclosed: the rest is one comment; nothing after it runs.
            None => return at,
        }
    }
}

/// Length of the comment `rest` opens, ended as the HTML tokenizer ends it:
/// `<!-->`, `<!--->`, else the first `-->` or `--!>`.
fn comment_len(rest: &[u8]) -> Option<usize> {
    let find = |from: usize, pat: &[u8]| {
        rest.get(from..)?
            .windows(pat.len())
            .position(|w| w == pat)
            .map(|i| from + i + pat.len())
    };
    // The `--` of `<!--` may close it too (`<!-->`, `<!--->`), but not as `--!>`.
    match (find(2, b"-->"), find(4, b"--!>")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Inserts the bridge (and `<base href>` when given and the author has none)
/// ahead of all author markup, right after the leading doctype and comments
/// (see [`prolog_end`]). The parser opens `<head>` for it; the author's own
/// `<html>`/`<head>` tags after it still apply.
///
/// # Errors
/// [`InjectError::TooLarge`] above [`MAX_INJECT_BYTES`]; rewrite errors.
pub fn inject_bridge(html: &[u8], base_href: Option<&str>) -> Result<Vec<u8>, InjectError> {
    inject_bridge_keyed(html, base_href, None)
}

/// [`inject_bridge`], handing the bridge an edit key (see [`crate::edit_key`]).
///
/// # Errors
/// As [`inject_bridge`].
pub fn inject_bridge_keyed(
    html: &[u8],
    base_href: Option<&str>,
    edit_key: Option<&str>,
) -> Result<Vec<u8>, InjectError> {
    if html.len() > MAX_INJECT_BYTES {
        return Err(InjectError::TooLarge(html.len()));
    }
    let base_href = match base_href {
        Some(href) if !has_authored_base(html)? => Some(href),
        _ => None,
    };
    let payload = payload(base_href, edit_key);
    let at = prolog_end(html);
    let mut out = Vec::with_capacity(html.len() + payload.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(payload.as_bytes());
    out.extend_from_slice(&html[at..]);
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
        assert!(out.starts_with("<script data-od-bridge>"));

        let out = injected("<!DOCTYPE html>\n<p>plain</p>", None);
        assert!(out.starts_with("<!DOCTYPE html>\n<script data-od-bridge>"));

        let out = injected("<div>frag</div><script>s()</script>", None);
        assert!(out.starts_with("<script data-od-bridge>"));
    }

    /// P1-2: elements that run code (or hide a `<script>`) before the first
    /// author script, with no `<head>` to anchor on.
    #[test]
    fn goes_before_any_author_markup() {
        for (html, prolog) in [
            ("<iframe onload=x()></iframe><script>s()</script>", ""),
            (
                "<!doctype html><svg><script>s()</script></svg>",
                "<!doctype html>",
            ),
            ("<math><script>s()</script></math>", ""),
            ("<template><script>s()</script></template>", ""),
            ("<noscript><script>s()</script></noscript>", ""),
            (
                "\u{feff} <!-- c -->\n<!DOCTYPE html><!--d--><p>",
                "\u{feff} <!-- c -->\n<!DOCTYPE html><!--d-->",
            ),
            ("<?xml version=\"1.0\"?><svg>", "<?xml version=\"1.0\"?>"),
            ("<!doctype html \"x>y\"><p>", "<!doctype html \"x>"),
        ] {
            let out = injected(html, None);
            assert!(
                out.starts_with(&format!("{prolog}<script data-od-bridge>")),
                "{html}: {out:.80}"
            );
        }
    }

    #[test]
    fn comments_end_where_the_tokenizer_ends_them() {
        for (html, prolog) in [
            ("<!--><p>", "<!-->"),
            ("<!---><p>", "<!--->"),
            ("<!----><p>", "<!---->"),
            ("<!-- a --!><p>", "<!-- a --!>"),
            ("<!----!><p>", "<!----!>"),
            // Not closed: `--!>` cannot reuse the opening dashes.
            ("<!--!><p>-->x", "<!--!><p>-->"),
            ("<!---!><p>-->x", "<!---!><p>-->"),
            ("<!-- -- > --><p>", "<!-- -- > -->"),
            // Unclosed: everything is comment, so the bridge goes before it.
            ("<!-- <p>", ""),
        ] {
            let out = injected(html, None);
            assert!(
                out.starts_with(&format!("{prolog}<script data-od-bridge>")),
                "{html}: {out:.80}"
            );
        }
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
    fn edit_key_rides_on_the_bridge_tag() {
        let out =
            String::from_utf8(inject_bridge_keyed(b"<head></head>", None, Some("k\"1")).unwrap())
                .unwrap();
        assert!(
            out.starts_with("<script data-od-bridge data-od-key=\"k&quot;1\">"),
            "{out:.80}"
        );
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
