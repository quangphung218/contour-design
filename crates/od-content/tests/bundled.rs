//! Loads the real `content/` folder and checks it against the folder count
//! and against golden output produced by the original app's TypeScript
//! loaders (`tests/fixtures/original-*.json`, see `tests/fixtures/README.md`).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::{ContentIndex, ContentRoots};
use od_contracts::{DesignSystemSummary, SkillInfo, SkillMode};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn index() -> ContentIndex {
    ContentIndex::build(&ContentRoots::bundled(&content_root()))
}

/// Child folders of `dir` (what `ls -d */` would print).
fn dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{} must exist: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

fn is_derived(id: &str) -> bool {
    id.contains(':')
}

#[test]
fn every_skill_folder_loads() {
    let idx = index();
    let folders = dirs(&content_root().join("skills"));
    assert!(folders.len() >= 150, "skills folder looks incomplete");
    let parents = idx
        .skills
        .iter()
        .filter(|s| !is_derived(&s.info.id))
        .count();
    assert_eq!(parents, folders.len(), "one skill per folder");
    for s in &idx.skills {
        assert!(SkillMode::ALL.contains(&s.info.mode));
        assert!(
            !s.body.is_empty() || s.info.id.contains(':'),
            "{} has a body",
            s.info.id
        );
    }
}

#[test]
fn every_design_template_folder_loads() {
    let idx = index();
    let folders = dirs(&content_root().join("design-templates"));
    assert!(
        folders.len() >= 100,
        "design-templates folder looks incomplete"
    );
    let parents = idx
        .design_templates
        .iter()
        .filter(|s| !is_derived(&s.info.id))
        .count();
    assert_eq!(parents, folders.len(), "one template per folder");
}

#[test]
fn every_design_system_folder_loads() {
    let idx = index();
    let folders = dirs(&content_root().join("design-systems"));
    let without_design_md: Vec<_> = folders
        .iter()
        .filter(|d| !d.join("DESIGN.md").is_file())
        .filter_map(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(
        without_design_md,
        ["_schema"],
        "only the schema folder has no DESIGN.md"
    );
    assert_eq!(idx.design_systems.len(), folders.len() - 1);
}

#[test]
fn craft_lists_every_markdown_file() {
    let idx = index();
    let files = std::fs::read_dir(content_root().join("craft"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".md")
        })
        .count();
    assert_eq!(idx.craft.len(), files);
    assert!(idx.craft.iter().any(|c| c.id == "typography"));
}

/// Numbers compare by value: the original writes `1`, serde may write `1.0`.
fn normalize(v: Value) -> Value {
    match v {
        Value::Number(n) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Number(n), Value::Number),
        Value::Array(items) => Value::Array(items.into_iter().map(normalize).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, normalize(v))).collect())
        }
        other => other,
    }
}

fn golden(name: &str) -> BTreeMap<String, Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let raw = std::fs::read_to_string(path).unwrap();
    let items: Vec<Value> = serde_json::from_str(&raw).unwrap();
    items
        .into_iter()
        .map(|v| (v["id"].as_str().unwrap().to_owned(), normalize(v)))
        .collect()
}

fn ours_skills(list: &[od_content::SkillEntry]) -> BTreeMap<String, Value> {
    list.iter()
        .map(|e| {
            let mut v = serde_json::to_value(&e.info).unwrap();
            // The golden run scanned a single root, so `source` is not comparable.
            v.as_object_mut().unwrap().remove("source");
            (e.info.id.clone(), normalize(v))
        })
        .collect()
}

fn assert_same(kind: &str, ours: &BTreeMap<String, Value>, theirs: &BTreeMap<String, Value>) {
    let ours_ids: Vec<_> = ours.keys().collect();
    let their_ids: Vec<_> = theirs.keys().collect();
    assert_eq!(ours_ids, their_ids, "{kind}: same ids as the original");
    let mismatches: Vec<String> = ours
        .iter()
        .filter(|(id, v)| theirs.get(*id) != Some(v))
        .map(|(id, v)| format!("{id}\n  ours:   {v}\n  theirs: {}", theirs[id]))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{kind}: {} entries differ from the original:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn skills_match_original_loader() {
    let idx = index();
    assert_same(
        "skills",
        &ours_skills(&idx.skills),
        &golden("original-skills.json"),
    );
}

#[test]
fn design_templates_match_original_loader() {
    let idx = index();
    assert_same(
        "design-templates",
        &ours_skills(&idx.design_templates),
        &golden("original-design-templates.json"),
    );
}

#[test]
fn design_systems_match_original_loader() {
    let idx = index();
    let ours: BTreeMap<String, Value> = idx
        .design_systems
        .iter()
        .map(|d| {
            (
                d.summary.id.clone(),
                normalize(serde_json::to_value(&d.summary).unwrap()),
            )
        })
        .collect();
    assert_same(
        "design-systems",
        &ours,
        &golden("original-design-systems.json"),
    );
}

#[test]
fn featured_entries_sort_first() {
    let idx = index();
    let ranks: Vec<f64> = idx
        .skills
        .iter()
        .map(|s| s.info.featured.unwrap_or(f64::INFINITY))
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn typed_round_trip() {
    let idx = index();
    let info: &SkillInfo = &idx.skills[0].info;
    let back: SkillInfo = serde_json::from_value(serde_json::to_value(info).unwrap()).unwrap();
    assert_eq!(&back, info);
    let ds: &DesignSystemSummary = &idx.design_systems[0].summary;
    let back: DesignSystemSummary =
        serde_json::from_value(serde_json::to_value(ds).unwrap()).unwrap();
    assert_eq!(&back, ds);
}

/// The `data-od-tweaks` declaration the preview bridge reads must reach
/// the agent: in the designer charter of every design-mode prompt, and in
/// the tweaks template when it is the active skill. The retired markers
/// (`EDITMODE` comments, `__edit_mode_*` messages) are only named as
/// things not to do.
#[test]
fn prompt_teaches_the_tweaks_declaration() {
    use od_content::prompt::{PromptInput, compose_instructions};
    let idx = index();
    let tweaks = idx
        .design_templates
        .iter()
        .find(|t| t.info.id == "tweaks")
        .expect("tweaks template");

    let plain = compose_instructions(&PromptInput::default());
    assert!(plain.contains("## Tweaks (in-design controls)"));
    assert!(plain.contains(r#"<script type="application/json" data-od-tweaks>"#));
    assert!(plain.contains("window.addEventListener('od:tweak'"));
    assert!(!plain.contains("/*EDITMODE-BEGIN*/"), "old marker example");
    assert!(!plain.contains("TWEAK_DEFAULTS"), "old marker example");

    let with_skill = compose_instructions(&PromptInput {
        skills: &[tweaks],
        ..PromptInput::default()
    });
    let skill = &with_skill[with_skill
        .find("## Active skill — tweaks")
        .expect("skill block")..];
    assert!(skill.contains("## Host integration contract (REQUIRED)"));
    assert!(skill.contains(r#"<script type="application/json" data-od-tweaks>"#));
    assert!(skill.contains("window.__odBridge"));
    assert!(
        !skill.contains("{ type: '__edit_mode_available'"),
        "old protocol"
    );
    assert!(!skill.contains("Protocol B"), "old protocol");

    // Chat mode has no designer charter, so no tweaks contract either.
    let chat = compose_instructions(&PromptInput {
        session_mode: od_contracts::SessionMode::Chat,
        ..PromptInput::default()
    });
    assert!(!chat.contains("data-od-tweaks"));
}
