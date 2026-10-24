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

/// [`to_html`] for the bundled guide, whose headings carry `{#id}`
/// anchors that links point at.
#[must_use]
pub fn guide_html(md: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_HEADING_ATTRIBUTES;
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, Parser::new_ext(md, opts));
    out
}

/// Points relative `<img src>` of rendered Markdown at `base` (the
/// document's folder as a URL ending in `/`), so a document's images show.
#[must_use]
pub fn rebase_images(html: &str, base: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<img src=\"") {
        let (head, tail) = rest.split_at(i + "<img src=\"".len());
        out.push_str(head);
        let lower = tail.to_ascii_lowercase();
        let absolute = ["http:", "https:", "data:", "/", "#"]
            .iter()
            .any(|p| lower.starts_with(p));
        if !absolute {
            out.push_str(base);
        }
        rest = tail;
    }
    out.push_str(rest);
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
    fn the_guide_has_heading_anchors() {
        assert_eq!(
            guide_html("## DeepSeek Harness {#deepseek-harness}"),
            "<h2 id=\"deepseek-harness\">DeepSeek Harness</h2>\n"
        );
        for guide in [
            include_str!("../docs/guide.en.md"),
            include_str!("../docs/guide.vi.md"),
        ] {
            assert!(guide_html(guide).contains("<h2 id=\"deepseek-harness\">"));
        }
    }

    #[test]
    fn renders_basic_markdown() {
        assert_eq!(
            to_html("**hi** `x`"),
            "<p><strong>hi</strong> <code>x</code></p>\n"
        );
        assert!(to_html("```rust\nfn a(){}\n```").contains("<code class=\"language-rust\">"));
    }

    #[test]
    fn relative_images_point_at_the_document_folder() {
        let html = to_html("![a](img/a.png) ![b](https://x.test/b.png)");
        let out = rebase_images(&html, "/api/projects/p/raw/docs/");
        assert!(
            out.contains(r#"src="/api/projects/p/raw/docs/img/a.png""#),
            "{out}"
        );
        assert!(out.contains(r#"src="https://x.test/b.png""#), "{out}");
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
