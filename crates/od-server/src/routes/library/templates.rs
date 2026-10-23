//! Templates the user saved, and Remix of a design template (spec 1.4):
//!
//! - `GET    /api/templates`: saved templates, newest first, without files.
//! - `POST   /api/templates` `{name, description?, sourceProjectId}`:
//!   snapshots the project's HTML, text and code files into the
//!   `templates` table; the same name from the same project updates it.
//! - `GET    /api/templates/:id`: one template with its files.
//! - `DELETE /api/templates/:id`.
//! - `POST   /api/templates/:id/use` `{name?, prompt?}`: a new project
//!   with the template's files.
//! - `POST   /api/design-templates/:id/remix` `{name?, prompt?}`: a new
//!   project bound to the template whose `index.html` is its example.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use od_content::SkillEntry;
use od_contracts::{CreateProjectRequest, FileKind, ProjectKind, ProjectMetadata, SkillMode};
use od_store::{Store, StoreError};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Upper bound of a template's files, all together.
const MAX_TEMPLATE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_NAME_CHARS: usize = 100;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/templates", get(list).post(save))
        .route("/api/templates/{id}", get(get_one).delete(delete))
        .route("/api/templates/{id}/use", post(use_template))
        .route("/api/design-templates/{id}/remix", post(remix))
}

/// One snapshotted file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_project_id: Option<String>,
    pub file_count: usize,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<TemplateFile>>,
}

fn row(r: &rusqlite::Row<'_>, with_files: bool) -> rusqlite::Result<SavedTemplate> {
    let files_json: String = r.get(4)?;
    let files: Vec<TemplateFile> = serde_json::from_str(&files_json).unwrap_or_default();
    Ok(SavedTemplate {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        source_project_id: r.get(3)?,
        file_count: files.len(),
        created_at: r.get(5)?,
        files: with_files.then_some(files),
    })
}

const COLUMNS: &str = "id, name, description, source_project_id, files_json, created_at";

fn list_templates(s: &Store) -> Result<Vec<SavedTemplate>, StoreError> {
    s.with_conn(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM templates ORDER BY created_at DESC, id"
        ))?;
        let rows = stmt.query_map([], |r| row(r, false))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

fn find_template(s: &Store, id: &str) -> Result<Option<SavedTemplate>, StoreError> {
    s.with_conn(|c| {
        Ok(c.query_row(
            &format!("SELECT {COLUMNS} FROM templates WHERE id = ?1"),
            [id],
            |r| row(r, true),
        )
        .optional()?)
    })
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let templates = state.blocking(list_templates).await?;
    Ok(Json(json!({ "templates": templates })))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let t = state
        .blocking(move |s| find_template(s, &id))
        .await?
        .ok_or_else(|| ApiErr::not_found("template not found"))?;
    Ok(Json(json!({ "template": t })))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let n = state
        .blocking(move |s| {
            s.with_conn(|c| Ok(c.execute("DELETE FROM templates WHERE id = ?1", [&id])?))
        })
        .await?;
    if n == 0 {
        return Err(ApiErr::not_found("template not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    source_project_id: String,
}

/// The project's HTML, text and code files (what the original keeps).
fn snapshot(s: &Store, project_id: &str) -> Result<Vec<TemplateFile>, StoreError> {
    let mut out = Vec::new();
    let mut total = 0u64;
    for f in s.list_files(project_id)? {
        if !matches!(f.kind, FileKind::Html | FileKind::Text | FileKind::Code) {
            continue;
        }
        total += f.size;
        if total > MAX_TEMPLATE_BYTES {
            return Err(StoreError::BadRequest(
                "the project's files are larger than 20 MB".to_owned(),
            ));
        }
        let (_, bytes) = s.read_file(project_id, &f.name)?;
        if let Ok(content) = String::from_utf8(bytes) {
            out.push(TemplateFile {
                path: f.name,
                content,
            });
        }
    }
    Ok(out)
}

pub async fn save(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<SaveBody>,
) -> ApiResult<Json<Value>> {
    let name = body.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiErr::bad_request(format!(
            "name must be 1–{MAX_NAME_CHARS} characters"
        )));
    }
    let description = body
        .description
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty());
    let pid = body.source_project_id;
    let saved = state
        .blocking(move |s| {
            s.get_project(&pid)?;
            let files = snapshot(s, &pid)?;
            if files.is_empty() {
                return Err(StoreError::BadRequest(
                    "the project has no HTML, text or code files".to_owned(),
                ));
            }
            let files_json =
                serde_json::to_string(&files).map_err(|e| StoreError::BadRequest(e.to_string()))?;
            let existing: Option<String> = s.with_conn(|c| {
                Ok(c.query_row(
                    "SELECT id FROM templates WHERE name = ?1 AND source_project_id = ?2",
                    (&name, &pid),
                    |r| r.get(0),
                )
                .optional()?)
            })?;
            let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            s.with_conn(|c| {
                c.execute(
                    "INSERT INTO templates (id, name, description, source_project_id, files_json, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT(id) DO UPDATE SET description = ?3, files_json = ?5, created_at = ?6",
                    (&id, &name, &description, &pid, &files_json, od_store::now_ms()),
                )?;
                Ok(())
            })?;
            find_template(s, &id)
        })
        .await?
        .ok_or_else(|| ApiErr::internal("the template was not saved"))?;
    Ok(Json(json!({ "template": SavedTemplate { files: None, ..saved } })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

pub async fn use_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let body: UseBody = super::optional_json(&body)?;
    let (project, conversation_id) = state
        .blocking(move |s| {
            let t = find_template(s, &id)?.ok_or(StoreError::BadRequest(
                "template not found".to_owned(),
            ))?;
            let entry = t
                .files
                .as_ref()
                .and_then(|f| f.iter().find(|f| f.path.ends_with(".html")))
                .map(|f| f.path.clone());
            let (project, cid) = s.create_project(CreateProjectRequest {
                id: None,
                name: non_empty(body.name).unwrap_or_else(|| t.name.clone()),
                skill_id: None,
                design_system_id: None,
                pending_prompt: non_empty(body.prompt),
                metadata: Some(ProjectMetadata {
                    entry_file: entry,
                    extra: [("templateId".to_owned(), json!(t.id))].into_iter().collect(),
                    ..ProjectMetadata::default()
                }),
                custom_instructions: None,
                conversation_id: None,
            })?;
            for f in t.files.unwrap_or_default() {
                s.write_file(&project.id, &f.path, f.content.as_bytes(), true, None)?;
            }
            Ok((project, cid))
        })
        .await?;
    Ok(Json(
        json!({ "project": project, "conversationId": conversation_id }),
    ))
}

fn kind_of(entry: &SkillEntry) -> Option<ProjectKind> {
    match entry.info.mode {
        SkillMode::Deck => Some(ProjectKind::Deck),
        SkillMode::Prototype | SkillMode::Template => Some(ProjectKind::Prototype),
        _ => None,
    }
}

pub async fn remix(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let body: UseBody = super::optional_json(&body)?;
    let entry = state
        .content
        .snapshot()
        .find_design_template(&id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("design template not found"))?;
    let example = entry
        .example_html()
        .ok_or_else(|| ApiErr::not_found("this template has no example to remix"))?;
    let html = tokio::fs::read(&example)
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let name = non_empty(body.name).unwrap_or_else(|| entry.info.name.clone());
    let kind = kind_of(&entry);
    let skill_id = entry.info.id.clone();
    let (project, conversation_id) = state
        .blocking(move |s| {
            let (project, cid) = s.create_project(CreateProjectRequest {
                id: None,
                name,
                skill_id: Some(skill_id),
                design_system_id: None,
                pending_prompt: non_empty(body.prompt),
                metadata: Some(ProjectMetadata {
                    kind,
                    entry_file: Some("index.html".to_owned()),
                    imported_from: Some("template-remix".to_owned()),
                    ..ProjectMetadata::default()
                }),
                custom_instructions: None,
                conversation_id: None,
            })?;
            s.write_file(&project.id, "index.html", &html, true, None)?;
            Ok((project, cid))
        })
        .await?;
    Ok(Json(
        json!({ "project": project, "conversationId": conversation_id }),
    ))
}
