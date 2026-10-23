//! Terminal tabs over WebSocket (spec 1.5 Terminal):
//!
//! - `GET /api/projects/:id/terminals` lists the project's shells.
//! - `POST /api/projects/:id/terminals` `{cols?, rows?}` starts a shell in
//!   the project folder → 201 with its info.
//! - `GET /api/terminals/:tid/ws` attaches: the scrollback first, then live
//!   output as binary frames, and `{"type":"exit",…}` when the shell ends.
//!   Client frames: binary or `{"type":"input","data"}` is typed into the
//!   shell, `{"type":"resize","cols","rows"}` resizes it.
//! - `DELETE /api/terminals/:tid` kills the shell's whole tree.

use super::Platform;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Extension, Router};
use od_term::{Output, Session, TermError, TerminalInfo};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Largest client frame (a paste) accepted.
const MAX_FRAME_BYTES: usize = 64 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/terminals", get(list).post(create))
        .route("/api/terminals/{tid}", delete(close))
        .route("/api/terminals/{tid}/ws", get(attach))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalsResponse {
    pub terminals: Vec<TerminalInfo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTerminalRequest {
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

/// A text frame from the client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientFrame {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

async fn list(
    State(state): State<AppState>,
    Extension(platform): Extension<Platform>,
    Path(project_id): Path<String>,
) -> ApiResult<ApiJson<TerminalsResponse>> {
    let id = project_id.clone();
    state.blocking(move |s| s.get_project(&id)).await?;
    Ok(ApiJson(TerminalsResponse {
        terminals: platform.terminals.list(&project_id),
    }))
}

async fn create(
    State(state): State<AppState>,
    Extension(platform): Extension<Platform>,
    Path(project_id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Response> {
    let req: CreateTerminalRequest = if body.is_empty() {
        CreateTerminalRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiErr::bad_request(e.to_string()))?
    };
    let id = project_id.clone();
    let cwd = state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            Ok(s.project_root(&project)?.dir)
        })
        .await?;
    let terminals = Arc::clone(&platform.terminals);
    let session = tokio::task::spawn_blocking(move || {
        terminals.create(&project_id, &cwd, req.cols, req.rows)
    })
    .await
    .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
    .map_err(|e| match e {
        TermError::Limit(_) => ApiErr::new(
            StatusCode::CONFLICT,
            od_contracts::ErrorCode::Conflict,
            e.to_string(),
        ),
        TermError::BadCwd(_) | TermError::Spawn(_) => ApiErr::internal(e.to_string()),
    })?;
    tracing::info!(terminal = %session.id(), project = %session.project_id(), "terminal opened");
    Ok((StatusCode::CREATED, ApiJson(session.info())).into_response())
}

async fn close(
    Extension(platform): Extension<Platform>,
    Path(tid): Path<String>,
) -> ApiResult<ApiJson<od_contracts::OkResponse>> {
    if platform.terminals.close(&tid).await {
        tracing::info!(terminal = %tid, "terminal closed");
        Ok(ApiJson(od_contracts::OkResponse { ok: true }))
    } else {
        Err(ApiErr::not_found("terminal not found"))
    }
}

async fn attach(
    Extension(platform): Extension<Platform>,
    Path(tid): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let session = platform
        .terminals
        .get(&tid)
        .ok_or_else(|| ApiErr::not_found("terminal not found"))?;
    Ok(ws
        .max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| pump(socket, session)))
}

fn exit_frame(exit: &od_term::TermExit) -> Message {
    let body = serde_json::json!({"type": "exit", "code": exit.code, "signal": exit.signal});
    Message::Text(body.to_string().into())
}

/// Shuttles bytes between one client and the shell until either side ends.
async fn pump(mut socket: WebSocket, session: Arc<Session>) {
    let mut attached = session.attach();
    if !attached.scrollback.is_empty()
        && socket
            .send(Message::Binary(
                std::mem::take(&mut attached.scrollback).into(),
            ))
            .await
            .is_err()
    {
        return;
    }
    if let Some(exit) = &attached.exit {
        let _ = socket.send(exit_frame(exit)).await;
        return;
    }
    loop {
        tokio::select! {
            out = attached.live.recv() => match out {
                Ok(Output::Data(bytes)) => {
                    if socket.send(Message::Binary(bytes.to_vec().into())).await.is_err() {
                        return;
                    }
                }
                Ok(Output::Exit(exit)) => {
                    let _ = socket.send(exit_frame(&exit)).await;
                    return;
                }
                // A slow client skipped some output; keep going.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!(terminal = %session.id(), skipped = n, "terminal client lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            msg = socket.recv() => {
                let input = match msg {
                    Some(Ok(Message::Binary(b))) => b.to_vec(),
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<ClientFrame>(&t) {
                        Ok(ClientFrame::Input { data }) => data.into_bytes(),
                        Ok(ClientFrame::Resize { cols, rows }) => {
                            session.resize(cols, rows);
                            continue;
                        }
                        Err(_) => continue, // unknown frames are ignored
                    },
                    Some(Ok(_)) => continue,
                    // The client went away; the shell stays for a reattach.
                    Some(Err(_)) | None => return,
                };
                // A failed write means the shell is gone: its exit frame
                // follows on the channel.
                let s = Arc::clone(&session);
                let _ = tokio::task::spawn_blocking(move || s.write(&input)).await;
            }
        }
    }
}
