//! `POST /api/projects/:id/export/mp4`: a HyperFrames composition → MP4
//! (spec 1.3, 4.2). Body: the shared export fields (`fileName` = the
//! composition's `index.html`), optional `fps`, optional `output` (a
//! project path ending in `.mp4`: the video is saved there and the file
//! is returned as JSON instead of a download).

use crate::routes::export::{
    Entry, ExportErr, ExportRequest, ExportResult, attachment, internal, resolve_entry, scratch,
};
use crate::routes::w3::motion::audio_plan;
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
use od_render_cdp::ffmpeg::{AudioTrack, encode_mp4_with_audio, find_ffmpeg};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

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
/// of `keep` (seconds), clamped to the last frame. `require_timeline`:
/// fail when the page never registered its root timeline. `audio` is
/// mixed into one AAC stream as long as the captured film; none leaves
/// the MP4 picture only.
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
    require_timeline: bool,
    audio: &[AudioTrack],
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
                require_timeline,
            },
            &frames_dir,
        )
        .await?;
    let mp4 = scratch.dir.path().join("out.mp4");
    let film_seconds = f64::from(frames.count) / f64::from(frames.fps.max(1));
    encode_mp4_with_audio(ffmpeg, &frames_dir, frames.fps, &mp4, audio, film_seconds).await?;
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

/// Response header of a downloaded MP4: how many audio tracks it mixes, for
/// the Export toast and `od export` (the JSON form says `audioTracks`).
pub const AUDIO_TRACKS_HEADER: &str = "x-od-audio-tracks";

/// One line per MP4 export about its sound, with the fields of the
/// `media generate` log, so both ways out of a film are counted.
fn log_audio(project: &str, started: Instant, policy: &str, tracks: usize, mux: &str) {
    tracing::info!(
        project,
        render_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        audio_policy = policy,
        audio_tracks = tracks,
        audio_mux = mux,
        "export mp4 audio"
    );
}

pub async fn export_mp4(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let started = Instant::now();
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
    // The composition's sound, as `od media generate` mixes it: checked
    // before capture, so a required track that is missing writes nothing.
    let html = String::from_utf8_lossy(&html).into_owned();
    let policy = hyperframes::audio_policy(&html).as_str();
    let comp_dir = match entry.path.rsplit_once('/') {
        Some((dir, _)) => entry.root.dir.join(dir),
        None => entry.root.dir.clone(),
    };
    #[allow(clippy::cast_precision_loss)] // film lengths are far below 2^52 ms
    let film_seconds = comp
        .duration_ms
        .map_or(f64::INFINITY, |ms| ms as f64 / 1000.0);
    let plan = tokio::task::spawn_blocking(move || audio_plan(&comp_dir, &html, film_seconds))
        .await
        .map_err(internal)?
        .map_err(|e| {
            log_audio(&entry.project.id, started, policy, 0, "refused");
            // Export keeps refusing (a film that names a track must not
            // leave silently), but says how to get the picture alone.
            bad(
                ErrorCode::ValidationFailed,
                StatusCode::UNPROCESSABLE_ENTITY,
                format!(
                    "{e} Or declare <meta name=\"od-motion-audio\" content=\"silent\"> to export without sound."
                ),
            )
        })?;
    let tracks = plan.tracks.len();
    let (bytes, frames, _) = render_mp4(
        &state,
        renderer.as_ref(),
        &entry,
        &mut req.base,
        &comp,
        fps,
        &ffmpeg,
        &[],
        false,
        &plan.tracks,
    )
    .await
    .inspect_err(|_| log_audio(&entry.project.id, started, policy, tracks, "failed"))?;
    log_audio(
        &entry.project.id,
        started,
        policy,
        tracks,
        if tracks == 0 { "none" } else { "mixed" },
    );
    let Some(output) = output else {
        let mut res = attachment(
            "video/mp4",
            &entry.filename(req.base.title.as_deref(), "mp4"),
            bytes,
        );
        res.headers_mut()
            .insert(AUDIO_TRACKS_HEADER, axum::http::HeaderValue::from(tracks));
        return Ok(res);
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
        "audioTracks": tracks,
    }))
    .into_response())
}
