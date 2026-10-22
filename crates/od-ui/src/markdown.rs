//! Markdown → HTML for assistant prose. Raw HTML from the agent is escaped,
//! never injected: chat runs in the app's own origin.

use pulldown_cmark::{Event, Options, Parser, html};

#[must_use]
pub fn to_html(md: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(md, opts).map(|ev| match ev {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(pulldown_cmark::Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) if !is_safe_url(&dest_url) => Event::Start(pulldown_cmark::Tag::Link {
            link_type,
            dest_url: "#".into(),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events);
    out
}

fn is_safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    !(lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_basic_markdown() {
        assert_eq!(
            to_html("**hi** `x`"),
            "<p><strong>hi</strong> <code>x</code></p>\n"
        );
        assert!(to_html("```rust\nfn a(){}\n```").contains("<code class=\"language-rust\">"));
    }

    #[test]
    fn escapes_raw_html_and_bad_links() {
        let out = to_html("<script>alert(1)</script>\n\nhi <img src=x onerror=y>");
        assert!(!out.contains("<script"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        assert!(out.contains("&lt;script&gt;"));
        let link = to_html("[x](javascript:alert(1))");
        assert!(link.contains("href=\"#\""), "{link}");
    }
}
