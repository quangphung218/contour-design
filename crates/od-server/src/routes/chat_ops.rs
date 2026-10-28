//! Track B3-chat routes, mounted with one `merge` in [`crate::router`]:
//!
//! - `DELETE /api/projects/:id/conversations/:cid`: the conversation and
//!   its messages, comments and agent sessions. `409` while one of its runs
//!   is still going. Answers with the conversations left, newest first.
//! - `POST /api/linked-dirs/check {paths}`: which folders may be linked to
//!   a project as a working directory (`metadata.linkedDirs`), each with
//!   its canonical path or the reason it is refused. The UI checks a typed
//!   or picked folder and filters its recent folders with it.
//!
//! Linked folders reach the agent as extra directories (`--add-dir` and
//! the like, see [`linked_dirs`]); they are never copied into the project.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, post};
use od_contracts::{ConversationsResponse, ErrorCode, Project};
use od_store::Store;
use serde::{Deserialize, Serialize};
use std::path::{Path as FsPath, PathBuf};

/// Most folders one check accepts.
const MAX_CHECK: usize = 32;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{id}/conversations/{cid}",
            delete(delete_conversation),
        )
        .route("/api/linked-dirs/check", post(check_dirs))
}

async fn delete_conversation(
    State(state): State<AppState>,
    Path((project_id, conversation_id)): Path<(String, String)>,
) -> ApiResult<ApiJson<ConversationsResponse>> {
    let running = state.runs.registry.list().into_iter().any(|r| {
        r.meta().conversation_id.as_deref() == Some(conversation_id.as_str())
            && !r.status().is_terminal()
    });
    if running {
        return Err(ApiErr::new(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "the conversation has a run in progress; stop it first",
        ));
    }
    let conversations = state
        .blocking(move |s| s.delete_conversation(&project_id, &conversation_id))
        .await?;
    Ok(ApiJson(ConversationsResponse { conversations }))
}

#[derive(Debug, Deserialize)]
struct CheckRequest {
    paths: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckedDir {
    /// As sent.
    input: String,
    /// Canonical folder, when it may be linked.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CheckResponse {
    dirs: Vec<CheckedDir>,
}

async fn check_dirs(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<CheckRequest>,
) -> ApiResult<ApiJson<CheckResponse>> {
    if req.paths.len() > MAX_CHECK {
        return Err(ApiErr::bad_request(format!(
            "at most {MAX_CHECK} folders per check"
        )));
    }
    let dirs = state
        .blocking(move |s| {
            Ok(req
                .paths
                .into_iter()
                .map(|input| match check_dir(s, FsPath::new(input.trim())) {
                    Ok(p) => CheckedDir {
                        input,
                        path: Some(p.to_string_lossy().into_owned()),
                        error: None,
                    },
                    Err(e) => CheckedDir {
                        input,
                        path: None,
                        error: Some(e.to_owned()),
                    },
                })
                .collect())
        })
        .await?;
    Ok(ApiJson(CheckResponse { dirs }))
}

/// The canonical folder when `dir` may be linked: an absolute existing
/// folder, not an app bundle, outside the data directory (except a
/// project's own folder, `<data>/projects/<id>`), without a hidden segment
/// (`~/.ssh`), and not the home folder or one of its parents. Otherwise the
/// same rules as folder import.
///
/// # Errors
/// Why the folder is refused.
pub fn check_dir(store: &Store, dir: &FsPath) -> Result<PathBuf, &'static str> {
    if !dir.is_absolute() {
        return Err("the folder must be an absolute path");
    }
    let dir = dir
        .canonicalize()
        .map_err(|_| "the folder does not exist")?;
    if !dir.is_dir() {
        return Err("not a folder");
    }
    if super::ui::is_bundle(&dir) {
        return Err("an app bundle, not a folder");
    }
    let root = store.data_dir().root();
    let data_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // Another project's own folder ("Reference another project").
    if dir.parent() == Some(data_root.join("projects").as_path()) {
        return Ok(dir);
    }
    if dir.starts_with(&data_root) || data_root.starts_with(&dir) {
        return Err("the folder overlaps the data directory");
    }
    if dir
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Err("the folder is inside a hidden folder");
    }
    if super::ui::is_home_or_parent(&dir) {
        return Err("the home folder or one of its parents");
    }
    Ok(dir)
}

/// The project's linked folders that still pass [`check_dir`], for the
/// agent's extra directories. A folder that is gone or refused is skipped.
#[must_use]
pub fn linked_dirs(store: &Store, project: &Project) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in project
        .metadata
        .as_ref()
        .map(|m| m.linked_dirs.as_slice())
        .unwrap_or_default()
    {
        if let Ok(p) = check_dir(store, FsPath::new(d)) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}
