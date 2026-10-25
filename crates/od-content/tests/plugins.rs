//! The bundled plugin presets (spec 3.7): 13 atoms and 10 scenarios load;
//! cloud, collab, rollout and connector scenarios are not shipped.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::plugins::list_plugins;
use serde_json::{Map, Value, json};
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

/// The Plugins page shows each plugin's title and description: they say
/// what the user gets. Internal names (kind, task kind, atoms, pipeline
/// stages) appear only under Developer details, which reads the manifest.
#[test]
fn bundled_plugin_text_is_for_people() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/plugins/_official");
    let internal = regex::Regex::new(
        r"(?i)task ?kind|\batoms?\b|scenario|devloop|pipeline|\bsignals?\b|downstream|\bstarter\b|media-\*|patch-edit|build-test|diff-review|live artifact",
    )
    .unwrap();
    let mut problems = Vec::new();
    for p in list_plugins(&root, None) {
        let m = &p.manifest;
        let mut texts = vec![
            ("title", p.title.clone()),
            ("description", p.description.clone()),
        ];
        for key in ["title_i18n", "description_i18n"] {
            for (tag, v) in m.get(key).and_then(Value::as_object).into_iter().flatten() {
                texts.push((key, format!("{tag}: {}", v.as_str().unwrap_or_default())));
            }
        }
        // The example brief shows on the plugin page, outside Developer
        // details, and is what the user sends: their words, not step names.
        match m
            .get("od")
            .and_then(|od| od.get("useCase"))
            .and_then(|u| u.get("query"))
        {
            Some(Value::String(q)) => texts.push(("query", q.clone())),
            Some(Value::Object(q)) => {
                for (tag, v) in q {
                    texts.push((
                        "query",
                        format!("{tag}: {}", v.as_str().unwrap_or_default()),
                    ));
                }
            }
            _ => {}
        }
        for (field, text) in texts {
            if internal.is_match(&text) {
                problems.push(format!("{} {field}: {text}", p.id));
            }
            // A localized "(default scenario)" suffix in any language.
            if field.starts_with("title") && text.trim_end().ends_with([')', '）']) {
                problems.push(format!("{} {field} ends with a note: {text}", p.id));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
