//! Speaker notes of a deck (spec 1.2), read from the deck's HTML source:
//! a `<script id="speaker-notes">` JSON array (or `{"notes": [...]}`), else
//! an `aside`/`div` with class `notes` inside each `section.slide`.

use serde_json::Value;

/// One note per slide (empty when a slide has none), padded to `count`.
#[must_use]
pub fn extract(html: &str, count: usize) -> Vec<String> {
    let mut notes = json_notes(html).unwrap_or_else(|| inline_notes(html));
    if notes.len() < count {
        notes.resize(count, String::new());
    }
    notes
}

fn json_notes(html: &str) -> Option<Vec<String>> {
    let (start, end) = json_block(html)?;
    let parsed: Value = serde_json::from_str(html[start..end].trim()).ok()?;
    let list = parsed
        .as_array()
        .or_else(|| parsed.get("notes").and_then(Value::as_array))?;
    Some(
        list.iter()
            .map(|v| match v {
                Value::String(s) => s.trim().to_owned(),
                Value::Null => String::new(),
                other => other.to_string(),
            })
            .collect(),
    )
}

/// Byte range of the body of the `<script id="speaker-notes">` block.
fn json_block(html: &str) -> Option<(usize, usize)> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<script") {
        let open = from + i;
        let tag_end = open + lower[open..].find('>')?;
        let tag = &lower[open..tag_end];
        let body_end = tag_end + lower[tag_end..].find("</script")?;
        from = body_end;
        if has_id(tag, "speaker-notes") {
            return Some((tag_end + 1, body_end));
        }
    }
    None
}

fn has_id(tag: &str, id: &str) -> bool {
    ["\"", "'"]
        .iter()
        .any(|q| tag.contains(&format!("id={q}{id}{q}")))
        || tag.contains(&format!("id={id}"))
}

fn has_class(tag: &str, class: &str) -> bool {
    let Some(i) = tag.find("class=") else {
        return false;
    };
    let rest = &tag[i + 6..];
    let value = match rest.chars().next() {
        Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
        _ => rest.split([' ', '>']).next().unwrap_or(""),
    };
    value.split_whitespace().any(|c| c == class)
}

/// Slices of `lower` (same offsets as the source) for each `<section>` whose
/// class list holds `slide`.
fn slide_sections(lower: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<section") {
        let open = from + i;
        let Some(tag_len) = lower[open..].find('>') else {
            break;
        };
        let tag = &lower[open..open + tag_len];
        let body = open + tag_len + 1;
        let end = lower[body..]
            .find("</section")
            .map_or(lower.len(), |e| body + e);
        if has_class(tag, "slide") {
            out.push((body, end));
            from = end;
        } else {
            from = body;
        }
    }
    out
}

fn inline_notes(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let sections = slide_sections(&lower);
    let blocks = if sections.is_empty() {
        vec![(0, html.len())]
    } else {
        sections
    };
    let notes: Vec<String> = blocks
        .into_iter()
        .map(|(start, end)| notes_in(html, &lower, start, end))
        .collect();
    if notes.iter().all(String::is_empty) {
        Vec::new()
    } else {
        notes
    }
}

fn notes_in(html: &str, lower: &str, start: usize, end: usize) -> String {
    let mut from = start;
    while from < end {
        let next = ["<aside", "<div"]
            .iter()
            .filter_map(|t| lower[from..end].find(t).map(|i| (from + i, *t)))
            .min_by_key(|(i, _)| *i);
        let Some((open, tag_name)) = next else {
            break;
        };
        let Some(tag_len) = lower[open..end].find('>') else {
            break;
        };
        let body = open + tag_len + 1;
        if has_class(&lower[open..open + tag_len], "notes") {
            let close = format!("</{}", &tag_name[1..]);
            let stop = lower[body..end].find(&close).map_or(end, |e| body + e);
            return strip_tags(&html[body..stop]);
        }
        from = body;
    }
    String::new()
}

fn strip_tags(fragment: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in fragment.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag.trim_start_matches('/').to_ascii_lowercase();
                if t.starts_with("br") || (tag.starts_with('/') && t.starts_with('p')) {
                    out.push('\n');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    let text = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// `html` with slide `index`'s note set to `note`, in the form the deck
/// already uses: its JSON block, else its inline `.notes` elements (one is
/// added to the slide when missing), else a new JSON block before
/// `</body>`. `count` is the deck's slide count.
#[must_use]
pub fn set_note(html: &str, index: usize, note: &str, count: usize) -> String {
    let note = note.trim();
    if let Some((start, end)) = json_block(html) {
        let parsed: Option<Value> = serde_json::from_str(html[start..end].trim()).ok();
        let wrapped = parsed.as_ref().is_some_and(|v| v.get("notes").is_some());
        let mut notes = json_notes(html).unwrap_or_default();
        notes.resize(notes.len().max(count).max(index + 1), String::new());
        notes[index] = note.to_owned();
        let list = Value::from(notes);
        let body = if wrapped {
            let mut obj = parsed.unwrap_or_default();
            obj["notes"] = list;
            obj
        } else {
            list
        };
        return format!("{}{}{}", &html[..start], script_json(&body), &html[end..]);
    }
    let lower = html.to_ascii_lowercase();
    let sections = slide_sections(&lower);
    let inline = !inline_notes(html).is_empty();
    if inline {
        if let Some(&(start, end)) = sections.get(index) {
            let escaped = escape_note(note);
            return match notes_range(&lower, start, end) {
                Some((a, b)) => format!("{}{escaped}{}", &html[..a], &html[b..]),
                None => format!(
                    "{}<aside class=\"notes\">{escaped}</aside>{}",
                    &html[..end],
                    &html[end..]
                ),
            };
        }
    }
    let mut notes = vec![String::new(); count.max(index + 1)];
    notes[index] = note.to_owned();
    let block = format!(
        "<script type=\"application/json\" id=\"speaker-notes\">{}</script>\n",
        script_json(&Value::from(notes))
    );
    match lower.rfind("</body") {
        Some(i) => format!("{}{block}{}", &html[..i], &html[i..]),
        None => format!("{html}\n{block}"),
    }
}

/// JSON that cannot close its `<script>` early.
fn script_json(v: &Value) -> String {
    v.to_string().replace("</", "<\\/")
}

fn escape_note(note: &str) -> String {
    note.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "<br>")
}

/// Byte range of the inner HTML of the first `.notes` element in
/// `start..end` (same offsets in `lower` and the source).
fn notes_range(lower: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let mut from = start;
    while from < end {
        let (open, tag_name) = ["<aside", "<div"]
            .iter()
            .filter_map(|t| lower[from..end].find(t).map(|i| (from + i, *t)))
            .min_by_key(|(i, _)| *i)?;
        let tag_len = lower[open..end].find('>')?;
        let body = open + tag_len + 1;
        if has_class(&lower[open..open + tag_len], "notes") {
            let close = format!("</{}", &tag_name[1..]);
            let stop = lower[body..end].find(&close).map_or(end, |e| body + e);
            return Some((body, stop));
        }
        from = body;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_block_wins() {
        let html = r#"<section class="slide"><aside class="notes">inline</aside></section>
<script type="application/json" id="speaker-notes">["one", null, " three "]</script>"#;
        assert_eq!(extract(html, 4), ["one", "", "three", ""]);
        let obj = r#"<script id='speaker-notes'>{"notes": ["a"]}</script>"#;
        assert_eq!(extract(obj, 1), ["a"]);
    }

    #[test]
    fn inline_notes_per_slide() {
        let html = r#"<body>
<section class="slide active"><h1>A</h1><aside class="notes">Say <b>hi</b>&amp; smile</aside></section>
<section class="slide"><h1>B</h1></section>
<section class="slide"><div class="speaker notes"><p>First</p><p>Second</p></div></section>
</body>"#;
        assert_eq!(extract(html, 3), ["Say hi& smile", "", "First\nSecond"]);
    }

    #[test]
    fn no_notes() {
        assert_eq!(extract("<section class=slide>x</section>", 2), ["", ""]);
        assert!(extract("<p>plain</p>", 0).is_empty());
        let not_slides = r#"<section class="slideshow"><aside class="notes">n</aside></section>"#;
        assert_eq!(
            extract(not_slides, 1),
            ["n"],
            "whole page when no slide sections"
        );
    }

    #[test]
    fn a_note_is_saved_where_the_deck_keeps_notes() {
        // JSON block: same shape back, other notes kept.
        let html = r#"<section class="slide">A</section><script type="application/json" id="speaker-notes">["one", null]</script>"#;
        let out = set_note(html, 1, " two\n", 3);
        assert_eq!(extract(&out, 3), ["one", "two", ""]);
        assert!(out.starts_with(r#"<section class="slide">A</section><script type="application/json" id="speaker-notes">"#));
        let obj = r#"<script id="speaker-notes">{"notes": ["a"], "v": 1}</script>"#;
        let out = set_note(obj, 0, "b </script> c", 1);
        assert_eq!(extract(&out, 1), ["b </script> c"]);
        assert!(
            out.contains(r#""v":1"#) && !out.contains("b </script>"),
            "{out}"
        );

        // Inline notes: replaced in place, or added to the slide without one.
        let inline = r#"<body><section class="slide"><h1>A</h1><aside class="notes">old</aside></section><section class="slide"><h1>B</h1></section></body>"#;
        let out = set_note(inline, 0, "new <b>&", 2);
        assert_eq!(extract(&out, 2), ["new <b>&", ""]);
        assert!(out.contains("new &lt;b&gt;&amp;"), "{out}");
        let out = set_note(inline, 1, "line 1\nline 2", 2);
        assert_eq!(extract(&out, 2), ["old", "line 1\nline 2"]);

        // No notes yet: a JSON block before </body>.
        let bare =
            "<body><section class=\"slide\">A</section><section class=\"slide\">B</section></body>";
        let out = set_note(bare, 1, "hi", 2);
        assert_eq!(extract(&out, 2), ["", "hi"]);
        assert!(out.ends_with("</script>\n</body>"), "{out}");
    }
}
