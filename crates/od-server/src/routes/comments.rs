//! Preview comments of a conversation (spec 1.2, 3.3). Mounted with one
//! `merge` in [`crate::router`].
//!
//! - `GET/POST /api/projects/:id/conversations/:cid/comments`
//! - `PATCH/DELETE /api/projects/:id/conversations/:cid/comments/:commentId`

use crate::error::{ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, patch};
use od_contracts::{
    CommentResponse, CommentsResponse, CreateCommentRequest, OkResponse, UpdateCommentRequest,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{id}/conversations/{cid}/comments",
            get(list).post(create),
        )
        .route(
            "/api/projects/{id}/conversations/{cid}/comments/{comment_id}",
            patch(update).delete(remove),
        )
}

async fn list(
    State(state): State<AppState>,
    Path((project_id, conversation_id)): Path<(String, String)>,
) -> ApiResult<ApiJson<CommentsResponse>> {
    let comments = state
        .blocking(move |s| s.list_comments(&project_id, &conversation_id))
        .await?;
    Ok(ApiJson(CommentsResponse { comments }))
}

async fn create(
    State(state): State<AppState>,
    Path((project_id, conversation_id)): Path<(String, String)>,
    ApiJson(req): ApiJson<CreateCommentRequest>,
) -> ApiResult<ApiJson<CommentResponse>> {
    let comment = state
        .blocking(move |s| s.create_comment(&project_id, &conversation_id, req))
        .await?;
    Ok(ApiJson(CommentResponse { comment }))
}

async fn update(
    State(state): State<AppState>,
    Path((project_id, conversation_id, id)): Path<(String, String, String)>,
    ApiJson(req): ApiJson<UpdateCommentRequest>,
) -> ApiResult<ApiJson<CommentResponse>> {
    let comment = state
        .blocking(move |s| s.update_comment(&project_id, &conversation_id, &id, req))
        .await?;
    Ok(ApiJson(CommentResponse { comment }))
}

async fn remove(
    State(state): State<AppState>,
    Path((project_id, conversation_id, id)): Path<(String, String, String)>,
) -> ApiResult<ApiJson<OkResponse>> {
    state
        .blocking(move |s| s.delete_comment(&project_id, &conversation_id, &id))
        .await?;
    Ok(ApiJson(OkResponse { ok: true }))
}
