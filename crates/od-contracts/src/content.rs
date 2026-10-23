use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The eight skill modes. Anything else in frontmatter is re-inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillMode {
    Prototype,
    Deck,
    Template,
    DesignSystem,
    Image,
    Video,
    Audio,
    Utility,
}

impl SkillMode {
    pub const ALL: [Self; 8] = [
        Self::Prototype,
        Self::Deck,
        Self::Template,
        Self::DesignSystem,
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::Utility,
    ];

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "prototype" => Self::Prototype,
            "deck" => Self::Deck,
            "template" => Self::Template,
            "design-system" => Self::DesignSystem,
            "image" => Self::Image,
            "video" => Self::Video,
            "audio" => Self::Audio,
            "utility" => Self::Utility,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Web,
    Image,
    Video,
    Audio,
}

impl Surface {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "web" => Self::Web,
            "image" => Self::Image,
            "video" => Self::Video,
            "audio" => Self::Audio,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Desktop,
    Mobile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fidelity {
    Wireframe,
    HighFidelity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CritiquePolicy {
    Required,
    OptIn,
    OptOut,
}

/// Where a catalogue entry came from. User roots shadow built-in ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentSource {
    User,
    BuiltIn,
}

/// Normalized skill or design-template metadata (no body).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<BTreeMap<String, String>>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_i18n: Option<BTreeMap<String, String>>,
    pub triggers: Vec<Value>,
    pub mode: SkillMode,
    pub surface: Surface,
    pub source: ContentSource,
    pub craft_requires: Vec<String>,
    pub platform: Option<Platform>,
    pub scenario: String,
    pub category: Option<String>,
    pub preview_type: String,
    pub design_system_required: bool,
    pub default_for: Vec<String>,
    pub upstream: Option<String>,
    pub featured: Option<f64>,
    pub fidelity: Option<Fidelity>,
    pub speaker_notes: Option<bool>,
    pub animations: Option<bool>,
    pub example_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_prompt_i18n: Option<BTreeMap<String, String>>,
    pub aggregates_examples: bool,
    pub critique_policy: Option<CritiquePolicy>,
}

/// Listing entry: metadata plus whether a body exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    #[serde(flatten)]
    pub info: SkillInfo,
    pub has_body: bool,
    /// The folder ships a rendered example (`example.html`, or
    /// `examples/<key>.html` for a derived entry), served at
    /// `GET /api/design-templates/:id/example`.
    #[serde(default)]
    pub has_example: bool,
}

/// Single-entry response: metadata plus the full prompt body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDetail {
    #[serde(flatten)]
    pub info: SkillInfo,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillsResponse {
    pub skills: Vec<SkillSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignTemplatesResponse {
    pub design_templates: Vec<SkillSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignSystemStatus {
    Draft,
    Published,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignSystemSummary {
    pub id: String,
    pub title: String,
    pub category: String,
    pub summary: String,
    /// `[bg, support, fg, accent]` hex colors, or empty.
    pub swatches: Vec<String>,
    pub surface: Surface,
    pub source: ContentSource,
    pub status: DesignSystemStatus,
    pub is_editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSystemsResponse {
    #[serde(rename = "designSystems")]
    pub design_systems: Vec<DesignSystemSummary>,
}

/// `GET /api/design-systems/:id`: summary, raw `DESIGN.md` and manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSystemDetail {
    #[serde(flatten)]
    pub summary: DesignSystemSummary,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CraftEntry {
    pub id: String,
    pub label: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CraftResponse {
    pub craft: Vec<CraftEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CraftDetail {
    pub id: String,
    pub body: String,
}
