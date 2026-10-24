use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
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
            has_example: e.example_html().is_some(),
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

/// Inline styles and `data:` media only: no script runs and nothing is
/// fetched (web fonts fall back to local ones), even when the page is
/// opened on its own rather than in the sandboxed thumbnail frame.
const EXAMPLE_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; \
     font-src data:; media-src data:; sandbox";

/// Example pages copied from the upstream app name it (sample decks and
/// dashboards about "OpenDesign"); the gallery thumbnails and design system
/// previews show Contour instead.
/// The files on disk keep their original text.
pub(crate) fn rebrand_example(html: Vec<u8>) -> Vec<u8> {
    match String::from_utf8(html) {
        Ok(text) if text.contains("OpenDesign") || text.contains("Open Design") => text
            .replace("OpenDesign", "Contour")
            .replace("Open Design", "Contour")
            .into_bytes(),
        Ok(text) => text.into_bytes(),
        Err(e) => e.into_bytes(),
    }
}

/// `GET /api/design-templates/:id/example`: the template's rendered
/// example page, for the Home gallery thumbnails.
pub async fn get_design_template_example(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let path = state
        .content
        .snapshot()
        .find_design_template(&id)
        .and_then(SkillEntry::example_html)
        .ok_or_else(|| ApiErr::not_found("design template example not found"))?;
    let html = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiErr::not_found("design template example not found"))?;
    let html = rebrand_example(html);
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, EXAMPLE_CSP),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response())
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

#[cfg(test)]
mod tests {
    use super::rebrand_example;

    #[test]
    fn gallery_examples_show_the_new_name() {
        let out = rebrand_example(b"<b>Show HN: OpenDesign v2</b> by Open Design".to_vec());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<b>Show HN: Contour v2</b> by Contour"
        );
        assert_eq!(rebrand_example(vec![0xff, b'O']), vec![0xff, b'O']);
    }
}
