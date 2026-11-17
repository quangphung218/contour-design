//! App settings as stored by `GET/PUT /api/app-config`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// The agent approves its own tool calls (decisions #1 default).
    #[default]
    Auto,
    /// Claude uses `acceptEdits`; other CLIs drop their auto-approve flag.
    Ask,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub agent_permission_mode: PermissionMode,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub onboarding_completed: bool,
    /// UI language tag (`en`, `vi`); absent: follow the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Settings this UI does not know, kept on round trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Setting key of the design system new projects start with (the
/// original's `designSystemId`).
pub const DEFAULT_DESIGN_SYSTEM_KEY: &str = "designSystemId";

impl AppConfig {
    /// The design system new chats default to.
    #[must_use]
    pub fn default_design_system(&self) -> Option<String> {
        self.extra
            .get(DEFAULT_DESIGN_SYSTEM_KEY)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }

    /// `PUT` body: every known field, with `null` clearing empty options so
    /// a blank model really resets to the agent's default.
    #[must_use]
    pub fn to_patch(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            for key in ["agentId", "model"] {
                let empty = m
                    .get(key)
                    .and_then(Value::as_str)
                    .is_none_or(|s| s.trim().is_empty());
                if empty {
                    m.insert(key.to_owned(), Value::Null);
                }
            }
        }
        v
    }

    /// `PUT` body holding only the leaves that differ from `base`, the copy
    /// `self` was edited from, as a JSON Merge Patch (RFC 7396): objects
    /// are compared key by key, a key `self` dropped is sent as `null`, an
    /// array that changed is sent whole. A window opened before another
    /// one changed something then sends back only what it changed itself,
    /// even inside `agentModels` or `notifications` (QA-108). A changed
    /// `byok` always carries its whole route (protocol, base URL, API
    /// version).
    #[must_use]
    pub fn patch_since(&self, base: &Self) -> Value {
        let (Value::Object(next), Value::Object(base)) = (self.to_patch(), base.to_patch()) else {
            return self.to_patch();
        };
        let mut out = diff(&base, &next);
        // The provider's route is one block: sent by leaf, an unchanged
        // protocol would be left out and a key typed here would land on
        // the route another window stored since.
        if let Some(Value::Object(sent)) = out.get_mut(BYOK_KEY) {
            let route = next.get(BYOK_KEY).and_then(Value::as_object);
            for key in BYOK_ROUTE {
                let value = route.and_then(|r| r.get(key)).cloned();
                sent.insert(key.to_owned(), value.unwrap_or(Value::Null));
            }
        }
        Value::Object(out)
    }
}

const BYOK_KEY: &str = "byok";
/// The `byok` fields that say where its key is sent.
const BYOK_ROUTE: [&str; 3] = ["protocol", "baseUrl", "apiVersion"];

/// The merge patch that turns `base` into `next`.
fn diff(base: &Map<String, Value>, next: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in next {
        match (base.get(key), value) {
            (Some(Value::Object(b)), Value::Object(n)) => {
                let inner = diff(b, n);
                if !inner.is_empty() {
                    out.insert(key.clone(), Value::Object(inner));
                }
            }
            (Some(b), n) if b == n => {}
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    for key in base.keys() {
        if !next.contains_key(key) {
            out.insert(key.clone(), Value::Null);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_unknown_keys_and_defaults() {
        let c: AppConfig =
            serde_json::from_value(json!({"agentId": "claude", "future": 1})).unwrap_or_default();
        assert_eq!(c.agent_permission_mode, PermissionMode::Auto);
        assert_eq!(c.theme, Theme::System);
        assert!(!c.onboarding_completed);
        let patch = c.to_patch();
        assert_eq!(patch["future"], 1);
        assert_eq!(patch["model"], Value::Null);
        assert_eq!(patch["agentPermissionMode"], "auto");
    }

    fn config(v: Value) -> AppConfig {
        serde_json::from_value(v).unwrap_or_default()
    }

    #[test]
    fn patch_since_sends_only_changed_keys() {
        let base = config(json!({"agentId": "byok", "theme": "dark", "locale": "vi", "future": 1}));
        let mut next = base.clone();
        next.agent_permission_mode = PermissionMode::Ask;
        assert_eq!(
            next.patch_since(&base),
            json!({"agentPermissionMode": "ask"})
        );
        assert_eq!(base.patch_since(&base), json!({}));
    }

    #[test]
    fn patch_since_sends_a_return_to_the_default() {
        let base = config(json!({"theme": "dark", "agentPermissionMode": "ask", "model": "opus"}));
        let mut next = base.clone();
        next.theme = Theme::System;
        next.agent_permission_mode = PermissionMode::Auto;
        next.model = None;
        assert_eq!(
            next.patch_since(&base),
            json!({"theme": "system", "agentPermissionMode": "auto", "model": null})
        );
    }

    #[test]
    fn patch_since_clears_dropped_keys() {
        let base = config(json!({"locale": "vi", "designSystemId": "ds-1", "future": 1}));
        let mut next = base.clone();
        next.locale = None;
        next.extra.remove(DEFAULT_DESIGN_SYSTEM_KEY);
        assert_eq!(
            next.patch_since(&base),
            json!({"locale": null, "designSystemId": null})
        );
    }

    #[test]
    fn patch_since_sends_new_keys_whole_and_nested_changes_by_leaf() {
        let base = config(json!({"agentModels": {"claude": {"model": "opus"}}}));
        let mut next = base.clone();
        next.locale = Some("en".to_owned());
        next.extra.insert(
            "agentModels".to_owned(),
            json!({"claude": {"model": "sonnet"}}),
        );
        next.extra
            .insert("notifications".to_owned(), json!({"sound": false}));
        assert_eq!(
            next.patch_since(&base),
            json!({
                "locale": "en",
                "agentModels": {"claude": {"model": "sonnet"}},
                "notifications": {"sound": false}
            })
        );
    }

    /// A window still showing Anthropic types a new key while another one
    /// moved the provider to an OpenAI host: the key goes out with the
    /// route this window shows, never alone.
    #[test]
    fn patch_since_sends_the_byok_route_as_one_block() {
        let base = config(json!({"byok": {
            "protocol": "anthropic", "model": "claude-x", "hasApiKey": true
        }}));
        let mut next = base.clone();
        next.extra.insert(
            "byok".to_owned(),
            json!({"protocol": "anthropic", "baseUrl": null, "model": "claude-x",
                   "apiKey": "sk-ant-NEW", "apiVersion": null}),
        );
        assert_eq!(
            next.patch_since(&base)["byok"],
            json!({"protocol": "anthropic", "baseUrl": null, "apiVersion": null,
                   "apiKey": "sk-ant-NEW", "hasApiKey": null})
        );
        // Only the model changed: the route still goes with it.
        let mut next = base.clone();
        next.extra.insert(
            "byok".to_owned(),
            json!({"protocol": "anthropic", "model": "claude-y", "hasApiKey": true}),
        );
        assert_eq!(
            next.patch_since(&base)["byok"],
            json!({"protocol": "anthropic", "baseUrl": null, "apiVersion": null,
                   "model": "claude-y"})
        );
        assert_eq!(
            base.patch_since(&base),
            json!({}),
            "no byok change, no route"
        );
    }

    /// A window that read `agentModels` before another one added Codex
    /// sends only its own agent's change, so the server keeps Codex.
    #[test]
    fn patch_since_sends_only_the_changed_leaves_of_an_object() {
        let base = config(json!({
            "agentModels": {"claude": {"model": "opus", "reasoning": "high"}},
            "notifications": {"soundEnabled": true, "failureSound": "buzz"}
        }));
        let mut next = base.clone();
        next.extra.insert(
            "agentModels".to_owned(),
            json!({"claude": {"model": "sonnet"}}),
        );
        next.extra.insert(
            "notifications".to_owned(),
            json!({"soundEnabled": true, "failureSound": "buzz"}),
        );
        assert_eq!(
            next.patch_since(&base),
            json!({"agentModels": {"claude": {"model": "sonnet", "reasoning": null}}})
        );
    }

    #[test]
    fn patch_since_sends_a_changed_array_whole() {
        let base = config(json!({"recentDirs": ["/a", "/b"], "byok": {"x": 1}}));
        let mut next = base.clone();
        next.extra
            .insert("recentDirs".to_owned(), json!(["/c", "/a", "/b"]));
        next.extra.insert("byok".to_owned(), json!("odd"));
        assert_eq!(
            next.patch_since(&base),
            json!({"recentDirs": ["/c", "/a", "/b"], "byok": "odd"})
        );
    }
}
