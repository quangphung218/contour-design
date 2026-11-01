//! `hyperframes-html` through the real media task queue (fake frame
//! renderer, real ffmpeg): `media/scaffold`, a queued render, the MP4 in
//! the asking run's turn (Write, then Edit on a re-render) and never in a
//! run of another project.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_contracts::CreateProjectRequest;
use od_export::hyperframes::frame_count;
use od_export::image::solid_png;
use od_export::renderer::frame_file_name;
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
use od_runs::{Run, RunMeta};
use od_server::routes::w3::{Deps, routes_with};
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Writes solid frames.
struct Frames;

impl Renderer for Frames {
    fn render_slides<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(async { Err(RenderError::Unsupported("slides")) })
    }

    fn render_frames<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: FrameOptions,
        out_dir: &'a Path,
    ) -> BoxFuture<'a, RenderResult<FrameSet>> {
        Box::pin(async move {
            let count = u32::try_from(frame_count(options.duration_ms, options.fps)).unwrap();
            std::fs::create_dir_all(out_dir).unwrap();
            for i in 0..count {
                let png = solid_png(target.width, target.height, [40, 40, 40]).unwrap();
                std::fs::write(out_dir.join(frame_file_name(i)), png).unwrap();
            }
            Ok(FrameSet {
                count,
                fps: options.fps,
                width: target.width,
                height: target.height,
            })
        })
    }

    fn print_pdf<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(RenderError::Unsupported("pdf")) })
    }

    fn capture_page<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(RenderError::Unsupported("capture")) })
    }
}

struct Harness {
    _tmp: tempfile::TempDir,
    state: AppState,
    app: Router,
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let state = AppState {
        store: Arc::new(Store::open(DataDir::new(&data)).unwrap()),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(&content))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: content,
            daemon_url: None,
            mcp_daemon_url: true,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    };
    let deps = Deps {
        renderer: Arc::new(Frames),
        env: Arc::new(|_: &str| None),
        limits: od_media::Limits::default(),
        // Two at once, as the daemon default: renders of one project overlap.
        media_concurrency: 2,
    };
    let app = routes_with(&state, deps).with_state(state.clone());
    Harness {
        _tmp: tmp,
        state,
        app,
    }
}

impl Harness {
    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn task(&self, id: &str) -> Value {
        let req = Request::builder()
            .uri(format!("/api/media/tasks/{id}?waitMs=20000"))
            .body(Body::empty())
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<Value>(&bytes).unwrap()["task"].clone()
    }

    fn project(&self, name: &str) -> String {
        self.state
            .store
            .create_project(CreateProjectRequest {
                name: name.to_owned(),
                ..CreateProjectRequest::default()
            })
            .unwrap()
            .0
            .id
    }

    fn run(&self, id: &str, project: &str) -> Arc<Run> {
        self.state
            .runs
            .registry
            .create_or_reuse(RunMeta {
                run_id: id.to_owned(),
                agent_id: "claude".to_owned(),
                project_id: Some(project.to_owned()),
                ..RunMeta::default()
            })
            .unwrap()
            .0
    }

    /// Renders `dir` of `pid` for `run`; the finished task.
    async fn render(&self, pid: &str, run: &str) -> Value {
        let (status, v) = self
            .post(
                &format!("/api/projects/{pid}/media/tasks"),
                json!({"surface": "video", "model": "hyperframes-html",
                    "compositionDir": "motion-source/film", "output": "film.mp4", "runId": run}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{v}");
        self.task(v["task"]["id"].as_str().unwrap()).await
    }
}

/// The `agent` events of `run` so far.
async fn agent_events(run: &Arc<Run>) -> Vec<Value> {
    let mut sub = run.subscribe(0);
    let mut out = Vec::new();
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(200), sub.next()).await {
        if ev.event == "agent" {
            out.push(ev.data.clone());
        }
    }
    out
}

#[tokio::test]
async fn a_render_joins_the_asking_run_only() {
    let h = harness();
    let pid = h.project("Film");
    let other = h.project("Other");
    let (status, v) = h
        .post(
            &format!("/api/projects/{pid}/media/scaffold"),
            json!({"compositionDir": "motion-source/film", "width": 64, "height": 36, "duration": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["created"], true);

    // A run id of another project: rendered, but not added to that run.
    let foreign = h.run("run-other", &other);
    let task = h.render(&pid, "run-other").await;
    assert_eq!(task["status"], "done", "{task}");
    assert!(
        agent_events(&foreign).await.is_empty(),
        "nothing joins another project's run"
    );

    // The asking run: the MP4 as an Edit (an earlier render exists).
    let run = h.run("run-1", &pid);
    let task = h.render(&pid, "run-1").await;
    assert_eq!(task["status"], "done", "{task}");
    assert_eq!(task["result"]["edited"], true);
    let events = agent_events(&run).await;
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["type"], "tool_use");
    assert_eq!(events[0]["name"], "Edit");
    let path = events[0]["input"]["file_path"].as_str().unwrap();
    assert!(
        path.ends_with("film.mp4") && Path::new(path).is_absolute(),
        "{path}"
    );
    assert!(Path::new(path).is_file());
    assert_eq!(events[1]["type"], "tool_result");
    assert!(
        events[1]["content"].as_str().unwrap().contains("30 fps"),
        "{events:?}"
    );

    // A first render of a new output is a Write.
    let (status, _) = h
        .post(
            &format!("/api/projects/{pid}/media/tasks"),
            json!({"surface": "video", "model": "hyperframes-html",
                "compositionDir": "motion-source/film", "output": "cut/b.mp4", "runId": "run-1"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut tries = 0;
    loop {
        let events = agent_events(&run).await;
        if events.len() == 4 {
            assert_eq!(events[2]["name"], "Write", "{events:?}");
            break;
        }
        tries += 1;
        assert!(tries < 100, "{events:?}");
    }
}

#[tokio::test]
async fn overlapping_renders_keep_their_own_review_frames() {
    let h = harness();
    let pid = h.project("Film");
    let (status, _) = h
        .post(
            &format!("/api/projects/{pid}/media/scaffold"),
            json!({"compositionDir": "motion-source/film", "width": 64, "height": 36, "duration": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // Queued together; the second spells the model with spaces, which
    // the queue stores trimmed so the worker renders it too.
    let mut ids = Vec::new();
    for (model, output) in [
        ("hyperframes-html", "a.mp4"),
        (" hyperframes-html ", "b.mp4"),
    ] {
        let (status, v) = h
            .post(
                &format!("/api/projects/{pid}/media/tasks"),
                json!({"surface": "video", "model": model,
                    "compositionDir": "motion-source/film", "output": output}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{v}");
        assert_eq!(v["task"]["request"]["model"], "hyperframes-html", "{v}");
        ids.push(v["task"]["id"].as_str().unwrap().to_owned());
    }
    let mut dirs = Vec::new();
    for id in &ids {
        let task = h.task(id).await;
        assert_eq!(task["status"], "done", "{task}");
        let frames: Vec<&str> = task["result"]["reviewFrames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert_eq!(frames.len(), 3, "{task}");
        let dir = Path::new(frames[0]).parent().unwrap().to_path_buf();
        assert!(dir.ends_with(id.as_str()), "{}", dir.display());
        dirs.push((
            dir,
            frames.iter().map(|f| (*f).to_owned()).collect::<Vec<_>>(),
        ));
    }
    assert_ne!(dirs[0].0, dirs[1].0);
    // Both sets survive the other render.
    for (_, frames) in &dirs {
        for f in frames {
            assert!(std::fs::read(f).unwrap().starts_with(b"\x89PNG"), "{f}");
        }
    }
}

#[tokio::test]
async fn a_bad_request_is_refused_before_queueing() {
    let h = harness();
    let pid = h.project("Film");
    for (body, want) in [
        (
            json!({"surface": "video", "model": "hyperframes-html"}),
            "--composition-dir",
        ),
        (
            json!({"surface": "video", "model": "hyperframes-html", "compositionDir": "../up"}),
            "inside the project",
        ),
    ] {
        let (status, v) = h
            .post(&format!("/api/projects/{pid}/media/tasks"), body)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains(want),
            "{v}"
        );
    }
}

// ---------------------------------------------------------------------------
// Audio: `<audio>` tracks mixed into the MP4 (real ffmpeg).

/// A mono 16-bit WAV of `seconds` of a sine at `freq` Hz, peak `amp` (0–1).
fn wav(seconds: f64, freq: f64, amp: f64) -> Vec<u8> {
    const RATE: u32 = 8_000;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (seconds * f64::from(RATE)) as u32;
    let mut out = Vec::with_capacity(44 + 2 * n as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + 2 * n).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(2 * n).to_le_bytes());
    for i in 0..n {
        let t = f64::from(i) / f64::from(RATE);
        #[allow(clippy::cast_possible_truncation)]
        let s = (amp * 32_000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16;
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// What ffmpeg reads from `mp4`: its stream lines (`Video:` / `Audio:`)
/// and the first audio stream decoded to mono 8 kHz samples.
fn probe(mp4: &Path) -> (Vec<String>, Vec<i16>) {
    let ffmpeg = od_render_cdp::ffmpeg::find_ffmpeg().unwrap();
    let info = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(mp4)
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&info.stderr)
        .lines()
        .filter(|l| l.trim_start().starts_with("Stream #"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let pcm = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(mp4)
        .args([
            "-map", "0:a:0?", "-ac", "1", "-ar", "8000", "-f", "s16le", "-",
        ])
        .output()
        .unwrap();
    let samples = pcm
        .stdout
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    (streams, samples)
}

/// The container duration of the first `kind` (`v` or `a`) stream of
/// `mp4`, read with the ffprobe next to ffmpeg.
fn duration(mp4: &Path, kind: &str) -> f64 {
    let ffmpeg = od_render_cdp::ffmpeg::find_ffmpeg().unwrap();
    let out = std::process::Command::new(ffmpeg.with_file_name("ffprobe"))
        .args(["-v", "error", "-select_streams", &format!("{kind}:0")])
        .args(["-show_entries", "stream=duration", "-of", "csv=p=0"])
        .arg(mp4)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .parse()
        .unwrap_or_else(|_| panic!("ffprobe {kind}: {text:?}"))
}

/// Root mean square of `samples` between `from` and `to` seconds (8 kHz).
fn rms(samples: &[i16], from: f64, to: f64) -> f64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let at = |t: f64| ((t * 8_000.0) as usize).min(samples.len());
    let part = &samples[at(from)..at(to)];
    #[allow(clippy::cast_precision_loss)]
    let mean = part.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / part.len().max(1) as f64;
    mean.sqrt()
}

impl Harness {
    /// Scaffolds `motion-source/film` (64×36, 10 fps, `seconds` long) and
    /// adds `head` to its `<head>` and `body` before `</body>`.
    async fn film(&self, pid: &str, seconds: f64, head: &str, body: &str) {
        let (status, v) = self
            .post(
                &format!("/api/projects/{pid}/media/scaffold"),
                json!({"compositionDir": "motion-source/film", "width": 64, "height": 36,
                    "fps": 10, "duration": seconds}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let (_, html) = self
            .state
            .store
            .read_file(pid, "motion-source/film/index.html")
            .unwrap();
        let html = String::from_utf8(html)
            .unwrap()
            .replace("</head>", &format!("{head}\n</head>"))
            .replace("</body>", &format!("{body}\n</body>"));
        self.write(pid, "motion-source/film/index.html", html.as_bytes());
    }

    fn write(&self, pid: &str, name: &str, bytes: &[u8]) {
        self.state
            .store
            .write_file(pid, name, bytes, true, None)
            .unwrap();
    }

    fn path(&self, pid: &str, name: &str) -> std::path::PathBuf {
        let project = self.state.store.get_project(pid).unwrap();
        self.state
            .store
            .project_root(&project)
            .unwrap()
            .dir
            .join(name)
    }
}

const REQUIRED: &str = r#"<meta name="od-motion-audio" content="required">"#;

#[tokio::test]
async fn music_and_voice_are_mixed_at_their_offsets() {
    let h = harness();
    let pid = h.project("Film");
    h.write(
        &pid,
        "motion-source/film/assets/music.wav",
        &wav(12.0, 220.0, 0.2),
    );
    h.write(
        &pid,
        "motion-source/film/assets/voice.wav",
        &wav(3.0, 440.0, 0.9),
    );
    h.film(
        &pid,
        10.0,
        REQUIRED,
        r#"<audio src="assets/music.wav" data-start="0" data-volume="0.3"></audio>
<audio src="assets/voice.wav" data-start="2" data-duration="3"></audio>"#,
    )
    .await;
    let task = h.render(&pid, "").await;
    assert_eq!(task["status"], "done", "{task}");
    assert_eq!(task["result"]["audioTracks"], 2, "{task}");
    assert_eq!(task["result"]["audioPolicy"], "required", "{task}");
    assert!(
        task["result"]["providerNote"]
            .as_str()
            .unwrap()
            .contains("AAC, 2 audio tracks mixed"),
        "{task}"
    );
    let (streams, samples) = probe(&h.path(&pid, "film.mp4"));
    let audio: Vec<_> = streams.iter().filter(|s| s.contains("Audio:")).collect();
    assert_eq!(audio.len(), 1, "{streams:?}");
    assert!(audio[0].contains("aac"), "{streams:?}");
    // As long as the picture (10 s): music is 12 s and gets cut. Measured
    // on the container, not by decoding: ffmpeg 6 decodes the AAC end
    // padding as extra samples (10.112 s from the same file), ffmpeg 7+
    // trims it.
    let mp4 = h.path(&pid, "film.mp4");
    let (video_s, audio_s) = (duration(&mp4, "v"), duration(&mp4, "a"));
    assert!((video_s - 10.0).abs() <= 0.01, "video lasts {video_s} s");
    assert!(
        (audio_s - video_s).abs() <= 0.1,
        "audio {audio_s} s, video {video_s} s"
    );
    // The voice sounds from 2 s to 5 s, over quieter music.
    let (before, voice, after) = (
        rms(&samples, 0.2, 1.8),
        rms(&samples, 2.2, 4.8),
        rms(&samples, 5.2, 9.8),
    );
    assert!(before > 100.0, "music plays from 0 s: {before}");
    assert!(voice > 3.0 * before, "voice at 2–5 s: {voice} vs {before}");
    assert!(
        after < 2.0 * before,
        "voice stops at 5 s: {after} vs {before}"
    );
}

#[tokio::test]
async fn a_track_in_a_sub_composition_is_mixed_at_its_start() {
    let h = harness();
    let pid = h.project("Film");
    h.write(
        &pid,
        "motion-source/film/compositions/voice.wav",
        &wav(5.0, 440.0, 0.9),
    );
    h.write(
        &pid,
        "motion-source/film/compositions/vo.html",
        br#"<template id="vo-template"><div data-composition-id="vo" data-width="64" data-height="36">
<audio src="voice.wav" data-start="0"></audio></div></template>"#,
    );
    h.film(
        &pid,
        6.0,
        "",
        r#"<div data-composition-id="vo" data-composition-src="compositions/vo.html" data-start="2" data-duration="2"></div>"#,
    )
    .await;
    let task = h.render(&pid, "").await;
    assert_eq!(task["status"], "done", "{task}");
    assert_eq!(task["result"]["audioTracks"], 1, "{task}");
    assert_eq!(task["result"]["audioPolicy"], "none", "{task}");
    let (_, samples) = probe(&h.path(&pid, "film.mp4"));
    // Silent before 2 s, the voice for the 2 s the sub-composition lasts.
    let (before, voice, after) = (
        rms(&samples, 0.2, 1.8),
        rms(&samples, 2.2, 3.8),
        rms(&samples, 4.2, 5.8),
    );
    assert!(voice > 1_000.0, "{voice}");
    assert!(before < 50.0 && after < 50.0, "{before} {after}");
}

#[tokio::test]
async fn a_film_without_audio_stays_picture_only() {
    let h = harness();
    let pid = h.project("Film");
    // A silent film keeps its tracks out of the MP4.
    h.write(
        &pid,
        "motion-source/film/assets/music.wav",
        &wav(1.0, 220.0, 0.5),
    );
    for (head, body, policy) in [
        ("", "", "none"),
        (
            r#"<meta name="od-motion-audio" content="silent">"#,
            r#"<audio src="assets/music.wav"></audio>"#,
            "silent",
        ),
    ] {
        h.film(&pid, 1.0, head, body).await;
        let task = h.render(&pid, "").await;
        assert_eq!(task["status"], "done", "{task}");
        assert_eq!(task["result"]["audioTracks"], 0, "{task}");
        assert_eq!(task["result"]["audioPolicy"], policy, "{task}");
        let (streams, samples) = probe(&h.path(&pid, "film.mp4"));
        assert!(streams.iter().all(|s| !s.contains("Audio:")), "{streams:?}");
        assert_eq!(
            streams.iter().filter(|s| s.contains("Video: h264")).count(),
            1
        );
        assert!(samples.is_empty());
        std::fs::remove_dir_all(h.path(&pid, "motion-source/film")).unwrap();
        std::fs::remove_file(h.path(&pid, "film.mp4")).unwrap();
        h.write(
            &pid,
            "motion-source/film/assets/music.wav",
            &wav(1.0, 220.0, 0.5),
        );
    }
}

#[tokio::test]
async fn audio_errors_fail_the_task_without_an_mp4() {
    let h = harness();
    let pid = h.project("Film");
    h.write(
        &pid,
        "motion-source/film/assets/a.wav",
        &wav(1.0, 220.0, 0.5),
    );
    h.write(&pid, "outside.wav", &wav(1.0, 220.0, 0.5));
    for (head, body, want) in [
        (
            REQUIRED,
            r#"<audio src="assets/music.wav" data-start="0"></audio>"#,
            "assets/music.wav is not in the composition folder",
        ),
        (REQUIRED, "", "no <audio> track"),
        (
            "",
            r#"<audio src="https://cdn.example/a.mp3"></audio>"#,
            "local files only",
        ),
        ("", r#"<audio src="assets/a.wav" loop></audio>"#, "loop"),
        (
            "",
            r#"<audio src="../../outside.wav"></audio>"#,
            "outside the composition folder",
        ),
        (
            "",
            r#"<audio src="assets/a.wav" data-volume="2"></audio>"#,
            "data-volume",
        ),
    ] {
        h.film(&pid, 1.0, head, body).await;
        let task = h.render(&pid, "").await;
        assert_eq!(task["status"], "failed", "{body}: {task}");
        let error = task["error"].to_string();
        assert!(error.contains(want), "{body}: {error}");
        assert!(
            !h.path(&pid, "film.mp4").exists(),
            "{body}: no MP4 is written"
        );
        std::fs::remove_file(h.path(&pid, "motion-source/film/index.html")).unwrap();
    }
}

#[tokio::test]
async fn a_silent_film_with_a_clip_reference_still_renders() {
    let h = harness();
    let pid = h.project("Film");
    h.write(
        &pid,
        "motion-source/film/compositions/outro.html",
        b"<template><div data-composition-id=\"outro\"><p>end</p></div></template>",
    );
    h.film(
        &pid,
        1.0,
        "",
        r#"<div data-composition-id="outro" data-composition-src="compositions/outro.html" data-start="intro + 2"></div>"#,
    )
    .await;
    let task = h.render(&pid, "").await;
    assert_eq!(task["status"], "done", "{task}");
    assert_eq!(task["result"]["audioTracks"], 0, "{task}");
    let (streams, _) = probe(&h.path(&pid, "film.mp4"));
    assert!(streams.iter().all(|s| !s.contains("Audio:")), "{streams:?}");
}
