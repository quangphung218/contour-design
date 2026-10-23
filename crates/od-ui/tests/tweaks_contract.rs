//! Every `data-od-tweaks` example the agent is taught (designer charter,
//! tweaks template and its assets) must survive both readers: the preview
//! bridge's filter (`readTweaks` in `bridges/od-bridge.js`) and the host
//! panel's [`TweakDef`] decoding. A knob either one drops would silently
//! vanish from the panel.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_ui::preview_bridge::{TweakDef, TweakKind};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

const OPEN: &str = r#"<script type="application/json" data-od-tweaks>"#;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The JSON of every declaration block in `text`.
fn blocks(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        let body = &rest[at + OPEN.len()..];
        let end = body.find("</script>").expect("closed block");
        out.push(serde_json::from_str(&body[..end]).expect("block is JSON"));
        rest = &body[end..];
    }
    out
}

fn chars_ok(s: &str, max: usize, ok: impl Fn(char) -> bool) -> bool {
    (1..=max).contains(&s.chars().count()) && s.chars().all(ok)
}

/// `readTweaks`' filter, rule for rule.
fn bridge_keeps(t: &Value) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let id = t["id"].as_str().is_some_and(|id| chars_ok(id, 40, word));
    let kind = matches!(
        t["type"].as_str(),
        Some("color" | "range" | "select" | "toggle" | "text")
    );
    let var = t["var"]
        .as_str()
        .and_then(|v| v.strip_prefix("--"))
        .is_some_and(|v| chars_ok(v, 60, word));
    let attr = t["attr"]
        .as_str()
        .and_then(|a| a.strip_prefix("data-"))
        .is_some_and(|a| {
            chars_ok(a, 40, |c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
            })
        });
    id && kind && (var || attr)
}

fn check(source: &str, block: &Value) {
    let list = block["tweaks"].as_array().expect("{\"tweaks\": [...]}");
    assert!(
        !list.is_empty() && list.len() <= 24,
        "{source}: 1..=24 knobs"
    );
    for t in list {
        assert!(bridge_keeps(t), "{source}: the bridge drops {t}");
        assert!(
            t.get("var").is_none() || t.get("attr").is_none(),
            "{source}: one target: {t}"
        );
    }
    let defs = TweakDef::parse_list(list);
    assert_eq!(defs.len(), list.len(), "{source}: the panel drops a knob");
    let mut ids = HashSet::new();
    for d in &defs {
        assert!(ids.insert(d.id.clone()), "{source}: duplicate id {}", d.id);
        match d.kind {
            TweakKind::Select => assert!(
                d.options.iter().any(|o| o.value() == d.initial()),
                "{source}: {} default is one of its options",
                d.id
            ),
            TweakKind::Toggle => assert!(
                matches!(d.default, Some(Value::Bool(_))),
                "{source}: {} default is true or false",
                d.id
            ),
            TweakKind::Range => assert!(
                d.min.is_some() && d.max.is_some(),
                "{source}: {} has min and max",
                d.id
            ),
            TweakKind::Color | TweakKind::Text => {}
        }
    }
}

#[test]
fn taught_declarations_reach_the_panel() {
    for (source, expected) in [
        (
            "../od-content/prompts/legacy/OFFICIAL_DESIGNER_PROMPT.md",
            1,
        ),
        ("../../content/design-templates/tweaks/SKILL.md", 1),
        ("../../content/design-templates/tweaks/assets/wrap.html", 1),
        ("../../content/design-templates/tweaks/example.html", 1),
    ] {
        let found = blocks(&read(source));
        assert_eq!(found.len(), expected, "{source}: declaration blocks");
        for block in &found {
            check(source, block);
        }
    }
}

#[test]
fn the_checks_catch_what_the_readers_drop() {
    let bad = [
        r#"{"id": "a b", "type": "text", "var": "--a"}"#,
        r#"{"id": "a", "type": "slider", "var": "--a"}"#,
        r#"{"id": "a", "type": "text", "var": "a"}"#,
        r#"{"id": "a", "type": "text", "attr": "mode"}"#,
        r#"{"id": "a", "type": "text", "attr": "data-Mode"}"#,
    ];
    for t in bad {
        assert!(!bridge_keeps(&serde_json::from_str(t).unwrap()), "{t}");
    }
    // A numeric option value decodes in neither shape of TweakOption.
    let numeric: Value = serde_json::from_str(
        r#"{"id": "s", "type": "select", "var": "--s", "options": [{"label": "S", "value": 0.85}]}"#,
    )
    .unwrap();
    assert!(bridge_keeps(&numeric));
    assert!(TweakDef::parse_list(&[numeric]).is_empty());
}
