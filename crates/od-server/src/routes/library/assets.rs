//! The media library (spec 1.5 "library"): every image, video and audio
//! file across projects, generated or uploaded.
//!
//! - `GET  /api/library/assets[?kind=&q=&projectId=]`: newest first.
//! - `POST /api/library/upload` (multipart `file` parts): stored under
//!   `uploads/` of the library project, created on first use.
//! - `POST /api/library/chat` `{assets: [{projectId, path}], prompt?,
//!   name?}`: "Chat to design", a new project with copies of the assets
//!   under `assets/` and a first turn that uses them.
//!
//! Deleting an asset is `DELETE /api/projects/:id/files/:path`.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Multipart, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use od_contracts::{CreateProjectRequest, FileKind, ProjectMetadata};
use od_store::{Store, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The project that holds library uploads.
pub const LIBRARY_PROJECT_ID: &str = "media-library";
const UPLOAD_DIR: &str = "uploads";
const MAX_CHAT_ASSETS: usize = 24;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/library/assets", get(list))
        .route("/api/library/upload", post(upload))
        .route("/api/library/chat", post(chat))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub project_id: String,
    pub project_name: String,
    pub path: String,
    pub kind: FileKind,
    pub mime: String,
    pub size: u64,
    pub mtime: f64,
    /// `true` for files uploaded to the library project.
    pub uploaded: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

fn is_media(kind: FileKind) -> bool {
    matches!(kind, FileKind::Image | FileKind::Video | FileKind::Audio)
}

fn kind_name(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Image => "image",
        FileKind::Video => "video",
        FileKind::Audio => "audio",
        _ => "other",
    }
}

/// Media files of every project, filtered and sorted newest first.
fn collect(s: &Store, q: &ListQuery) -> Result<Vec<Asset>, StoreError> {
    let needle = q
        .q
        .as_deref()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty());
    let mut out = Vec::new();
    for p in s.list_projects()? {
        if q.project_id.as_deref().is_some_and(|id| id != p.id) {
            continue;
        }
        // A project whose folder is gone lists nothing, not an error.
        let Ok(files) = s.list_files(&p.id) else {
            continue;
        };
        for f in files {
            if !is_media(f.kind) {
                continue;
            }
            if q.kind.as_deref().is_some_and(|k| k != kind_name(f.kind)) {
                continue;
            }
            if let Some(n) = &needle {
                let hay = format!("{} {}", f.name, p.name).to_lowercase();
                if !hay.contains(n.as_str()) {
                    continue;
                }
            }
            out.push(Asset {
                uploaded: p.id == LIBRARY_PROJECT_ID,
                project_id: p.id.clone(),
                project_name: p.name.clone(),
                path: f.name,
                kind: f.kind,
                mime: f.mime,
                size: f.size,
                mtime: f.mtime,
            });
        }
    }
    out.sort_by(|a, b| {
        b.mtime
            .partial_cmp(&a.mtime)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(out)
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let assets = state.blocking(move |s| collect(s, &q)).await?;
    Ok(Json(json!({ "assets": assets })))
}

/// The library project, created the first time it is needed.
fn library_project(s: &Store) -> Result<String, StoreError> {
    match s.get_project(LIBRARY_PROJECT_ID) {
        Ok(p) => Ok(p.id),
        Err(StoreError::ProjectNotFound) => {
            let (p, _) = s.create_project(CreateProjectRequest {
                id: Some(LIBRARY_PROJECT_ID.to_owned()),
                name: "Media library".to_owned(),
                skill_id: None,
                design_system_id: None,
                pending_prompt: None,
                metadata: Some(ProjectMetadata {
                    intent: Some("media-library".to_owned()),
                    ..ProjectMetadata::default()
                }),
                custom_instructions: None,
                conversation_id: None,
            })?;
            Ok(p.id)
        }
        Err(e) => Err(e),
    }
}

pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let mut parts = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiErr::bad_request(e.body_text()))?
    {
        let Some(name) = field.file_name().map(str::to_owned) else {
            continue;
        };
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiErr::bad_request(e.body_text()))?;
        if !is_media(od_store::paths::kind_for(&name)) {
            return Err(ApiErr::bad_request(format!(
                "{name} is not an image, video or audio file"
            )));
        }
        parts.push((name, bytes));
    }
    if parts.is_empty() {
        return Err(ApiErr::bad_request("no files in upload"));
    }
    let files = state
        .blocking(move |s| {
            let pid = library_project(s)?;
            parts
                .iter()
                .map(|(name, bytes)| s.upload_file(&pid, Some(UPLOAD_DIR), name, bytes))
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(Json(
        json!({ "projectId": LIBRARY_PROJECT_ID, "files": files }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    project_id: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBody {
    assets: Vec<AssetRef>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// The first turn of a "Chat to design" project.
#[must_use]
pub fn chat_prompt(user: Option<&str>, paths: &[String]) -> String {
    let list: Vec<String> = paths.iter().map(|p| format!("- `{p}`")).collect();
    let ask = user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .unwrap_or("Design a page that makes good use of these assets.");
    format!(
        "{ask}\n\nUse these files from the project as the source material:\n\n{}",
        list.join("\n")
    )
}

pub async fn chat(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ChatBody>,
) -> ApiResult<Json<Value>> {
    if body.assets.is_empty() || body.assets.len() > MAX_CHAT_ASSETS {
        return Err(ApiErr::bad_request(format!(
            "pick 1–{MAX_CHAT_ASSETS} assets"
        )));
    }
    let (project, conversation_id) = state
        .blocking(move |s| {
            let mut copies = Vec::new();
            for a in &body.assets {
                let (file, bytes) = s.read_file(&a.project_id, &a.path)?;
                if !is_media(file.kind) {
                    return Err(StoreError::BadRequest(format!(
                        "{} is not a media file",
                        a.path
                    )));
                }
                let base = a.path.rsplit('/').next().unwrap_or(&a.path).to_owned();
                copies.push((base, bytes));
            }
            let name = body
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or("Design from library")
                .to_owned();
            let (project, cid) = s.create_project(CreateProjectRequest {
                id: None,
                name,
                skill_id: None,
                design_system_id: None,
                pending_prompt: None,
                metadata: Some(ProjectMetadata {
                    imported_from: Some("library".to_owned()),
                    ..ProjectMetadata::default()
                }),
                custom_instructions: None,
                conversation_id: None,
            })?;
            let mut paths = Vec::new();
            for (base, bytes) in &copies {
                let f = s.upload_file(&project.id, Some("assets"), base, bytes)?;
                paths.push(f.name);
            }
            let prompt = chat_prompt(body.prompt.as_deref(), &paths);
            let project = s.update_project(
                &project.id,
                serde_json::from_value(json!({ "pendingPrompt": prompt }))
                    .map_err(|e| StoreError::BadRequest(e.to_string()))?,
            )?;
            Ok((project, cid))
        })
        .await?;
    Ok(Json(
        json!({ "project": project, "conversationId": conversation_id }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_prompt_lists_the_copied_files() {
        let p = chat_prompt(None, &["assets/a.png".into(), "assets/b.mp3".into()]);
        assert!(p.starts_with("Design a page"));
        assert!(p.contains("- `assets/a.png`\n- `assets/b.mp3`"));
        let p = chat_prompt(Some("  A poster  "), &["assets/a.png".into()]);
        assert!(p.starts_with("A poster\n"));
    }
}
