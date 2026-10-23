//! Settings ▸ Memory (spec 1.5): facts every project's prompt carries.
//!
//! - `GET /api/memory` → `{memories, kinds}`
//! - `POST /api/memory` `{kind, text}` → `{memory}`
//! - `DELETE /api/memory/:id`

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use od_store::manage::{GlobalMemory, MEMORY_KINDS};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct MemoryList {
    pub memories: Vec<GlobalMemory>,
    pub kinds: [&'static str; 6],
}

#[derive(Debug, Deserialize)]
pub struct NewMemory {
    #[serde(default = "default_kind")]
    pub kind: String,
    pub text: String,
}

fn default_kind() -> String {
    "user".to_owned()
}

#[derive(Debug, Serialize)]
pub struct MemoryResponse {
    pub memory: GlobalMemory,
}

pub async fn list(State(state): State<AppState>) -> ApiResult<ApiJson<MemoryList>> {
    let memories = state.blocking(|s| s.list_global_memory()).await?;
    Ok(ApiJson(MemoryList {
        memories,
        kinds: MEMORY_KINDS,
    }))
}

pub async fn add(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<NewMemory>,
) -> ApiResult<ApiJson<MemoryResponse>> {
    let memory = state
        .blocking(move |s| {
            let entry = s.add_global_memory(&req.kind, &req.text)?;
            // A saved MEMORY.md links the new entry, so it is injected.
            od_store::memory_index::MemoryFiles::in_data_dir(s.data_dir().root()).link(&entry)?;
            Ok(entry)
        })
        .await?;
    Ok(ApiJson(MemoryResponse { memory }))
}

pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<od_contracts::OkResponse>> {
    let removed = state
        .blocking(move |s| {
            let removed = s.delete_global_memory(&id)?;
            od_store::memory_index::MemoryFiles::in_data_dir(s.data_dir().root()).unlink(&id)?;
            Ok(removed)
        })
        .await?;
    if removed {
        Ok(ApiJson(od_contracts::OkResponse { ok: true }))
    } else {
        Err(ApiErr::not_found("memory not found"))
    }
}
