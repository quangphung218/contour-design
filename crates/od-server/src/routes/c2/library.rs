//! "Import from library" in the composer (the original's
//! `POST /api/library/assets/:id/apply`): `POST /api/library/assets/apply`
//! `{projectId, assets: [{projectId, path}], dir?}` → `{files}`.
//!
//! Each asset is copied into `dir` (default `library/`) of the target
//! project under a fresh name, so the turn can attach it. An asset that
//! already lives in the target project is attached in place, not copied.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use od_contracts::{FileKind, UploadedFile};
use od_store::StoreError;
use serde::Deserialize;
use serde_json::{Value, json};

/// Most assets one import copies.
pub const MAX_APPLY: usize = 24;
const DEFAULT_DIR: &str = "library";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    pub project_id: String,
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBody {
    pub project_id: String,
    pub assets: Vec<AssetRef>,
    #[serde(default)]
    pub dir: Option<String>,
}

pub async fn apply(
    axum::extract::State(state): axum::extract::State<AppState>,
    ApiJson(body): ApiJson<ApplyBody>,
) -> ApiResult<axum::Json<Value>> {
    if body.assets.is_empty() || body.assets.len() > MAX_APPLY {
        return Err(ApiErr::bad_request(format!("pick 1–{MAX_APPLY} assets")));
    }
    let files = state
        .blocking(move |s| {
            s.get_project(&body.project_id)?;
            let dir = body.dir.as_deref().unwrap_or(DEFAULT_DIR);
            let mut out = Vec::with_capacity(body.assets.len());
            for a in &body.assets {
                let (file, bytes) = s.read_file(&a.project_id, &a.path)?;
                if !matches!(
                    file.kind,
                    FileKind::Image | FileKind::Video | FileKind::Audio
                ) {
                    return Err(StoreError::BadRequest(format!(
                        "{} is not a library asset",
                        a.path
                    )));
                }
                if a.project_id == body.project_id {
                    out.push(UploadedFile {
                        name: file.name.clone(),
                        path: file.name,
                        size: file.size,
                        mtime: file.mtime,
                        original_name: a.path.clone(),
                    });
                    continue;
                }
                let base = a.path.rsplit('/').next().unwrap_or(&a.path);
                out.push(s.upload_file(&body.project_id, Some(dir), base, &bytes)?);
            }
            Ok(out)
        })
        .await?;
    Ok(axum::Json(json!({ "files": files })))
}
