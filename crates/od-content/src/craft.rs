//! Craft rules: `craft/<slug>.md` sections injected into prompts (spec 3.6).

use crate::skills::normalize_craft_slugs;
use od_contracts::CraftEntry;
use serde_json::Value;
use std::path::Path;

/// Every `*.md` in `dir`, sorted by slug, labelled by its first `# ` line.
#[must_use]
pub fn list_craft(dir: &Path) -> Vec<CraftEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<CraftEntry> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let slug = name.strip_suffix(".md")?.to_owned();
            let text = std::fs::read_to_string(e.path()).ok()?;
            let label = text.split('\n').find(|l| l.starts_with("# ")).map_or_else(
                || slug.clone(),
                |l| l.trim_start_matches('#').trim().to_owned(),
            );
            Some(CraftEntry {
                id: slug,
                label,
                bytes: text.len() as u64,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Valid craft slug: lowercase letters, digits, dashes; no leading dash.
#[must_use]
pub fn is_valid_slug(slug: &str) -> bool {
    let mut bytes = slug.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Inputs to [`resolve_craft_requirements`].
#[derive(Debug, Clone, Default)]
pub struct CraftRequest<'a> {
    pub is_deck: bool,
    pub skill_requires: &'a [String],
    pub design_system_applies: &'a [String],
    pub design_system_exemptions: &'a [String],
}

/// `skill requires ∪ design-system applies ∪ {typography if deck}`, minus
/// exemptions, deduplicated in that order.
#[must_use]
pub fn resolve_craft_requirements(req: &CraftRequest<'_>) -> Vec<String> {
    let as_values = |items: &[String]| items.iter().cloned().map(Value::String).collect::<Vec<_>>();
    let mut requested = as_values(req.skill_requires);
    requested.extend(as_values(req.design_system_applies));
    if req.is_deck {
        requested.push(Value::String("typography".to_owned()));
    }
    let excluded = normalize_craft_slugs(&as_values(req.design_system_exemptions));
    normalize_craft_slugs(&requested)
        .into_iter()
        .filter(|s| !excluded.contains(s))
        .collect()
}

/// Concatenates the requested sections as `### <slug>` blocks separated by
/// rules. Missing or empty files are skipped silently. Returns the body and
/// the slugs that resolved.
#[must_use]
pub fn load_craft_sections(dir: &Path, requested: &[String]) -> (String, Vec<String>) {
    let mut parts = Vec::new();
    let mut sections = Vec::new();
    for slug in normalize_craft_slugs(
        &requested
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>(),
    ) {
        let Ok(text) = std::fs::read_to_string(dir.join(format!("{slug}.md"))) else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        parts.push(format!("### {slug}\n\n{trimmed}"));
        sections.push(slug);
    }
    (parts.join("\n\n---\n\n"), sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_union_minus_exemptions() {
        let skill = [
            "Color".to_owned(),
            "color".to_owned(),
            "bad slug".to_owned(),
        ];
        let ds = ["anti-ai-slop".to_owned()];
        let exempt = ["color".to_owned()];
        let got = resolve_craft_requirements(&CraftRequest {
            is_deck: true,
            skill_requires: &skill,
            design_system_applies: &ds,
            design_system_exemptions: &exempt,
        });
        assert_eq!(got, ["anti-ai-slop", "typography"]);
    }

    #[test]
    fn sections_skip_missing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("color.md"), "  Use tokens.  ").unwrap();
        let (body, sections) =
            load_craft_sections(tmp.path(), &["color".to_owned(), "motion".to_owned()]);
        assert_eq!(body, "### color\n\nUse tokens.");
        assert_eq!(sections, ["color"]);
    }
}
