//! Messages of the preview bridge (`bridges/od-bridge.js`, spec 4.1) as the
//! host sees them. Decoding is lenient: unknown types become `Other`.

use od_contracts::CommentRect;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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

/// Where a located comment's element is now (`None`: no longer matches).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Located {
    pub id: String,
    #[serde(default)]
    pub rect: Option<CommentRect>,
}

/// An inspected element (`od:inspect-result`): computed styles of the
/// inspected properties and the overrides set on it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspected {
    pub selector: String,
    /// The selector no longer matches (the page reloaded or changed).
    #[serde(default)]
    pub missing: bool,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub styles: BTreeMap<String, String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub text_only: bool,
}

/// One inline style override, as set in the preview.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct Override {
    pub selector: String,
    pub prop: String,
    pub value: String,
}

/// Text changed in the preview (`od:text-edit`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TextEdit {
    pub selector: String,
    #[serde(default)]
    pub before: String,
    pub text: String,
}

/// Reply to `od:snapshot`: an SVG or PNG data URL, or an error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub request_id: String,
    #[serde(default)]
    pub data_url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// How a tweak is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TweakKind {
    Color,
    Range,
    Select,
    Toggle,
    Text,
}

/// A choice of a `select` tweak: `"0.75"` or `{label, value}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TweakOption {
    Plain(String),
    Labelled { label: String, value: String },
}

impl TweakOption {
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Self::Plain(v) | Self::Labelled { value: v, .. } => v,
        }
    }

    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Plain(v) | Self::Labelled { label: v, .. } => v,
        }
    }
}

/// A knob the artifact declares (`data-od-tweaks`, see `bridges/README.md`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TweakDef {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: TweakKind,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub options: Vec<TweakOption>,
    #[serde(default)]
    pub default: Option<Value>,
}

impl TweakDef {
    /// The defs that decode; a malformed one is skipped, not fatal.
    #[must_use]
    pub fn parse_list(raw: &[Value]) -> Vec<Self> {
        raw.iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect()
    }

    #[must_use]
    pub fn title(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.id)
    }

    /// The starting value as the control shows it.
    #[must_use]
    pub fn initial(&self) -> String {
        match &self.default {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => match self.kind {
                TweakKind::Select => self
                    .options
                    .first()
                    .map(|o| o.value().to_owned())
                    .unwrap_or_default(),
                TweakKind::Toggle => "false".to_owned(),
                TweakKind::Range => self.min.unwrap_or(0.0).to_string(),
                TweakKind::Color => "#000000".to_owned(),
                TweakKind::Text => String::new(),
            },
        }
    }
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
    #[serde(rename = "od:located")]
    Located { items: Vec<Located> },
    #[serde(rename = "od:inspect-result")]
    InspectResult(Inspected),
    #[serde(rename = "od:inspect-overrides")]
    InspectOverrides {
        #[serde(default)]
        items: Vec<Override>,
        #[serde(default)]
        css: String,
    },
    #[serde(rename = "od:text-edit")]
    TextEdit(TextEdit),
    #[serde(rename = "od:tweaks")]
    Tweaks {
        #[serde(default)]
        tweaks: Vec<Value>,
    },
    #[serde(rename = "od:palette-state")]
    PaletteState {
        #[serde(default)]
        id: Option<String>,
    },
    #[serde(rename = "od:snapshot-result")]
    SnapshotResult(Snapshot),
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

/// Host → bridge: report (and keep reporting) where these comments'
/// elements are, by `(id, selector)`.
#[must_use]
pub fn locate<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Value {
    let items: Vec<Value> = items
        .into_iter()
        .map(|(id, selector)| json!({ "id": id, "selector": selector }))
        .collect();
    json!({ "type": "od:locate", "items": items })
}

/// Host → bridge: report `selector`'s computed style and overrides.
#[must_use]
pub fn inspect(selector: &str) -> Value {
    json!({ "type": "od:inspect", "selector": selector })
}

/// Host → bridge: override one inline style property.
#[must_use]
pub fn inspect_set(selector: &str, prop: &str, value: &str) -> Value {
    json!({ "type": "od:inspect-set", "selector": selector, "prop": prop, "value": value })
}

/// Host → bridge: undo overrides of `selector` (one `prop`, or all), or of
/// every element when `selector` is `None`.
#[must_use]
pub fn inspect_reset(selector: Option<&str>, prop: Option<&str>) -> Value {
    json!({ "type": "od:inspect-reset", "selector": selector, "prop": prop })
}

/// Host → bridge: set these overrides again (after a reload).
#[must_use]
pub fn inspect_replay(items: &[Override]) -> Value {
    json!({ "type": "od:inspect-replay", "items": items })
}

/// Host → bridge: report every override as items and CSS text.
#[must_use]
pub fn inspect_extract() -> Value {
    json!({ "type": "od:inspect-extract" })
}

/// Host → bridge: text-edit mode on or off.
#[must_use]
pub fn edit_mode(enabled: bool) -> Value {
    json!({ "type": "od:edit-mode", "enabled": enabled })
}

/// Palette presets the bridge knows, in menu order.
pub const PALETTES: [&str; 5] = ["coral", "electric", "acid-forest", "risograph", "mono-noir"];

/// Host → bridge: recolor with a preset, or restore with `None`.
#[must_use]
pub fn palette(id: Option<&str>) -> Value {
    json!({ "type": "od:palette", "id": id })
}

/// Host → bridge: set a declared tweak.
#[must_use]
pub fn tweak_set(id: &str, value: &str) -> Value {
    json!({ "type": "od:tweak-set", "id": id, "value": value })
}

/// Host → bridge: a PNG snapshot of the visible viewport.
#[must_use]
pub fn snapshot(request_id: &str) -> Value {
    json!({ "type": "od:snapshot", "requestId": request_id, "format": "png" })
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
        assert_eq!(
            BridgeMsg::decode(
                r#"{"type":"od:located","items":[{"id":"c1","rect":{"x":1,"y":2,"width":3,"height":4}},{"id":"c2","rect":null}]}"#
            ),
            Some(BridgeMsg::Located {
                items: vec![
                    Located {
                        id: "c1".into(),
                        rect: Some(CommentRect {
                            x: 1.0,
                            y: 2.0,
                            width: 3.0,
                            height: 4.0
                        })
                    },
                    Located {
                        id: "c2".into(),
                        rect: None
                    },
                ]
            })
        );
        assert_eq!(
            locate([("c1", "h1")]),
            json!({"type": "od:locate", "items": [{"id": "c1", "selector": "h1"}]})
        );
        assert_eq!(BridgeMsg::decode(r#"{"type":"webpack"}"#), None);
        let Some(BridgeMsg::InspectResult(i)) = BridgeMsg::decode(
            r#"{"type":"od:inspect-result","selector":"h1","tag":"h1","styles":{"color":"rgb(0, 0, 0)"},"overrides":{"color":"red"},"textOnly":true,"rect":{}}"#,
        ) else {
            panic!("inspect");
        };
        assert_eq!(i.styles["color"], "rgb(0, 0, 0)");
        assert_eq!(i.overrides["color"], "red");
        assert!(i.text_only && !i.missing);
        assert_eq!(
            BridgeMsg::decode(
                r#"{"type":"od:text-edit","selector":"h1","odId":null,"tag":"h1","before":"A","text":"B"}"#
            ),
            Some(BridgeMsg::TextEdit(TextEdit {
                selector: "h1".into(),
                before: "A".into(),
                text: "B".into()
            }))
        );
        let Some(BridgeMsg::Tweaks { tweaks }) = BridgeMsg::decode(
            r##"{"type":"od:tweaks","tweaks":[{"id":"accent","type":"color","var":"--accent","default":"#c96442"},{"id":"bad","type":"dial"},{"id":"d","type":"select","options":["1",{"label":"Roomy","value":"1.4"}]}]}"##,
        ) else {
            panic!("tweaks");
        };
        let defs = TweakDef::parse_list(&tweaks);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].initial(), "#c96442");
        assert_eq!(defs[1].initial(), "1");
        assert_eq!(defs[1].options[1].label(), "Roomy");
        assert_eq!(defs[1].options[1].value(), "1.4");
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
