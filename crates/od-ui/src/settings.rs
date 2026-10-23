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
}
