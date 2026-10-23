//! Skill library writes (spec 1.4 "Thư viện skill", P1 CRUD), the
//! original `SkillsSection`:
//!
//! - `POST   /api/skills` `{name, description?, triggers?, body}`: a new
//!   user skill in `<data>/skills/<slug>/SKILL.md`; 409 when the id exists.
//! - `PUT    /api/skills/:id` (same body): edits a user skill, or writes a
//!   user override of a bundled one (side files copied once).
//! - `DELETE /api/skills/:id`: removes a user skill or override; the
//!   bundled skill shows again.

use super::write_and_reload;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{post, put};
use axum::{Json, Router};
use od_content::user_content::{self, SkillDraft};
use od_contracts::ContentSource;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/skills", post(create))
        .route("/api/skills/{id}", put(save).delete(delete))
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
