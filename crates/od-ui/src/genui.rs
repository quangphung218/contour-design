//! GenUI surfaces (track C4-depth): what the chat shows for a surface a
//! plugin run asks, and the answer each control sends. Port of the
//! original `GenUISurfaceRenderer.tsx` decisions; the view lives in
//! `app/genui_view.rs`.

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Ids the daemon gives the diff-review choice of a pipeline stage.
pub const DIFF_REVIEW_PREFIX: &str = "__auto_diff_review_";

/// A row of `/api/projects/:id/genui`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceRow {
    pub id: String,
    pub surface_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    pub kind: String,
    pub persist: String,
    #[serde(default)]
    pub spec: Value,
    pub status: String,
    #[serde(default)]
    pub responded_by: Option<String>,
    pub requested_at: i64,
}

impl SurfaceRow {
    /// The surface's own question, if it has one.
    #[must_use]
    pub fn prompt(&self) -> Option<&str> {
        self.spec
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|p| !p.is_empty())
    }

    /// A remembered answer the user can revoke.
    #[must_use]
    pub fn is_remembered(&self) -> bool {
        self.status == "resolved" && self.persist != "run"
    }
}

/// The surfaces waiting for an answer in `conversation`, oldest first.
#[must_use]
pub fn pending_in(rows: &[SurfaceRow], conversation: Option<&str>) -> Vec<SurfaceRow> {
    let mut out: Vec<SurfaceRow> = rows
        .iter()
        .filter(|r| r.status == "pending" && r.run_id.is_some())
        .filter(|r| conversation.is_none() || r.conversation_id.as_deref() == conversation)
        .cloned()
        .collect();
    out.sort_by_key(|r| r.requested_at);
    out
}

/// One field of a form surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    Text {
        multiline: bool,
    },
    Number {
        integer: bool,
        min: Option<f64>,
        max: Option<f64>,
    },
    Bool,
    Enum(Vec<String>),
}

/// How a surface is shown.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Continue / Cancel.
    Confirmation,
    /// Authorize / Skip; `target` names the connector or MCP server.
    OAuth {
        route: String,
        target: Option<String>,
    },
    /// Accept all / Reject all / Partial over the touched files.
    DiffReview { files: Vec<String> },
    /// One button per value of the schema's enum property.
    Choice { key: String, options: Vec<String> },
    /// Fields of an object schema.
    Form(Vec<Field>),
    /// Anything else: a JSON answer typed by hand.
    Raw,
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The first property with a string enum (the original's
/// `pickPrimaryEnum`), for single-question choices.
fn primary_enum(schema: &Value) -> Option<(String, Vec<String>)> {
    let props = schema.get("properties")?.as_object()?;
    let enums: Vec<(String, Vec<String>)> = props
        .iter()
        .filter_map(|(k, p)| {
            let options = strings(p.get("enum"));
            (!options.is_empty()).then(|| (k.clone(), options))
        })
        .collect();
    // Only when the enum is the whole question.
    (enums.len() == 1 && props.len() == 1).then(|| enums.into_iter().next())?
}

/// Object schemas whose properties are scalars or enums (the original's
/// `readObjectSchemaFields`); `None` for anything else.
fn fields(schema: &Value) -> Option<Vec<Field>> {
    let props = schema.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }
    let required = strings(schema.get("required"));
    props
        .iter()
        .map(|(key, p)| {
            let options = strings(p.get("enum"));
            let kind = if !options.is_empty() {
                FieldKind::Enum(options)
            } else {
                match p.get("type").and_then(Value::as_str) {
                    Some("string") | None => FieldKind::Text {
                        multiline: p.get("format").and_then(Value::as_str) == Some("textarea"),
                    },
                    Some(t @ ("number" | "integer")) => FieldKind::Number {
                        integer: t == "integer",
                        min: p.get("minimum").and_then(Value::as_f64),
                        max: p.get("maximum").and_then(Value::as_f64),
                    },
                    Some("boolean") => FieldKind::Bool,
                    Some(_) => return None,
                }
            };
            Some(Field {
                key: key.clone(),
                label: p
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .to_owned(),
                description: p
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                required: required.contains(key),
                kind,
            })
        })
        .collect()
}

/// Picks the control for a surface, in the original's order.
#[must_use]
pub fn shape(row: &SurfaceRow) -> Shape {
    let spec = &row.spec;
    let schema = spec.get("schema").unwrap_or(&Value::Null);
    match row.kind.as_str() {
        "confirmation" => Shape::Confirmation,
        "choice" if row.surface_id.starts_with(DIFF_REVIEW_PREFIX) => Shape::DiffReview {
            files: strings(spec.pointer("/context/touchedFiles")),
        },
        "oauth-prompt" => {
            let oauth = spec.get("oauth");
            let route = oauth
                .and_then(|o| o.get("route"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let target = oauth
                .and_then(|o| o.get("connectorId").or_else(|| o.get("mcpServerId")))
                .and_then(Value::as_str)
                .map(str::to_owned);
            Shape::OAuth { route, target }
        }
        "choice" | "form" => {
            if row.kind == "choice" {
                if let Some((key, options)) = primary_enum(schema) {
                    return Shape::Choice { key, options };
                }
            }
            fields(schema).map_or(Shape::Raw, Shape::Form)
        }
        _ => Shape::Raw,
    }
}

/// The Authorize answer.
#[must_use]
pub fn authorize_answer(row: &SurfaceRow) -> Value {
    let mut out = json!({ "authorized": true });
    if let Some(oauth) = row.spec.get("oauth") {
        for key in ["connectorId", "mcpServerId"] {
            if let Some(v) = oauth.get(key).filter(|v| v.is_string()) {
                out[key] = v.clone();
            }
        }
    }
    out
}

/// What the user picked for one file in a partial review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePick {
    Undecided,
    Accept,
    Reject,
}

/// Accept all / Reject all, with the optional note.
#[must_use]
pub fn diff_answer(accept: bool, files: &[String], reason: &str) -> Value {
    let mut out = if accept {
        json!({ "decision": "accept" })
    } else {
        json!({ "decision": "reject", "accepted_files": [] })
    };
    if !files.is_empty() {
        let (a, r): (&[String], &[String]) = if accept { (files, &[]) } else { (&[], files) };
        out["accepted_files"] = json!(a);
        out["rejected_files"] = json!(r);
    }
    if !reason.trim().is_empty() {
        out["reason"] = json!(reason.trim());
    }
    out
}

/// The partial answer; `Err` lists the files still undecided.
///
/// # Errors
/// The files without a pick.
pub fn partial_answer(picks: &[(String, FilePick)], reason: &str) -> Result<Value, Vec<String>> {
    let undecided: Vec<String> = picks
        .iter()
        .filter(|(_, p)| *p == FilePick::Undecided)
        .map(|(f, _)| f.clone())
        .collect();
    if !undecided.is_empty() {
        return Err(undecided);
    }
    let of = |want: FilePick| -> Vec<&String> {
        picks
            .iter()
            .filter(|(_, p)| *p == want)
            .map(|(f, _)| f)
            .collect()
    };
    let mut out = json!({
        "decision": "partial",
        "accepted_files": of(FilePick::Accept),
        "rejected_files": of(FilePick::Reject),
    });
    if !reason.trim().is_empty() {
        out["reason"] = json!(reason.trim());
    }
    Ok(out)
}

/// Why a form cannot be sent yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    Required(String),
    NotANumber(String),
    NotWhole(String),
    OutOfRange(String),
}

/// The form's answer from the typed values (`"true"`/`"false"` for
/// checkboxes), with numbers checked like the original.
///
/// # Errors
/// The first field that is missing or out of shape, by label.
pub fn form_answer(fields: &[Field], values: &Map<String, Value>) -> Result<Value, FormError> {
    let mut out = Map::new();
    for f in fields {
        let raw = values
            .get(&f.key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let value = match &f.kind {
            FieldKind::Bool => Value::Bool(raw == "true"),
            _ if raw.is_empty() => {
                if f.required {
                    return Err(FormError::Required(f.label.clone()));
                }
                continue;
            }
            FieldKind::Number { integer, min, max } => {
                let n: f64 = raw
                    .parse()
                    .map_err(|_| FormError::NotANumber(f.label.clone()))?;
                if *integer && n.fract() != 0.0 {
                    return Err(FormError::NotWhole(f.label.clone()));
                }
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    return Err(FormError::OutOfRange(f.label.clone()));
                }
                if *integer {
                    #[allow(clippy::cast_possible_truncation)] // whole, checked above
                    let whole = n as i64;
                    json!(whole)
                } else {
                    json!(n)
                }
            }
            FieldKind::Text { .. } | FieldKind::Enum(_) => json!(raw),
        };
        out.insert(f.key.clone(), value);
    }
    Ok(Value::Object(out))
}

/// A hand-typed answer: JSON, else the text as a string.
#[must_use]
pub fn raw_answer(text: &str) -> Value {
    serde_json::from_str(text.trim()).unwrap_or_else(|_| json!(text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, id: &str, spec: Value) -> SurfaceRow {
        SurfaceRow {
            id: format!("row-{id}"),
            surface_id: id.to_owned(),
            conversation_id: Some("c1".to_owned()),
            run_id: Some("r1".to_owned()),
            kind: kind.to_owned(),
            persist: "run".to_owned(),
            spec,
            status: "pending".to_owned(),
            responded_by: None,
            requested_at: 1,
        }
    }

    #[test]
    fn shapes_follow_the_original_renderer() {
        assert_eq!(
            shape(&row("confirmation", "go", json!({}))),
            Shape::Confirmation
        );
        let oauth = row(
            "oauth-prompt",
            "auth",
            json!({"oauth": {"route": "mcp", "mcpServerId": "figma"}}),
        );
        assert_eq!(
            shape(&oauth),
            Shape::OAuth {
                route: "mcp".into(),
                target: Some("figma".into())
            }
        );
        assert_eq!(
            authorize_answer(&oauth),
            json!({"authorized": true, "mcpServerId": "figma"})
        );
        let review = row(
            "choice",
            "__auto_diff_review_s",
            json!({"context": {"touchedFiles": ["a.js", "b.js"]}, "schema": {"properties": {"decision": {"enum": ["accept"]}}}}),
        );
        assert_eq!(
            shape(&review),
            Shape::DiffReview {
                files: vec!["a.js".into(), "b.js".into()]
            }
        );
        let choice = row(
            "choice",
            "tone",
            json!({"schema": {"properties": {"tone": {"enum": ["calm", "bold"]}}}}),
        );
        assert_eq!(
            shape(&choice),
            Shape::Choice {
                key: "tone".into(),
                options: vec!["calm".into(), "bold".into()]
            }
        );
        let form = row(
            "form",
            "brief",
            json!({"schema": {"required": ["name"], "properties": {
                "name": {"type": "string", "title": "Name"},
                "count": {"type": "integer", "minimum": 1, "maximum": 5},
                "dark": {"type": "boolean"},
                "size": {"enum": ["s", "m"]}
            }}}),
        );
        let Shape::Form(fields) = shape(&form) else {
            panic!("form")
        };
        assert_eq!(fields.len(), 4);
        assert!(fields[0].required && fields[0].label == "Name");
        assert_eq!(
            shape(&row(
                "form",
                "x",
                json!({"schema": {"properties": {"l": {"type": "array"}}}})
            )),
            Shape::Raw
        );
        assert_eq!(shape(&row("form", "x", json!({}))), Shape::Raw);
        assert_eq!(shape(&row("weird", "x", json!({}))), Shape::Raw);
    }

    #[test]
    fn diff_review_answers() {
        let files = vec!["a.js".to_owned(), "b.js".to_owned()];
        assert_eq!(
            diff_answer(true, &files, " ok "),
            json!({"decision": "accept", "accepted_files": ["a.js", "b.js"], "rejected_files": [], "reason": "ok"})
        );
        assert_eq!(
            diff_answer(false, &[], ""),
            json!({"decision": "reject", "accepted_files": []})
        );
        let picks = vec![
            ("a.js".to_owned(), FilePick::Accept),
            ("b.js".to_owned(), FilePick::Undecided),
        ];
        assert_eq!(partial_answer(&picks, ""), Err(vec!["b.js".to_owned()]));
        let picks = vec![
            ("a.js".to_owned(), FilePick::Accept),
            ("b.js".to_owned(), FilePick::Reject),
        ];
        assert_eq!(
            partial_answer(&picks, "").unwrap_or_default(),
            json!({"decision": "partial", "accepted_files": ["a.js"], "rejected_files": ["b.js"]})
        );
    }

    #[test]
    fn form_answers_are_checked() {
        let Shape::Form(fields) = shape(&row(
            "form",
            "f",
            json!({"schema": {"required": ["name"], "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer", "minimum": 1, "maximum": 5},
                "dark": {"type": "boolean"}
            }}}),
        )) else {
            panic!("form")
        };
        let vals = |v: Value| v.as_object().cloned().unwrap_or_default();
        assert_eq!(
            form_answer(&fields, &vals(json!({}))),
            Err(FormError::Required("name".into()))
        );
        assert_eq!(
            form_answer(&fields, &vals(json!({"name": "x", "count": "2.5"}))),
            Err(FormError::NotWhole("count".into()))
        );
        assert_eq!(
            form_answer(&fields, &vals(json!({"name": "x", "count": "9"}))),
            Err(FormError::OutOfRange("count".into()))
        );
        assert_eq!(
            form_answer(&fields, &vals(json!({"name": "x", "count": "abc"}))),
            Err(FormError::NotANumber("count".into()))
        );
        assert_eq!(
            form_answer(
                &fields,
                &vals(json!({"name": " x ", "count": "3", "dark": "true"}))
            ),
            Ok(json!({"name": "x", "count": 3, "dark": true}))
        );
        assert_eq!(raw_answer("{\"a\":1}"), json!({"a": 1}));
        assert_eq!(raw_answer(" hello "), json!("hello"));
    }

    #[test]
    fn pending_surfaces_of_the_conversation() {
        let mut a = row("confirmation", "a", json!({}));
        a.requested_at = 5;
        let mut b = row("confirmation", "b", json!({}));
        b.requested_at = 2;
        let mut done = row("confirmation", "c", json!({}));
        done.status = "resolved".into();
        done.persist = "project".into();
        let mut other = row("confirmation", "d", json!({}));
        other.conversation_id = Some("c2".into());
        let mut prefilled = row("confirmation", "e", json!({}));
        prefilled.run_id = None;
        let rows = [a, b, done.clone(), other, prefilled];
        let ids: Vec<String> = pending_in(&rows, Some("c1"))
            .into_iter()
            .map(|r| r.surface_id)
            .collect();
        assert_eq!(ids, ["b", "a"]);
        assert!(done.is_remembered());
    }
}
