//! `html_template_v1` (spec 4.2): `{{data.x}}` scalar bindings and one
//! level of `data-od-repeat="item in data.items"`, rendered without a
//! browser. Templates with scripts, iframes, event handlers or
//! `javascript:` URLs are refused, so the generated preview cannot run
//! code. Every substituted value is HTML-escaped and rendered in one pass:
//! data that looks like `{{…}}` stays inert.

use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::sync::LazyLock;

pub const RENDER_FORMAT: &str = "html_template_v1";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct RenderError(pub String);

type Result<T> = std::result::Result<T, RenderError>;

fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(RenderError(message.into()))
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static regex")
}

static INTERPOLATION: LazyLock<Regex> = LazyLock::new(|| re(r"\{\{\s*([^{}]+?)\s*\}\}"));
static RAW_INTERPOLATION: LazyLock<Regex> =
    LazyLock::new(|| re(r"\{\{\{[^{}]*\}\}\}|\{\{\s*&[^{}]*\}\}"));
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(?:data|[A-Za-z_][A-Za-z0-9_]*)(?:\.(?:[A-Za-z_][A-Za-z0-9_-]*|\d+))*$")
});
static REPEAT: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)\s*\bdata-od-repeat\s*=\s*"([^"]*)""#));
static REPEAT_SPEC: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s+in\s+(data(?:\.(?:[A-Za-z_][A-Za-z0-9_-]*|\d+))*)\s*$")
});
static TAG_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"^<([A-Za-z][A-Za-z0-9_-]*)"));

/// Patterns that would let the preview run code, with the refusal message.
static EXECUTABLE: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            re(r"(?i)<\s*script\b"),
            "script elements are not supported in live artifact previews",
        ),
        (
            re(r"(?i)<\s*iframe\b"),
            "iframe elements are not supported in live artifact previews",
        ),
        (
            re(r"(?i)\bsrcdoc\s*="),
            "srcdoc attributes are not supported in live artifact previews",
        ),
        (
            re(r#"(?i)[\s/"'`]on[a-z][a-z0-9_-]*\s*="#),
            "event handler attributes are not supported in live artifact previews",
        ),
        (
            re(r#"(?i)(?:href|src|action|formaction)\s*=\s*['"]?\s*javascript\s*:"#),
            "javascript: URLs are not supported in live artifact previews",
        ),
        (
            re(r"(?i)\bdata-od-(?:html|raw|bind-html)\b"),
            "raw HTML insertion directives are not supported",
        ),
    ]
});

/// Refuses templates that could execute code in the preview. The rules run
/// on the template and on a copy with character references decoded and
/// tab/newline removed (`&#106;avascript:`, `java\tscript:`), as browsers
/// read URLs. This is a second layer: the preview's CSP forbids scripts.
///
/// # Errors
/// The first matching rule's message.
pub fn validate_security(template: &str) -> Result<()> {
    let decoded = decode_for_check(template);
    match EXECUTABLE
        .iter()
        .find(|(p, _)| p.is_match(template) || p.is_match(&decoded))
    {
        Some((_, message)) => err(*message),
        None => Ok(()),
    }
}

static CHAR_REF: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)&#x([0-9a-f]{1,6});?|&#([0-9]{1,7});?|&(colon|tab|newline);"));

/// Decodes numeric and the URL-relevant named character references, then
/// drops tab, CR and LF.
fn decode_for_check(text: &str) -> String {
    let decoded = CHAR_REF.replace_all(text, |c: &Captures<'_>| {
        let code = match (c.get(1), c.get(2), c.get(3)) {
            (Some(hex), _, _) => u32::from_str_radix(hex.as_str(), 16).ok(),
            (_, Some(dec), _) => dec.as_str().parse().ok(),
            (_, _, Some(name)) => match name.as_str().to_ascii_lowercase().as_str() {
                "colon" => Some(u32::from(b':')),
                "tab" => Some(u32::from(b'\t')),
                _ => Some(u32::from(b'\n')),
            },
            _ => None,
        };
        code.and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_default()
    });
    decoded
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// Where a binding sits in the template.
enum BindingContext {
    Text,
    /// Inside an open tag, in the value of attribute `name`.
    Attribute {
        name: String,
        quoted: bool,
    },
}

static ATTR_BEFORE_VALUE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"([A-Za-z_:][-A-Za-z0-9_:.]*)\s*=\s*["']?[^"'\s>]*$"#));

/// Classifies the binding that starts right after `before`.
fn binding_context(before: &str) -> BindingContext {
    let Some(open) = before.rfind('<') else {
        return BindingContext::Text;
    };
    let head = &before[open..];
    let mut quote = None;
    let mut quote_start = 0;
    for (i, b) in head.bytes().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => {
                quote = Some(b);
                quote_start = i;
            }
            None if b == b'>' => return BindingContext::Text,
            None => {}
        }
    }
    let upto = if quote.is_some() {
        &head[..=quote_start]
    } else {
        head
    };
    match ATTR_BEFORE_VALUE.captures(upto) {
        Some(c) => BindingContext::Attribute {
            name: c[1].to_ascii_lowercase(),
            quoted: quote.is_some(),
        },
        None => BindingContext::Text,
    }
}

const URL_ATTRIBUTES: [&str; 7] = [
    "href",
    "src",
    "action",
    "formaction",
    "xlink:href",
    "poster",
    "background",
];

/// A value that turns a URL attribute into code or an HTML document.
fn is_script_url(value: &str) -> bool {
    let v: String = decode_for_check(value)
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    v.starts_with("javascript:") || v.starts_with("vbscript:") || v.starts_with("data:text/html")
}

/// Unquoted attribute values end at whitespace; keep data inside them.
fn escape_unquoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            ' ' => out.push_str("&#32;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\x0c' => out.push_str("&#12;"),
            '=' => out.push_str("&#61;"),
            '`' => out.push_str("&#96;"),
            c => out.push(c),
        }
    }
    out
}

#[must_use]
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// JS `String(value)` for scalars; `null` and missing render empty.
fn scalar_text(value: Option<&Value>, binding: &str) -> Result<String> {
    Ok(match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => match n.as_f64() {
            #[allow(clippy::cast_possible_truncation)] // integral and in range
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => (f as i64).to_string(),
            _ => n.to_string(),
        },
        Some(Value::Array(_) | Value::Object(_)) => {
            return err(format!(
                "template binding must resolve to a scalar: {binding}"
            ));
        }
    })
}

/// Walks `segments` from `root`; `None` is JS `undefined`.
fn walk<'a>(root: Option<&'a Value>, segments: &[&str], raw: &str) -> Result<Option<&'a Value>> {
    let mut current = root;
    for segment in segments {
        current = match current {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::Array(items)) => {
                if !segment.bytes().all(|b| b.is_ascii_digit()) {
                    return err(format!(
                        "invalid array segment in template binding path: {raw}"
                    ));
                }
                segment.parse::<usize>().ok().and_then(|i| items.get(i))
            }
            Some(Value::Object(map)) => map.get(*segment),
            Some(_) => return Ok(None),
        };
    }
    Ok(current)
}

/// Resolves a binding to its escaped text. Loop scopes delegate other
/// heads (including `data.*`) to their parent.
enum Scope<'a> {
    Root(&'a Value),
    Item {
        parent: &'a Scope<'a>,
        name: &'a str,
        item: &'a Value,
    },
}

impl Scope<'_> {
    fn resolve(&self, binding: &str) -> Result<String> {
        if !PATH.is_match(binding) {
            return err(format!("invalid template binding path: {binding}"));
        }
        let segments: Vec<&str> = binding.split('.').collect();
        match self {
            Scope::Item { parent, name, item } => {
                if segments[0] == *name {
                    let v = walk(Some(item), &segments[1..], binding)?;
                    Ok(escape(&scalar_text(v, binding)?))
                } else {
                    parent.resolve(binding)
                }
            }
            Scope::Root(data) => {
                if segments[0] != "data" {
                    return err(format!("unsupported template binding path: {binding}"));
                }
                let v = walk(Some(data), &segments[1..], binding)?;
                Ok(escape(&scalar_text(v, binding)?))
            }
        }
    }
}

fn read_array<'a>(data: &'a Value, path: &str) -> Result<&'a [Value]> {
    let segments: Vec<&str> = path.split('.').collect();
    match walk(Some(data), &segments[1..], path)? {
        Some(Value::Array(items)) => Ok(items),
        _ => err(format!("data-od-repeat source is not an array: {path}")),
    }
}

fn interpolate(fragment: &str, scope: &Scope<'_>) -> Result<String> {
    let mut out = String::with_capacity(fragment.len());
    let mut last = 0;
    for caps in INTERPOLATION.captures_iter(fragment) {
        let whole = caps.get(0).expect("group 0");
        out.push_str(&fragment[last..whole.start()]);
        let value = scope.resolve(caps[1].trim())?;
        match binding_context(&fragment[..whole.start()]) {
            BindingContext::Text => out.push_str(&value),
            BindingContext::Attribute { name, quoted } => {
                if URL_ATTRIBUTES.contains(&name.as_str()) && is_script_url(&value) {
                    return err(format!(
                        "template binding {} would make a script URL in {name}",
                        caps[1].trim()
                    ));
                }
                if quoted {
                    out.push_str(&value);
                } else {
                    out.push_str(&escape_unquoted(&value));
                }
            }
        }
        last = whole.end();
    }
    out.push_str(&fragment[last..]);
    Ok(out)
}

/// Index of the `>` closing the tag opened at `start`, respecting quotes.
fn tag_end(html: &str, start: usize) -> Result<usize> {
    let mut quote = None;
    for (i, b) in html.bytes().enumerate().skip(start) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Ok(i),
            None => {}
        }
    }
    err("unterminated tag in live artifact template")
}

/// When `index` is inside `<!-- … -->`, the position just past it.
fn inside_comment(html: &str, index: usize) -> Option<usize> {
    let search_end = (index + 4).min(html.len());
    let start = html.get(..search_end)?.rfind("<!--")?;
    match html[start..].find("-->").map(|e| start + e) {
        Some(end) if end < index => None,
        Some(end) => Some(end + 3),
        None => Some(html.len()),
    }
}

/// Index just past the `</tag>` matching the element whose open tag ends
/// before `from`.
fn element_end(html: &str, tag: &str, from: usize) -> Result<usize> {
    let pattern = re(&format!(r"(?i)<(/?){}[\s/>]", regex::escape(tag)));
    let mut depth = 1usize;
    let mut pos = from;
    while let Some(caps) = pattern.captures_at(html, pos) {
        let m = caps.get(0).expect("group 0");
        if let Some(resume) = inside_comment(html, m.start()) {
            pos = resume.max(m.start() + 1);
            continue;
        }
        let end = tag_end(html, m.start())?;
        if &caps[1] == "/" {
            depth -= 1;
            if depth == 0 {
                return Ok(end + 1);
            }
        } else if html.as_bytes()[end - 1] != b'/' {
            depth += 1;
        }
        pos = end + 1;
    }
    err(format!("unbalanced data-od-repeat element <{tag}>"))
}

struct Directive {
    open_tag_start: usize,
    tag: String,
    spec: String,
}

/// The next real `data-od-repeat` at or after `from`: inside an open tag,
/// not in a comment, not inside another attribute's quoted value.
fn find_directive(html: &str, from: usize) -> Option<Directive> {
    let mut cursor = from;
    while cursor < html.len() {
        let caps = REPEAT.captures(&html[cursor..])?;
        let m = caps.get(0).expect("group 0");
        let index = cursor + m.start();
        let match_end = cursor + m.end();
        let open = html[..index].rfind('<');
        let tag = open.and_then(|o| TAG_NAME.captures(&html[o..]).map(|c| c[1].to_owned()));
        if let (Some(open), Some(tag)) = (open, tag) {
            let head = &html[open..index];
            let mut quote = None;
            for b in head.bytes() {
                match quote {
                    Some(q) if b == q => quote = None,
                    Some(_) => {}
                    None if b == b'"' || b == b'\'' => quote = Some(b),
                    None => {}
                }
            }
            if inside_comment(html, index).is_none() && quote.is_none() && !head.contains('>') {
                return Some(Directive {
                    open_tag_start: open,
                    tag,
                    spec: caps[1].to_owned(),
                });
            }
        }
        cursor = match_end;
    }
    None
}

fn render_fragment(html: &str, scope: &Scope<'_>, data: &Value) -> Result<String> {
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    while cursor < html.len() {
        let Some(directive) = find_directive(html, cursor) else {
            out.push_str(&interpolate(&html[cursor..], scope)?);
            break;
        };
        let spec: Captures<'_> = REPEAT_SPEC.captures(&directive.spec).ok_or_else(|| {
            RenderError(format!(
                "invalid data-od-repeat directive: \"{}\" (expected \"item in data.path\")",
                directive.spec
            ))
        })?;
        let open_end = tag_end(html, directive.open_tag_start)?;
        let self_closed = html.as_bytes()[open_end - 1] == b'/';
        let end = if self_closed {
            open_end + 1
        } else {
            element_end(html, &directive.tag, open_end + 1)?
        };
        let element = &html[directive.open_tag_start..end];
        let item_template = REPEAT.replacen(element, 1, "");
        if find_directive(&item_template, 0).is_some() {
            return err("nested data-od-repeat is not supported");
        }
        out.push_str(&interpolate(
            &html[cursor..directive.open_tag_start],
            scope,
        )?);
        for item in read_array(data, &spec[2])? {
            let child = Scope::Item {
                parent: scope,
                name: spec.get(1).map_or("", |m| m.as_str()),
                item,
            };
            out.push_str(&render_fragment(&item_template, &child, data)?);
        }
        cursor = end;
    }
    Ok(out)
}

/// Renders `template` against `data`.
///
/// # Errors
/// Unsafe templates (see [`validate_security`]), raw interpolation,
/// invalid bindings or repeat directives, non-scalar bindings.
pub fn render(template: &str, data: &Map<String, Value>) -> Result<String> {
    validate_security(template)?;
    if RAW_INTERPOLATION.is_match(template) {
        return err("raw template interpolation is not supported");
    }
    let data = Value::Object(data.clone());
    render_fragment(template, &Scope::Root(&data), &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn scalars_are_escaped_and_inert() {
        let d = data(
            json!({ "title": "<b>&\"x'</b>", "n": 3, "f": 2.0, "z": null, "t": "{{data.n}}" }),
        );
        let html = render(
            "<h1>{{ data.title }}</h1>{{data.n}}/{{data.f}}/{{data.z}}/{{data.missing}}/{{data.t}}",
            &d,
        )
        .unwrap();
        assert_eq!(
            html,
            "<h1>&lt;b&gt;&amp;&quot;x&#39;&lt;/b&gt;</h1>3/2///{{data.n}}"
        );
    }

    #[test]
    fn repeat_expands_one_level() {
        let d = data(
            json!({ "head": "H", "items": [{ "label": "a", "v": 1 }, { "label": "b", "v": 2 }] }),
        );
        let tpl = r#"<ul><!-- <li data-od-repeat="x in data.nope"> --><li class="r" data-od-repeat="item in data.items" title="{{data.head}}">{{item.label}}={{item.v}}<span>.</span></li></ul><p title='data-od-repeat="q in data.items"'>{{data.head}}</p>"#;
        let html = render(tpl, &d).unwrap();
        assert_eq!(
            html,
            r#"<ul><!-- <li data-od-repeat="x in data.nope"> --><li class="r" title="H">a=1<span>.</span></li><li class="r" title="H">b=2<span>.</span></li></ul><p title='data-od-repeat="q in data.items"'>H</p>"#
        );
    }

    #[test]
    fn executable_templates_are_refused() {
        let d = Map::new();
        for (tpl, msg) in [
            ("<div><script>alert(1)</script></div>", "script elements"),
            ("<SCRIPT src=x></SCRIPT>", "script elements"),
            ("<iframe src=x></iframe>", "iframe elements"),
            (r#"<div srcdoc="x"></div>"#, "srcdoc"),
            (r#"<img src=x onerror="alert(1)">"#, "event handler"),
            (r#"<a href="javascript:alert(1)">x</a>"#, "javascript:"),
            (r#"<div data-od-html="data.x"></div>"#, "raw HTML"),
        ] {
            let e = render(tpl, &d).unwrap_err();
            assert!(e.0.contains(msg), "{tpl}: {e}");
        }
    }

    #[test]
    fn filter_bypasses_from_review_are_refused() {
        let d = Map::new();
        for tpl in [
            r#"<img src="x"/onerror=alert(1)>"#,
            r#"<img src=x"onerror=alert(1)>"#,
            r#"<a href="&#106;avascript:alert(1)">x</a>"#,
            r#"<a href="&#x6A;avascript&colon;alert(1)">x</a>"#,
            "<a href=\"java\tscript:alert(1)\">x</a>",
        ] {
            assert!(render(tpl, &d).is_err(), "{tpl}");
        }
    }

    #[test]
    fn data_cannot_become_script_urls_or_attributes() {
        for url in [
            "javascript:alert(1)",
            " JavaScript:alert(1)",
            "data:text/html,<b>",
        ] {
            let d = data(json!({ "u": url }));
            assert!(
                render(r#"<a href="{{data.u}}">x</a>"#, &d).is_err(),
                "{url}"
            );
            assert!(render("<img src={{data.u}}>", &d).is_err(), "{url}");
        }
        let d = data(json!({ "u": "https://example.com/a", "c": "x onmouseover=alert(1)" }));
        assert_eq!(
            render(r#"<a href="{{data.u}}">{{data.c}}</a>"#, &d).unwrap(),
            r#"<a href="https://example.com/a">x onmouseover=alert(1)</a>"#,
            "text and safe URLs are untouched"
        );
        let html = render("<p class={{data.c}}>t</p>", &d).unwrap();
        assert_eq!(html, "<p class=x&#32;onmouseover&#61;alert(1)>t</p>");
    }

    #[test]
    fn invalid_bindings_fail() {
        let d = data(json!({ "obj": { "a": 1 }, "items": [1] }));
        for (tpl, msg) in [
            ("{{{data.x}}}", "raw template"),
            ("{{data.obj}}", "scalar"),
            ("{{other.x}}", "unsupported"),
            ("{{data.items.x}}", "array segment"),
            (
                r#"<li data-od-repeat="i in data.obj">x</li>"#,
                "not an array",
            ),
            (
                r#"<li data-od-repeat="bad spec">x</li>"#,
                "invalid data-od-repeat",
            ),
            (
                r#"<ul data-od-repeat="i in data.items"><li data-od-repeat="j in data.items"></li></ul>"#,
                "nested",
            ),
            (r#"<li data-od-repeat="i in data.items">"#, "unbalanced"),
        ] {
            let e = render(tpl, &d).unwrap_err();
            assert!(e.0.contains(msg), "{tpl}: {e}");
        }
    }
}
