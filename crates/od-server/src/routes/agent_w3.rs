//! Track W3-agent routes (spec 1.1, 1.5), mounted with one `merge` in
//! [`crate::router`]:
//!
//! - `PATCH /api/projects/:id/conversations/:cid`: session mode, title.
//! - `GET /api/projects/:id/conversations/:cid/transcript?format=md|json`.
//! - `GET/POST/DELETE /api/projects/:id/memory`, `DELETE …/memory/:mid`.
//! - `GET/PUT /api/projects/:id/critique`: the Design Jury switch.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::export::attachment;
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::{delete, get, patch};
use od_contracts::{
    Conversation, ConversationResponse, CreateMemoryRequest, CritiqueSettings, MemoryResponse,
    MemorySource, Message, MessageRole, OkResponse, Project, TranscriptFormat,
    UpdateConversationRequest,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Project metadata key of the Design Jury switch (the original's name).
pub const CRITIQUE_METADATA_KEY: &str = "critiqueTheaterEnabled";
/// Forces the Design Jury on (`1`/`true`) or off for every project.
pub const CRITIQUE_ENV: &str = "OD_CRITIQUE_ENABLED";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{id}/conversations/{cid}",
            patch(update_conversation),
        )
        .route(
            "/api/projects/{id}/conversations/{cid}/transcript",
            get(transcript),
        )
        .route(
            "/api/projects/{id}/memory",
            get(list_memory).post(add_memory).delete(clear_memory),
        )
        .route("/api/projects/{id}/memory/{mid}", delete(delete_memory))
        .route(
            "/api/projects/{id}/critique",
            get(get_critique).put(put_critique),
        )
}

async fn update_conversation(
    State(state): State<AppState>,
    Path((project_id, conversation_id)): Path<(String, String)>,
    ApiJson(req): ApiJson<UpdateConversationRequest>,
) -> ApiResult<ApiJson<ConversationResponse>> {
    let conversation = state
        .blocking(move |s| s.update_conversation(&project_id, &conversation_id, req))
        .await?;
    Ok(ApiJson(ConversationResponse { conversation }))
}

#[derive(Debug, Default, Deserialize)]
struct TranscriptQuery {
    #[serde(default)]
    format: Option<String>,
}

async fn transcript(
    State(state): State<AppState>,
    Path((project_id, conversation_id)): Path<(String, String)>,
    Query(q): Query<TranscriptQuery>,
) -> ApiResult<Response> {
    let format = match q.format.as_deref() {
        None | Some("md" | "markdown") => TranscriptFormat::Md,
        Some("json") => TranscriptFormat::Json,
        Some(other) => return Err(ApiErr::bad_request(format!("unknown format {other:?}"))),
    };
    let (project, conversation, messages) = state
        .blocking(move |s| {
            let project = s.get_project(&project_id)?;
            let conversation = s.get_conversation(&project.id, &conversation_id)?;
            let messages = s.list_messages(&conversation.id)?;
            Ok((project, conversation, messages))
        })
        .await?;
    let stem = format!(
        "conversation-{}",
        conversation.id.chars().take(8).collect::<String>()
    );
    Ok(match format {
        TranscriptFormat::Md => attachment(
            "text/markdown; charset=utf-8",
            &format!("{stem}.md"),
            transcript_markdown(&project, &conversation, &messages),
        ),
        TranscriptFormat::Json => attachment(
            "application/json",
            &format!("{stem}.json"),
            serde_json::to_vec_pretty(&json!({
                "project": { "id": project.id, "name": project.name },
                "conversation": conversation,
                "messages": messages,
            }))
            .map_err(|e| ApiErr::internal(e.to_string()))?,
        ),
    })
}

/// The conversation as Markdown: one `##` section per message, tool calls
/// listed under the assistant text.
#[must_use]
pub fn transcript_markdown(
    project: &Project,
    conversation: &Conversation,
    messages: &[Message],
) -> String {
    let title = conversation
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("Conversation");
    let mut out = format!(
        "# {title}\n\n- Project: {}\n- Mode: {}\n- Messages: {}\n",
        project.name,
        serde_json::to_value(conversation.session_mode)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
        messages.len()
    );
    for m in messages {
        let heading = match m.role {
            MessageRole::User => "User".to_owned(),
            MessageRole::Assistant => {
                let agent = m.agent_id.as_deref().unwrap_or("agent");
                match m.run_status {
                    Some(status) => format!("Assistant ({agent}, {})", status.as_str()),
                    None => format!("Assistant ({agent})"),
                }
            }
        };
        out.push_str(&format!("\n## {heading}\n\n"));
        let body = m.content.trim();
        out.push_str(if body.is_empty() { "_(no text)_" } else { body });
        out.push('\n');
        let tools: Vec<String> = m
            .events
            .iter()
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|e| e.get("name").and_then(Value::as_str))
            .map(|name| format!("- `{name}`"))
            .collect();
        if !tools.is_empty() {
            out.push_str(&format!("\nTools:\n{}\n", tools.join("\n")));
        }
    }
    out
}

async fn list_memory(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ApiJson<MemoryResponse>> {
    let memory = state.blocking(move |s| s.list_memory(&project_id)).await?;
    Ok(ApiJson(MemoryResponse { memory }))
}

async fn add_memory(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    ApiJson(req): ApiJson<CreateMemoryRequest>,
) -> ApiResult<ApiJson<MemoryResponse>> {
    let memory = state
        .blocking(move |s| {
            s.add_memory(&project_id, &req.text, MemorySource::Manual)?;
            s.list_memory(&project_id)
        })
        .await?;
    Ok(ApiJson(MemoryResponse { memory }))
}

async fn delete_memory(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(String, String)>,
) -> ApiResult<ApiJson<MemoryResponse>> {
    let memory = state
        .blocking(move |s| {
            s.get_project(&project_id)?;
            s.delete_memory(&project_id, &id)?;
            s.list_memory(&project_id)
        })
        .await?;
    Ok(ApiJson(MemoryResponse { memory }))
}

async fn clear_memory(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ApiJson<OkResponse>> {
    state.blocking(move |s| s.clear_memory(&project_id)).await?;
    Ok(ApiJson(OkResponse { ok: true }))
}

/// The project's own Design Jury switch (metadata), off when unset.
#[must_use]
pub fn critique_switch(project: &Project) -> Option<bool> {
    project
        .metadata
        .as_ref()
        .and_then(|m| m.extra.get(CRITIQUE_METADATA_KEY))
        .and_then(Value::as_bool)
}

async fn get_critique(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<ApiJson<CritiqueSettings>> {
    let project = state.blocking(move |s| s.get_project(&project_id)).await?;
    Ok(ApiJson(CritiqueSettings {
        enabled: critique_switch(&project).unwrap_or(false),
    }))
}

/// Merges the switch into the metadata so other keys survive.
async fn put_critique(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    ApiJson(req): ApiJson<CritiqueSettings>,
) -> ApiResult<ApiJson<CritiqueSettings>> {
    state
        .blocking(move |s| {
            let project = s.get_project(&project_id)?;
            let mut metadata = project.metadata.unwrap_or_default();
            metadata
                .extra
                .insert(CRITIQUE_METADATA_KEY.to_owned(), Value::Bool(req.enabled));
            s.update_project(
                &project_id,
                od_contracts::UpdateProjectRequest {
                    metadata: Some(Some(metadata)),
                    ..od_contracts::UpdateProjectRequest::default()
                },
            )
        })
        .await?;
    Ok(ApiJson(req))
}
