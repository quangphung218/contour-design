//! Writes edits made in the preview back into the HTML source (spec 1.2:
//! inspect, direct text edit). An edit names its element by the bridge's
//! selector (`[data-od-id=…]` or a `tag:nth-of-type()` path from `html`),
//! which `lol_html` matches on the source token stream.
//!
//! The browser's DOM is not the source: it adds implied `<html>`/`<body>`
//! and runs author scripts. So an edit applies only when its selector hits
//! exactly one source element that still has the tag and reads the text
//! the preview showed (`expect_tag`, `expect_text`), and a text edit only
//! when that element is text-only. Anything else is refused rather than
//! guessed.

use lol_html::html_content::{ContentType, Element, TextChunk};
use lol_html::{ElementContentHandlers, HtmlRewriter, Selector, Settings};
use od_contracts::SourceEdit;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};

/// Elements whose text is code, never prose.
const RAW_TEXT_TAGS: [&str; 4] = ["script", "style", "template", "noscript"];
/// Longest selector accepted (the bridge's paths stay far below).
const MAX_SELECTOR_LEN: usize = 2048;
/// A style edit's `expect_text` shorter than the element's text must be at
/// least this long to count as its start (the bridge caps what it sends).
const MIN_TEXT_PREFIX: usize = 64;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("invalid selector {0:?}")]
    InvalidSelector(String),
    #[error("no element in the source matches {0:?} (it may be created by a script)")]
    NotFound(String),
    #[error("{count} elements in the source match {selector:?}")]
    Ambiguous { selector: String, count: usize },
    #[error("only text-only elements can be edited in place")]
    NotTextOnly,
    #[error("the source text changed since the preview loaded (found {found:?})")]
    TextMismatch { found: String },
    #[error("the source element is a <{found}>, the preview's is not")]
    TagMismatch { found: String },
    #[error("invalid style property {0:?}")]
    InvalidStyle(String),
    #[error("an edit needs a style change or new text")]
    Empty,
    #[error("HTML rewrite failed: {0}")]
    Rewrite(String),
}

impl From<lol_html::errors::RewritingError> for PatchError {
    fn from(e: lol_html::errors::RewritingError) -> Self {
        Self::Rewrite(e.to_string())
    }
}

/// Applies `edits` in order and returns the new source.
///
/// # Errors
/// The first edit that cannot be placed exactly (see [`PatchError`]).
pub fn apply_edits(html: &str, edits: &[SourceEdit]) -> Result<String, PatchError> {
    let mut out = html.to_owned();
    for edit in edits {
        out = apply_edit(&out, edit)?;
    }
    Ok(out)
}

fn apply_edit(html: &str, edit: &SourceEdit) -> Result<String, PatchError> {
    if edit.style.is_empty() && edit.text.is_none() {
        return Err(PatchError::Empty);
    }
    for (prop, value) in &edit.style {
        check_style(prop, value.as_deref())?;
    }
    let target = locate(html, &edit.selector)?;
    if let Some(expected) = &edit.expect_tag
        && !target.tag.eq_ignore_ascii_case(expected.trim())
    {
        return Err(PatchError::TagMismatch { found: target.tag });
    }
    if edit.text.is_some()
        && (target.descendants > 0 || RAW_TEXT_TAGS.contains(&target.tag.as_str()))
    {
        return Err(PatchError::NotTextOnly);
    }
    if let Some(expected) = &edit.expect_text {
        let found = normalize(&decode_entities(&target.text));
        let expected = normalize(expected);
        // A text edit replaces the whole text, so it must match whole.
        let starts = edit.text.is_none()
            && expected.chars().count() >= MIN_TEXT_PREFIX
            && found.starts_with(&expected);
        if found != expected && !starts {
            return Err(PatchError::TextMismatch { found });
        }
    }
    rewrite(html, &target.selector, edit)
}

/// The one source element an edit addresses.
#[derive(Debug)]
struct Target {
    selector: String,
    tag: String,
    descendants: usize,
    /// Raw (entity-encoded) text inside the element, without the code of
    /// nested `script`/`style`/… elements.
    text: String,
}

fn parse(selector: &str) -> Result<Selector, PatchError> {
    selector
        .parse()
        .map_err(|_| PatchError::InvalidSelector(selector.to_owned()))
}

/// The bridge's selector, then the same path without the parts the browser
/// implied: `html >`, then `head`/`body` right under it, each dropped only
/// when the source has no such element.
fn candidates(html: &str, selector: &str) -> Result<Vec<String>, PatchError> {
    let mut out = vec![selector.to_owned()];
    if let Some(rest) = selector.strip_prefix("html > ")
        && !has_element(html, "html")?
    {
        out.push(rest.to_owned());
        for (tag, implied) in [
            ("body", "body:nth-of-type(1) > "),
            ("head", "head:nth-of-type(1) > "),
        ] {
            if let Some(r) = rest.strip_prefix(implied)
                && !has_element(html, tag)?
            {
                out.push(r.to_owned());
            }
        }
    }
    Ok(out)
}

fn has_element(html: &str, tag: &str) -> Result<bool, PatchError> {
    Ok(scan(html, tag)?.matches > 0)
}

fn locate(html: &str, selector: &str) -> Result<Target, PatchError> {
    let selector = selector.trim();
    if selector.is_empty() || selector.len() > MAX_SELECTOR_LEN {
        return Err(PatchError::InvalidSelector(selector.to_owned()));
    }
    let mut ambiguous = None;
    for candidate in candidates(html, selector)? {
        let scan = scan(html, &candidate)?;
        match scan.matches {
            1 => return Ok(scan.target),
            0 => {}
            count => {
                ambiguous.get_or_insert(PatchError::Ambiguous {
                    selector: selector.to_owned(),
                    count,
                });
            }
        }
    }
    Err(ambiguous.unwrap_or_else(|| PatchError::NotFound(selector.to_owned())))
}

struct Scan {
    matches: usize,
    target: Target,
}

fn scan(html: &str, selector: &str) -> Result<Scan, PatchError> {
    let sel = parse(selector)?;
    let inner = parse(&format!("{selector} *"))?;
    let matches = Cell::new(0usize);
    let descendants = Cell::new(0usize);
    let tag = RefCell::new(String::new());
    // Text chunks of the target, and which of them are code: the handler of
    // a nested raw-text element sees a chunk right after the target's.
    let chunks = RefCell::new(Vec::<(String, bool)>::new());
    {
        let handlers = ElementContentHandlers::default()
            .element(|el: &mut Element<'_, '_>| {
                if matches.get() == 0 {
                    *tag.borrow_mut() = el.tag_name();
                }
                matches.set(matches.get() + 1);
                Ok(())
            })
            .text(|t: &mut TextChunk<'_>| {
                if matches.get() == 1 {
                    chunks.borrow_mut().push((t.as_str().to_owned(), false));
                }
                Ok(())
            });
        let raw = || {
            ElementContentHandlers::default().text(|_: &mut TextChunk<'_>| {
                if matches.get() == 1
                    && let Some(last) = chunks.borrow_mut().last_mut()
                {
                    last.1 = true;
                }
                Ok(())
            })
        };
        let nested = ElementContentHandlers::default().element(|_: &mut Element<'_, '_>| {
            if matches.get() == 1 {
                descendants.set(descendants.get() + 1);
            }
            Ok(())
        });
        let mut settings = Settings::new()
            .append_element_content_handler((Cow::Owned(sel), handlers))
            .append_element_content_handler((Cow::Owned(inner), nested));
        for code in RAW_TEXT_TAGS {
            let sel = parse(&format!("{selector} {code}"))?;
            settings = settings.append_element_content_handler((Cow::Owned(sel), raw()));
        }
        let mut rewriter = HtmlRewriter::new(settings, |_: &[u8]| {});
        rewriter.write(html.as_bytes())?;
        rewriter.end()?;
    }
    Ok(Scan {
        matches: matches.get(),
        target: Target {
            selector: selector.to_owned(),
            tag: tag.into_inner(),
            descendants: descendants.get(),
            text: chunks
                .into_inner()
                .into_iter()
                .filter_map(|(t, code)| (!code).then_some(t))
                .collect(),
        },
    })
}

fn rewrite(html: &str, selector: &str, edit: &SourceEdit) -> Result<String, PatchError> {
    let sel = parse(selector)?;
    let mut out = Vec::with_capacity(html.len() + 64);
    {
        let handlers = ElementContentHandlers::default().element(|el: &mut Element<'_, '_>| {
            if !edit.style.is_empty() {
                let current = el.get_attribute("style").unwrap_or_default();
                let merged = merge_style(&current, edit);
                if merged.is_empty() {
                    el.remove_attribute("style");
                } else {
                    el.set_attribute("style", &merged)?;
                }
            }
            if let Some(text) = &edit.text {
                el.set_inner_content(text, ContentType::Text);
            }
            Ok(())
        });
        let settings = Settings::new().append_element_content_handler((Cow::Owned(sel), handlers));
        let mut rewriter = HtmlRewriter::new(settings, |c: &[u8]| out.extend_from_slice(c));
        rewriter.write(html.as_bytes())?;
        rewriter.end()?;
    }
    String::from_utf8(out).map_err(|e| PatchError::Rewrite(e.to_string()))
}

/// Property names are CSS identifiers; values cannot end the declaration
/// or the attribute.
fn check_style(prop: &str, value: Option<&str>) -> Result<(), PatchError> {
    let name = prop.strip_prefix("--").unwrap_or(prop);
    let ok_name = !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    let ok_value =
        value.is_none_or(|v| v.len() <= 512 && !v.contains([';', '{', '}', '<', '>', '\n', '\r']));
    if ok_name && ok_value {
        Ok(())
    } else {
        Err(PatchError::InvalidStyle(prop.to_owned()))
    }
}

/// Splits an inline style into `(prop, value)` pairs. Semicolons inside
/// parentheses or quotes (`url(a;b)`) stay in the value.
fn declarations(style: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut depth, mut quote) = (0u32, None::<char>);
    let mut start = 0;
    let mut push = |chunk: &str| {
        if let Some((p, v)) = chunk.split_once(':') {
            let (p, v) = (p.trim(), v.trim());
            if !p.is_empty() {
                out.push((p.to_owned(), v.to_owned()));
            }
        }
    };
    for (i, c) in style.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ';') if depth == 0 => {
                push(&style[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&style[start..]);
    out
}

/// The element's inline style with `edit.style` applied: set properties
/// replace earlier ones in place (or go last), `None` removes them.
fn merge_style(current: &str, edit: &SourceEdit) -> String {
    let mut decls = declarations(current);
    for (prop, value) in &edit.style {
        let key = prop.to_ascii_lowercase();
        let at = decls.iter().position(|(p, _)| p.eq_ignore_ascii_case(&key));
        match (value, at) {
            (Some(v), Some(i)) => {
                decls[i].1.clone_from(v);
                let mut n = 0;
                decls.retain(|(p, _)| {
                    n += 1;
                    n <= i + 1 || !p.eq_ignore_ascii_case(&key)
                });
            }
            (Some(v), None) => decls.push((key, v.clone())),
            (None, _) => decls.retain(|(p, _)| !p.eq_ignore_ascii_case(&key)),
        }
    }
    decls
        .iter()
        .map(|(p, v)| format!("{p}: {v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decodes the entities source text commonly carries, so it compares with
/// what the browser rendered.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let decoded = tail.find(';').filter(|&end| end <= 10).and_then(|end| {
            let name = &tail[1..end];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((ch, end + 1))
        });
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const PAGE: &str = "<!doctype html><html><head><title>T</title></head><body>\
<h1 id=\"title\" style=\"color: red; margin: 0\">Tea &amp; cake</h1>\
<p>One</p><p>Two <b>bold</b></p>\
<section data-od-id=\"hero\"><h2>Hero</h2></section>\
<script>var s = '<p>x</p>';</script></body></html>";

    fn style(pairs: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.map(str::to_owned)))
            .collect()
    }

    fn style_edit(selector: &str, pairs: &[(&str, Option<&str>)]) -> SourceEdit {
        SourceEdit {
            selector: selector.to_owned(),
            style: style(pairs),
            ..SourceEdit::default()
        }
    }

    fn text_edit(selector: &str, text: &str, expect: Option<&str>) -> SourceEdit {
        SourceEdit {
            selector: selector.to_owned(),
            text: Some(text.to_owned()),
            expect_text: expect.map(str::to_owned),
            ..SourceEdit::default()
        }
    }

    const H1: &str = "html > body:nth-of-type(1) > h1:nth-of-type(1)";

    #[test]
    fn sets_replaces_and_removes_inline_style() {
        let out = apply_edits(
            PAGE,
            &[style_edit(
                H1,
                &[
                    ("color", Some("#0a7d3b")),
                    ("margin", None),
                    ("font-size", Some("40px")),
                ],
            )],
        )
        .unwrap();
        assert!(
            out.contains("<h1 id=\"title\" style=\"color: #0a7d3b; font-size: 40px\">"),
            "{out}"
        );
        // Everything else is byte-identical.
        assert_eq!(
            out.replace("color: #0a7d3b; font-size: 40px", "color: red; margin: 0"),
            PAGE
        );
        let cleared = apply_edits(
            PAGE,
            &[style_edit(H1, &[("color", None), ("margin", None)])],
        )
        .unwrap();
        assert!(cleared.contains("<h1 id=\"title\">"));
    }

    #[test]
    fn edits_text_of_text_only_elements() {
        let out = apply_edits(
            PAGE,
            &[text_edit(H1, "Tea <& biscuits>", Some("Tea & cake"))],
        )
        .unwrap();
        assert!(out.contains(">Tea &lt;&amp; biscuits&gt;</h1>"), "{out}");
        // The second <p> holds markup: refused, not flattened.
        let p2 = "html > body:nth-of-type(1) > p:nth-of-type(2)";
        assert_eq!(
            apply_edits(PAGE, &[text_edit(p2, "x", None)]),
            Err(PatchError::NotTextOnly)
        );
        let script = "html > body:nth-of-type(1) > script:nth-of-type(1)";
        assert_eq!(
            apply_edits(PAGE, &[text_edit(script, "alert(1)", None)]),
            Err(PatchError::NotTextOnly)
        );
    }

    #[test]
    fn text_edit_refused_when_source_moved_on() {
        let err = apply_edits(PAGE, &[text_edit(H1, "New", Some("Old title"))]).unwrap_err();
        assert_eq!(
            err,
            PatchError::TextMismatch {
                found: "Tea & cake".into()
            }
        );
        // Whitespace differences are not a change.
        assert!(apply_edits(PAGE, &[text_edit(H1, "New", Some("  Tea &\n cake "))]).is_ok());
    }

    #[test]
    fn data_od_id_paths_and_implied_html_body() {
        let out = apply_edits(
            PAGE,
            &[style_edit(
                "[data-od-id=\"hero\"] > h2:nth-of-type(1)",
                &[("color", Some("blue"))],
            )],
        )
        .unwrap();
        assert!(out.contains("<h2 style=\"color: blue\">Hero</h2>"));
        // A fragment: the browser adds html and body, the source has neither.
        let frag = "<h1>Hi</h1><p>a</p>";
        let out = apply_edits(frag, &[text_edit(H1, "Hello", Some("Hi"))]).unwrap();
        assert_eq!(out, "<h1>Hello</h1><p>a</p>");
    }

    #[test]
    fn refuses_missing_ambiguous_and_bad_input() {
        assert!(matches!(
            apply_edits(
                PAGE,
                &[style_edit(
                    "html > body:nth-of-type(1) > h3:nth-of-type(1)",
                    &[("color", Some("red"))]
                )]
            ),
            Err(PatchError::NotFound(_))
        ));
        // The relaxed `p:nth-of-type(1)` hits a first <p> in each parent:
        // two matches, so nothing is guessed.
        let nested = "<div><p>a</p></div><div><p>b</p></div>";
        assert!(matches!(
            apply_edits(
                nested,
                &[text_edit(
                    "html > body:nth-of-type(1) > p:nth-of-type(1)",
                    "x",
                    None
                )]
            ),
            Err(PatchError::Ambiguous { count: 2, .. })
        ));
        let twice = "<p class=a>x</p><p class=a>y</p>";
        assert!(matches!(
            apply_edits(twice, &[text_edit("p.a", "z", None)]),
            Err(PatchError::Ambiguous { count: 2, .. })
        ));
        assert!(matches!(
            apply_edits(PAGE, &[style_edit("h1[", &[("color", Some("red"))])]),
            Err(PatchError::InvalidSelector(_))
        ));
        for (prop, value) in [
            ("color", "red; background: url(x)"),
            ("color", "red\" onclick=\"x"),
            ("col or", "red"),
            ("1x", "red"),
        ] {
            let quoted = value.contains('"');
            let res = apply_edits(PAGE, &[style_edit(H1, &[(prop, Some(value))])]);
            if quoted {
                // Quotes are escaped by the rewriter; they never break out.
                let out = res.unwrap();
                assert!(out.contains("color: red&quot; onclick=&quot;x"), "{out}");
            } else {
                assert_eq!(res, Err(PatchError::InvalidStyle(prop.into())), "{prop}");
            }
        }
        assert_eq!(
            apply_edits(
                PAGE,
                &[SourceEdit {
                    selector: H1.into(),
                    ..SourceEdit::default()
                }]
            ),
            Err(PatchError::Empty)
        );
    }

    fn styled(selector: &str, expect: &str) -> SourceEdit {
        SourceEdit {
            expect_text: Some(expect.to_owned()),
            ..style_edit(selector, &[("color", Some("red"))])
        }
    }

    #[test]
    fn style_edit_refused_when_the_element_reads_differently() {
        // A script prepended a <div>, so the browser's second <div> is the
        // source's first: the path lands on #b, the text says #a.
        let scripted = "<body><script>document.body.prepend(document.createElement('div'))\
</script><div id=\"a\">A</div><div id=\"b\">B</div></body>";
        let div2 = "html > body:nth-of-type(1) > div:nth-of-type(2)";
        assert_eq!(
            apply_edits(scripted, &[styled(div2, "A")]),
            Err(PatchError::TextMismatch { found: "B".into() })
        );
        // `<p>` does not close in the source scan, so B and C nest in A.
        let unclosed = "<p>A<p>B</p><p>C</p>";
        let p2 = "html > body:nth-of-type(1) > p:nth-of-type(2)";
        assert!(matches!(
            apply_edits(unclosed, &[styled(p2, "B")]),
            Err(PatchError::TextMismatch { .. })
        ));
        // The right element still takes the style.
        let out = apply_edits(PAGE, &[styled(H1, "Tea & cake")]).unwrap();
        assert!(
            out.contains("<h1 id=\"title\" style=\"color: red; margin: 0\">"),
            "{out}"
        );
    }

    #[test]
    fn style_edit_checks_tag_and_reads_text_without_code() {
        let edit = SourceEdit {
            expect_tag: Some("h2".into()),
            ..style_edit(H1, &[("color", Some("red"))])
        };
        assert_eq!(
            apply_edits(PAGE, &[edit]),
            Err(PatchError::TagMismatch { found: "h1".into() })
        );
        // The section's text leaves out its script, as the bridge does.
        let page = "<section><h2>Hi</h2><script>let x = 1;</script><p>there</p></section>";
        let section = "html > body:nth-of-type(1) > section:nth-of-type(1)";
        assert_eq!(
            apply_edits(page, &[styled(section, "Hithere")]).map(|_| ()),
            Ok(())
        );
        // A long text may be sent capped; a short start is not enough.
        let long = format!("<p>{}</p>", "word ".repeat(40));
        let p1 = "html > body:nth-of-type(1) > p:nth-of-type(1)";
        assert!(apply_edits(&long, &[styled(p1, &"word ".repeat(16))]).is_ok());
        assert!(matches!(
            apply_edits(&long, &[styled(p1, "word word")]),
            Err(PatchError::TextMismatch { .. })
        ));
        // A text edit replaces the whole text: its start is not enough.
        assert!(matches!(
            apply_edits(&long, &[text_edit(p1, "x", Some(&"word ".repeat(16)))]),
            Err(PatchError::TextMismatch { .. })
        ));
    }

    #[test]
    fn implied_parents_are_dropped_only_when_the_source_lacks_them() {
        // The source has <html> and <body>; a script made the body's first
        // <p>. The relaxed `p:nth-of-type(1)` would hit the nested one.
        let edit = style_edit(
            "html > body:nth-of-type(1) > p:nth-of-type(1)",
            &[("color", Some("red"))],
        );
        for page in [
            "<html><body><div><p>B</p></div></body></html>",
            "<html><div><p>B</p></div></html>",
            "<body><div><p>B</p></div></body>",
        ] {
            assert!(
                matches!(
                    apply_edits(page, std::slice::from_ref(&edit)),
                    Err(PatchError::NotFound(_))
                ),
                "{page}"
            );
        }
    }

    #[test]
    fn style_parsing_keeps_parens_and_quotes() {
        assert_eq!(
            declarations("background: url(\"a;b.png\"); font-family: 'A;B', serif;"),
            [
                ("background".to_owned(), "url(\"a;b.png\")".to_owned()),
                ("font-family".to_owned(), "'A;B', serif".to_owned()),
            ]
        );
        let edit = style_edit("x", &[("COLOR", Some("blue"))]);
        assert_eq!(
            merge_style("color: red; Color: green", &edit),
            "color: blue"
        );
        assert_eq!(
            merge_style("", &style_edit("x", &[("--accent", Some("#c96442"))])),
            "--accent: #c96442"
        );
    }

    #[test]
    fn entities_decode_for_comparison() {
        assert_eq!(
            decode_entities("a &amp; b &#x41;&#66; &lt;&nbsp;&bogus; &"),
            "a & b AB <\u{a0}&bogus; &"
        );
    }
}
