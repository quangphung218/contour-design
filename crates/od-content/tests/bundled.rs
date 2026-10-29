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

/// A derived example's UI name and description come from `od.examples`,
/// which only Contour reads; every other field must still match.
fn without_example_text(mut map: BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    for (id, v) in &mut map {
        if is_derived(id) {
            let o = v.as_object_mut().unwrap();
            o.remove("displayName");
            o.remove("descriptionI18n");
        }
    }
    map
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

/// Templates left out of the bundle (NOTICE): OpenDesign's own marketing
/// page and deck.
const REMOVED_TEMPLATES: [&str; 2] = ["open-design-landing", "open-design-landing-deck"];

#[test]
fn design_templates_match_original_loader() {
    let idx = index();
    let mut theirs = golden("original-design-templates.json");
    theirs.retain(|id, _| {
        let parent = id.split(':').next().unwrap_or(id);
        !REMOVED_TEMPLATES.contains(&parent)
    });
    assert_same(
        "design-templates",
        &without_example_text(ours_skills(&idx.design_templates)),
        &without_example_text(theirs),
    );
}

/// One industry, one id: the Templates filter lists each id, so
/// "operation" next to "operations" read as two industries.
#[test]
fn no_industry_is_the_plural_of_another() {
    let idx = index();
    let ids: std::collections::BTreeSet<&str> = idx
        .design_templates
        .iter()
        .map(|t| t.info.scenario.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    let twins: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| ids.contains(format!("{id}s").as_str()))
        .collect();
    assert!(twins.is_empty(), "industries with a plural twin: {twins:?}");
}

/// Nothing that ships points an agent or a page at a removed template.
#[test]
fn no_content_names_a_removed_template() {
    let hits: Vec<String> = walk(&content_root())
        .into_iter()
        .filter(|path| {
            let bytes = std::fs::read(path).unwrap();
            REMOVED_TEMPLATES
                .iter()
                .any(|id| bytes.windows(id.len()).any(|w| w == id.as_bytes()))
        })
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        hits.is_empty(),
        "files naming a removed template: {hits:#?}"
    );
}

/// Whether `dir` ships a license file of its own.
fn has_license(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().any(|e| {
        let name = e.unwrap().file_name().to_string_lossy().to_lowercase();
        name.starts_with("license") || name.starts_with("licence")
    })
}

/// A third-party upstream: a URL outside nexu-io, the OpenDesign org whose
/// work the top-level LICENSE (Apache-2.0) covers.
fn third_party(upstream: &str) -> bool {
    upstream.contains("://") && !upstream.contains("github.com/nexu-io/")
}

/// Every skill or template copied from a third party keeps its upstream's
/// LICENSE. A stub (a lone SKILL.md under 4 KB that points at its upstream
/// for the real files) copies nothing and needs none.
#[test]
fn every_third_party_entry_ships_its_license() {
    let mut missing = Vec::new();
    for kind in ["skills", "design-templates"] {
        for dir in dirs(&content_root().join(kind)) {
            let raw = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
            let (data, _) = od_content::frontmatter::parse(&raw).unwrap();
            let data = Value::Object(data);
            let upstream = data["od"]["upstream"]
                .as_str()
                .or_else(|| data["upstream"].as_str())
                .unwrap_or_default();
            let files = walk(&dir);
            let stub = files.len() == 1 && raw.len() < 4096;
            if third_party(upstream) && !stub && !has_license(&dir) {
                missing.push(format!(
                    "{kind}/{} ({upstream})",
                    dir.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    for dir in dirs(&content_root().join("design-systems")) {
        let Ok(raw) = std::fs::read_to_string(dir.join("manifest.json")) else {
            continue;
        };
        let manifest: Value = serde_json::from_str(&raw).unwrap();
        let source = manifest["source"].to_string();
        if third_party(&source) && !has_license(&dir) {
            missing.push(format!(
                "design-systems/{} ({source})",
                dir.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    // Whole folders derived from MIT projects (refero_skill; awesome-design-md
    // and the other design-system catalogues).
    for folder in ["craft", "design-systems"] {
        if !has_license(&content_root().join(folder)) {
            missing.push(folder.to_owned());
        }
    }
    assert!(
        missing.is_empty(),
        "third-party content without a LICENSE: {missing:#?}"
    );
}

/// Every file under `dir`, recursively.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// English text of an i18n map.
fn en(map: Option<&BTreeMap<String, String>>) -> Option<&str> {
    map.and_then(|m| m.get("en")).map(String::as_str)
}

/// Mode inference reads the whole body, so a page that says "not animated"
/// once is filed as video. Bundled templates state their mode instead.
#[test]
fn every_design_template_declares_its_mode() {
    let undeclared: Vec<String> = dirs(&content_root().join("design-templates"))
        .iter()
        .filter_map(|dir| {
            let raw = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
            let (data, _) = od_content::frontmatter::parse(&raw).unwrap();
            let declared = data["od"]["mode"]
                .as_str()
                .and_then(SkillMode::parse)
                .is_some();
            (!declared).then(|| dir.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    assert!(undeclared.is_empty(), "no od.mode: {undeclared:?}");
    let idx = index();
    let mode = |id: &str| {
        idx.design_templates
            .iter()
            .find(|t| t.info.id == id)
            .unwrap()
            .info
            .mode
    };
    assert_eq!(mode("web-prototype-taste-brutalist"), SkillMode::Prototype);
    assert_eq!(mode("html-ppt-taste-brutalist"), SkillMode::Deck);
    // Platform is inferred from the body too: a "Mobile:" checklist line
    // must not file a desktop page under Mobile.
    let platform = |id: &str| {
        idx.design_templates
            .iter()
            .find(|t| t.info.id == id)
            .unwrap()
            .info
            .platform
    };
    assert_eq!(
        platform("web-prototype-taste-soft"),
        Some(od_contracts::Platform::Desktop)
    );
}

/// What the Templates page shows: a name in words, not an id, and a
/// description for people. The agent-facing `description` (with "Use when
/// the brief asks for…") stays for routing but is never the card text.
#[test]
fn design_templates_have_text_for_people() {
    let trigger = regex::Regex::new(
        r"(?i)\b(use|trigger|select)(ed)?( this)?( template| skill)?( when| by)\b|\bwhen (the )?(user|brief)|\b(brief|user)s? (asks?|mentions?|wants?)",
    )
    .unwrap();
    let idx = index();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut problems = Vec::new();
    for t in &idx.design_templates {
        let id = &t.info.id;
        match en(t.info.display_name.as_ref()) {
            Some(name) if !name.trim().is_empty() => {
                if let Some(other) = names.insert(name.to_owned(), id.clone()) {
                    problems.push(format!("{id}: same name as {other}"));
                }
            }
            _ => problems.push(format!("{id}: no en_name")),
        }
        let Some(text) = en(t.info.description_i18n.as_ref()) else {
            problems.push(format!("{id}: no en_description"));
            continue;
        };
        if trigger.is_match(text) {
            problems.push(format!("{id}: agent wording in {text:?}"));
        }
        // Sample decks are about made-up companies, not about this app:
        // "Contour Teams" or "Contour's Series A" would read as real.
        if ["OpenDesign", "Open Design", "Open-Design"]
            .iter()
            .any(|n| text.contains(n))
        {
            problems.push(format!("{id}: names the original app in {text:?}"));
        }
        if let Some(other) = seen.insert(text.to_owned(), id.clone()) {
            problems.push(format!("{id}: same description as {other}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn design_systems_match_original_loader() {
    let idx = index();
    let ours: BTreeMap<String, Value> = idx
        .design_systems
        .iter()
        .map(|d| {
            let mut v = serde_json::to_value(&d.summary).unwrap();
            // Contour-only fields; every field the original has must match.
            let o = v.as_object_mut().unwrap();
            o.remove("description");
            o.remove("brand");
            (d.summary.id.clone(), normalize(v))
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

/// Brand-named design systems ship with a non-affiliation note
/// (`docs/decisions.md` §3): each states its brand in `manifest.json`, the
/// app shows the note, and NOTICE lists the same brands.
#[test]
fn brand_named_design_systems_are_listed_in_notice() {
    // CRLF when git checks it out on Windows.
    let notice = std::fs::read_to_string(content_root().join("../NOTICE"))
        .unwrap()
        .replace("\r\n", "\n");
    let idx = index();
    let brands: Vec<(String, String)> = idx
        .design_systems
        .iter()
        .filter_map(|d| {
            let brand = d.manifest.as_ref()?.get("brand")?.as_str()?;
            Some((d.summary.id.clone(), brand.to_owned()))
        })
        .collect();
    assert!(brands.len() >= 90, "{} brand-named systems", brands.len());
    for id in ["airbnb", "apple", "stripe", "material"] {
        assert!(brands.iter().any(|(b, _)| b == id), "{id} names its brand");
    }
    for id in ["minimal", "editorial", "default"] {
        assert!(!brands.iter().any(|(b, _)| b == id), "{id} is not a brand");
    }
    let listed = &notice[notice.find("\nTrademarks\n").expect("Trademarks section")..];
    assert!(listed.contains(&format!("\n{} of the 152 design systems", brands.len())));
    let missing: Vec<_> = brands
        .iter()
        .filter(|(_, b)| !listed.contains(&format!("{b},")) && !listed.contains(&format!("{b}.")))
        .collect();
    assert!(missing.is_empty(), "not in NOTICE: {missing:?}");
}

/// The design system page and the Home picker describe the look, never the
/// bundled maintainer note ("Bundled OpenDesign package for X, derived from
/// curated DESIGN.md…") that `summary` keeps for parity.
#[test]
fn design_systems_have_a_description_for_people() {
    let idx = index();
    let bad: Vec<String> = idx
        .design_systems
        .iter()
        .filter(|d| {
            let t = &d.summary.description;
            t.trim().is_empty() || t.contains(" package for ") || t.contains("OpenDesign")
        })
        .map(|d| format!("{}: {:?}", d.summary.id, d.summary.description))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    let lingo = idx
        .design_systems
        .iter()
        .find(|d| d.summary.id == "lingo")
        .unwrap();
    assert!(
        lingo
            .summary
            .description
            .starts_with("Playful, minimal design"),
        "{:?}",
        lingo.summary.description
    );
    let airbnb = idx
        .design_systems
        .iter()
        .find(|d| d.summary.id == "airbnb")
        .unwrap();
    assert_eq!(airbnb.summary.brand.as_deref(), Some("Airbnb"));
}
