//! HyperFrames → MP4 with real Chrome and ffmpeg (spec 5.7 M6): a 2 s,
//! 30 fps composition becomes an H.264 `yuv420p` MP4 of exactly 60 frames,
//! counted by ffprobe.
//!
//! Needs Chrome (as `chrome.rs`) and ffmpeg + ffprobe (`OD_FFMPEG_PATH` or
//! a standard install). Missing tools = failure, unless
//! `OD_SKIP_CHROME_TESTS=1` is set.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::hyperframes::composition;
use od_export::image::{decode_rgb, png_size};
use od_export::renderer::frame_file_name;
use od_export::{FrameOptions, RenderTarget, Renderer};
use od_render_cdp::ffmpeg::{encode_mp4, find_ffmpeg};
use od_render_cdp::find_chrome;
use serde_json::Value;

/// Odd size on purpose: the encoder pads to 642×362. The timeline is a
/// plain object with `seek` (no GSAP: the browser has no network); it
/// paints red for the first second and blue after, and moves a bar.
const COMPOSITION: &str = r#"<!doctype html><html><head><style>
html,body{margin:0;background:#000}
#stage{position:relative;width:641px;height:361px;background:rgb(220,30,30);overflow:hidden}
#bar{position:absolute;left:0;top:0;width:10px;height:361px;background:#fff}
</style></head><body>
<div id="stage" data-composition-id="main" data-width="641" data-height="361" data-fps="30" data-start="0" data-duration="2">
  <div id="bar"></div>
</div>
<script>
  const stage = document.getElementById('stage');
  const bar = document.getElementById('bar');
  window.__timelines = window.__timelines || {};
  window.__timelines.main = {
    t: 0,
    pause() {},
    duration() { return 2; },
    seek(t) {
      this.t = t;
      stage.style.background = t < 1 ? 'rgb(220,30,30)' : 'rgb(30,30,220)';
      bar.style.left = Math.round(t * 300) + 'px';
      return this;
    },
  };
</script></body></html>"#;

fn pixel(png: &[u8], x: usize, y: usize) -> [u8; 3] {
    let rgb = decode_rgb(png).unwrap();
    let i = (y * rgb.width as usize + x) * 3;
    [rgb.pixels[i], rgb.pixels[i + 1], rgb.pixels[i + 2]]
}

fn ffprobe(ffmpeg: &std::path::Path, mp4: &std::path::Path) -> Value {
    let probe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    assert!(
        probe.is_file(),
        "ffprobe not found next to {}",
        ffmpeg.display()
    );
    let out = std::process::Command::new(probe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,pix_fmt,width,height,nb_read_frames",
            "-of",
            "json",
        ])
        .arg(mp4)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    v["streams"][0].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composition_becomes_a_60_frame_h264_mp4() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let ffmpeg = find_ffmpeg().expect("ffmpeg is required");
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("index.html");
    std::fs::write(&html, COMPOSITION).unwrap();

    let meta = composition(COMPOSITION).unwrap();
    let mut target = RenderTarget::new(html);
    target.width = meta.width;
    target.height = meta.height;
    let frames_dir = dir.path().join("frames");
    let frames = renderer
        .render_frames(
            &target,
            FrameOptions {
                fps: meta.fps,
                duration_ms: meta.duration_ms.unwrap(),
                require_timeline: false,
            },
            &frames_dir,
        )
        .await
        .unwrap();
    assert_eq!(frames.count, 60);
    assert_eq!((frames.width, frames.height), (641, 361));

    // The timeline really was seeked: red, then blue; the bar moves.
    let first = std::fs::read(frames_dir.join(frame_file_name(0))).unwrap();
    let last = std::fs::read(frames_dir.join(frame_file_name(59))).unwrap();
    assert_eq!(png_size(&first), Some((641, 361)));
    let red = pixel(&first, 600, 300);
    let blue = pixel(&last, 100, 300);
    assert!(red[0] > 180 && red[2] < 80, "first frame {red:?}");
    assert!(blue[2] > 180 && blue[0] < 80, "last frame {blue:?}");
    assert!(pixel(&first, 5, 5)[1] > 200, "bar at x=0 on frame 1");
    assert!(pixel(&last, 5, 5)[1] < 80, "bar has moved by frame 60");

    let mp4 = dir.path().join("out.mp4");
    encode_mp4(&ffmpeg, &frames_dir, frames.fps, &mp4)
        .await
        .unwrap();
    let stream = ffprobe(&ffmpeg, &mp4);
    assert_eq!(stream["codec_name"], "h264", "{stream}");
    assert_eq!(stream["pix_fmt"], "yuv420p", "{stream}");
    assert_eq!(stream["nb_read_frames"], "60", "{stream}");
    assert_eq!(
        (stream["width"].as_u64(), stream["height"].as_u64()),
        (Some(642), Some(362))
    );
    // +faststart: the index comes before the media data.
    let bytes = std::fs::read(&mp4).unwrap();
    let find = |tag: &[u8]| bytes.windows(4).position(|w| w == tag).unwrap();
    assert!(find(b"moov") < find(b"mdat"), "moov must precede mdat");
}

/// CSS-only: a declared duration and no timeline to wait for.
const CSS_ONLY: &str = r#"<!doctype html><html><head><style>
html,body{margin:0;background:#000}
#stage{width:320px;height:180px;background:rgb(30,200,30)}
</style></head><body>
<div id="stage" data-composition-id="main" data-width="320" data-height="180" data-fps="10" data-duration="0.5"></div>
<script>document.title = 'no timeline here';</script>
</body></html>"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_composition_without_timelines_does_not_wait_15_s() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("index.html");
    std::fs::write(&html, CSS_ONLY).unwrap();
    let mut target = RenderTarget::new(html);
    target.width = 320;
    target.height = 180;
    let started = std::time::Instant::now();
    let frames = renderer
        .render_frames(
            &target,
            FrameOptions {
                fps: 10,
                duration_ms: 500,
                require_timeline: false,
            },
            &dir.path().join("frames"),
        )
        .await
        .unwrap();
    assert_eq!(frames.count, 5);
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(8), "took {took:?}");
}

/// Declares its duration but registers the timeline 2 s late, as a slow
/// script would. Green after the timeline seeks to 0.
const LATE_TIMELINE: &str = r#"<!doctype html><html><head><style>
html,body{margin:0;background:#000}
#stage{width:320px;height:180px;background:rgb(220,30,30)}
</style></head><body>
<div id="stage" data-composition-id="main" data-width="320" data-height="180" data-fps="10" data-duration="0.5"></div>
<script>
  setTimeout(() => {
    const stage = document.getElementById('stage');
    window.__timelines = window.__timelines || {};
    window.__timelines.main = {
      pause() {},
      duration() { return 0.5; },
      seek(t) { stage.style.background = 'rgb(30,200,30)'; return this; },
    };
  }, 2000);
</script></body></html>"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_required_timeline_registered_late_still_renders() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("index.html");
    std::fs::write(&html, LATE_TIMELINE).unwrap();
    let mut target = RenderTarget::new(html);
    target.width = 320;
    target.height = 180;
    let frames_dir = dir.path().join("frames");
    let frames = renderer
        .render_frames(
            &target,
            FrameOptions {
                fps: 10,
                duration_ms: 500,
                require_timeline: true,
            },
            &frames_dir,
        )
        .await
        .unwrap();
    assert_eq!(frames.count, 5);
    let first = std::fs::read(frames_dir.join(frame_file_name(0))).unwrap();
    let green = pixel(&first, 100, 100);
    assert!(green[1] > 180 && green[0] < 80, "first frame {green:?}");
}
