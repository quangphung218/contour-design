//! `POST /api/projects/:id/export/mp4`: a HyperFrames composition → MP4
//! (spec 1.3, 4.2). Body: the shared export fields (`fileName` = the
//! composition's `index.html`), optional `fps`, optional `output` (a
//! project path ending in `.mp4`: the video is saved there and the file
//! is returned as JSON instead of a download).

use crate::routes::export::{
    Entry, ExportErr, ExportRequest, ExportResult, attachment, internal, resolve_entry, scratch,
};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use od_contracts::ErrorCode;
use od_export::hyperframes::{self, Composition, MAX_FPS};
use od_export::renderer::frame_file_name;
use od_export::{FrameOptions, FrameSet, Renderer};
use od_render_cdp::ffmpeg::{encode_mp4, find_ffmpeg};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mp4Request {
    #[serde(flatten)]
    base: ExportRequest,
    #[serde(default)]
    fps: Option<u32>,
    #[serde(default)]
    output: Option<String>,
}

fn bad(code: ErrorCode, status: StatusCode, message: impl Into<String>) -> ExportErr {
    ExportErr::new(status, code, message)
}

/// Captures `entry` (a composition read as `comp`) at `fps` and encodes
/// it with `ffmpeg`: the MP4, its frames, and the PNG of the frame at each
/// of `keep` (seconds), clamped to the last frame.
///
/// # Errors
/// Renderer and ffmpeg failures, I/O.
#[allow(clippy::too_many_arguments)] // one call per route, all required
pub(crate) async fn render_mp4(
    state: &AppState,
    renderer: &dyn Renderer,
    entry: &Entry,
    base: &mut ExportRequest,
    comp: &Composition,
    fps: u32,
    ffmpeg: &std::path::Path,
    keep: &[f64],
) -> ExportResult<(Vec<u8>, FrameSet, Vec<Vec<u8>>)> {
    base.width = Some(comp.width);
    base.height = Some(comp.height);
    let scratch = scratch(state, entry, base).await?;
    let frames_dir = scratch.dir.path().join("frames");
    let frames = renderer
        .render_frames(
            &scratch.target,
            FrameOptions {
                fps,
                duration_ms: comp.duration_ms.unwrap_or(0),
            },
            &frames_dir,
        )
        .await?;
    let mp4 = scratch.dir.path().join("out.mp4");
    encode_mp4(ffmpeg, &frames_dir, frames.fps, &mp4).await?;
    let bytes = tokio::fs::read(&mp4).await.map_err(internal)?;
    let mut kept = Vec::with_capacity(keep.len());
    for t in keep {
        let png = frames_dir.join(frame_file_name(frame_at(*t, frames)));
        kept.push(tokio::fs::read(&png).await.map_err(internal)?);
    }
    Ok((bytes, frames, kept))
}

/// 0-based index of the frame shown at `seconds`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped
fn frame_at(seconds: f64, frames: FrameSet) -> u32 {
    let last = frames.count.saturating_sub(1);
    let i = (seconds.max(0.0) * f64::from(frames.fps)).floor();
    if i.is_finite() && i < f64::from(last) {
        i as u32
    } else {
        last
    }
}

pub async fn export_mp4(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let mut req: Mp4Request = if body.iter().all(u8::is_ascii_whitespace) {
        Mp4Request::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| {
            bad(
                ErrorCode::BadRequest,
                StatusCode::BAD_REQUEST,
                e.to_string(),
            )
        })?
    };
    let output = req
        .output
        .as_deref()
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .map(str::to_owned);
    if output
        .as_deref()
        .is_some_and(|o| !o.to_ascii_lowercase().ends_with(".mp4"))
    {
        return Err(bad(
            ErrorCode::BadRequest,
            StatusCode::BAD_REQUEST,
            "output must be a project path ending in .mp4",
        ));
    }
    let entry = resolve_entry(&state, id, &req.base).await?;
    let (root, path) = (entry.root.clone(), entry.path.clone());
    let html = tokio::task::spawn_blocking(move || od_store::files::read_file(&root, &path))
        .await
        .map_err(internal)??
        .1;
    let comp = hyperframes::composition(&String::from_utf8_lossy(&html)).map_err(|e| {
        bad(
            ErrorCode::ValidationFailed,
            StatusCode::UNPROCESSABLE_ENTITY,
            e.to_string(),
        )
    })?;
    let fps = req
        .fps
        .filter(|f| (1..=MAX_FPS).contains(f))
        .unwrap_or(comp.fps);
    // Fail fast, before minutes of capture, when ffmpeg is missing.
    let ffmpeg = find_ffmpeg()?;
    let (bytes, frames, _) = render_mp4(
        &state,
        renderer.as_ref(),
        &entry,
        &mut req.base,
        &comp,
        fps,
        &ffmpeg,
        &[],
    )
    .await?;
    let Some(output) = output else {
        return Ok(attachment(
            "video/mp4",
            &entry.filename(req.base.title.as_deref(), "mp4"),
            bytes,
        ));
    };
    let project_id = entry.project.id.clone();
    let file = state
        .blocking(move |s| s.write_file(&project_id, &output, &bytes, true, None))
        .await?;
    Ok(Json(json!({
        "file": file,
        "frames": frames.count,
        "fps": frames.fps,
        "width": frames.width,
        "height": frames.height,
    }))
    .into_response())
}
