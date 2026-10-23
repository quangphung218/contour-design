use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use od_contracts::{
    CreateProjectRequest, CreateProjectResponse, DuplicateProjectRequest, DuplicateProjectResponse,
    OkResponse, ProjectResponse, ProjectsResponse, UpdateProjectRequest,
};

pub async fn list(State(state): State<AppState>) -> ApiResult<ApiJson<ProjectsResponse>> {
    let projects = state.blocking(|s| s.list_projects_with_entry()).await?;
    Ok(ApiJson(ProjectsResponse { projects }))
}

pub async fn create(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<CreateProjectRequest>,
) -> ApiResult<ApiJson<CreateProjectResponse>> {
    // baseDir points a project at the user's own folder; only folder import
    // (which validates the folder) may set it.
    if req.metadata.as_ref().is_some_and(|m| m.base_dir.is_some()) {
        return Err(ApiErr::bad_request(
            "baseDir can only be set by folder import",
        ));
    }
    let mut req = req;
    let (project, conversation_id) = state
        .blocking(move |s| {
            // Settings ▸ Project locations: a custom default base.
            crate::routes::manage::locations::place_in_default(s, &mut req)?;
            s.create_project(req)
        })
        .await?;
    Ok(ApiJson(CreateProjectResponse {
        project,
        conversation_id,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<ProjectResponse>> {
    let project = state.blocking(move |s| s.get_project(&id)).await?;
    Ok(ApiJson(ProjectResponse { project }))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(patch): ApiJson<UpdateProjectRequest>,
) -> ApiResult<ApiJson<ProjectResponse>> {
    let project = state
        .blocking(move |s| s.update_project(&id, patch))
        .await?;
    Ok(ApiJson(ProjectResponse { project }))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<OkResponse>> {
    let pid = id.clone();
    state.blocking(move |s| s.delete_project(&pid)).await?;
    // Its shells would keep running in a folder that is gone.
    state.platform.terminals.close_project(&id).await;
    Ok(ApiJson(OkResponse { ok: true }))
}

/// Body is optional: an empty body duplicates with the default name.
pub async fn duplicate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<ApiJson<DuplicateProjectResponse>> {
    let req: DuplicateProjectRequest = if body.iter().all(u8::is_ascii_whitespace) {
        DuplicateProjectRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiErr::bad_request(e.to_string()))?
    };
    let res = state
        .blocking(move |s| s.duplicate_project(&id, req.name.as_deref()))
        .await?;
    Ok(ApiJson(res))
}
