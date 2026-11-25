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
//! - `POST   /api/skills/:id/export` `{target: "claude"}`: "Copy to Claude
//!   Code" copies a Personal skill (`SKILL.md` and side files) into
//!   `<agents' HOME>/.claude/skills/<id>`; 201 `{id, path, contourTools}`.
//!   Only a user skill (403 otherwise); 409 when that folder has an entry
//!   of that name in any case or Unicode form: nothing is written over.
//!   `contourTools`: the skill calls `od`, `$OD_BIN`… in its SKILL.md or
//!   `scripts/` (it may not work outside Contour). 400 for a skill with no
//!   `description` (Claude Code offers its model only skills with one).
//!   `GET` with `?target=claude`: the same checks, read only, as
//!   `{id, path, contourTools, hasDescription}`.

use super::write_and_reload;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
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
        .route("/api/skills/{id}/export", post(export).get(export_preview))
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

/// `HOME` of the agents' environment (the folders they read their skills
/// from): the last `HOME` set, as an agent process gets it, else the
/// daemon's.
fn agent_home(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Option<std::path::PathBuf> {
    env.iter()
        .rfind(|(k, _)| k == "HOME")
        .filter(|(_, v)| !v.is_empty())
        .map(|(_, v)| std::path::PathBuf::from(v))
        .or_else(od_runtime::login_path::home_dir)
}

pub async fn local_sources(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let home = agent_home(&state.runs.engine.config().base_env);
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
pub struct ExportBody {
    target: String,
}

/// The Personal skill `id` and the folder "Copy to Claude Code" writes
/// it to (`<agents' HOME>/.claude/skills/<id>`), after the checks every
/// export makes: a valid target and id, a user skill.
fn export_source(
    state: &AppState,
    id: &str,
    target: &str,
) -> ApiResult<(od_content::skills::SkillEntry, std::path::PathBuf)> {
    if target != "claude" {
        return Err(ApiErr::bad_request("target must be \"claude\""));
    }
    if !od_content::design_systems::is_valid_id(id) || id.starts_with(['.', '_']) {
        return Err(ApiErr::bad_request(format!("invalid skill id {id:?}")));
    }
    let entry = state
        .content
        .snapshot()
        .find_skill(id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("skill not found"))?;
    if entry.info.source != ContentSource::User {
        return Err(ApiErr::forbidden("only Personal skills can be copied"));
    }
    let home = agent_home(&state.runs.engine.config().base_env)
        .ok_or_else(|| ApiErr::internal("no home folder"))?;
    // Per component: `.claude/skills` as one string shows `/` on Windows.
    let dest_root = home.join(".claude").join("skills");
    Ok((entry, dest_root))
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    target: String,
}

/// `GET /api/skills/:id/export?target=claude`: what a copy would do, read
/// only: `{id, path, contourTools, hasDescription}`. The app asks it when
/// "Copy to Claude Code" is clicked, to warn first and to name the folder.
pub async fn export_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ExportQuery>,
) -> ApiResult<Json<Value>> {
    let (entry, dest_root) = export_source(&state, &id, &q.target)?;
    let id = entry.info.id.clone();
    let check =
        crate::spawn::spawn_blocking_in_span(move || user_content::export_check(&entry.dir))
            .await
            .map_err(|e| ApiErr::internal(e.to_string()))?;
    Ok(Json(json!({
        "id": id,
        "path": dest_root.join(&id),
        "contourTools": check.contour_tools,
        "hasDescription": check.has_description,
    })))
}

pub async fn export(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<ExportBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let (entry, dest_root) = export_source(&state, &id, &body.target)?;
    let root = user_root(&state);
    let id = entry.info.id.clone();
    let dest = dest_root.join(&id);
    let skill_id = id.clone();
    let (path, check) = crate::spawn::spawn_blocking_in_span(move || {
        let path = user_content::export_skill(&root, &entry.dir, &skill_id, &dest_root)?;
        Ok::<_, user_content::WriteError>((path, user_content::export_check(&entry.dir)))
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(|e| match e {
        user_content::WriteError::Exists(_) => ApiErr::new(
            StatusCode::CONFLICT,
            od_contracts::ErrorCode::Conflict,
            format!(
                "Claude Code already has a skill named {id:?} at {}. Remove that folder to copy again.",
                dest.display()
            ),
        ),
        other => other.into(),
    })?;
    tracing::info!(skill = %id, path = %path.display(), "skill copied to Claude Code");
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "path": path,
            "contourTools": check.contour_tools,
        })),
    ))
}
