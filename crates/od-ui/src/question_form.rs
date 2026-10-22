//! `<question-form>` blocks in assistant text (spec 1.1).
//!
//! The agent writes a tag with a JSON body:
//!
//! ```text
//! <question-form id="discovery" title="Quick brief">
//! {"questions": [{"id": "platform", "label": "Platform", "type": "radio",
//!                 "options": ["Mobile", "Desktop"], "required": true}]}
//! </question-form>
//! ```
//!
//! `<ask-question>` is accepted as an alias. Answers go back as the next user
//! message, starting with `[form answers — <id>]` so the agent recognises
//! them (same shape as the original app).

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

const TAGS: [&str; 2] = ["question-form", "ask-question"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    Radio,
    Checkbox,
    Select,
    Text,
    Textarea,
}

impl QuestionKind {
    fn parse(s: &str) -> Self {
        match s {
            "radio" | "direction-cards" | "switch" => Self::Radio,
            "checkbox" => Self::Checkbox,
            "select" => Self::Select,
            "textarea" => Self::Textarea,
            _ => Self::Text,
        }
    }

    #[must_use]
    pub fn has_options(self) -> bool {
        matches!(self, Self::Radio | Self::Checkbox | Self::Select)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub label: String,
    pub kind: QuestionKind,
    pub options: Vec<FormOption>,
    pub placeholder: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionForm {
    pub id: String,
    pub title: String,
    pub questions: Vec<Question>,
}

/// A piece of assistant text: prose, a form, or a form still streaming in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Form(QuestionForm),
    /// An opened tag whose close has not arrived yet.
    PendingForm,
}

/// An answer: one value, or several for checkboxes.
pub type Answers = BTreeMap<String, Vec<String>>;

#[derive(Deserialize)]
struct RawBody {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    questions: Vec<RawQuestion>,
}

#[derive(Deserialize)]
struct RawQuestion {
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    options: Vec<Value>,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    required: bool,
}

fn parse_option(v: &Value) -> Option<FormOption> {
    match v {
        Value::String(s) => Some(FormOption {
            label: s.clone(),
            value: s.clone(),
        }),
        Value::Object(o) => {
            let label = o.get("label").and_then(Value::as_str);
            let value = o.get("value").and_then(Value::as_str);
            let label = label.or(value)?.to_owned();
            Some(FormOption {
                value: value.map_or_else(|| label.clone(), str::to_owned),
                label,
            })
        }
        _ => None,
    }
}

/// Reads `name="value"` attributes from an opening tag.
fn attr(open_tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = open_tag.find(&needle)? + needle.len();
    let len = open_tag[start..].find('"')?;
    Some(open_tag[start..start + len].to_owned())
}

/// Parses a form from its opening tag and JSON body.
#[must_use]
pub fn parse_form(open_tag: &str, body: &str) -> Option<QuestionForm> {
    let raw: RawBody = serde_json::from_str(body.trim()).ok()?;
    let questions: Vec<Question> = raw
        .questions
        .into_iter()
        .filter(|q| !q.id.is_empty())
        .map(|q| {
            let kind = QuestionKind::parse(&q.kind);
            Question {
                label: if q.label.is_empty() {
                    q.id.clone()
                } else {
                    q.label
                },
                id: q.id,
                kind,
                options: q.options.iter().filter_map(parse_option).collect(),
                placeholder: q.placeholder,
                required: q.required,
            }
        })
        .collect();
    if questions.is_empty() {
        return None;
    }
    Some(QuestionForm {
        id: attr(open_tag, "id")
            .or(raw.id)
            .unwrap_or_else(|| "form".to_owned()),
        title: attr(open_tag, "title").or(raw.title).unwrap_or_default(),
        questions,
    })
}

/// Earliest opening tag at or after `from`: (start, end of `>`, tag name).
fn find_open(text: &str, from: usize) -> Option<(usize, usize, &'static str)> {
    TAGS.iter()
        .filter_map(|tag| {
            let mut at = from;
            while let Some(i) = text[at..].find(&format!("<{tag}")) {
                let start = at + i;
                let after = start + 1 + tag.len();
                match text[after..].chars().next() {
                    Some(c) if c == '>' || c.is_whitespace() => {
                        let end = text[after..].find('>').map(|j| after + j + 1);
                        return Some((start, end.unwrap_or(text.len()), *tag));
                    }
                    None => return Some((start, text.len(), *tag)),
                    _ => at = after,
                }
            }
            None
        })
        .min_by_key(|(start, _, _)| *start)
}

/// Splits assistant text into prose and forms. A tag that cannot be parsed
/// stays as prose; an unterminated one becomes [`Segment::PendingForm`].
#[must_use]
pub fn split(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let push_text = |out: &mut Vec<Segment>, s: &str| {
        if !s.trim().is_empty() {
            out.push(Segment::Text(s.to_owned()));
        }
    };
    while let Some((start, open_end, tag)) = find_open(text, cursor) {
        push_text(&mut out, &text[cursor..start]);
        let close = format!("</{tag}>");
        let Some(rel) = text[open_end..].find(&close) else {
            out.push(Segment::PendingForm);
            return out;
        };
        let body_end = open_end + rel;
        let block_end = body_end + close.len();
        match parse_form(&text[start..open_end], &text[open_end..body_end]) {
            Some(form) => out.push(Segment::Form(form)),
            None => push_text(&mut out, &text[start..block_end]),
        }
        cursor = block_end;
    }
    push_text(&mut out, &text[cursor..]);
    out
}

fn display_value(q: &Question, value: &str) -> String {
    match q
        .options
        .iter()
        .find(|o| o.value == value || o.label == value)
    {
        Some(o) if o.value != o.label => format!("{} [value: {}]", o.label, o.value),
        Some(o) => o.label.clone(),
        None => value.to_owned(),
    }
}

/// Formats answers as the next user message.
#[must_use]
pub fn format_answers(form: &QuestionForm, answers: &Answers) -> String {
    let mut lines = vec![format!("[form answers — {}]", form.id)];
    for q in &form.questions {
        let values: Vec<String> = answers
            .get(&q.id)
            .into_iter()
            .flatten()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(|v| display_value(q, v))
            .collect();
        let shown = if values.is_empty() {
            "(skipped)".to_owned()
        } else {
            values.join(", ")
        };
        lines.push(format!("- {}: {shown}", q.label));
    }
    lines.join("\n")
}

/// Required questions still unanswered.
#[must_use]
pub fn missing_required<'a>(form: &'a QuestionForm, answers: &Answers) -> Vec<&'a str> {
    form.questions
        .iter()
        .filter(|q| q.required)
        .filter(|q| {
            answers
                .get(&q.id)
                .is_none_or(|v| v.iter().all(|s| s.trim().is_empty()))
        })
        .map(|q| q.label.as_str())
        .collect()
}

/// Whether a user message is a form submission (shown compactly).
#[must_use]
pub fn is_answers_message(content: &str) -> bool {
    content.trim_start().starts_with("[form answers")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORM: &str = r#"Before I start:
<question-form id="discovery" title="Quick brief">
{"questions": [
  {"id": "platform", "label": "Platform", "type": "radio",
   "options": ["Mobile", {"label": "Desktop web", "value": "desktop"}], "required": true},
  {"id": "tone", "label": "Tone", "type": "checkbox", "options": ["Calm", "Bold"]},
  {"id": "audience", "label": "Audience", "type": "text", "placeholder": "e.g. SaaS"}
]}
</question-form>
Thanks!"#;

    fn form() -> QuestionForm {
        match split(FORM).into_iter().nth(1) {
            Some(Segment::Form(f)) => f,
            other => panic!("expected form, got {other:?}"),
        }
    }

    #[test]
    fn splits_prose_and_form() {
        let segs = split(FORM);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], Segment::Text("Before I start:\n".into()));
        assert_eq!(segs[2], Segment::Text("\nThanks!".into()));
        let f = form();
        assert_eq!(
            (f.id.as_str(), f.title.as_str()),
            ("discovery", "Quick brief")
        );
        assert_eq!(f.questions[0].kind, QuestionKind::Radio);
        assert_eq!(
            f.questions[0].options[1],
            FormOption {
                label: "Desktop web".into(),
                value: "desktop".into()
            }
        );
        assert_eq!(f.questions[2].placeholder, "e.g. SaaS");
    }

    #[test]
    fn streaming_form_is_pending() {
        let partial = &FORM[..FORM.find("\"tone\"").unwrap_or(0)];
        assert_eq!(
            split(partial),
            vec![
                Segment::Text("Before I start:\n".into()),
                Segment::PendingForm
            ]
        );
        // Even a half-typed tag name counts once it is unambiguous.
        assert_eq!(
            split("Hi <question-form"),
            vec![Segment::Text("Hi ".into()), Segment::PendingForm]
        );
    }

    #[test]
    fn invalid_body_stays_prose_and_alias_works() {
        let bad = "<question-form>not json</question-form>";
        assert_eq!(split(bad), vec![Segment::Text(bad.into())]);
        let alias = r#"<ask-question id="a">{"questions":[{"id":"q","label":"Q"}]}</ask-question>"#;
        assert!(matches!(split(alias).as_slice(), [Segment::Form(f)] if f.id == "a"));
        // Mentioning a similar tag name is not a form.
        assert_eq!(
            split("use <question-formatter> here"),
            vec![Segment::Text("use <question-formatter> here".into())]
        );
    }

    #[test]
    fn formats_answers_like_the_original() {
        let f = form();
        let mut a = Answers::new();
        a.insert("platform".into(), vec!["desktop".into()]);
        a.insert("tone".into(), vec!["Calm".into(), "Bold".into()]);
        assert_eq!(
            format_answers(&f, &a),
            "[form answers — discovery]\n- Platform: Desktop web [value: desktop]\n- Tone: Calm, Bold\n- Audience: (skipped)"
        );
        assert!(is_answers_message(&format_answers(&f, &a)));
    }

    #[test]
    fn required_check() {
        let f = form();
        assert_eq!(missing_required(&f, &Answers::new()), vec!["Platform"]);
        let mut a = Answers::new();
        a.insert("platform".into(), vec!["Mobile".into()]);
        assert!(missing_required(&f, &a).is_empty());
    }
}
