//! Files panel operations beyond single files: folders, a ZIP of selected
//! files, and saving a project file as a user design template.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldersResponse {
    /// Project-relative folder paths, sorted; empty folders included.
    pub folders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderResponse {
    /// The folder as stored (sanitized).
    pub folder: String,
}

/// `POST move-folder`: folder `from` becomes exactly `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveFolderRequest {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveFolderResponse {
    pub folder: String,
    /// Listed files that moved with it.
    pub files: usize,
}

/// `POST archive/selection`: a ZIP of these files, paths kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveSelectionRequest {
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTemplateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The HTML file the template starts from (its `example.html`).
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveTemplateResponse {
    /// Id of the new user design template.
    pub id: String,
}

/// `GET office-text/<path>`: the readable text of a docx, pptx or xlsx.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeTextResponse {
    /// `document`, `presentation` or `spreadsheet`.
    pub kind: String,
    pub sections: Vec<OfficeSection>,
}

/// A slide, a sheet, or the whole document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeSection {
    pub title: String,
    pub text: String,
}
