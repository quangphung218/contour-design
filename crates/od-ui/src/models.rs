//! Model choice per agent (track B3-chat): the composer's model picker and
//! what a run sends. Stored like the original, globally, in app settings:
//! `agentModels[agentId] = {model, reasoning, serviceTier}`, with the top-level
//! `model` mirroring the current agent's choice for older readers.

use crate::settings::AppConfig;
use serde_json::{Map, Value, json};

/// Built-in model choices per agent (the original's fallback lists);
/// anything else is typed.
#[must_use]
pub fn models_for(agent: &str) -> &'static [&'static str] {
    match agent {
        "claude" => &["default", "opus", "sonnet", "haiku"],
        "codex" => &["default", "gpt-5.5", "gpt-5.4", "gpt-5.4-mini"],
        "opencode" => &["default", "anthropic/claude-sonnet-4-6", "openai/gpt-5.5"],
        _ => &["default"],
    }
}

/// Reasoning effort values the agent accepts (empty: no picker).
#[must_use]
pub fn reasoning_for(agent: &str) -> &'static [&'static str] {
    match agent {
        "codex" => &[
            "default", "none", "minimal", "low", "medium", "high", "xhigh",
        ],
        _ => &[],
    }
}

/// The model and effort a run of `agent` uses; `None` is the agent's own
/// default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub model: Option<String>,
    pub reasoning: Option<String>,
}

fn clean(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|v| !v.is_empty() && *v != "default")
        .map(str::to_owned)
}

/// The stored choice for `agent`: its `agentModels` entry, else the
/// top-level `model` when `agent` is the configured one.
#[must_use]
pub fn selection(config: &AppConfig, agent: &str) -> Selection {
    let entry = config
        .extra
        .get("agentModels")
        .and_then(|m| m.get(agent))
        .and_then(Value::as_object);
    if let Some(entry) = entry {
        return Selection {
            model: clean(entry.get("model").and_then(Value::as_str)),
            reasoning: clean(entry.get("reasoning").and_then(Value::as_str))
                .filter(|r| reasoning_for(agent).contains(&r.as_str())),
        };
    }
    let current = config.agent_id.as_deref() == Some(agent);
    Selection {
        model: current.then(|| clean(config.model.as_deref())).flatten(),
        reasoning: None,
    }
}

/// `config` with `agent` as the agent and `sel` as its choice. Other agents'
/// entries and this entry's other keys (`serviceTier`) are kept.
#[must_use]
pub fn with_selection(config: &AppConfig, agent: &str, sel: &Selection) -> AppConfig {
    let mut next = config.clone();
    next.agent_id = Some(agent.to_owned());
    next.model = sel.model.clone();
    let mut all: Map<String, Value> = next
        .extra
        .get("agentModels")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut entry = all
        .get(agent)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    entry.insert("model".to_owned(), json!(sel.model));
    entry.insert("reasoning".to_owned(), json!(sel.reasoning));
    all.insert(agent.to_owned(), Value::Object(entry));
    next.extra
        .insert("agentModels".to_owned(), Value::Object(all));
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(v: Value) -> AppConfig {
        serde_json::from_value(v).unwrap_or_default()
    }

    #[test]
    fn selection_reads_the_agent_entry_then_the_top_level_model() {
        let c = config(json!({
            "agentId": "claude", "model": "opus",
            "agentModels": { "codex": { "model": "gpt-5.5", "reasoning": "high" } }
        }));
        assert_eq!(
            selection(&c, "codex"),
            Selection {
                model: Some("gpt-5.5".into()),
                reasoning: Some("high".into())
            }
        );
        assert_eq!(selection(&c, "claude").model.as_deref(), Some("opus"));
        assert_eq!(selection(&c, "qwen"), Selection::default());
        let odd = config(
            json!({ "agentModels": { "claude": { "model": "default", "reasoning": "high" } } }),
        );
        assert_eq!(
            selection(&odd, "claude"),
            Selection::default(),
            "default is no model; claude takes no effort"
        );
    }

    #[test]
    fn with_selection_keeps_other_agents_and_keys() {
        let c = config(json!({
            "agentId": "claude",
            "agentModels": {
                "claude": { "model": "opus" },
                "codex": { "model": "gpt-5.4", "serviceTier": "priority" }
            },
            "theme": "dark"
        }));
        let sel = Selection {
            model: Some("gpt-5.5".into()),
            reasoning: Some("low".into()),
        };
        let next = with_selection(&c, "codex", &sel);
        assert_eq!(next.agent_id.as_deref(), Some("codex"));
        assert_eq!(next.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(
            next.extra["agentModels"],
            json!({
                "claude": { "model": "opus" },
                "codex": { "model": "gpt-5.5", "reasoning": "low", "serviceTier": "priority" }
            })
        );
        assert_eq!(selection(&next, "codex"), sel);
    }
}
