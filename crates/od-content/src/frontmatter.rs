//! Front-matter parser: a line-for-line port of the original app's YAML
//! subset (`design-systems/frontmatter.ts`).
//!
//! Supported: scalars, block scalars (`|`, `|-`, `>`, `>-`, all kept
//! literal), flat and nested block sequences, nested maps by indentation,
//! inline `[a, b]` arrays. Not supported: flow maps, anchors, multi-docs.
//! A full YAML parser would read some bundled files differently, so the
//! subset is kept on purpose (spec 3.4).

use regex::Regex;
use serde_json::{Map, Number, Value};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrontmatterError {
    #[error("invalid frontmatter array nesting")]
    ArrayNesting,
    #[error("frontmatter object container expected")]
    ObjectExpected,
}

static DOC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\A---\r?\n(.*?)\r?\n---\r?\n?(.*)\z").expect("static regex"));
static KV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\A([^:]+):\s*(.*)\z").expect("static regex"));
static INT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A-?[0-9]+\z").expect("static regex"));
static FLOAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A-?[0-9]*\.[0-9]+\z").expect("static regex"));

/// Splits `src` into front-matter data and body. Text without a leading
/// `---` block is all body.
///
/// # Errors
/// Structural errors the original parser also throws (the caller skips the
/// file, as the original does).
pub fn parse(src: &str) -> Result<(Map<String, Value>, &str), FrontmatterError> {
    let text = src.strip_prefix('\u{feff}').unwrap_or(src);
    let Some(caps) = DOC_RE.captures(text) else {
        return Ok((Map::new(), text));
    };
    let yaml = caps.get(1).map_or("", |m| m.as_str());
    let body = caps.get(2).map_or("", |m| m.as_str());
    Ok((parse_yaml_subset(yaml)?, body))
}

#[derive(Debug, Clone)]
enum Seg {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone)]
struct Frame {
    indent: isize,
    /// Address of this frame's container from the root.
    path: Vec<Seg>,
    key: Option<String>,
}

fn at<'a>(root: &'a mut Value, path: &[Seg]) -> &'a mut Value {
    let mut cur = root;
    for seg in path {
        // Paths are only built from containers this parser just inserted.
        cur = match (seg, cur) {
            (Seg::Key(k), Value::Object(map)) => map.entry(k.clone()).or_insert(Value::Null),
            (Seg::Index(i), Value::Array(items)) => match items.get_mut(*i) {
                Some(v) => v,
                None => unreachable_container(),
            },
            _ => unreachable_container(),
        };
    }
    cur
}

#[cold]
fn unreachable_container() -> ! {
    unreachable!("frontmatter path points at a container the parser created")
}

fn leading_ws(s: &str) -> (usize, usize) {
    let mut chars = 0;
    let mut bytes = 0;
    for ch in s.chars() {
        if !ch.is_whitespace() {
            break;
        }
        chars += 1;
        bytes += ch.len_utf8();
    }
    (chars, bytes)
}

fn is_blank_or_comment(line: &str) -> bool {
    let rest = line.trim_start();
    rest.is_empty() || rest.starts_with('#')
}

fn is_empty_container(v: &Value) -> bool {
    match v {
        Value::Array(_) => true,
        Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

fn object_mut(v: &mut Value) -> Result<&mut Map<String, Value>, FrontmatterError> {
    v.as_object_mut().ok_or(FrontmatterError::ObjectExpected)
}

#[allow(clippy::too_many_lines)]
fn parse_yaml_subset(src: &str) -> Result<Map<String, Value>, FrontmatterError> {
    let lines: Vec<&str> = src
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut root = Value::Object(Map::new());
    let mut stack = vec![Frame {
        indent: -1,
        path: Vec::new(),
        key: None,
    }];
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        if is_blank_or_comment(raw) {
            i += 1;
            continue;
        }
        let (indent_chars, indent_bytes) = leading_ws(raw);
        let indent = isize::try_from(indent_chars).unwrap_or(isize::MAX);
        let line = &raw[indent_bytes..];
        let is_seq_item = line.starts_with("- ");

        while stack.len() > 1 {
            let top = &stack[stack.len() - 1];
            if indent > top.indent {
                break;
            }
            if is_seq_item
                && indent == top.indent
                && top.key.is_some()
                && is_empty_container(at(&mut root, &top.path))
            {
                break;
            }
            stack.pop();
        }

        if let Some(rest) = line.strip_prefix("- ") {
            let value = rest.trim();
            let top_index = stack.len() - 1;
            if !at(&mut root, &stack[top_index].path).is_array() {
                let (Some(key), true) = (stack[top_index].key.clone(), top_index >= 1) else {
                    i += 1;
                    continue;
                };
                let parent_path = stack[top_index - 1].path.clone();
                let parent = at(&mut root, &parent_path);
                if parent.is_array() {
                    return Err(FrontmatterError::ArrayNesting);
                }
                object_mut(parent)?.insert(key, Value::Array(Vec::new()));
            }
            let top_path = stack[top_index].path.clone();
            let Value::Array(items) = at(&mut root, &top_path) else {
                return Err(FrontmatterError::ArrayNesting);
            };
            if let Some(colon) = value.find(':') {
                let mut obj = Map::new();
                let key = value[..colon].trim().to_owned();
                let val_raw = value[colon + 1..].trim();
                if !val_raw.is_empty() {
                    obj.insert(key, coerce(val_raw));
                }
                items.push(Value::Object(obj));
                let mut path = top_path;
                path.push(Seg::Index(items.len() - 1));
                stack.push(Frame {
                    indent,
                    path,
                    key: None,
                });
            } else {
                items.push(coerce(value));
            }
            i += 1;
            continue;
        }

        let Some(kv) = KV_RE.captures(line) else {
            i += 1;
            continue;
        };
        let key = kv.get(1).map_or("", |m| m.as_str()).trim().to_owned();
        let val = kv.get(2).map_or("", |m| m.as_str());
        let top_path = stack[stack.len() - 1].path.clone();

        if val.is_empty() {
            object_mut(at(&mut root, &top_path))?.insert(key.clone(), Value::Object(Map::new()));
            let mut path = top_path;
            path.push(Seg::Key(key.clone()));
            stack.push(Frame {
                indent,
                path,
                key: Some(key),
            });
            i += 1;
            continue;
        }

        if matches!(val, "|" | "|-" | ">" | ">-") {
            let mut collected: Vec<String> = Vec::new();
            let mut block_indent: Option<usize> = None;
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if next.trim().is_empty() {
                    collected.push(String::new());
                    i += 1;
                    continue;
                }
                let (n_indent, _) = leading_ws(next);
                if isize::try_from(n_indent).unwrap_or(isize::MAX) <= indent {
                    break;
                }
                let base = *block_indent.get_or_insert(n_indent);
                if n_indent < base {
                    break;
                }
                collected.push(next.chars().skip(base).collect());
                i += 1;
            }
            let text = collected.join("\n");
            object_mut(at(&mut root, &top_path))?
                .insert(key, Value::String(text.trim_end().to_owned()));
            continue;
        }

        let value = if val == "[]" {
            Value::Array(Vec::new())
        } else if val.len() >= 2 && val.starts_with('[') && val.ends_with(']') {
            Value::Array(
                split_inline_array(&val[1..val.len() - 1])
                    .iter()
                    .map(|s| coerce(s.trim()))
                    .filter(|v| v.as_str() != Some(""))
                    .collect(),
            )
        } else {
            coerce(val)
        };
        object_mut(at(&mut root, &top_path))?.insert(key, value);
        i += 1;
    }
    match root {
        Value::Object(map) => Ok(map),
        _ => Err(FrontmatterError::ObjectExpected),
    }
}

/// Splits on top-level commas; a quote opens a quoted item only as the
/// item's first non-space character.
fn split_inline_array(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut buf = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        if let Some(q) = quote {
            buf.push(ch);
            if ch == q {
                quote = None;
            }
        } else if (ch == '"' || ch == '\'') && buf.trim().is_empty() {
            quote = Some(ch);
            buf.push(ch);
        } else if ch == ',' {
            items.push(std::mem::take(&mut buf));
        } else {
            buf.push(ch);
        }
    }
    items.push(buf);
    items
}

fn coerce(raw: &str) -> Value {
    let v = raw.trim();
    let quoted = v.chars().count() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')));
    if quoted {
        return Value::String(v[1..v.len() - 1].to_owned());
    }
    match v {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "~" => return Value::Null,
        _ => {}
    }
    if INT_RE.is_match(v) {
        if let Ok(n) = v.parse::<i64>() {
            return Value::Number(n.into());
        }
    }
    if INT_RE.is_match(v) || FLOAT_RE.is_match(v) {
        if let Some(n) = v.parse::<f64>().ok().and_then(Number::from_f64) {
            return Value::Number(n);
        }
    }
    Value::String(v.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(src: &str) -> Value {
        Value::Object(parse(src).unwrap().0)
    }

    #[test]
    fn no_frontmatter_is_all_body() {
        let (map, body) = parse("# Title\nbody").unwrap();
        assert!(map.is_empty());
        assert_eq!(body, "# Title\nbody");
    }

    #[test]
    fn scalars_nested_maps_and_lists() {
        let src = "---\nname: demo\ndescription: \"Hi: there\"\ncount: 3\nratio: .5\nflag: true\nnone: ~\nod:\n  mode: deck\n  preview:\n    type: html\n  craft:\n    requires: [typography, 'color']\ntriggers:\n- one\n- two\n---\nBody";
        let (map, body) = parse(src).unwrap();
        assert_eq!(body, "Body");
        assert_eq!(
            Value::Object(map),
            json!({
                "name": "demo",
                "description": "Hi: there",
                "count": 3,
                "ratio": 0.5,
                "flag": true,
                "none": null,
                "od": {"mode": "deck", "preview": {"type": "html"}, "craft": {"requires": ["typography", "color"]}},
                "triggers": ["one", "two"]
            })
        );
    }

    #[test]
    fn block_scalar_uses_first_line_indent() {
        let v =
            data("---\ndescription: |\n    line one\n      deeper\n\n    line two\nnext: x\n---\n");
        assert_eq!(v["description"], "line one\n  deeper\n\nline two");
        assert_eq!(v["next"], "x");
    }

    #[test]
    fn sequence_of_maps() {
        let v = data("---\ninputs:\n  - name: a\n    type: text\n  - name: b\n---\n");
        assert_eq!(
            v["inputs"],
            json!([{"name": "a", "type": "text"}, {"name": "b"}])
        );
    }

    #[test]
    fn inline_array_quotes_and_empty() {
        let v = data("---\na: [x, \"y,z\", don't, \"\"]\nb: []\n---\n");
        assert_eq!(v["a"], json!(["x", "y,z", "don't"]));
        assert_eq!(v["b"], json!([]));
    }

    #[test]
    fn crlf_and_bom() {
        let v = data("\u{feff}---\r\nname: x\r\n---\r\nbody");
        assert_eq!(v["name"], "x");
    }
}
