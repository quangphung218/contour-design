//! The bundled Motion templates (`content/design-templates/motion-*`, Industry
//! "Motion") rendered to MP4 with real Chrome and ffmpeg, as `media generate
//! --model hyperframes-html` would. Each film must render every frame, move
//! (the poster frame differs from the opening frame) and encode to H.264.
//!
//! A full film is hundreds of frames, so this is opt-in:
//! `cargo test -p od-render-cdp --test motion_templates -- --ignored --nocapture`.
//! `OD_MOTION_TEMPLATES=a,b` picks the films (default: three), and
//! `OD_MOTION_OUT=<dir>` keeps the MP4s and poster frames there.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::hyperframes::{composition, frame_count};
use od_export::renderer::frame_file_name;
use od_export::{FrameOptions, RenderTarget, Renderer};
use od_render_cdp::ffmpeg::{encode_mp4, find_ffmpeg};
use od_render_cdp::find_chrome;
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT: &str = "motion-logo-reveal,motion-social-story,motion-example-bike-share-year";

fn templates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "renders whole films (minutes); run with --ignored"]
async fn motion_templates_render_to_mp4() {
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let ffmpeg = find_ffmpeg().expect("ffmpeg is required");
    let ids = std::env::var("OD_MOTION_TEMPLATES").unwrap_or_else(|_| DEFAULT.to_owned());
    let scratch = tempfile::tempdir().unwrap();
    let out = std::env::var_os("OD_MOTION_OUT")
        .map_or_else(|| scratch.path().to_path_buf(), PathBuf::from);
    std::fs::create_dir_all(&out).unwrap();
    for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let source = templates_root().join(id).join("example.html");
        let html = std::fs::read_to_string(&source)
            .unwrap_or_else(|e| panic!("{}: {e}", source.display()));
        let meta = composition(&html).unwrap();
        let duration_ms = meta.duration_ms.unwrap();
        let mut target = RenderTarget::new(source);
        target.width = meta.width;
        target.height = meta.height;
        let frames_dir = scratch.path().join(format!("{id}-frames"));
        let started = Instant::now();
        let frames = renderer
            .render_frames(
                &target,
                FrameOptions {
                    fps: meta.fps,
                    duration_ms,
                    require_timeline: true,
                },
                &frames_dir,
            )
            .await
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let captured = started.elapsed();
        assert_eq!(
            u64::from(frames.count),
            frame_count(duration_ms, meta.fps),
            "{id}"
        );
        assert_eq!((frames.width, frames.height), (meta.width, meta.height));
        // The film moves: opening and poster frames differ.
        let poster_at = poster_time(&html) * f64::from(meta.fps);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small, positive
        let poster_frame = poster_at.round() as u32;
        let first = std::fs::read(frames_dir.join(frame_file_name(0))).unwrap();
        let poster = std::fs::read(frames_dir.join(frame_file_name(poster_frame))).unwrap();
        assert_ne!(
            first, poster,
            "{id}: the poster frame equals the first frame"
        );
        std::fs::write(out.join(format!("{id}-poster.png")), &poster).unwrap();
        let mp4 = out.join(format!("{id}.mp4"));
        encode_mp4(&ffmpeg, &frames_dir, frames.fps, &mp4)
            .await
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let size = std::fs::metadata(&mp4).unwrap().len();
        assert!(size > 10_000, "{id}: {size} bytes");
        println!(
            "{id}: {} frames {}x{} @ {} fps, captured in {:.1} s, total {:.1} s, {} KiB",
            frames.count,
            frames.width,
            frames.height,
            frames.fps,
            captured.as_secs_f64(),
            started.elapsed().as_secs_f64(),
            size / 1024
        );
    }
}

/// `data-poster-time` of the root, in seconds.
fn poster_time(html: &str) -> f64 {
    let at = html.find("data-poster-time=\"").unwrap() + "data-poster-time=\"".len();
    html[at..].split('"').next().unwrap().parse().unwrap()
}
