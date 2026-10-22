//! `collect_brief` / `confirm_brief`: a short questionnaire per artifact
//! type, held in this MCP session's memory (like the original, which needs
//! the same session for both calls). The MCP Apps card of the original is
//! not rendered; clients get the questions as data.

use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

struct Question {
    id: &'static str,
    label: &'static str,
    options: &'static [&'static str],
    recommended: &'static str,
}

const COMMON: [Question; 3] = [
    Question {
        id: "audience",
        label: "Who is it for?",
        options: &["general", "technical", "executive", "consumer"],
        recommended: "general",
    },
    Question {
        id: "tone",
        label: "Visual tone",
        options: &["clean-minimal", "bold-expressive", "playful", "corporate"],
        recommended: "clean-minimal",
    },
    Question {
        id: "fidelity",
        label: "Fidelity",
        options: &["wireframe", "high-fidelity"],
        recommended: "high-fidelity",
    },
];

fn specific(artifact_type: &str) -> Option<Question> {
    let q = |id, label, options, recommended| Question {
        id,
        label,
        options,
        recommended,
    };
    Some(match artifact_type {
        "presentation" => q("length", "How many slides?", &["5", "10", "20"], "10"),
        "website" => q(
            "pages",
            "Pages",
            &["single-page", "multi-page"],
            "single-page",
        ),
        "product-prototype" => q("platform", "Platform", &["web", "mobile", "desktop"], "web"),
        "document" => q(
            "format",
            "Format",
            &["report", "one-pager", "guide"],
            "one-pager",
        ),
        "image" => q(
            "aspect",
            "Aspect ratio",
            &["1:1", "16:9", "9:16", "4:5"],
            "16:9",
        ),
        "video" | "audio" => q("duration", "Duration", &["15s", "30s", "60s"], "30s"),
        "design-system" => q("scope", "Scope", &["tokens", "components", "full"], "full"),
        _ => return None,
    })
}

fn questions(artifact_type: &str) -> Vec<Question> {
    let mut out: Vec<Question> = specific(artifact_type).into_iter().collect();
    out.extend(COMMON);
    out
}

struct Draft {
    nonce: String,
    artifact_type: String,
    title: Option<String>,
    locale: Option<String>,
    known: Map<String, Value>,
}

#[derive(Default)]
pub struct BriefStore {
    drafts: Mutex<HashMap<String, Draft>>,
}

fn str_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

impl BriefStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Draft>> {
        self.drafts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// # Errors
    /// Unknown `artifactType`.
    pub fn collect(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let artifact_type = str_arg(args, "artifactType").ok_or("artifactType is required")?;
        if specific(artifact_type).is_none() {
            return Err(format!("unknown artifactType {artifact_type:?}"));
        }
        let known = args
            .get("knownAnswers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let draft = Draft {
            nonce: uuid::Uuid::new_v4().simple().to_string(),
            artifact_type: artifact_type.to_owned(),
            title: str_arg(args, "projectTitle").map(str::to_owned),
            locale: str_arg(args, "locale").map(str::to_owned),
            known,
        };
        if args.get("skip") == Some(&Value::Bool(true)) {
            return Ok(summarize(&draft, &Map::new()));
        }
        let qs: Vec<Value> = questions(artifact_type)
            .iter()
            .filter(|q| !draft.known.contains_key(q.id))
            .map(|q| {
                json!({
                    "id": q.id,
                    "label": q.label,
                    "options": q.options,
                    "recommended": q.recommended,
                })
            })
            .collect();
        let id = uuid::Uuid::new_v4().to_string();
        let out = json!({
            "briefDraftId": id,
            "nonce": draft.nonce,
            "artifactType": artifact_type,
            "projectTitle": draft.title,
            "questions": qs,
            "instructions": "Ask the user these questions (recommended values are the defaults), then call confirm_brief with their answers.",
        });
        self.lock().insert(id, draft);
        Ok(out)
    }

    /// # Errors
    /// Unknown draft, wrong nonce, or an answer outside its options.
    pub fn confirm(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let id = str_arg(args, "briefDraftId").ok_or("briefDraftId is required")?;
        let nonce = str_arg(args, "nonce").ok_or("nonce is required")?;
        let answers = args
            .get("answers")
            .and_then(Value::as_object)
            .ok_or("answers must be an object")?;
        let mut drafts = self.lock();
        let draft = drafts
            .get(id)
            .ok_or("unknown brief draft: confirm_brief must run in the MCP session that called collect_brief")?;
        if draft.nonce != nonce {
            return Err("brief nonce does not match".to_owned());
        }
        for q in questions(&draft.artifact_type) {
            if let Some(v) = answers.get(q.id).and_then(Value::as_str) {
                if !q.options.contains(&v) {
                    return Err(format!(
                        "{} must be one of {:?}, got {v:?}",
                        q.id, q.options
                    ));
                }
            }
        }
        let mut draft = drafts.remove(id).ok_or("unknown brief draft")?;
        if draft.locale.is_none() {
            draft.locale = str_arg(args, "locale").map(str::to_owned);
        }
        Ok(summarize(&draft, answers))
    }
}

/// Answers (given, known, else recommended) and a prompt-ready summary.
fn summarize(draft: &Draft, answers: &Map<String, Value>) -> Value {
    let mut resolved = Map::new();
    let mut lines = vec![format!(
        "Brief: {}{}",
        draft.artifact_type,
        draft
            .title
            .as_deref()
            .map(|t| format!(" \"{t}\""))
            .unwrap_or_default()
    )];
    for q in questions(&draft.artifact_type) {
        let value = answers
            .get(q.id)
            .or_else(|| draft.known.get(q.id))
            .cloned()
            .unwrap_or_else(|| json!(q.recommended));
        let shown = value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned);
        lines.push(format!("- {}: {shown}", q.label));
        resolved.insert(q.id.to_owned(), value);
    }
    json!({
        "confirmed": true,
        "artifactType": draft.artifact_type,
        "projectTitle": draft.title,
        "locale": draft.locale,
        "answers": resolved,
        "summary": lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn collect_then_confirm() {
        let store = BriefStore::default();
        let c = store
            .collect(&obj(json!({ "artifactType": "presentation", "projectTitle": "Q3", "knownAnswers": { "tone": "playful" } })))
            .unwrap();
        let ids: Vec<&str> = c["questions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| q["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["length", "audience", "fidelity"]);
        let args = |nonce: &Value, answers: Value| {
            obj(json!({ "briefDraftId": c["briefDraftId"], "nonce": nonce, "answers": answers }))
        };
        assert!(store.confirm(&args(&json!("wrong"), json!({}))).is_err());
        assert!(
            store
                .confirm(&args(&c["nonce"], json!({ "length": "7" })))
                .is_err()
        );
        let done = store
            .confirm(&args(&c["nonce"], json!({ "length": "5" })))
            .unwrap();
        assert_eq!(
            done["answers"],
            json!({ "length": "5", "audience": "general", "tone": "playful", "fidelity": "high-fidelity" })
        );
        assert!(
            done["summary"]
                .as_str()
                .unwrap()
                .starts_with("Brief: presentation \"Q3\"")
        );
        // One use per draft.
        assert!(store.confirm(&args(&c["nonce"], json!({}))).is_err());
    }

    #[test]
    fn skip_uses_defaults_and_unknown_type_fails() {
        let store = BriefStore::default();
        let done = store
            .collect(&obj(json!({ "artifactType": "image", "skip": true })))
            .unwrap();
        assert_eq!(done["answers"]["aspect"], "16:9");
        assert!(
            store
                .collect(&obj(json!({ "artifactType": "hologram" })))
                .is_err()
        );
    }
}
