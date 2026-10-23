//! Files panel operations (Studio files, export). Mounted with one `merge`
//! in [`crate::router`].
//!
//! - `GET/POST /api/projects/:id/folders`: list folders (empty ones too),
//!   create one.
//! - `DELETE /api/projects/:id/folders/*path`: a folder and its files.
//! - `POST /api/projects/:id/archive/selection`: ZIP of the chosen files.
//! - `PUT  /api/projects/:id/content/*path`: overwrite an existing file at
//!   exactly that path (HTML gets a manual version).
//! - `GET  /api/projects/:id/office-text/*path`: text of a docx, pptx or
//!   xlsx, per slide or sheet.
//! - `POST /api/projects/:id/save-template`: an HTML file, bundled to one
//!   file, becomes a user design template (`<data>/design-templates`).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::export::{ExportRequest, ExportResult, attachment, bundle, resolve_entry};
use crate::state::AppState;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::{delete, get, post, put};
use od_contracts::{
    ArchiveSelectionRequest, CreateFolderRequest, FileResponse, FolderResponse, FoldersResponse,
    OfficeSection, OfficeTextResponse, ProjectKind, SaveTemplateRequest, SaveTemplateResponse,
};
use od_export::archive::{sanitize_filename, write_zip};
use serde::Deserialize;
use std::path::PathBuf;

/// Most files one selection ZIP takes.
const MAX_SELECTION: usize = 2000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{id}/folders",
            get(list_folders).post(create_folder),
        )
        .route("/api/projects/{id}/folders/{*path}", delete(delete_folder))
        .route(
            "/api/projects/{id}/archive/selection",
            post(archive_selection),
        )
        .route("/api/projects/{id}/save-template", post(save_template))
        .route("/api/projects/{id}/content/{*path}", put(replace_content))
        .route("/api/projects/{id}/office-text/{*path}", get(office_text))
}

/// Largest Office file read for its text.
const MAX_OFFICE_BYTES: u64 = 64 * 1024 * 1024;

/// `GET office-text/<path>`: text of a Word, PowerPoint or Excel file, per
/// slide or sheet, for the Studio viewer (no preview otherwise).
async fn office_text(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> ApiResult<ApiJson<OfficeTextResponse>> {
    if od_export::office::OfficeKind::of(&path).is_none() {
        return Err(ApiErr::bad_request(
            "not an Office file (docx, pptx or xlsx)",
        ));
    }
    let (file, bytes) = state
        .blocking(move |s| {
            let (file, _) = s.stat_file(&id, &path)?;
            if file.size > MAX_OFFICE_BYTES {
                return Err(od_store::StoreError::BadRequest(
                    "file too large to read".to_owned(),
                ));
            }
            s.read_file(&id, &path)
        })
        .await?;
    let (kind, sections) =
        tokio::task::spawn_blocking(move || od_export::office::extract(&file.name, &bytes))
            .await
            .map_err(|e| ApiErr::internal(e.to_string()))?
            .map_err(|e| {
                ApiErr::new(
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    od_contracts::ErrorCode::ValidationFailed,
                    e.to_string(),
                )
            })?;
    Ok(ApiJson(OfficeTextResponse {
        kind: kind.as_str().to_owned(),
        sections: sections
            .into_iter()
            .map(|s| OfficeSection {
                title: s.title,
                text: s.text,
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
struct ReplaceQuery {
    /// Version label for HTML files.
    #[serde(default)]
    label: Option<String>,
}

/// `PUT content/<path>`: the raw body becomes the existing file's content,
/// at exactly that path (editors save with it: Markdown, sketches, notes).
async fn replace_content(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    Query(q): Query<ReplaceQuery>,
    body: Bytes,
) -> ApiResult<ApiJson<FileResponse>> {
    let label = q
        .label
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| "Edited in Nét".to_owned());
    let file = state
        .blocking(move |s| s.replace_file(&id, &path, &body, &label))
        .await?;
    Ok(ApiJson(FileResponse { file }))
}

async fn list_folders(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<FoldersResponse>> {
    let folders = state.blocking(move |s| s.list_folders(&id)).await?;
    Ok(ApiJson(FoldersResponse { folders }))
}

async fn create_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<CreateFolderRequest>,
) -> ApiResult<ApiJson<FolderResponse>> {
    let folder = state
        .blocking(move |s| s.create_folder(&id, &req.path))
        .await?;
    Ok(ApiJson(FolderResponse { folder }))
}

async fn delete_folder(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> ApiResult<ApiJson<FolderResponse>> {
    let folder = state.blocking(move |s| s.delete_folder(&id, &path)).await?;
    Ok(ApiJson(FolderResponse { folder }))
}

/// A ZIP of the selected files under their project paths, named after the
/// project. Built in memory: a selection is what a person ticks by hand.
async fn archive_selection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<ArchiveSelectionRequest>,
) -> ApiResult<Response> {
    let mut wanted = req.files;
    wanted.sort();
    wanted.dedup();
    if wanted.is_empty() {
        return Err(ApiErr::bad_request("files required"));
    }
    if wanted.len() > MAX_SELECTION {
        return Err(ApiErr::bad_request(format!(
            "at most {MAX_SELECTION} files per ZIP"
        )));
    }
    let (name, zip) = state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            let entries = wanted
                .iter()
                .map(|f| {
                    let (file, full) = s.stat_file(&id, f)?;
                    Ok((file.name, full))
                })
                .collect::<od_store::Result<Vec<(String, PathBuf)>>>()?;
            let zip = write_zip(Vec::new(), &entries)?;
            Ok((project.name, zip))
        })
        .await?;
    let base = sanitize_filename(&name);
    let filename = format!("{}.zip", if base.is_empty() { "files" } else { &base });
    Ok(attachment("application/zip", &filename, zip))
}

/// A template id from its name: lowercase ASCII letters, digits and `-`.
fn template_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "template".to_owned()
    } else {
        out.chars().take(48).collect()
    }
}

/// `SKILL.md` of a saved template. Strings are JSON-quoted, which YAML
/// reads as double-quoted scalars, so any name is safe in the front matter.
fn template_skill_md(id: &str, name: &str, description: &str, mode: &str) -> String {
    let q = |s: &str| serde_json::Value::from(s).to_string();
    format!(
        "---\nname: {}\nen_name: {}\ndescription: {}\nod:\n  mode: {mode}\n  preview:\n    type: html\n    entry: example.html\n  design_system:\n    requires: false\n---\n\n# {name}\n\nStart from `example.html`: the saved design, in one file. Keep its layout and visual language; change the content to fit the brief.\n",
        q(id),
        q(name),
        q(description),
    )
}

async fn save_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<SaveTemplateRequest>,
) -> ExportResult<ApiJson<SaveTemplateResponse>> {
    let name = req.name.trim().to_owned();
    if name.is_empty() {
        return Err(ApiErr::bad_request("name required").into());
    }
    let export: ExportRequest =
        serde_json::from_value(serde_json::json!({ "fileName": req.file_name }))
            .map_err(|e| ApiErr::bad_request(e.to_string()))?;
    let entry = resolve_entry(&state, id, &export).await?;
    let html = bundle(&entry, true).await?.html;
    let mode = match entry.project.metadata.as_ref().and_then(|m| m.kind) {
        Some(ProjectKind::Deck) => "deck",
        _ => "prototype",
    };
    let description = req
        .description
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| format!("Saved from the project \"{}\".", entry.project.name));
    let root = state.store.data_dir().user_design_templates_dir();
    let slug = template_slug(&name);
    let skill = move |tid: &str| template_skill_md(tid, &name, &description, mode);
    let id = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        std::fs::create_dir_all(&root)?;
        // `create_dir` claims the id: two saves of one name never share it.
        let (tid, dir) = (1..1000)
            .map(|n| {
                if n == 1 {
                    format!("my-{slug}")
                } else {
                    format!("my-{slug}-{n}")
                }
            })
            .find_map(|tid| {
                let dir = root.join(&tid);
                std::fs::create_dir(&dir).ok().map(|()| (tid, dir))
            })
            .ok_or_else(|| std::io::Error::other("no free template id"))?;
        std::fs::write(dir.join("example.html"), html)?;
        std::fs::write(dir.join("SKILL.md"), skill(&tid))?;
        Ok(tid)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    state.content.reload();
    Ok(ApiJson(SaveTemplateResponse { id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_plain_ascii() {
        assert_eq!(template_slug("  Tea Shop: Landing!  "), "tea-shop-landing");
        assert_eq!(template_slug("Trà sữa"), "tr-s-a");
        assert_eq!(template_slug("…"), "template");
    }

    #[test]
    fn front_matter_quotes_the_name() {
        let md = template_skill_md("my-x", "A \"quoted\": name", "d", "deck");
        assert!(md.contains("en_name: \"A \\\"quoted\\\": name\""), "{md}");
        assert!(md.contains("  mode: deck\n"));
    }
}
