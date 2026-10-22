//! The bundled plugin presets (spec 3.7): 13 atoms and 10 scenarios load;
//! cloud, collab, rollout and connector scenarios are not shipped.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::plugins::list_plugins;
use serde_json::{Map, json};
use std::path::Path;

#[test]
fn bundled_plugins_load_as_presets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/plugins/_official");
    let all = list_plugins(&root, None);
    let atoms = all.iter().filter(|p| p.kind == "atom").count();
    let scenarios: Vec<&str> = all
        .iter()
        .filter(|p| p.kind == "scenario")
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(
        atoms,
        13,
        "{:?}",
        all.iter().map(|p| &p.id).collect::<Vec<_>>()
    );
    assert_eq!(scenarios.len(), 10, "{scenarios:?}");
    for gone in [
        "od-next-strategy",
        "od-share-to-community",
        "od-tune-collab",
        "od-figma-migration",
    ] {
        assert!(!scenarios.contains(&gone), "{gone} must not ship");
    }
    for p in &all {
        assert!(!p.body.is_empty(), "{} has no SKILL.md body", p.id);
        assert_eq!(p.source, "bundled");
    }
    let default = all.iter().find(|p| p.id == "od-default").unwrap();
    assert!(default.hidden);
    assert!(default.context.atoms.contains(&"todo-write".to_owned()));

    // Every scenario applies with its required inputs filled.
    for p in all.iter().filter(|p| p.kind == "scenario") {
        let mut raw = Map::new();
        for f in &p.inputs {
            if f["required"] == true {
                raw.insert(f["name"].as_str().unwrap().to_owned(), json!("x"));
            }
        }
        let a = p.apply(&raw, Some("vi")).unwrap();
        assert!(a.prompt_block.contains("## Active plugin"), "{}", p.id);
        assert!(
            a.prompt_block.contains("## Plugin instructions"),
            "{}",
            p.id
        );
    }
}
