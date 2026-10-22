use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use od_content::SkillEntry;
use od_contracts::{
    CraftDetail, CraftResponse, DesignSystemDetail, DesignSystemsResponse, DesignTemplatesResponse,
    SkillDetail, SkillSummary, SkillsResponse,
};
use serde_json::Value;

fn summaries(entries: &[SkillEntry]) -> Vec<SkillSummary> {
    entries
        .iter()
        .map(|e| SkillSummary {
            info: e.info.clone(),
            has_body: !e.body.is_empty(),
        })
        .collect()
}

fn detail(entry: &SkillEntry) -> SkillDetail {
    SkillDetail {
        info: entry.info.clone(),
        body: entry.body.clone(),
    }
}

pub async fn list_skills(State(state): State<AppState>) -> ApiJson<SkillsResponse> {
    ApiJson(SkillsResponse {
        skills: summaries(&state.content.snapshot().skills),
    })
}

pub async fn get_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<SkillDetail>> {
    let index = state.content.snapshot();
    index
        .find_skill(&id)
        .map(|e| ApiJson(detail(e)))
        .ok_or_else(|| ApiErr::not_found("skill not found"))
}

pub async fn list_design_templates(
    State(state): State<AppState>,
) -> ApiJson<DesignTemplatesResponse> {
    ApiJson(DesignTemplatesResponse {
        design_templates: summaries(&state.content.snapshot().design_templates),
    })
}

pub async fn get_design_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<SkillDetail>> {
    let index = state.content.snapshot();
    index
        .find_design_template(&id)
        .map(|e| ApiJson(detail(e)))
        .ok_or_else(|| ApiErr::not_found("design template not found"))
}

pub async fn list_design_systems(State(state): State<AppState>) -> ApiJson<DesignSystemsResponse> {
    ApiJson(DesignSystemsResponse {
        design_systems: state
            .content
            .snapshot()
            .design_systems
            .iter()
            .map(|d| d.summary.clone())
            .collect(),
    })
}

/// Detail, also mirrored under `designSystem` like the original response.
pub async fn get_design_system(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<Value>> {
    let index = state.content.snapshot();
    let entry = index
        .find_design_system(&id)
        .ok_or_else(|| ApiErr::not_found("design system not found"))?;
    let detail = DesignSystemDetail {
        summary: entry.summary.clone(),
        body: entry.body.clone(),
        manifest: entry.manifest.clone(),
    };
    let mut value = serde_json::to_value(&detail).map_err(|e| ApiErr::internal(e.to_string()))?;
    if let Value::Object(map) = &mut value {
        let mirror = Value::Object(map.clone());
        map.insert("designSystem".to_owned(), mirror);
    }
    Ok(ApiJson(value))
}

pub async fn list_craft(State(state): State<AppState>) -> ApiJson<CraftResponse> {
    ApiJson(CraftResponse {
        craft: state.content.snapshot().craft.clone(),
    })
}

pub async fn get_craft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<CraftDetail>> {
    if !od_content::craft::is_valid_slug(&id) {
        return Err(ApiErr::bad_request("invalid craft id"));
    }
    let path = state.content.snapshot().craft_dir.join(format!("{id}.md"));
    match tokio::fs::read_to_string(&path).await {
        Ok(body) => Ok(ApiJson(CraftDetail { id, body })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(ApiErr::not_found("craft section not found"))
        }
        Err(e) => Err(ApiErr::internal(e.to_string())),
    }
}
