use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What a project produces. Drives default skill and preview behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectKind {
    Prototype,
    Deck,
    Template,
    Image,
    Video,
    Audio,
    Other,
}

/// Project metadata stored as JSON in `projects.metadata_json`.
///
/// Known fields are typed; anything else round-trips through `extra` so a
/// newer client never loses data written by an older daemon.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ProjectKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fidelity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_file: Option<String>,
    /// Folder-import root. Only the import route may set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_dirs: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub skill_id: Option<String>,
    pub design_system_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProjectMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    /// Unix epoch milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
}

/// `.open-design/project.json` inside every project folder, so a project
/// still describes itself if the SQLite database is lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub skill_id: Option<String>,
    #[serde(default)]
    pub design_system_id: Option<String>,
}

impl ProjectManifest {
    pub const SCHEMA_VERSION: u32 = 1;
}

impl From<&Project> for ProjectManifest {
    fn from(p: &Project) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            id: p.id.clone(),
            name: p.name.clone(),
            created_at: p.created_at,
            updated_at: p.updated_at,
            skill_id: p.skill_id.clone(),
            design_system_id: p.design_system_id.clone(),
        }
    }
}

/// `POST /api/projects`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    /// Client-chosen id; the daemon generates one when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_system_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ProjectMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectResponse {
    pub project: Project,
    pub conversation_id: String,
}

/// `PATCH /api/projects/:id`. Absent fields are untouched; `null` clears.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub skill_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub design_system_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub pending_prompt: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub metadata: Option<Option<ProjectMetadata>>,
    #[serde(
        default,
        deserialize_with = "crate::double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_instructions: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectResponse {
    pub project: Project,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<Project>,
}

/// `POST /api/projects/:id/duplicate`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateProjectRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateProjectResponse {
    pub project: Project,
    pub conversation_id: String,
    pub copied_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}
