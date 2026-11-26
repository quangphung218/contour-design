//! Skill library writes (spec 1.4 "Thư viện skill", P1 CRUD), the
//! original `SkillsSection`:
//!
//! - `POST   /api/skills` `{name, description?, triggers?, body}`: a new
//!   user skill in `<data>/skills/<slug>/SKILL.md`; 409 when the id exists.
//! - `PUT    /api/skills/:id` (same body): edits a user skill, or writes a
//!   user override of a bundled one (side files copied once).
//! - `DELETE /api/skills/:id`: removes a user skill or override; the
//!   bundled skill shows again.
//! - `POST   /api/skills/install` `{path, replace?}`: copies a local skill
//!   folder (`SKILL.md` and its side files) into `<data>/skills/<id>`; 409
//!   for a bundled id, and for a user skill's id unless `replace` is set
//!   (a skill of another project is never lost). "Save as skill" in the
//!   Studio has the agent call it. With `bareOnly` a folder that is a
//!   plugin (`open-design.json`, `.claude-plugin/`) is left alone and the
//!   answer is `{id: null}`: "Install from folder" then installs a plugin.
//! - `GET    /api/skills/local-sources`: the skill folders of agent CLIs
//!   (`~/.claude/skills`, `~/.codex/skills`…, one level, read only) with
//!   whether each is imported, and whether an imported one `differs` from
//!   the copy in Contour; "From agent CLI" installs the ones picked.
//! - `GET    /api/skills/:id/source-preview?path=…`: read only, the agent
//!   CLI folder `path` (one `local-sources` lists as `id`) as an install
//!   would copy it, the user skill `id`, the files that differ.
//! - `POST   /api/skills/:id/replace-from-source` `{path, hash}`: replaces
//!   the user skill with that folder if it is still the content shown
//!   (`hash`), else 409 with nothing written. The copy in Contour is kept
//!   in `<data>/skills/.previous/<id>`. Only `<data>/skills` is written,
//!   never the agent's folder (decision #9).
//! - `GET    /api/skills/:id/previous`: whether a copy is kept.
//! - `POST   /api/skills/:id/restore-previous`: swaps it back; 404 without.
//! - `GET    /api/skills/:id/export?target=claude`: "Copy to Claude Code",
//!   read only: where a Personal skill would go (`<agents' HOME>/.claude/
//!   skills/<id>`), its source folder, what stands there already, whether
//!   it has a description and calls Contour's tools. The copy itself is
//!   the desktop app's (`crate::skill_export`); no request writes there.

use super::write_and_reload;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use od_content::user_content::{self, SkillDraft};
use od_contracts::{ContentSource, name_key};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/skills", post(create))
        .route("/api/skills/install", post(install))
        .route("/api/skills/local-sources", get(local_sources))
        .route("/api/skills/{id}", put(save).delete(delete))
        .route("/api/skills/{id}/export", get(export_preview))
        .route("/api/skills/{id}/source-preview", get(source_preview))
        .route(
            "/api/skills/{id}/replace-from-source",
            post(replace_from_source),
        )
        .route("/api/skills/{id}/previous", get(previous))
        .route("/api/skills/{id}/restore-previous", post(restore_previous))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    triggers: Vec<String>,
    #[serde(default)]
    body: String,
}

impl From<SkillBody> for SkillDraft {
    fn from(b: SkillBody) -> Self {
        Self {
            name: b.name,
            description: b.description,
            triggers: b.triggers,
            body: b.body,
        }
    }
}

fn user_root(state: &AppState) -> std::path::PathBuf {
    state.store.data_dir().user_skills_dir()
}

pub async fn create(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<SkillBody>,
) -> ApiResult<Json<Value>> {
    let root = user_root(&state);
    let content = state.content.snapshot();
    let id = write_and_reload(&state, move || {
        let taken = |id: &str| content.find_skill(id).is_some();
        Ok(user_content::create_skill(&root, &body.into(), taken)?)
    })
    .await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Debug, Deserialize)]
pub struct InstallBody {
    path: String,
    /// Replace a user skill with the same id (the agent reinstalling what
    /// its own turn installed).
    #[serde(default)]
    replace: bool,
    /// Install only a bare skill; a plugin folder answers `{id: null}`.
    #[serde(default, rename = "bareOnly")]
    bare_only: bool,
}

pub async fn install(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<InstallBody>,
) -> ApiResult<Json<Value>> {
    let source = super::typed_folder(&body.path);
    let replace = body.replace;
    if !source.is_absolute() {
        return Err(ApiErr::bad_request("path must be an absolute folder path"));
    }
    if body.bare_only && !user_content::is_bare_skill(&source) {
        return Ok(Json(json!({ "id": null })));
    }
    let root = user_root(&state);
    // The bundled root itself: an override in the user root hides its
    // bundled skill from the index, and must not let an install take it.
    let bundled_root = state.content.roots().bundled_skills.clone();
    let id = write_and_reload(&state, move || {
        let bundled: Vec<String> = od_content::skills::list_skills(&[bundled_root], false)
            .into_iter()
            .map(|e| name_key(&e.info.id))
            .collect();
        let taken = |id: &str| {
            bundled.contains(&name_key(id))
                || (!replace && user_content::user_skill_exists(&root, id))
        };
        Ok(user_content::install_skill(&source, &root, taken)?)
    })
    .await?;
    tracing::info!(skill = %id, "skill installed");
    Ok(Json(json!({ "id": id })))
}

fn agent_roots(state: &AppState) -> Vec<(String, std::path::PathBuf)> {
    crate::skill_export::agent_home(&state.runs.engine.config().base_env)
        .map(|h| user_content::agent_skill_roots(&h))
        .unwrap_or_default()
}

pub async fn local_sources(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let roots = agent_roots(&state);
    let root = user_root(&state);
    let sources = crate::spawn::spawn_blocking_in_span(move || {
        user_content::scan_agent_skills(&roots, &root)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    Ok(Json(json!({ "sources": sources })))
}

pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(mut body): ApiJson<SkillBody>,
) -> ApiResult<Json<Value>> {
    let entry = state
        .content
        .snapshot()
        .find_skill(&id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("skill not found"))?;
    if id.contains(':') {
        return Err(ApiErr::bad_request("an example entry cannot be edited"));
    }
    if body.name.trim().is_empty() {
        body.name.clone_from(&entry.info.id);
    }
    let root = user_root(&state);
    let is_user = entry.info.source == ContentSource::User;
    let skill_id = entry.info.id.clone();
    write_and_reload(&state, move || {
        Ok(user_content::save_skill(
            &root,
            &skill_id,
            &entry.dir,
            is_user,
            &body.into(),
        )?)
    })
    .await?;
    Ok(Json(json!({ "id": entry.info.id, "override": !is_user })))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let entry = state
        .content
        .snapshot()
        .find_skill(&id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("skill not found"))?;
    if entry.info.source != ContentSource::User || id.contains(':') {
        return Err(ApiErr::forbidden("only user skills can be deleted"));
    }
    let root = user_root(&state);
    write_and_reload(&state, move || {
        Ok(user_content::delete_user_dir(&root, &entry.dir)?)
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    target: String,
}

/// `GET /api/skills/:id/export?target=claude`: what "Copy to Claude Code"
/// would do, read only: `{id, path, source, contourTools, hasDescription,
/// existing, hash, files, skillMd}`. `hash` names the content shown
/// (`files`, `skillMd`); the app's copy takes it and refuses a skill
/// changed since. Nothing over HTTP copies (`crate::skill_export`).
pub async fn export_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ExportQuery>,
) -> ApiResult<Json<Value>> {
    let (entry, dest_root) = crate::skill_export::export_source(&state, &id, &q.target)?;
    let id = entry.info.id.clone();
    let dest = dest_root.join(&id);
    let source = entry.dir.clone();
    let skill_id = id.clone();
    let (files, existing) = crate::spawn::spawn_blocking_in_span(move || {
        (
            user_content::ExportFiles::read(&entry.dir),
            user_content::existing_export(&dest_root, &skill_id)
                .ok()
                .flatten(),
        )
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    let files = files?;
    let check = files.check();
    Ok(Json(json!({
        "id": id,
        "path": dest,
        "source": source,
        "contourTools": check.contour_tools,
        "hasDescription": check.has_description,
        "existing": existing,
        "hash": files.hash(),
        "files": files.paths(),
        "skillMd": files.skill_md(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct SourceQuery {
    #[serde(default)]
    path: String,
}

/// The agent CLI folder `path` when `local-sources` lists it as the
/// imported skill `id`: 400 for any other path (or one listed under
/// another id), 404 when no user skill `id` was imported.
fn listed_source(
    roots: &[(String, std::path::PathBuf)],
    path: &str,
    user_root: &std::path::Path,
    id: &str,
) -> ApiResult<std::path::PathBuf> {
    let path = std::path::PathBuf::from(path);
    let listed = user_content::agent_skill_at(roots, &path, user_root)
        .filter(|s| s.error.is_none() && s.id == id)
        .ok_or_else(|| ApiErr::bad_request("path is no agent CLI skill listed under this id"))?;
    if !listed.imported {
        return Err(ApiErr::not_found("skill not found"));
    }
    Ok(listed.path)
}

/// `GET /api/skills/:id/source-preview?path=…`: `{source: {skillMd,
/// files, hash}, current: {skillMd, files}, changedFiles}`, read only.
/// `hash` names the source content shown; the replace takes it.
pub async fn source_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<SourceQuery>,
) -> ApiResult<Json<Value>> {
    let roots = agent_roots(&state);
    let root = user_root(&state);
    let preview = crate::spawn::spawn_blocking_in_span(move || {
        let source = listed_source(&roots, &q.path, &root, &id)?;
        Ok::<_, ApiErr>(user_content::source_preview(&source, &root, &id)?)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))??;
    Ok(Json(json!({
        "source": {
            "skillMd": preview.source.skill_md(),
            "files": preview.source.paths(),
            "hash": preview.source.hash(),
        },
        "current": {
            "skillMd": preview.current.skill_md(),
            "files": preview.current.paths(),
        },
        "changedFiles": preview.changed_files,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ReplaceBody {
    path: String,
    hash: String,
}

/// `POST /api/skills/:id/replace-from-source` `{path, hash}`: 200 `{id}`;
/// 409 when the folder changed since its preview (nothing written); 400
/// for a path not listed under `id` or a source that names another skill
/// now; 404 without the user skill.
pub async fn replace_from_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<ReplaceBody>,
) -> ApiResult<Json<Value>> {
    let roots = agent_roots(&state);
    let root = user_root(&state);
    let skill_id = id.clone();
    let source = write_and_reload(&state, move || {
        let source = listed_source(&roots, &body.path, &root, &skill_id)?;
        user_content::replace_skill_from_source(&source, &root, &skill_id, &body.hash)?;
        Ok(source)
    })
    .await?;
    tracing::info!(skill = %id, source = %source.display(), "skill replaced from its source");
    Ok(Json(json!({ "id": id })))
}

/// `GET /api/skills/:id/previous`: `{previous}`, whether a replace kept
/// a copy to restore.
pub async fn previous(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let root = user_root(&state);
    let kept =
        crate::spawn::spawn_blocking_in_span(move || user_content::has_previous_skill(&root, &id))
            .await
            .map_err(|e| ApiErr::internal(e.to_string()))?;
    Ok(Json(json!({ "previous": kept })))
}

/// `POST /api/skills/:id/restore-previous`: the kept copy and the user
/// skill swap places; 404 when none is kept.
pub async fn restore_previous(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let root = user_root(&state);
    let skill_id = id.clone();
    write_and_reload(&state, move || {
        Ok(user_content::restore_previous_skill(&root, &skill_id)?)
    })
    .await?;
    tracing::info!(skill = %id, "skill restored to its previous version");
    Ok(Json(json!({ "id": id })))
}
