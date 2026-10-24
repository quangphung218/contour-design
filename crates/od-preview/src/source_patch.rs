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
//!
//! Manual edit adds attribute changes, a replacement for the element's
//! whole HTML and removal. Those two splice the source at the element's
//! exact byte span (start tag to end tag), so the rest stays byte-identical
//! and an element whose end tag the source leaves implied is refused.

use lol_html::html_content::{ContentType, Element, EndTag, TextChunk};
use lol_html::{DocumentContentHandlers, ElementContentHandlers, HtmlRewriter, Selector, Settings};
use od_contracts::{SourceAttribute, SourceEdit, SourceElementResponse};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Elements whose text is code, never prose.
const RAW_TEXT_TAGS: [&str; 4] = ["script", "style", "template", "noscript"];
/// Longest selector accepted (the bridge's paths stay far below).
const MAX_SELECTOR_LEN: usize = 2048;
/// Longest attribute value and replacement HTML accepted.
const MAX_ATTR_VALUE: usize = 64 * 1024;
const MAX_OUTER_HTML: usize = 512 * 1024;
/// The attribute edits and comments find elements by; it never changes.
const ANCHOR_ATTR: &str = "data-od-id";
/// Elements that are not drawn on the page.
const UNRENDERED_TAGS: [&str; 10] = [
    "html", "head", "body", "script", "style", "template", "noscript", "meta", "link", "title",
];
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
    #[error("an edit needs a style change, new text, attributes, new HTML or a removal")]
    Empty,
    #[error("invalid attribute {0:?}")]
    InvalidAttribute(String),
    #[error("{0:?} cannot be changed here: edits and comments find the element by it")]
    ProtectedAttribute(String),
    #[error("{0}")]
    InvalidEdit(&'static str),
    #[error("the replacement must be exactly one element ({0})")]
    InvalidHtml(&'static str),
    #[error("the source leaves this element's end tag implied; edit it in the source")]
    NoEndTag,
    #[error("<{0}> cannot be removed")]
    NotRemovable(String),
    #[error("the page's last visible element cannot be removed")]
    LastElement,
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
    let whole = edit.outer_html.is_some() || edit.remove;
    let changes = !edit.style.is_empty() || edit.text.is_some() || !edit.attributes.is_empty();
    if !whole && !changes {
        return Err(PatchError::Empty);
    }
    if (whole && changes) || (edit.outer_html.is_some() && edit.remove) {
        return Err(PatchError::InvalidEdit(
            "new HTML or a removal goes alone in its edit",
        ));
    }
    for (prop, value) in &edit.style {
        check_style(prop, value.as_deref())?;
    }
    for (name, value) in &edit.attributes {
        check_attribute(name, value.as_deref())?;
        if name.eq_ignore_ascii_case("style") && !edit.style.is_empty() {
            return Err(PatchError::InvalidEdit(
                "set the style attribute or style properties, not both",
            ));
        }
    }
    // A text edit replaces the whole text, so it must match whole.
    let target = checked_target(
        html,
        &edit.selector,
        edit.expect_tag.as_deref(),
        edit.expect_text.as_deref(),
        edit.text.is_some(),
    )?;
    if edit.text.is_some()
        && (target.descendants > 0 || RAW_TEXT_TAGS.contains(&target.tag.as_str()))
    {
        return Err(PatchError::NotTextOnly);
    }
    if let Some(new) = &edit.outer_html {
        let (start, end) = span(html, &target.selector)?;
        let replacement = one_root(new, anchor_of(html, &target.selector)?.as_deref())?;
        return Ok(format!("{}{replacement}{}", &html[..start], &html[end..]));
    }
    if edit.remove {
        if ["html", "head", "body"].contains(&target.tag.as_str()) {
            return Err(PatchError::NotRemovable(target.tag));
        }
        let (start, end) = span(html, &target.selector)?;
        let out = format!("{}{}", &html[..start], &html[end..]);
        if rendered_count(&out)? == 0 {
            return Err(PatchError::LastElement);
        }
        return Ok(out);
    }
    rewrite(html, &target.selector, edit, target.chunks > 0)
}

/// The one source element `selector` names, checked against the tag and
/// text the preview showed. `whole_text`: the text must match whole, not
/// just its start.
fn checked_target(
    html: &str,
    selector: &str,
    expect_tag: Option<&str>,
    expect_text: Option<&str>,
    whole_text: bool,
) -> Result<Target, PatchError> {
    let target = locate(html, selector)?;
    if let Some(expected) = expect_tag
        && !target.tag.eq_ignore_ascii_case(expected.trim())
    {
        return Err(PatchError::TagMismatch { found: target.tag });
    }
    if let Some(expected) = expect_text {
        let found = normalize(&decode_entities(&target.text));
        let expected = normalize(expected);
        let starts = !whole_text
            && expected.chars().count() >= MIN_TEXT_PREFIX
            && found.starts_with(&expected);
        if found != expected && !starts {
            return Err(PatchError::TextMismatch { found });
        }
    }
    Ok(target)
}

/// One element as the source has it (manual edit reads it before showing
/// its attributes and HTML).
///
/// # Errors
/// As [`apply_edits`] for a selector that does not name exactly one
/// element that still reads as expected; [`PatchError::NoEndTag`].
pub fn read_element(
    html: &str,
    selector: &str,
    expect_tag: Option<&str>,
    expect_text: Option<&str>,
) -> Result<SourceElementResponse, PatchError> {
    let target = checked_target(html, selector, expect_tag, expect_text, false)?;
    let (start, end) = span(html, &target.selector)?;
    let attributes = RefCell::new(Vec::new());
    {
        let handlers = ElementContentHandlers::default().element(|el: &mut Element<'_, '_>| {
            *attributes.borrow_mut() = el
                .attributes()
                .iter()
                .map(|a| SourceAttribute {
                    name: a.name(),
                    value: decode_entities(&a.value()),
                })
                .collect();
            Ok(())
        });
        run(html, &target.selector, handlers)?;
    }
    let text_only = target.descendants == 0 && !RAW_TEXT_TAGS.contains(&target.tag.as_str());
    Ok(SourceElementResponse {
        text: normalize(&decode_entities(&target.text)),
        tag: target.tag,
        attributes: attributes.into_inner(),
        outer_html: html[start..end].to_owned(),
        text_only,
    })
}

/// Runs `handlers` on the elements `selector` matches, output dropped.
fn run(html: &str, selector: &str, handlers: ElementContentHandlers<'_>) -> Result<(), PatchError> {
    let settings =
        Settings::new().append_element_content_handler((Cow::Owned(parse(selector)?), handlers));
    let mut rewriter = HtmlRewriter::new(settings, |_: &[u8]| {});
    rewriter.write(html.as_bytes())?;
    rewriter.end()?;
    Ok(())
}

/// Start of an element and, once its end tag is seen, its end.
type Span = (usize, Option<usize>);

/// Byte span of the one element `selector` matches: its start tag through
/// its end tag (the start tag alone for a void element).
fn span(html: &str, selector: &str) -> Result<(usize, usize), PatchError> {
    let found: Rc<Cell<Option<Span>>> = Rc::new(Cell::new(None));
    let seen = Rc::clone(&found);
    let handlers = ElementContentHandlers::default().element(move |el: &mut Element<'_, '_>| {
        let start = el.source_location().bytes();
        if !el.can_have_content() || el.is_self_closing() {
            seen.set(Some((start.start, Some(start.end))));
            return Ok(());
        }
        seen.set(Some((start.start, None)));
        let close = Rc::clone(&seen);
        let tag = el.tag_name();
        if let Some(handlers) = el.end_tag_handlers() {
            handlers.push(Box::new(move |end: &mut EndTag<'_>| {
                let at = end.source_location().bytes();
                // An implied end tag has no bytes of its own: the handler
                // then runs on the parent's end tag (or none).
                if let Some((s, None)) = close.get()
                    && at.end > at.start
                    && end.name().eq_ignore_ascii_case(&tag)
                {
                    close.set(Some((s, Some(at.end))));
                }
                Ok(())
            }));
        }
        Ok(())
    });
    run(html, selector, handlers)?;
    match found.get() {
        Some((start, Some(end))) if end <= html.len() => Ok((start, end)),
        Some(_) => Err(PatchError::NoEndTag),
        None => Err(PatchError::NotFound(selector.to_owned())),
    }
}

/// The element's `data-od-id`, if it has one.
fn anchor_of(html: &str, selector: &str) -> Result<Option<String>, PatchError> {
    let anchor = RefCell::new(None);
    run(
        html,
        selector,
        ElementContentHandlers::default().element(|el: &mut Element<'_, '_>| {
            *anchor.borrow_mut() = el.get_attribute(ANCHOR_ATTR);
            Ok(())
        }),
    )?;
    Ok(anchor.into_inner())
}

/// `html` when it is exactly one element (comments and whitespace around
/// it allowed) that closes; the old element's `data-od-id` goes on the new
/// root when it has none, so edits and comments still find it.
fn one_root(html: &str, anchor: Option<&str>) -> Result<String, PatchError> {
    if html.len() > MAX_OUTER_HTML {
        return Err(PatchError::InvalidHtml("too long"));
    }
    let depth = Rc::new(Cell::new(0usize));
    let roots = Cell::new(0usize);
    let stray_text = Cell::new(false);
    let mut out = Vec::with_capacity(html.len() + 32);
    {
        let level = Rc::clone(&depth);
        let handlers = ElementContentHandlers::default().element(|el: &mut Element<'_, '_>| {
            if level.get() == 0 {
                roots.set(roots.get() + 1);
                if roots.get() == 1
                    && let Some(id) = anchor
                    && el.get_attribute(ANCHOR_ATTR).is_none()
                {
                    el.set_attribute(ANCHOR_ATTR, id)?;
                }
            }
            if el.can_have_content() && !el.is_self_closing() {
                level.set(level.get() + 1);
                let up = Rc::clone(&level);
                if let Some(handlers) = el.end_tag_handlers() {
                    handlers.push(Box::new(move |_: &mut EndTag<'_>| {
                        up.set(up.get().saturating_sub(1));
                        Ok(())
                    }));
                }
            }
            Ok(())
        });
        let text_depth = Rc::clone(&depth);
        let stray = &stray_text;
        let text = DocumentContentHandlers::default().text(move |t: &mut TextChunk<'_>| {
            if text_depth.get() == 0 && !t.as_str().trim().is_empty() {
                stray.set(true);
            }
            Ok(())
        });
        let settings = Settings::new()
            .append_element_content_handler((Cow::Owned(parse("*")?), handlers))
            .append_document_content_handler(text);
        let mut rewriter = HtmlRewriter::new(settings, |c: &[u8]| out.extend_from_slice(c));
        rewriter.write(html.as_bytes())?;
        rewriter.end()?;
    }
    match roots.get() {
        0 => return Err(PatchError::InvalidHtml("no element")),
        1 => {}
        _ => return Err(PatchError::InvalidHtml("several elements")),
    }
    if stray_text.get() {
        return Err(PatchError::InvalidHtml("text outside the element"));
    }
    if depth.get() != 0 {
        return Err(PatchError::InvalidHtml("an element is not closed"));
    }
    String::from_utf8(out).map_err(|e| PatchError::Rewrite(e.to_string()))
}

/// How many drawn elements the page has (outside `<head>`).
fn rendered_count(html: &str) -> Result<usize, PatchError> {
    let in_head = Rc::new(Cell::new(false));
    let count = Cell::new(0usize);
    {
        let head = Rc::clone(&in_head);
        let head_handlers =
            ElementContentHandlers::default().element(move |el: &mut Element<'_, '_>| {
                head.set(true);
                let out = Rc::clone(&head);
                if let Some(handlers) = el.end_tag_handlers() {
                    handlers.push(Box::new(move |_: &mut EndTag<'_>| {
                        out.set(false);
                        Ok(())
                    }));
                }
                Ok(())
            });
        let all = ElementContentHandlers::default().element(|el: &mut Element<'_, '_>| {
            if !in_head.get() && !UNRENDERED_TAGS.contains(&el.tag_name().as_str()) {
                count.set(count.get() + 1);
            }
            Ok(())
        });
        let settings = Settings::new()
            .append_element_content_handler((Cow::Owned(parse("head")?), head_handlers))
            .append_element_content_handler((Cow::Owned(parse("*")?), all));
        let mut rewriter = HtmlRewriter::new(settings, |_: &[u8]| {});
        rewriter.write(html.as_bytes())?;
        rewriter.end()?;
    }
    Ok(count.get())
}

/// Attribute names as the HTML parser reads them (no quotes, `=`, `>` or
/// spaces); `data-od-id` never changes; values have a size cap.
fn check_attribute(name: &str, value: Option<&str>) -> Result<(), PatchError> {
    let ok_name = name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b':')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'));
    if !ok_name || value.is_some_and(|v| v.len() > MAX_ATTR_VALUE) {
        return Err(PatchError::InvalidAttribute(name.to_owned()));
    }
    if name.eq_ignore_ascii_case(ANCHOR_ATTR) {
        return Err(PatchError::ProtectedAttribute(name.to_owned()));
    }
    Ok(())
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
    /// Text chunks seen inside the element, code included.
    chunks: usize,
}

fn parse(selector: &str) -> Result<Selector, PatchError> {
    selector
        .parse()
        .map_err(|_| PatchError::InvalidSelector(selector.to_owned()))
}

/// The bridge's selector, then the same path without the parts the browser
/// implied: `html >`, then `head`/`body` right under it, each dropped only
/// when the source has no such element (`<html>` without `<body>` keeps
/// `html >`).
fn candidates(html: &str, selector: &str) -> Result<Vec<String>, PatchError> {
    let mut out = vec![selector.to_owned()];
    let Some(rest) = selector.strip_prefix("html > ") else {
        return Ok(out);
    };
    let has_html = has_element(html, "html")?;
    if !has_html {
        out.push(rest.to_owned());
    }
    for (tag, implied) in [
        ("body", "body:nth-of-type(1) > "),
        ("head", "head:nth-of-type(1) > "),
    ] {
        if let Some(r) = rest.strip_prefix(implied)
            && !has_element(html, tag)?
        {
            out.push(if has_html {
                format!("html > {r}")
            } else {
                r.to_owned()
            });
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
    let chunks = chunks.into_inner();
    Ok(Scan {
        matches: matches.get(),
        target: Target {
            selector: selector.to_owned(),
            tag: tag.into_inner(),
            descendants: descendants.get(),
            chunks: chunks.len(),
            text: chunks
                .into_iter()
                .filter_map(|(t, code)| (!code).then_some(t))
                .collect(),
        },
    })
}

/// `has_text`: whether the target holds any text chunk (else new text is
/// prepended).
fn rewrite(
    html: &str,
    selector: &str,
    edit: &SourceEdit,
    has_text: bool,
) -> Result<String, PatchError> {
    let sel = parse(selector)?;
    let mut out = Vec::with_capacity(html.len() + 64);
    {
        // New text takes the place of the first text chunk; the others go.
        // Comments inside the element stay where they were.
        let first = Cell::new(true);
        let handlers = ElementContentHandlers::default()
            .element(|el: &mut Element<'_, '_>| {
                if !edit.style.is_empty() {
                    let current = el.get_attribute("style").unwrap_or_default();
                    let merged = merge_style(&current, edit);
                    if merged.is_empty() {
                        el.remove_attribute("style");
                    } else {
                        el.set_attribute("style", &merged)?;
                    }
                }
                for (name, value) in &edit.attributes {
                    match value {
                        Some(v) => el.set_attribute(name, v)?,
                        None => el.remove_attribute(name),
                    }
                }
                if let Some(text) = &edit.text
                    && !has_text
                {
                    el.prepend(text, ContentType::Text);
                }
                Ok(())
            })
            .text(|chunk: &mut TextChunk<'_>| {
                if let Some(text) = &edit.text {
                    if first.replace(false) {
                        chunk.replace(text, ContentType::Text);
                    } else {
                        chunk.remove();
                    }
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
/// or the attribute, or open a comment that swallows what follows.
fn check_style(prop: &str, value: Option<&str>) -> Result<(), PatchError> {
    let name = prop.strip_prefix("--").unwrap_or(prop);
    let ok_name = !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    let ok_value = value.is_none_or(|v| {
        v.len() <= 512
            && !v.contains([';', '{', '}', '<', '>', '\n', '\r'])
            && !v.contains("/*")
            && !v.contains("*/")
    });
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

/// Decodes the source text's character references (every HTML5 named
/// one, and numeric ones), so it compares with what the browser rendered.
fn decode_entities(s: &str) -> String {
    html_escape::decode_html_entities(s).into_owned()
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
    fn html_without_body_still_takes_edits() {
        // `<body>` may be left out of a document that has `<html>`.
        let page = "<html><head><title>T</title></head><p>Hi</p></html>";
        let p1 = "html > body:nth-of-type(1) > p:nth-of-type(1)";
        assert_eq!(
            apply_edits(page, &[styled(p1, "Hi")]),
            Ok("<html><head><title>T</title></head><p style=\"color: red\">Hi</p></html>".into())
        );
        let title = "html > head:nth-of-type(1) > title:nth-of-type(1)";
        let headless = "<html><title>T</title><body><p>Hi</p></body></html>";
        assert!(apply_edits(headless, &[text_edit(title, "U", Some("T"))]).is_ok());
    }

    #[test]
    fn text_edit_keeps_comments() {
        let page = "<p>Hi <!-- keep me --> there</p><p><!-- only --></p>";
        let p1 = "html > body:nth-of-type(1) > p:nth-of-type(1)";
        assert_eq!(
            apply_edits(page, &[text_edit(p1, "Hello", Some("Hi there"))]),
            Ok("<p>Hello<!-- keep me --></p><p><!-- only --></p>".into())
        );
        let p2 = "html > body:nth-of-type(1) > p:nth-of-type(2)";
        assert_eq!(
            apply_edits(page, &[text_edit(p2, "New", Some(""))]),
            Ok("<p>Hi <!-- keep me --> there</p><p>New<!-- only --></p>".into())
        );
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
    fn typographic_entities_and_comment_openers() {
        // The browser shows every named entity decoded.
        let page = "<p>A &mdash; B&rsquo;s &hellip;</p>";
        let p1 = "html > body:nth-of-type(1) > p:nth-of-type(1)";
        assert_eq!(
            apply_edits(
                page,
                &[text_edit(p1, "C", Some("A \u{2014} B\u{2019}s \u{2026}"))]
            ),
            Ok("<p>C</p>".to_owned())
        );
        // A comment opener in a value would swallow the declarations after it.
        for value in ["blue /*", "red */ x"] {
            assert_eq!(
                apply_edits(PAGE, &[style_edit(H1, &[("color", Some(value))])]),
                Err(PatchError::InvalidStyle("color".into())),
                "{value}"
            );
        }
    }

    fn attrs(pairs: &[(&str, Option<&str>)]) -> SourceEdit {
        SourceEdit {
            selector: H1.into(),
            attributes: style(pairs),
            ..SourceEdit::default()
        }
    }

    #[test]
    fn sets_and_removes_attributes() {
        let out = apply_edits(
            PAGE,
            &[attrs(&[
                ("id", None),
                ("title", Some("A \"quote\"")),
                ("data-x", Some("1")),
            ])],
        )
        .unwrap();
        assert!(
            out.contains(
                "<h1 style=\"color: red; margin: 0\" data-x=\"1\" title=\"A &quot;quote&quot;\">"
            ),
            "{out}"
        );
        for bad in ["", "a b", "x\"y", "1a", "a>b"] {
            assert_eq!(
                apply_edits(PAGE, &[attrs(&[(bad, Some("v"))])]),
                Err(PatchError::InvalidAttribute(bad.into())),
                "{bad:?}"
            );
        }
        // The anchor of edits and comments never changes, set or removed.
        for value in [Some("other"), None] {
            assert_eq!(
                apply_edits(PAGE, &[attrs(&[("data-od-id", value)])]),
                Err(PatchError::ProtectedAttribute("data-od-id".into()))
            );
        }
        let both = SourceEdit {
            style: style(&[("color", Some("blue"))]),
            ..attrs(&[("style", Some("x: y"))])
        };
        assert!(matches!(
            apply_edits(PAGE, &[both]),
            Err(PatchError::InvalidEdit(_))
        ));
        // A link: new text and href in one edit.
        let link = "<p><a href=\"/a\">Go</a></p>";
        let a = "html > body:nth-of-type(1) > p:nth-of-type(1) > a:nth-of-type(1)";
        let out = apply_edits(
            link,
            &[SourceEdit {
                selector: a.into(),
                text: Some("Next".into()),
                expect_text: Some("Go".into()),
                attributes: style(&[("href", Some("/b"))]),
                ..SourceEdit::default()
            }],
        )
        .unwrap();
        assert_eq!(out, "<p><a href=\"/b\">Next</a></p>");
    }

    fn whole(selector: &str, html: Option<&str>) -> SourceEdit {
        SourceEdit {
            selector: selector.into(),
            outer_html: html.map(str::to_owned),
            remove: html.is_none(),
            ..SourceEdit::default()
        }
    }

    const HERO: &str = "[data-od-id=\"hero\"]";

    #[test]
    fn replaces_an_elements_html_byte_exactly() {
        let out = apply_edits(
            PAGE,
            &[whole(HERO, Some("<div class=\"x\"><h3>New</h3></div>"))],
        )
        .unwrap();
        assert_eq!(
            out,
            PAGE.replace(
                "<section data-od-id=\"hero\"><h2>Hero</h2></section>",
                "<div class=\"x\" data-od-id=\"hero\"><h3>New</h3></div>"
            ),
            "the anchor moves to the new root; the rest stays byte for byte"
        );
        // A root that brings its own anchor keeps it; comments around are fine.
        let own = apply_edits(
            PAGE,
            &[whole(HERO, Some(" <!-- c --><p data-od-id=\"p1\">P</p>\n"))],
        )
        .unwrap();
        assert!(
            own.contains("<!-- c --><p data-od-id=\"p1\">P</p>"),
            "{own}"
        );
        for (html, why) in [
            ("<p>a</p><p>b</p>", "several elements"),
            ("just text", "no element"),
            ("<p>a</p> tail", "text outside the element"),
            ("<div><p>a</p>", "an element is not closed"),
        ] {
            assert_eq!(
                apply_edits(PAGE, &[whole(HERO, Some(html))]),
                Err(PatchError::InvalidHtml(why)),
                "{html}"
            );
        }
        // Void elements have no end tag to look for.
        let img = "<p><img src=\"a.png\" alt=\"A\"><br></p>";
        let sel = "html > body:nth-of-type(1) > p:nth-of-type(1) > img:nth-of-type(1)";
        assert_eq!(
            apply_edits(img, &[whole(sel, Some("<img src=\"b.png\">"))]),
            Ok("<p><img src=\"b.png\"><br></p>".into())
        );
        // An implied end tag gives no span to replace.
        let open = "<ul><li>One<li>Two</ul>";
        let li = "html > body:nth-of-type(1) > ul:nth-of-type(1) > li:nth-of-type(1)";
        assert_eq!(
            apply_edits(open, &[whole(li, Some("<li>X</li>"))]),
            Err(PatchError::NoEndTag)
        );
        // New HTML goes alone.
        let mixed = SourceEdit {
            text: Some("x".into()),
            ..whole(HERO, Some("<p>x</p>"))
        };
        assert!(matches!(
            apply_edits(PAGE, &[mixed]),
            Err(PatchError::InvalidEdit(_))
        ));
    }

    #[test]
    fn removes_elements_but_never_the_last_one() {
        let out = apply_edits(PAGE, &[whole(HERO, None)]).unwrap();
        assert_eq!(
            out,
            PAGE.replace("<section data-od-id=\"hero\"><h2>Hero</h2></section>", "")
        );
        let body = "html > body:nth-of-type(1)";
        assert_eq!(
            apply_edits(PAGE, &[whole(body, None)]),
            Err(PatchError::NotRemovable("body".into()))
        );
        let one = "<!doctype html><html><head><title>T</title></head><body><main>M</main><script>1</script></body></html>";
        let main = "html > body:nth-of-type(1) > main:nth-of-type(1)";
        assert_eq!(
            apply_edits(one, &[whole(main, None)]),
            Err(PatchError::LastElement)
        );
        // Removal checks the element as the preview showed it.
        let edit = SourceEdit {
            expect_tag: Some("div".into()),
            ..whole(HERO, None)
        };
        assert!(matches!(
            apply_edits(PAGE, &[edit]),
            Err(PatchError::TagMismatch { .. })
        ));
    }

    #[test]
    fn reads_one_element_from_the_source() {
        let el = read_element(PAGE, H1, Some("h1"), Some("Tea & cake")).unwrap();
        assert_eq!(el.tag, "h1");
        assert_eq!(
            el.attributes,
            [
                SourceAttribute {
                    name: "id".into(),
                    value: "title".into()
                },
                SourceAttribute {
                    name: "style".into(),
                    value: "color: red; margin: 0".into()
                },
            ]
        );
        assert_eq!(
            el.outer_html,
            "<h1 id=\"title\" style=\"color: red; margin: 0\">Tea &amp; cake</h1>"
        );
        assert_eq!(el.text, "Tea & cake");
        assert!(el.text_only);
        let hero = read_element(PAGE, HERO, None, None).unwrap();
        assert_eq!(
            hero.outer_html,
            "<section data-od-id=\"hero\"><h2>Hero</h2></section>"
        );
        assert!(!hero.text_only);
        assert!(matches!(
            read_element(PAGE, H1, None, Some("Other")),
            Err(PatchError::TextMismatch { .. })
        ));
    }

    #[test]
    fn entities_decode_for_comparison() {
        assert_eq!(
            decode_entities("a &amp; b &#x41;&#66; &lt;&nbsp;&bogus; &"),
            "a & b AB <\u{a0}&bogus; &"
        );
    }
}
