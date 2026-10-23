use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Coarse viewer bucket for a project file, derived from its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileKind {
    Html,
    Sketch,
    Image,
    Video,
    Audio,
    Text,
    Code,
    Pdf,
    Document,
    Presentation,
    Spreadsheet,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    /// Project-relative path with `/` separators.
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub size: u64,
    /// Modification time, Unix epoch milliseconds.
    pub mtime: f64,
    pub kind: FileKind,
    pub mime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_manifest: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesResponse {
    pub files: Vec<ProjectFile>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentEncoding {
    #[default]
    Utf8,
    Base64,
}

/// `POST /api/projects/:id/files` with a JSON body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileRequest {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub encoding: ContentEncoding,
    /// Defaults to `true`; `false` returns 409 when the file exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_manifest: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileResponse {
    pub file: ProjectFile,
}

/// `POST /api/projects/:id/files/rename`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameFileRequest {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub mtime: f64,
    pub original_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadResponse {
    pub files: Vec<UploadedFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionSource {
    Ai,
    Manual,
    Restore,
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptSource {
    Message,
    Project,
    Manual,
    Restore,
}

/// One entry of a file's version history (HTML files only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    pub id: String,
    pub file_name: String,
    pub version: u32,
    pub label: String,
    pub created_at: i64,
    pub source: VersionSource,
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_source: Option<PromptSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_from_version_id: Option<String>,
    pub size: u64,
    pub mime: String,
    pub kind: FileKind,
    pub current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_version_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileVersionsResponse {
    pub file: ProjectFile,
    pub versions: Vec<FileVersion>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateVersionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<VersionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionResponse {
    pub version: FileVersion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionContentResponse {
    pub version: FileVersion,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreVersionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Restore only while the file still holds this version's content
    /// (manual-edit undo and redo); otherwise 409.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_version_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestoreVersionResponse {
    pub file: ProjectFile,
    pub version: FileVersion,
}
