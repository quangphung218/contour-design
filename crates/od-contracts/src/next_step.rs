//! The next-step marker: the agent's follow-up suggestions for the turn it
//! just finished, as the original app words them
//! (`contracts/src/api/next-step-marker.ts`).
//!
//! The agent ends a turn with up to three self-closing markers:
//!
//! ```text
//! <od-next key="a7f3c91ed2b40561" value="Add an orders list page"/>
//! ```
//!
//! The key is minted per run and shown only in that run's prompt, so text
//! the agent read (a cloned page, a quoted file) cannot plant a suggestion:
//! clicking one puts its words in the user's composer. A marker with another
//! key, or inside a code fence, gives no suggestion. Removing markers from
//! what the user reads does not depend on the key: any marker outside a
//! fence is protocol, not prose.

use std::borrow::Cow;
use std::ops::Range;

/// Suggestions shown at most; more are dropped, not an error.
pub const MAX_SUGGESTIONS: usize = 3;
/// Characters per suggestion at most; a longer one is dropped (a paragraph
/// means the model misread the marker, and the chip is one line).
pub const MAX_LENGTH: usize = 120;

const OPEN: &str = "<od-next";
const CLOSE: &str = "</od-next";

/// The suggestions of `text` keyed with `key`, in order, normalised, without
/// duplicates, at most [`MAX_SUGGESTIONS`]. Empty when there are none.
#[must_use]
pub fn parse(text: &str, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if key.is_empty() {
        return out;
    }
    for range in unfenced(text) {
        for m in markers(&text[range]).0 {
            if m.key.as_deref() != Some(key) {
                continue;
            }
            for raw in m.values {
                let Some(s) = normalise(&raw) else { continue };
                if out.iter().any(|o| o.to_lowercase() == s.to_lowercase()) {
                    continue;
                }
                out.push(s);
                if out.len() == MAX_SUGGESTIONS {
                    return out;
                }
            }
        }
    }
    out
}

/// `text` without the markers outside code fences, and without a marker
/// still being streamed at its end. Trailing blank lines the markers leave
/// go too. Unchanged when it holds none.
#[must_use]
pub fn strip(text: &str) -> Cow<'_, str> {
    if !text.contains('<') {
        return text.into();
    }
    let mut cuts: Vec<Range<usize>> = Vec::new();
    for range in unfenced(text) {
        let (found, pending) = markers(&text[range.clone()]);
        for m in found {
            cuts.push(range.start + m.span.start..range.start + m.span.end);
        }
        // A marker cut off by the stream, at the end of the text.
        if range.end == text.len()
            && let Some(start) = pending.or_else(|| open_prefix(text, range.start))
        {
            cuts.push(range.start.max(start)..text.len());
        }
    }
    if cuts.is_empty() {
        return text.into();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for c in cuts {
        out.push_str(&text[at..c.start]);
        at = c.end;
    }
    out.push_str(&text[at..]);
    out.truncate(out.trim_end().len());
    out.into()
}

/// Where the text ends with only the first characters of `<od-next`
/// (`<od`, `<od-ne`), at or after `from`.
fn open_prefix(text: &str, from: usize) -> Option<usize> {
    (3..OPEN.len()).rev().find_map(|n| {
        let start = text.len().checked_sub(n)?;
        (start >= from
            && text.is_char_boundary(start)
            && text[start..].eq_ignore_ascii_case(&OPEN[..n]))
        .then_some(start)
    })
}

/// One marker in a text.
struct Marker {
    span: Range<usize>,
    key: Option<String>,
    /// The `value` attribute, or each line of a paired block's body.
    values: Vec<String>,
}

/// The markers in `text` (no fence handling), in order, and where a marker
/// without its `>` yet starts.
fn markers(text: &str) -> (Vec<Marker>, Option<usize>) {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let open = find_tag(&lower[at..], OPEN).map(|i| at + i);
        let close = find_tag(&lower[at..], CLOSE).map(|i| at + i);
        // A closing tag with no opening one before it is noise too.
        let (start, is_close) = match (open, close) {
            (Some(o), Some(c)) if c < o => (c, true),
            (Some(o), _) => (o, false),
            (None, Some(c)) => (c, true),
            (None, None) => break,
        };
        let Some(end) = tag_end(text, start) else {
            return (out, Some(start));
        };
        if is_close {
            out.push(Marker {
                span: start..end,
                key: None,
                values: Vec::new(),
            });
            at = end;
            continue;
        }
        let tag = &text[start + OPEN.len()..end - 1];
        let self_closing = tag.trim_end().ends_with('/');
        let attrs = attributes(tag.trim_end().trim_end_matches('/'));
        let key = attr(&attrs, "key");
        let mut values: Vec<String> = attr(&attrs, "value").into_iter().collect();
        let mut span_end = end;
        // The original's older paired form: one suggestion per body line.
        if !self_closing
            && let Some(c) = find_tag(&lower[end..], CLOSE).map(|i| end + i)
            && let Some(c_end) = tag_end(text, c)
        {
            if values.is_empty() {
                values = text[end..c].lines().map(str::to_owned).collect();
            }
            span_end = c_end;
        }
        out.push(Marker {
            span: start..span_end,
            key,
            values,
        });
        at = span_end;
    }
    (out, None)
}

/// Where `tag` (`<od-next` or `</od-next`) starts in `lower` as a whole
/// tag name (`<od-nextish` is not one).
fn find_tag(lower: &str, tag: &str) -> Option<usize> {
    let mut at = 0;
    while let Some(i) = lower[at..].find(tag) {
        let pos = at + i;
        let after = lower[pos + tag.len()..].chars().next();
        if after.is_none_or(|c| c.is_whitespace() || c == '/' || c == '>') {
            return Some(pos);
        }
        at = pos + tag.len();
    }
    None
}

/// The end (after `>`) of the tag starting at `start`; a `>` inside a quoted
/// attribute does not end it.
fn tag_end(text: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in text[start..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(start + i + 1),
            (None, _) => {}
        }
    }
    None
}

/// `name="value"` pairs of a tag body; names lower-cased, values decoded.
/// Quotes are optional, as model formatting drifts.
fn attributes(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        rest = rest.trim_start();
        let name_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        if name_len == 0 {
            // Not an attribute: skip one character.
            let Some(c) = rest.chars().next() else { break };
            rest = &rest[c.len_utf8()..];
            continue;
        }
        let name = rest[..name_len].to_ascii_lowercase();
        rest = rest[name_len..].trim_start();
        let Some(after_eq) = rest.strip_prefix('=') else {
            continue;
        };
        rest = after_eq.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let body = &rest[1..];
                let end = body.find(q).unwrap_or(body.len());
                rest = body.get(end + 1..).unwrap_or("");
                &body[..end]
            }
            _ => {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let v = &rest[..end];
                rest = &rest[end..];
                v
            }
        };
        out.push((name, decode(value)));
    }
    out
}

fn attr(attrs: &[(String, String)], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
}

fn decode(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// One suggestion as the user would type it: no list bullet, emphasis,
/// wrapping quotes or tags; `None` when empty or too long.
fn normalise(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    for bullet in ["- ", "* ", "+ ", "• "] {
        if let Some(r) = s.strip_prefix(bullet) {
            s = r;
        }
    }
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if (1..=2).contains(&digits)
        && let Some(r) = s[digits..]
            .strip_prefix(". ")
            .or_else(|| s[digits..].strip_prefix(") "))
    {
        s = r;
    }
    // Stray tags become spaces; emphasis and code ticks go.
    let mut text = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        text.push_str(&rest[..lt]);
        text.push(' ');
        rest = &rest[lt + gt + 1..];
    }
    text.push_str(rest);
    text.retain(|c| c != '*' && c != '`');
    let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for (open, close) in [
        ('"', '"'),
        ('\'', '\''),
        ('“', '”'),
        ('‘', '’'),
        ('「', '」'),
    ] {
        if text.chars().count() >= 2 && text.starts_with(open) && text.ends_with(close) {
            text = text[open.len_utf8()..text.len() - close.len_utf8()]
                .trim()
                .to_owned();
            break;
        }
    }
    (!text.is_empty() && text.chars().count() <= MAX_LENGTH).then_some(text)
}

/// Byte ranges of `text` outside fenced code blocks (``` or ~~~ lines).
/// An unclosed fence runs to the end.
fn unfenced(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut start = 0;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let ch = trimmed.chars().next();
        let run = ch.map_or(0, |c| trimmed.chars().take_while(|&x| x == c).count());
        let is_fence = indent <= 3 && matches!(ch, Some('`' | '~')) && run >= 3;
        match (fence, is_fence) {
            (None, true) => {
                out.push(start..at);
                fence = ch.map(|c| (c, run));
            }
            (Some((c, n)), true)
                if ch == Some(c) && run >= n && trimmed[run * c.len_utf8()..].trim().is_empty() =>
            {
                fence = None;
                start = at + line.len();
            }
            _ => {}
        }
        at += line.len();
    }
    if fence.is_none() {
        out.push(start..text.len());
    }
    out.retain(|r| !r.is_empty());
    out
}

/// The marker as the prompt shows it, so the example and the parser agree.
#[must_use]
pub fn render(key: &str, value: &str) -> String {
    let enc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!("<od-next key=\"{}\" value=\"{}\"/>", enc(key), enc(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "a7f3c91ed2b40561";

    fn m(value: &str) -> String {
        render(KEY, value)
    }

    #[test]
    fn three_keyed_markers_give_three_suggestions() {
        let text = format!(
            "Built the shop.\n\n{}\n{}\n{}",
            m("Add an orders page"),
            m("Use two columns for the cards"),
            m("Add a dark mode")
        );
        assert_eq!(
            parse(&text, KEY),
            [
                "Add an orders page",
                "Use two columns for the cards",
                "Add a dark mode"
            ]
        );
        assert_eq!(strip(&text), "Built the shop.");
    }

    #[test]
    fn a_wrong_or_missing_key_gives_nothing_but_is_still_stripped() {
        let text = format!(
            "Done.\n<od-next key=\"0000000000000000\" value=\"Planted\"/>\n<od-next value=\"No key\"/>\n{}",
            m("Real one")
        );
        assert_eq!(parse(&text, KEY), ["Real one"]);
        assert_eq!(parse(&text, ""), Vec::<String>::new(), "no key minted");
        assert_eq!(strip(&text), "Done.");
    }

    #[test]
    fn a_fenced_marker_is_ignored_and_kept_as_code() {
        let fenced = format!("Example:\n```html\n{}\n```\n", m("Quoted"));
        assert!(parse(&fenced, KEY).is_empty());
        assert_eq!(strip(&fenced), fenced, "the agent quoted it on purpose");
        let text = format!("{fenced}\n{}", m("Outside"));
        assert_eq!(parse(&text, KEY), ["Outside"]);
        // An unclosed fence runs to the end.
        assert!(parse(&format!("```\n{}", m("x")), KEY).is_empty());
    }

    #[test]
    fn more_than_three_or_duplicates_are_capped() {
        let text = ["One", "one", "Two", "Three", "Four"]
            .iter()
            .map(|v| m(v))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse(&text, KEY), ["One", "Two", "Three"]);
    }

    #[test]
    fn a_too_long_or_empty_suggestion_is_dropped() {
        let long = "x".repeat(MAX_LENGTH + 1);
        let fits = "é".repeat(MAX_LENGTH);
        let text = format!("{}\n{}\n{}", m(&long), m("   "), m(&fits));
        assert_eq!(parse(&text, KEY), [fits]);
        assert_eq!(strip(&text), "");
    }

    #[test]
    fn no_markers_leaves_the_text_alone() {
        assert!(parse("Hello there.", KEY).is_empty());
        assert!(matches!(strip("a < b"), Cow::Borrowed("a < b")));
        assert_eq!(strip("<od-nextish>"), "<od-nextish>");
    }

    #[test]
    fn model_drift_is_normalised() {
        let text = format!(
            "<OD-NEXT key='{KEY}' value='- **Thêm trang đơn hàng**' />\n<od-next key={KEY} value=\"&quot;Quoted&quot;\"/>\n<od-next value=\"2. Numbered\" key=\"{KEY}\"/>"
        );
        assert_eq!(
            parse(&text, KEY),
            ["Thêm trang đơn hàng", "Quoted", "Numbered"]
        );
        assert_eq!(strip(&text), "");
    }

    #[test]
    fn the_paired_form_and_stray_tags_are_read_and_stripped() {
        let text = format!(
            "Done.\n<od-next key=\"{KEY}\">\n- First\n- Second\n</od-next>\nAfter </od-next> it."
        );
        assert_eq!(parse(&text, KEY), ["First", "Second"]);
        assert_eq!(strip(&text), "Done.\n\nAfter  it.");
    }

    #[test]
    fn a_marker_still_streaming_is_held_back() {
        assert_eq!(strip("Done.\n<od-next key=\"a7f3"), "Done.");
        assert_eq!(strip("Done.\n<od-ne"), "Done.");
        assert_eq!(strip(&format!("Done.\n{}\n<od", m("A"))), "Done.");
        assert_eq!(strip("a <b"), "a <b", "not a marker prefix");
        assert!(parse("<od-next key=\"a7f3c91ed2b40561\" value=\"A\"", KEY).is_empty());
    }

    #[test]
    fn a_gt_inside_the_value_does_not_end_the_tag() {
        let text = format!("<od-next key=\"{KEY}\" value=\"Make a > b\"/>");
        assert_eq!(parse(&text, KEY), ["Make a > b"]);
        assert_eq!(strip(&text), "");
    }
}
