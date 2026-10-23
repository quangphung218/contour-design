//! When the memory toast shows (the original's `MemoryToast`): a batched
//! `extract` change that saved something, not a manual edit.

use crate::i18n::{T, tf};
use serde_json::Value;

/// The toast's text for a `/api/memory/events` change, if it shows one.
#[must_use]
pub fn toast_text(change: &Value) -> Option<String> {
    let count = change["count"].as_u64().unwrap_or(0);
    if change["kind"] != "extract" || count == 0 || change["source"] == "manual" {
        return None;
    }
    let key = if change["scope"] == "project" {
        T::MemoryToastProject
    } else {
        T::MemoryToastGlobal
    };
    Some(tf(key, &[&count.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_learned_facts_show_a_toast() {
        let learned =
            json!({ "kind": "extract", "count": 2, "source": "heuristic", "scope": "global" });
        assert_eq!(toast_text(&learned).as_deref(), Some("Memory updated (2)"));
        let project =
            json!({ "kind": "extract", "count": 1, "source": "heuristic", "scope": "project" });
        assert_eq!(
            toast_text(&project).as_deref(),
            Some("Saved to project memory (1)")
        );
        for quiet in [
            json!({ "kind": "extract", "count": 0, "source": "heuristic" }),
            json!({ "kind": "extract", "count": 1, "source": "manual" }),
            json!({ "kind": "index" }),
            json!({ "kind": "config", "enabled": false }),
        ] {
            assert_eq!(toast_text(&quiet), None, "{quiet}");
        }
    }
}
