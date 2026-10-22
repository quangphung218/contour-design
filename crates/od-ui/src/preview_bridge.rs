//! Messages of the preview bridge (`bridges/od-bridge.js`, spec 4.1) as the
//! host sees them. Decoding is lenient: unknown types become `Other`.

use od_contracts::CommentRect;
use serde::Deserialize;
use serde_json::{Value, json};

/// An element picked in comment mode (`od:select`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picked {
    #[serde(default)]
    pub od_id: Option<String>,
    pub selector: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub rect: CommentRect,
}

/// Bridge → host.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeMsg {
    #[serde(rename = "od:ready")]
    Ready,
    #[serde(rename = "od:deck-ready")]
    DeckReady { count: u32 },
    #[serde(rename = "od:slide-state")]
    SlideState { active: u32, count: u32 },
    #[serde(rename = "od:esc")]
    Esc,
    #[serde(rename = "od:select")]
    Select(Picked),
    #[serde(other)]
    Other,
}

impl BridgeMsg {
    /// `None` for anything that is not a bridge message.
    #[must_use]
    pub fn decode(json: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(json).ok()?;
        let ty = v.get("type")?.as_str()?;
        if !ty.starts_with("od:") {
            return None;
        }
        Some(serde_json::from_value(v).unwrap_or(Self::Other))
    }
}

/// Deck navigation (`od:slide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slide {
    Next,
    Prev,
    First,
    Last,
    Go(u32),
}

impl Slide {
    /// Host → bridge message.
    #[must_use]
    pub fn message(self) -> Value {
        let (action, index) = match self {
            Self::Next => ("next", None),
            Self::Prev => ("prev", None),
            Self::First => ("first", None),
            Self::Last => ("last", None),
            Self::Go(i) => ("go", Some(i)),
        };
        let mut m = json!({ "type": "od:slide", "action": action, "protocolVersion": 1 });
        if let Some(i) = index {
            m["index"] = json!(i);
        }
        m
    }
}

/// Host → bridge: comment (selection) mode on or off.
#[must_use]
pub fn select_mode(enabled: bool) -> Value {
    json!({ "type": "od:select-mode", "enabled": enabled })
}

/// Host → bridge: ask for the current slide.
#[must_use]
pub fn slide_state_request() -> Value {
    json!({ "type": "od:slide-state-request" })
}

/// Where a deck stands, as last reported by the bridge (0-based active).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckState {
    pub active: u32,
    pub count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_bridge_messages() {
        assert_eq!(
            BridgeMsg::decode(
                r#"{"type":"od:slide-state","protocolVersion":1,"active":2,"count":5}"#
            ),
            Some(BridgeMsg::SlideState {
                active: 2,
                count: 5
            })
        );
        assert_eq!(
            BridgeMsg::decode(r#"{"type":"od:deck-ready","count":3,"capabilities":[]}"#),
            Some(BridgeMsg::DeckReady { count: 3 })
        );
        let Some(BridgeMsg::Select(p)) = BridgeMsg::decode(
            r#"{"type":"od:select","odId":null,"selector":"html > body:nth-of-type(1)","tag":"body","text":"x","rect":{"x":1,"y":2,"width":3,"height":4}}"#,
        ) else {
            panic!("select");
        };
        assert_eq!(p.tag, "body");
        assert_eq!(p.rect.height, 4.0);
        assert_eq!(
            BridgeMsg::decode(r#"{"type":"od:size","width":1}"#),
            Some(BridgeMsg::Other)
        );
        assert_eq!(BridgeMsg::decode(r#"{"type":"webpack"}"#), None);
        assert_eq!(BridgeMsg::decode("3"), None);
    }

    #[test]
    fn host_commands() {
        assert_eq!(
            Slide::Go(4).message(),
            json!({"type": "od:slide", "action": "go", "index": 4, "protocolVersion": 1})
        );
        assert_eq!(Slide::Next.message()["action"], "next");
        assert!(Slide::Next.message().get("index").is_none());
        assert_eq!(select_mode(true)["enabled"], true);
    }
}
