//! Conversations and messages of a project (spec 2.2).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use od_contracts::{
    ConversationResponse, ConversationsResponse, CreateConversationRequest, MessageResponse,
    MessagesResponse, UpsertMessageRequest,
};
use od_preview::ProjectEvent;

pub async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ApiJson<ConversationsResponse>> {
    let conversations = state
        .blocking(move |s| s.list_conversations(&project_id))
        .await?;
    Ok(ApiJson(ConversationsResponse { conversations }))
}

/// Body is optional.
pub async fn create(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    body: Bytes,
) -> ApiResult<ApiJson<ConversationResponse>> {
    let req: CreateConversationRequest = if body.iter().all(u8::is_ascii_whitespace) {
        CreateConversationRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiErr::bad_request(e.to_string()))?
    };
    let conversation = state
        .blocking(move |s| s.create_conversation(&project_id, req))
        .await?;
    state.preview.events.publish(
        &conversation.project_id,
        ProjectEvent::ConversationCreated {
            project_id: conversation.project_id.clone(),
            conversation_id: conversation.id.clone(),
            title: conversation.title.clone(),
            created_at: conversation.created_at,
        },
    );
    Ok(ApiJson(ConversationResponse { conversation }))
}

pub async fn messages(
    State(state): State<AppState>,
    Path((project_id, conversation_id)): Path<(String, String)>,
) -> ApiResult<ApiJson<MessagesResponse>> {
    let messages = state
        .blocking(move |s| {
            s.get_conversation(&project_id, &conversation_id)?;
            s.list_messages(&conversation_id)
        })
        .await?;
    Ok(ApiJson(MessagesResponse { messages }))
}

pub async fn put_message(
    State(state): State<AppState>,
    Path((project_id, conversation_id, message_id)): Path<(String, String, String)>,
    ApiJson(req): ApiJson<UpsertMessageRequest>,
) -> ApiResult<ApiJson<MessageResponse>> {
    let message = state
        .blocking(move |s| {
            s.get_conversation(&project_id, &conversation_id)?;
            s.upsert_message(
                &conversation_id,
                &message_id,
                req.role,
                &req.content,
                req.agent_id.as_deref(),
            )
        })
        .await?;
    Ok(ApiJson(MessageResponse { message }))
}
