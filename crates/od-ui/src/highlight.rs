//! Syntax colors for the code view (spec 1.2 Preview/Code toggle): a small
//! hand-written tokenizer for HTML (with its `<style>` and `<script>`),
//! CSS, JavaScript and Markdown. It only colors; it never rejects input, so
//! broken code still shows, just with fewer colors.

/// What a piece of source is, shown as `tok-<kind>` in the code view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Comment,
    Keyword,
    String,
    Number,
    Tag,
    Attr,
    Punct,
    Prop,
    Heading,
    Emphasis,
    Code,
    Link,
    Meta,
}

impl Tok {
    /// CSS class of the span; `None` for plain text.
    #[must_use]
    pub fn class(self) -> Option<&'static str> {
        Some(match self {
            Self::Plain => return None,
            Self::Comment => "tok-comment",
            Self::Keyword => "tok-keyword",
            Self::String => "tok-string",
            Self::Number => "tok-number",
            Self::Tag => "tok-tag",
            Self::Attr => "tok-attr",
            Self::Punct => "tok-punct",
            Self::Prop => "tok-prop",
            Self::Heading => "tok-heading",
            Self::Emphasis => "tok-emphasis",
            Self::Code => "tok-code",
            Self::Link => "tok-link",
            Self::Meta => "tok-meta",
        })
    }
}

/// Source language, from the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Html,
    Css,
    Js,
    Markdown,
    Plain,
}

impl Lang {
    #[must_use]
    pub fn for_path(path: &str) -> Self {
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "html" | "htm" | "svg" | "xml" => Self::Html,
            "css" => Self::Css,
            "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "json" => Self::Js,
            "md" | "markdown" => Self::Markdown,
            _ => Self::Plain,
        }
    }
}

/// Largest source colored; past it the code view shows plain lines.
pub const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;

/// One line of colored source.
pub type Line<'a> = Vec<(Tok, &'a str)>;

/// `src` cut into lines of colored pieces. Joining every line's pieces
/// with `\n` gives `src` back (a final newline ends the last line).
#[must_use]
pub fn highlight(src: &str, lang: Lang) -> Vec<Line<'_>> {
    let mut spans = Vec::new();
    if src.len() > MAX_HIGHLIGHT_BYTES {
        spans.push((Tok::Plain, src));
    } else {
        match lang {
            Lang::Html => html(src, &mut spans),
            Lang::Css => css(src, &mut spans),
            Lang::Js => js(src, &mut spans),
            Lang::Markdown => markdown(src, &mut spans),
            Lang::Plain => spans.push((Tok::Plain, src)),
        }
    }
    into_lines(&spans)
}

fn into_lines<'a>(spans: &[(Tok, &'a str)]) -> Vec<Line<'a>> {
    let mut lines = vec![Vec::new()];
    for &(tok, text) in spans {
        let mut parts = text.split('\n');
        if let Some(first) = parts.next()
            && !first.is_empty()
            && let Some(line) = lines.last_mut()
        {
            line.push((tok, first));
        }
        for part in parts {
            let mut line = Vec::new();
            if !part.is_empty() {
                line.push((tok, part));
            }
            lines.push(line);
        }
    }
    // A trailing newline does not open another (empty) line.
    if lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    lines
}

type Spans<'a> = Vec<(Tok, &'a str)>;

/// Pushes `src[from..to]` as `tok`, skipping empty pieces.
fn push<'a>(out: &mut Spans<'a>, src: &'a str, from: usize, to: usize, tok: Tok) {
    if to > from {
        out.push((tok, &src[from..to]));
    }
}

/// Byte index of the first `needle` at or after `from` (ASCII
/// case-insensitive), or the end.
fn find_ci(src: &str, from: usize, needle: &str) -> usize {
    let hay = src.as_bytes();
    let n = needle.as_bytes();
    (from..hay.len())
        .find(|&i| hay.len() - i >= n.len() && hay[i..i + n.len()].eq_ignore_ascii_case(n))
        .unwrap_or(src.len())
}

fn find(src: &str, from: usize, needle: &str) -> Option<usize> {
    src.get(from..)?.find(needle).map(|i| i + from)
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

fn html<'a>(src: &'a str, out: &mut Spans<'a>) {
    let b = src.as_bytes();
    let mut i = 0;
    let mut text = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if src[i..].starts_with("<!--") {
            push(out, src, text, i, Tok::Plain);
            let end = find(src, i + 4, "-->").map_or(src.len(), |e| e + 3);
            push(out, src, i, end, Tok::Comment);
            i = end;
            text = i;
            continue;
        }
        if src[i..].starts_with("<!") || src[i..].starts_with("<?") {
            push(out, src, text, i, Tok::Plain);
            let end = find(src, i, ">").map_or(src.len(), |e| e + 1);
            push(out, src, i, end, Tok::Meta);
            i = end;
            text = i;
            continue;
        }
        let close = b.get(i + 1) == Some(&b'/');
        let name_at = i + 1 + usize::from(close);
        if !b.get(name_at).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        push(out, src, text, i, Tok::Plain);
        push(out, src, i, name_at, Tok::Punct);
        let mut j = name_at;
        while j < b.len() && is_name_byte(b[j]) {
            j += 1;
        }
        let name = src[name_at..j].to_ascii_lowercase();
        push(out, src, name_at, j, Tok::Tag);
        j = attributes(src, j, out);
        i = j;
        text = i;
        // Raw text: style and script bodies take their own colors.
        if !close && (name == "style" || name == "script") {
            let end = find_ci(src, i, &format!("</{name}"));
            if name == "style" {
                css(&src[i..end], out);
            } else {
                js(&src[i..end], out);
            }
            i = end;
            text = i;
        }
    }
    push(out, src, text, b.len(), Tok::Plain);
}

/// Attributes of a start tag from `at` through its `>`; returns the index
/// after it.
fn attributes<'a>(src: &'a str, at: usize, out: &mut Spans<'a>) -> usize {
    let b = src.as_bytes();
    let mut j = at;
    while j < b.len() {
        match b[j] {
            b'>' => {
                push(out, src, j, j + 1, Tok::Punct);
                return j + 1;
            }
            b'/' | b'=' => {
                push(out, src, j, j + 1, Tok::Punct);
                j += 1;
            }
            q @ (b'"' | b'\'') => {
                let end = src[j + 1..]
                    .find(q as char)
                    .map_or(src.len(), |e| j + 2 + e);
                push(out, src, j, end, Tok::String);
                j = end;
            }
            c if c.is_ascii_whitespace() => {
                let start = j;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                push(out, src, start, j, Tok::Plain);
            }
            _ => {
                let start = j;
                let unquoted = start > 0 && b[start - 1] == b'=';
                while j < b.len() && !b[j].is_ascii_whitespace() && !matches!(b[j], b'>' | b'=') {
                    if !unquoted && matches!(b[j], b'"' | b'\'' | b'/') {
                        break;
                    }
                    j += 1;
                }
                if j == start {
                    j += 1;
                }
                let tok = if unquoted { Tok::String } else { Tok::Attr };
                push(out, src, start, j, tok);
            }
        }
    }
    j
}

/// A quoted string from `at` (on its quote) through the closing quote,
/// honoring backslash escapes; returns the index after it.
fn quoted(b: &[u8], at: usize) -> usize {
    let q = b[at];
    let mut j = at + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            // An unclosed ' or " string ends with its line.
            b'\n' if q != b'`' => return j,
            _ => j += 1,
        }
    }
    b.len()
}

fn number_end(b: &[u8], at: usize) -> usize {
    let mut j = at;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'.' | b'%' | b'_')) {
        j += 1;
    }
    j
}

fn css<'a>(src: &'a str, out: &mut Spans<'a>) {
    let b = src.as_bytes();
    let mut i = 0;
    let mut text = 0;
    // Inside a declaration value (after its `:`, up to `;` or `}`).
    let mut value = false;
    // A colon that starts a value, not one inside a selector (`a:hover {`).
    let declares = |at: usize| next_of(src, at, b"{;}") != Some(b'{');
    while i < b.len() {
        let c = b[i];
        let starts_number = c.is_ascii_digit()
            || c == b'#'
            || (matches!(c, b'.' | b'-') && b.get(i + 1).is_some_and(u8::is_ascii_digit));
        let end = if src[i..].starts_with("/*") {
            Some((
                find(src, i + 2, "*/").map_or(src.len(), |e| e + 2),
                Tok::Comment,
            ))
        } else if c == b'"' || c == b'\'' {
            Some((quoted(b, i), Tok::String))
        } else if c == b'@' {
            let mut e = i + 1;
            while e < b.len() && is_name_byte(b[e]) {
                e += 1;
            }
            Some((e, Tok::Keyword))
        } else if matches!(c, b'{' | b'}' | b';') {
            value = false;
            Some((i + 1, Tok::Punct))
        } else if c == b':' && !value && declares(i) {
            value = true;
            Some((i + 1, Tok::Punct))
        } else if c == b',' {
            Some((i + 1, Tok::Punct))
        } else if value && starts_number {
            Some((number_end(b, i + 1), Tok::Number))
        } else if !value
            && (c.is_ascii_alphabetic() || matches!(c, b'-' | b'.' | b'#' | b'*' | b'['))
        {
            let mut e = i;
            while e < b.len()
                && !matches!(b[e], b'{' | b'}' | b';' | b'/' | b'"' | b'\'')
                && !(b[e] == b':' && declares(e))
            {
                e += 1;
            }
            let tok = if declares(i) { Tok::Prop } else { Tok::Tag };
            Some((e.max(i + 1), tok))
        } else {
            None
        };
        match end {
            Some((e, tok)) => {
                push(out, src, text, i, Tok::Plain);
                push(out, src, i, e, tok);
                i = e;
                text = i;
            }
            None => i += 1,
        }
    }
    push(out, src, text, b.len(), Tok::Plain);
}

/// The first of `set` at or after `at`.
fn next_of(src: &str, at: usize, set: &[u8]) -> Option<u8> {
    src.as_bytes()[at..]
        .iter()
        .copied()
        .find(|c| set.contains(c))
}

const JS_KEYWORDS: [&str; 44] = [
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

fn js<'a>(src: &'a str, out: &mut Spans<'a>) {
    let b = src.as_bytes();
    let mut i = 0;
    let mut text = 0;
    while i < b.len() {
        let c = b[i];
        let end = if src[i..].starts_with("//") {
            Some((find(src, i, "\n").unwrap_or(src.len()), Tok::Comment))
        } else if src[i..].starts_with("/*") {
            Some((
                find(src, i + 2, "*/").map_or(src.len(), |e| e + 2),
                Tok::Comment,
            ))
        } else if matches!(c, b'"' | b'\'' | b'`') {
            Some((quoted(b, i), Tok::String))
        } else if c.is_ascii_digit() && (i == 0 || !is_ident_byte(b[i - 1])) {
            Some((number_end(b, i), Tok::Number))
        } else if is_ident_start(c) && (i == 0 || !is_ident_byte(b[i - 1])) {
            let mut e = i;
            while e < b.len() && is_ident_byte(b[e]) {
                e += 1;
            }
            let word = &src[i..e];
            if JS_KEYWORDS.contains(&word) {
                Some((e, Tok::Keyword))
            } else if src[e..].trim_start().starts_with(':') && json_key(src, i) {
                Some((e, Tok::Prop))
            } else {
                push(out, src, text, e, Tok::Plain);
                i = e;
                text = i;
                None
            }
        } else {
            i += 1;
            continue;
        };
        if let Some((e, tok)) = end {
            push(out, src, text, i, Tok::Plain);
            // A string key in an object or JSON reads as a property.
            let tok =
                if tok == Tok::String && src[e..].trim_start().starts_with(':') && json_key(src, i)
                {
                    Tok::Prop
                } else {
                    tok
                };
            push(out, src, i, e, tok);
            i = e;
            text = i;
        }
    }
    push(out, src, text, b.len(), Tok::Plain);
}

/// Whether the word at `at` follows `{` or `,` (an object key, not a
/// ternary or label).
fn json_key(src: &str, at: usize) -> bool {
    src[..at].trim_end().ends_with(['{', ','])
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

fn markdown<'a>(src: &'a str, out: &mut Spans<'a>) {
    let mut fence = false;
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let rest = &line[body.len()..];
        let trimmed = body.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
            out.push((Tok::Code, body));
        } else if fence {
            out.push((Tok::Code, body));
        } else if trimmed.starts_with('#') {
            out.push((Tok::Heading, body));
        } else if trimmed.starts_with('>') {
            out.push((Tok::Comment, body));
        } else {
            let lead = body.len() - trimmed.len();
            let marker = list_marker(trimmed);
            push(out, line, 0, lead, Tok::Plain);
            push(out, line, lead, lead + marker, Tok::Punct);
            inline_md(&body[lead + marker..], out);
        }
        if !rest.is_empty() {
            out.push((Tok::Plain, rest));
        }
        at += line.len();
    }
    debug_assert_eq!(at, src.len());
}

/// Length of a list marker (`- `, `* `, `+ `, `1. `) at the line start.
fn list_marker(s: &str) -> usize {
    let b = s.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return 2;
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && b.get(digits) == Some(&b'.') && b.get(digits + 1) == Some(&b' ') {
        return digits + 2;
    }
    0
}

fn inline_md<'a>(s: &'a str, out: &mut Spans<'a>) {
    let b = s.as_bytes();
    let mut i = 0;
    let mut text = 0;
    while i < b.len() {
        let end = match b[i] {
            b'`' => find(s, i + 1, "`").map(|e| (e + 1, Tok::Code)),
            b'*' | b'_' => {
                let double = b.get(i + 1) == Some(&b[i]);
                let mark = &s[i..i + 1 + usize::from(double)];
                find(s, i + mark.len() + 1, mark).map(|e| (e + mark.len(), Tok::Emphasis))
            }
            b'[' => find(s, i, "](")
                .and_then(|m| find(s, m, ")"))
                .map(|e| (e + 1, Tok::Link)),
            _ => None,
        };
        match end {
            Some((e, tok)) => {
                push(out, s, text, i, Tok::Plain);
                push(out, s, i, e, tok);
                i = e;
                text = i;
            }
            None => i += 1,
        }
    }
    push(out, s, text, b.len(), Tok::Plain);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| l.iter().map(|(_, s)| *s).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn of(lines: &[Line<'_>], tok: Tok) -> Vec<String> {
        lines
            .iter()
            .flatten()
            .filter(|(t, _)| *t == tok)
            .map(|(_, s)| (*s).to_owned())
            .collect()
    }

    #[test]
    fn every_language_gives_the_source_back() {
        let cases = [
            (
                "a.html",
                "<!doctype html>\n<p class=\"x\" hidden>Hi &amp; <b>you</b></p>\n<style>a{color:red}</style><script>let x = '<p>';</script>\n",
            ),
            (
                "a.css",
                "@media (max-width: 600px) {\n  a:hover, .b { color: #fff; margin: 0 4px }\n}\n/* end",
            ),
            (
                "a.js",
                "const s = `multi\nline`; // done\nif (x > 1) { return {a: 'b'}; }",
            ),
            (
                "a.md",
                "# Title\n\n- item with `code` and **bold**\n```\nfenced\n```\n> quote [link](u)",
            ),
            ("a.txt", "plain\ntext\n"),
            ("broken.html", "<div class=\"open\n<p unclosed"),
        ];
        for (path, src) in cases {
            let lines = highlight(src, Lang::for_path(path));
            let expect = src.strip_suffix('\n').unwrap_or(src);
            assert_eq!(joined(&lines), expect, "{path}");
            assert_eq!(lines.len(), expect.split('\n').count(), "{path}");
        }
    }

    #[test]
    fn html_colors_tags_attributes_and_embedded_code() {
        let src = "<!-- c --><a href=\"/x\" data-id=7>Go</a><style>p { color: #0a7; }</style><script>if (a) {}</script>";
        let lines = highlight(src, Lang::Html);
        assert_eq!(of(&lines, Tok::Comment), ["<!-- c -->"]);
        assert_eq!(
            of(&lines, Tok::Tag),
            ["a", "a", "style", "p ", "style", "script", "script"]
        );
        assert_eq!(of(&lines, Tok::Attr), ["href", "data-id"]);
        assert_eq!(of(&lines, Tok::String), ["\"/x\"", "7"]);
        assert_eq!(of(&lines, Tok::Prop), ["color"]);
        assert_eq!(of(&lines, Tok::Number), ["#0a7"]);
        assert_eq!(of(&lines, Tok::Keyword), ["if"]);
        // Text between tags stays plain.
        assert!(of(&lines, Tok::Plain).contains(&"Go".to_owned()));
    }

    #[test]
    fn css_tells_selectors_from_properties() {
        let lines = highlight("a:hover { color: red; width: 10px }", Lang::Css);
        assert_eq!(of(&lines, Tok::Tag), ["a:hover "]);
        assert_eq!(of(&lines, Tok::Prop), ["color", "width"]);
        assert_eq!(of(&lines, Tok::Number), ["10px"]);
    }

    #[test]
    fn js_keywords_strings_and_comments() {
        let lines = highlight("const a = \"x\"; // note\nlet n = 42;", Lang::Js);
        assert_eq!(of(&lines, Tok::Keyword), ["const", "let"]);
        assert_eq!(of(&lines, Tok::String), ["\"x\""]);
        assert_eq!(of(&lines, Tok::Comment), ["// note"]);
        assert_eq!(of(&lines, Tok::Number), ["42"]);
        // Identifiers that contain a keyword are not keywords.
        assert!(of(&highlight("letter = format", Lang::Js), Tok::Keyword).is_empty());
        let json = highlight("{\"name\": \"Nét\", \"n\": 1}", Lang::Js);
        assert_eq!(of(&json, Tok::Prop), ["\"name\"", "\"n\""]);
    }

    #[test]
    fn markdown_headings_fences_and_inline() {
        let lines = highlight("# H\n```js\nlet x\n```\n1. `c` *e* [l](u)", Lang::Markdown);
        assert_eq!(of(&lines, Tok::Heading), ["# H"]);
        assert_eq!(of(&lines, Tok::Code), ["```js", "let x", "```", "`c`"]);
        assert_eq!(of(&lines, Tok::Emphasis), ["*e*"]);
        assert_eq!(of(&lines, Tok::Link), ["[l](u)"]);
        assert_eq!(of(&lines, Tok::Punct), ["1. "]);
    }

    #[test]
    fn huge_sources_stay_plain() {
        let big = "<p>x</p>\n".repeat(MAX_HIGHLIGHT_BYTES / 9 + 1);
        let lines = highlight(&big, Lang::Html);
        assert!(lines.iter().flatten().all(|(t, _)| *t == Tok::Plain));
        assert_eq!(Lang::for_path("index.HTML"), Lang::Html);
        assert_eq!(Lang::for_path("README"), Lang::Plain);
    }
}
