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
//!   whether each is imported; "From agent CLI" installs the ones picked.
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

pub async fn local_sources(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let home = crate::skill_export::agent_home(&state.runs.engine.config().base_env);
    let root = user_root(&state);
    let sources = crate::spawn::spawn_blocking_in_span(move || {
        home.map(|h| user_content::scan_agent_skills(&user_content::agent_skill_roots(&h), &root))
            .unwrap_or_default()
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
/// existing}`. Nothing over HTTP copies (`crate::skill_export`).
pub async fn export_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ExportQuery>,
) -> ApiResult<Json<Value>> {
    let (entry, dest_root) = crate::skill_export::export_source(&state, &id, &q.target)?;
    let id = entry.info.id.clone();
    let dest = dest_root.join(&id);
    let skill_id = id.clone();
    let (check, existing) = crate::spawn::spawn_blocking_in_span(move || {
        (
            user_content::export_check(&entry.dir),
            user_content::existing_export(&dest_root, &skill_id)
                .ok()
                .flatten(),
        )
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    Ok(Json(json!({
        "id": id,
        "path": dest,
        "source": state.content.snapshot().find_skill(&id).map(|e| e.dir.clone()),
        "contourTools": check.contour_tools,
        "hasDescription": check.has_description,
        "existing": existing,
    })))
}
