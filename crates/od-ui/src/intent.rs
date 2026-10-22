//! Home composer logic: chip → project kind and default skill, project
//! naming, and the create request (spec 1.1).

use od_contracts::{CreateProjectRequest, ProjectKind, ProjectMetadata, SkillMode, SkillSummary};

/// Longest generated project name, in characters.
const NAME_CHARS: usize = 60;

/// Output-type chips under the brief box. Only prototype and deck are P0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chip {
    Prototype,
    Deck,
}

impl Chip {
    pub const ALL: [Self; 2] = [Self::Prototype, Self::Deck];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Prototype => "Prototype",
            Self::Deck => "Slide deck",
        }
    }

    #[must_use]
    pub fn mode(self) -> SkillMode {
        match self {
            Self::Prototype => SkillMode::Prototype,
            Self::Deck => SkillMode::Deck,
        }
    }

    #[must_use]
    pub fn kind(self) -> ProjectKind {
        match self {
            Self::Prototype => ProjectKind::Prototype,
            Self::Deck => ProjectKind::Deck,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Prototype => "prototype",
            Self::Deck => "deck",
        }
    }
}

/// The skill a chip runs when no template is picked: one declaring
/// `default_for` the chip, else the most featured skill of that mode, else
/// the first by id.
#[must_use]
pub fn default_skill(chip: Chip, skills: &[SkillSummary]) -> Option<String> {
    let of_mode = || skills.iter().filter(|s| s.info.mode == chip.mode());
    if let Some(s) = skills
        .iter()
        .find(|s| s.info.default_for.iter().any(|d| d == chip.key()))
    {
        return Some(s.info.id.clone());
    }
    of_mode()
        .filter(|s| s.info.featured.is_some())
        .min_by(|a, b| {
            let (fa, fb) = (
                a.info.featured.unwrap_or(0.0),
                b.info.featured.unwrap_or(0.0),
            );
            fa.total_cmp(&fb).then_with(|| a.info.id.cmp(&b.info.id))
        })
        .or_else(|| of_mode().min_by(|a, b| a.info.id.cmp(&b.info.id)))
        .map(|s| s.info.id.clone())
}

/// Templates offered for a chip (same mode), featured first, then by name.
#[must_use]
pub fn templates_for(chip: Chip, templates: &[SkillSummary]) -> Vec<SkillSummary> {
    let mut out: Vec<SkillSummary> = templates
        .iter()
        .filter(|t| t.info.mode == chip.mode())
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        let fa = a.info.featured.unwrap_or(f64::MAX);
        let fb = b.info.featured.unwrap_or(f64::MAX);
        fa.total_cmp(&fb)
            .then_with(|| a.info.name.cmp(&b.info.name))
    });
    out
}

/// Project name from the brief's first non-empty line.
#[must_use]
pub fn project_name(brief: &str) -> String {
    let line = brief
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Untitled");
    if line.chars().count() <= NAME_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(NAME_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Everything the Home composer collected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeIntent {
    pub brief: String,
    pub chip: Chip,
    pub design_system_id: Option<String>,
    /// A picked template replaces the chip's default skill.
    pub template_id: Option<String>,
}

impl HomeIntent {
    /// `POST /api/projects` body. The brief rides along as `pendingPrompt`;
    /// the Studio sends it as the first turn.
    #[must_use]
    pub fn create_request(&self, skills: &[SkillSummary]) -> CreateProjectRequest {
        CreateProjectRequest {
            name: project_name(&self.brief),
            skill_id: self
                .template_id
                .clone()
                .or_else(|| default_skill(self.chip, skills)),
            design_system_id: self.design_system_id.clone(),
            pending_prompt: Some(self.brief.trim().to_owned()).filter(|p| !p.is_empty()),
            metadata: Some(ProjectMetadata {
                kind: Some(self.chip.kind()),
                ..ProjectMetadata::default()
            }),
            ..CreateProjectRequest::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn skill(id: &str, mode: &str, featured: Option<f64>, default_for: &[&str]) -> SkillSummary {
        serde_json::from_value(json!({
            "id": id, "name": id, "description": "", "triggers": [], "mode": mode,
            "surface": "web", "source": "built-in", "craftRequires": [], "platform": null,
            "scenario": "", "category": null, "previewType": "html",
            "designSystemRequired": false, "defaultFor": default_for, "upstream": null,
            "featured": featured, "fidelity": null, "speakerNotes": null, "animations": null,
            "examplePrompt": "", "aggregatesExamples": false, "critiquePolicy": null,
            "hasBody": true
        }))
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn default_skill_prefers_default_for_then_featured() {
        let skills = vec![
            skill("b-proto", "prototype", None, &[]),
            skill("a-proto", "prototype", None, &[]),
            skill("feat-proto", "prototype", Some(2.0), &[]),
            skill("top-proto", "prototype", Some(1.0), &[]),
            skill("my-deck", "deck", None, &["deck"]),
        ];
        assert_eq!(
            default_skill(Chip::Prototype, &skills).as_deref(),
            Some("top-proto")
        );
        assert_eq!(
            default_skill(Chip::Deck, &skills).as_deref(),
            Some("my-deck")
        );
        let plain = vec![
            skill("b", "prototype", None, &[]),
            skill("a", "prototype", None, &[]),
        ];
        assert_eq!(default_skill(Chip::Prototype, &plain).as_deref(), Some("a"));
        assert_eq!(default_skill(Chip::Deck, &plain), None);
    }

    #[test]
    fn names_from_brief() {
        assert_eq!(
            project_name("\n  A landing page for tea\nmore"),
            "A landing page for tea"
        );
        assert_eq!(project_name("   "), "Untitled");
        let long = "word ".repeat(30);
        let name = project_name(&long);
        assert_eq!(name.chars().count(), NAME_CHARS);
        assert!(name.ends_with('…'));
    }

    #[test]
    fn template_replaces_default_skill() {
        let skills = vec![skill("proto", "prototype", None, &[])];
        let mut intent = HomeIntent {
            brief: "Tea shop".into(),
            chip: Chip::Prototype,
            design_system_id: Some("ds1".into()),
            template_id: None,
        };
        let req = intent.create_request(&skills);
        assert_eq!(req.skill_id.as_deref(), Some("proto"));
        assert_eq!(req.pending_prompt.as_deref(), Some("Tea shop"));
        assert_eq!(
            req.metadata.and_then(|m| m.kind),
            Some(ProjectKind::Prototype)
        );
        intent.template_id = Some("tpl".into());
        assert_eq!(
            intent.create_request(&skills).skill_id.as_deref(),
            Some("tpl")
        );
    }

    #[test]
    fn templates_filtered_and_sorted() {
        let t = vec![
            skill("z", "deck", None, &[]),
            skill("y", "prototype", None, &[]),
            skill("x", "deck", Some(1.0), &[]),
        ];
        let ids: Vec<_> = templates_for(Chip::Deck, &t)
            .into_iter()
            .map(|s| s.info.id)
            .collect();
        assert_eq!(ids, vec!["x", "z"]);
    }
}
