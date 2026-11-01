//! `hyperframes-html`: the local MP4 render of an editable HyperFrames
//! composition, for the `motion-design` skill (spec 2.6 `media scaffold`,
//! 4.2 MP4 pipeline).
//!
//! - `POST /api/projects/:id/media/scaffold` (`od media scaffold`): writes
//!   `index.html`, `hyperframes.json` and `meta.json` into a new
//!   composition folder; a folder that already has an `index.html` is left
//!   as it is.
//! - `od media generate --model hyperframes-html --composition-dir <dir>`
//!   queues a task like any provider model; [`render`] runs it with the
//!   MP4 exporter of [`crate::routes::media::mp4`] and the Chrome and
//!   ffmpeg already installed (decisions #2: nothing is downloaded).
//!
//! The finished MP4 joins the agent's turn (`OD_RUN_ID`) as a Write, or an
//! Edit when it replaced an earlier render, so "Files from this turn" lists
//! it. A composition that declares `<meta name="od-motion-output"
//! content="video">` makes the MP4 the project's entry file.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::export::ExportRequest;
use crate::routes::media::mp4::render_mp4;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use od_contracts::{AgentEvent, UpdateProjectRequest, sse};
use od_export::Renderer;
use od_export::hyperframes::{self, AudioPolicy, Composition, MAX_AUDIO_TRACKS, MAX_FPS};
use od_media::EnvLookup;
use od_media::tasks::TaskRecord;
use od_render_cdp::ffmpeg::{AudioTrack, FFMPEG_PATH_ENV, audio_format, find_ffmpeg_with};
use od_store::paths::validate_project_path;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path as FsPath, PathBuf};
use std::sync::LazyLock;
use std::time::Instant;

/// The model id the skill names.
pub const MODEL: &str = "hyperframes-html";
/// Provider id in the catalogue: local, no key.
pub const PROVIDER: &str = "hyperframes";

/// The `media/tasks` body fields this model reads besides `model`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionRequest {
    #[serde(default)]
    composition_dir: String,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    fps: Option<u32>,
    /// The agent run that asked (`OD_RUN_ID`); the MP4 joins its turn.
    #[serde(default)]
    run_id: Option<String>,
}

/// A checked request: project paths, normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub dir: String,
    pub output: String,
}

/// The catalogue row of `GET /api/media/models`.
#[must_use]
pub fn catalogue_entry() -> Value {
    json!({
        "id": MODEL,
        "label": "HyperFrames HTML → MP4 (local)",
        "provider": PROVIDER,
        "surface": "video",
    })
}

fn dir_of(raw: &str) -> Result<String, String> {
    validate_project_path(raw.trim())
        .map_err(|_| format!("--composition-dir must be a folder inside the project, got {raw:?}"))
}

/// Checks a `media/tasks` body for this model.
///
/// # Errors
/// A missing or unsafe composition folder, or an output that is not a
/// project path ending in `.mp4`.
pub fn check(body: &Value) -> Result<Checked, String> {
    let req: MotionRequest = serde_json::from_value(body.clone()).map_err(|e| e.to_string())?;
    if body.get("surface").and_then(Value::as_str) != Some("video") {
        return Err(format!("{MODEL} renders video: use --surface video"));
    }
    if req.composition_dir.trim().is_empty() {
        return Err(format!(
            "{MODEL} needs --composition-dir (the folder `od media scaffold` made)"
        ));
    }
    let dir = dir_of(&req.composition_dir)?;
    let output = match req
        .output
        .as_deref()
        .map(str::trim)
        .filter(|o| !o.is_empty())
    {
        Some(o) => o.to_owned(),
        None => format!("{}.mp4", dir.rsplit('/').next().unwrap_or("film")),
    };
    if !output.to_ascii_lowercase().ends_with(".mp4") {
        return Err(format!("--output must end in .mp4, got {output:?}"));
    }
    let output = validate_project_path(&output)
        .map_err(|_| format!("--output must be a path inside the project, got {output:?}"))?;
    if req.fps.is_some_and(|f| !(1..=MAX_FPS).contains(&f)) {
        return Err(format!("--fps must be 1–{MAX_FPS}"));
    }
    Ok(Checked { dir, output })
}

// ---------------------------------------------------------------------------
// Scaffold

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaffoldRequest {
    composition_dir: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    fps: Option<u32>,
    /// Seconds.
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    run_id: Option<String>,
}

const SCAFFOLD_FILES: [&str; 3] = ["index.html", "hyperframes.json", "meta.json"];

/// The starting `index.html`: a valid composition root, a seekable
/// timeline at `window.__timelines.main` with no library, and the source
/// player loop for the preview.
fn scaffold_html(width: u32, height: u32, fps: u32, duration: f64) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Motion</title>
<meta name="od-motion-output" content="video">
<style>
  html, body {{ margin: 0; background: #0b0b0c; }}
  #stage {{ position: relative; overflow: hidden; width: {width}px; height: {height}px;
    color: #f4f1ea; font: 700 120px/1 system-ui, sans-serif; }}
  #title {{ position: absolute; inset: 0; display: grid; place-items: center; }}
</style>
</head>
<body>
<div id="stage" data-composition-id="main" data-start="0" data-duration="{duration}"
  data-width="{width}" data-height="{height}" data-fps="{fps}" data-poster-time="{poster}">
  <div id="title">Title</div>
</div>
<script>
// The master timeline. The renderer pauses it and calls seek(t) for every
// frame, so draw everything from `t` alone (no timers, no randomness that
// is not seeded). A GSAP timeline registered here works the same way.
(() => {{
  const DURATION = {duration};
  const title = document.getElementById('title');
  const ease = (x) => 1 - Math.pow(1 - Math.min(Math.max(x, 0), 1), 3);
  function draw(t) {{
    const k = ease(t / 0.8);
    title.style.opacity = String(k);
    title.style.transform = `translateY(${{(1 - k) * 40}}px)`;
  }}
  let time = 0, playing = true, last = null;
  const timeline = {{
    duration: () => DURATION,
    time: () => time,
    pause() {{ playing = false; return timeline; }},
    play() {{ playing = true; last = null; requestAnimationFrame(tick); return timeline; }},
    seek(t) {{ time = Math.min(Math.max(Number(t) || 0, 0), DURATION); draw(time); return timeline; }},
  }};
  // Preview player: loops until the renderer pauses it.
  function tick(now) {{
    if (!playing) return;
    if (last !== null) timeline.seek((time + (now - last) / 1000) % DURATION);
    last = now;
    requestAnimationFrame(tick);
  }}
  window.__timelines = window.__timelines || {{}};
  window.__timelines.main = timeline;
  timeline.seek(0);
  requestAnimationFrame(tick);
}})();
</script>
</body>
</html>
"#,
        poster = duration / 2.0,
    )
}

pub async fn scaffold(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<ScaffoldRequest>,
) -> ApiResult<Json<Value>> {
    let dir = dir_of(&req.composition_dir).map_err(ApiErr::bad_request)?;
    let (width, height) = (req.width.unwrap_or(1920), req.height.unwrap_or(1080));
    let fps = req.fps.unwrap_or(hyperframes::DEFAULT_FPS);
    let duration = req
        .duration
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(4.0);
    let limit = hyperframes::MAX_EDGE;
    if !(1..=limit).contains(&width) || !(1..=limit).contains(&height) {
        return Err(ApiErr::bad_request(format!(
            "width and height must be 1–{limit}"
        )));
    }
    if !(1..=MAX_FPS).contains(&fps) {
        return Err(ApiErr::bad_request(format!("fps must be 1–{MAX_FPS}")));
    }
    let entry = format!("{dir}/index.html");
    let files = [
        (entry.clone(), scaffold_html(width, height, fps, duration)),
        (
            format!("{dir}/hyperframes.json"),
            serde_json::to_string_pretty(&json!({
                "composition": "main",
                "entry": "index.html",
                "width": width,
                "height": height,
                "fps": fps,
                "duration": duration,
            }))
            .unwrap_or_default(),
        ),
        (
            format!("{dir}/meta.json"),
            serde_json::to_string_pretty(&json!({
                "output": "video",
                "renderer": MODEL,
                "createdBy": "od media scaffold",
            }))
            .unwrap_or_default(),
        ),
    ];
    let pid = id.clone();
    let created = state
        .blocking(move |s| {
            // A folder with a source is the user's film: never overwritten.
            if s.read_file(&pid, &entry).is_ok() {
                return Ok(false);
            }
            for (name, body) in &files {
                if s.read_file(&pid, name).is_err() {
                    s.write_file(&pid, name, body.as_bytes(), false, None)?;
                }
            }
            Ok(true)
        })
        .await?;
    tracing::info!(
        project = %id,
        run_id = req.run_id.as_deref().unwrap_or(""),
        dir = %dir,
        created,
        "od media scaffold"
    );
    let files: Vec<String> = SCAFFOLD_FILES
        .iter()
        .map(|f| format!("{dir}/{f}"))
        .collect();
    Ok(Json(json!({
        "compositionDir": dir,
        "created": created,
        "files": files,
        "message": if created {
            format!("scaffolded {dir}: edit {dir}/index.html, then render with `media generate --model {MODEL}`")
        } else {
            format!("{dir} already has index.html; nothing was written. Edit it in place.")
        },
    })))
}

// ---------------------------------------------------------------------------
// Composition checks

static SCRIPT_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']"#).expect("static regex")
});
static META_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<meta\b[^>]*>").expect("static regex"));
static POSTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<[a-z][a-z0-9-]*\s[^>]*?\bdata-composition-id\s*=[^>]*?\bdata-poster-time\s*=\s*["']?([^"'\s>]+)"#)
        .expect("static regex")
});

/// Local script paths of `html`, relative to its folder.
fn local_scripts(html: &str) -> Vec<String> {
    SCRIPT_SRC
        .captures_iter(html)
        .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_owned()))
        .filter(|src| {
            let lower = src.to_ascii_lowercase();
            !(lower.contains("://") || lower.starts_with("//") || lower.starts_with("data:"))
        })
        .collect()
}

/// The project path of script `src` loaded by a page in `dir`: `./` and
/// `..` resolved, query and fragment dropped. `None` when it leaves the
/// project or is absolute.
fn script_path(dir: &str, src: &str) -> Option<String> {
    let src = src.split(['?', '#']).next().unwrap_or_default();
    if src.starts_with('/') || src.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in src.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Whether `html` declares `<meta name="od-motion-output" content="video">`.
#[must_use]
pub fn declares_video(html: &str) -> bool {
    META_TAG.find_iter(html).any(|m| {
        let tag = m.as_str().to_ascii_lowercase().replace('\'', "\"");
        tag.contains("name=\"od-motion-output\"") && tag.contains("content=\"video\"")
    })
}

/// The composition `html` declares, checked for what the render needs:
/// a root, `data-duration`, a timeline at `window.__timelines` (in the page
/// or in one of `scripts`, the local scripts' text) and a poster time
/// inside the film. Errors name the missing field.
///
/// # Errors
/// What is missing or out of range, as a message for the agent.
pub fn check_composition(html: &str, scripts: &[String]) -> Result<(Composition, f64), String> {
    let comp = hyperframes::composition(html).map_err(|e| format!("index.html: {e}"))?;
    let Some(duration_ms) = comp.duration_ms else {
        return Err("index.html: the composition root ([data-composition-id]) has no data-duration; set it to the film length in seconds".to_owned());
    };
    let registers = |text: &str| text.contains("__timelines");
    if !registers(html) && !scripts.iter().any(|s| registers(s)) {
        return Err(format!(
            "index.html: no timeline at window.__timelines.{id}; register the seekable master timeline there (window.__timelines = {{ {id}: timeline }})",
            id = comp.id
        ));
    }
    #[allow(clippy::cast_precision_loss)] // film lengths are far below 2^52 ms
    let seconds = duration_ms as f64 / 1000.0;
    let poster = match POSTER.captures(html).and_then(|c| c.get(1)) {
        Some(v) => {
            let t: f64 = v.as_str().parse().map_err(|_| {
                format!(
                    "index.html: data-poster-time {:?} is not a number of seconds",
                    v.as_str()
                )
            })?;
            if !(t.is_finite() && t >= 0.0 && t < seconds) {
                return Err(format!(
                    "index.html: data-poster-time {t} must be at least 0 and below data-duration {seconds}"
                ));
            }
            t
        }
        None => seconds / 2.0,
    };
    Ok((comp, poster))
}

// ---------------------------------------------------------------------------
// Audio

/// The sound a render mixes: the film's policy and its tracks as files.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPlan {
    pub policy: AudioPolicy,
    /// The tracks that sound inside the film, in film time.
    pub tracks: Vec<AudioTrack>,
}

/// The file `src` names from folder `from`, which must exist inside the
/// composition folder `root` (both canonical). `what` names it in errors.
fn local_file(root: &FsPath, from: &FsPath, src: &str, what: &str) -> Result<PathBuf, String> {
    let raw = src.split(['?', '#']).next().unwrap_or_default();
    // A URL path: `my%20song.wav` is the file `my song.wav`, as a browser
    // loads it.
    let decoded = percent_decode(raw);
    let src = decoded.as_str();
    let missing = || {
        format!(
            "{what}: {src} is not in the composition folder. Add the file there (Contour mixes local files only) or remove the track, then render again"
        )
    };
    let path = from.join(src).canonicalize().map_err(|_| missing())?;
    if !path.starts_with(root) {
        return Err(format!(
            "{what}: {src} is outside the composition folder; keep audio and sub-compositions inside it"
        ));
    }
    if !path.is_file() {
        return Err(missing());
    }
    Ok(path)
}

/// `src` with each `%XX` turned back into its byte; a `%` not followed by
/// two hex digits, or bytes that are not UTF-8, leave `src` as written.
fn percent_decode(src: &str) -> String {
    let raw = src.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = |b: Option<&u8>| b.and_then(|b| char::from(*b).to_digit(16));
        match (raw[i], hex(raw.get(i + 1)), hex(raw.get(i + 2))) {
            (b'%', Some(h), Some(l)) => {
                out.push(u8::try_from(h * 16 + l).unwrap_or(b'%'));
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_owned())
}

/// Collects the `<audio>` tracks of a composition: those of `html` (its
/// `index.html`) and of each sub-composition it loads with
/// `data-composition-src`, shifted by that element's `data-start` and cut
/// at its `data-duration`. A sub-composition's `src` paths are relative to
/// its own file. `dir` is the composition folder on disk; tracks that start
/// after `film_seconds` are left out.
///
/// # Errors
/// A track or sub-composition that is not a local file in `dir`, an
/// unsupported attribute or format, more than [`MAX_AUDIO_TRACKS`], or a
/// `required` film without a track that sounds.
pub fn audio_plan(dir: &FsPath, html: &str, film_seconds: f64) -> Result<AudioPlan, String> {
    let policy = hyperframes::audio_policy(html);
    if policy == AudioPolicy::Silent {
        return Ok(AudioPlan {
            policy,
            tracks: Vec::new(),
        });
    }
    let root = dir
        .canonicalize()
        .map_err(|e| format!("composition folder {}: {e}", dir.display()))?;
    // (file the tags come from, its folder, start offset, cut, tags)
    let mut sources = vec![(
        "index.html".to_owned(),
        root.clone(),
        0.0,
        None,
        hyperframes::audio_tags(html).map_err(|e| format!("index.html: {e}"))?,
    )];
    let required = policy == AudioPolicy::Required;
    for sub in hyperframes::composition_refs(html) {
        let what = format!("index.html data-composition-src={:?}", sub.src);
        // A sub-composition that cannot be read only matters when the
        // film requires sound: the audio plan never blocks a film that
        // rendered without it.
        let file = if hyperframes::is_local_src(&sub.src) {
            local_file(&root, &root, &sub.src, &what)
        } else {
            Err(format!("{what}: Contour loads local sub-compositions only"))
        };
        let (file, text) = match file.and_then(|f| {
            std::fs::read(&f)
                .map(|t| (f, t))
                .map_err(|e| format!("{what}: {e}"))
        }) {
            Ok(read) => read,
            Err(e) if required => return Err(e),
            Err(e) => {
                tracing::debug!(error = %e, "audio plan: sub-composition skipped");
                continue;
            }
        };
        let tags = hyperframes::audio_tags(&String::from_utf8_lossy(&text))
            .map_err(|e| format!("{}: {e}", sub.src))?;
        if tags.is_empty() {
            continue;
        }
        // Its tracks need to know where it starts in the film.
        let (start, duration) = sub
            .timing
            .map_err(|e| format!("index.html {e}: it has <audio> tracks, so its data-start and data-duration must be seconds"))?;
        let folder = file
            .parent()
            .map_or_else(|| root.clone(), FsPath::to_path_buf);
        sources.push((sub.src, folder, start, duration, tags));
    }
    let mut tracks = Vec::new();
    for (label, folder, offset, cut, tags) in sources {
        for tag in tags {
            let what = format!("{label} <audio src={:?}>", tag.src);
            let path = local_file(&root, &folder, &tag.src, &what)?;
            let format = audio_format(&path).ok_or_else(|| {
                format!("{what}: not a supported audio file; use WAV, MP3, OGG, FLAC, AAC or M4A")
            })?;
            // Inside a sub-composition the track stops where it ends.
            let duration = match (tag.duration, cut.map(|c: f64| c - tag.start)) {
                (_, Some(left)) if left <= 0.0 => continue,
                (Some(d), Some(left)) => Some(d.min(left)),
                (d, left) => d.or(left),
            };
            let start = offset + tag.start;
            if start >= film_seconds {
                continue;
            }
            tracks.push(AudioTrack {
                path,
                format,
                start,
                duration,
                volume: tag.volume,
            });
        }
    }
    if tracks.len() > MAX_AUDIO_TRACKS {
        return Err(format!(
            "the film has {} audio tracks; Contour mixes at most {MAX_AUDIO_TRACKS}. Pre-mix some of them into one file",
            tracks.len()
        ));
    }
    if policy == AudioPolicy::Required && tracks.is_empty() {
        return Err("index.html declares od-motion-audio=required but has no <audio> track that plays inside the film. Add one with a local src (for example <audio src=\"assets/music.wav\" data-start=\"0\" data-volume=\"0.3\">), or declare content=\"silent\" and tell the user the film is silent".to_owned());
    }
    Ok(AudioPlan { policy, tracks })
}

/// What the render log says about sound: the policy, the track count and
/// what happened to the mux.
#[derive(Debug, Clone, Copy)]
struct AudioLog {
    policy: &'static str,
    tracks: usize,
    /// `mixed`, `none` (no track), `refused` (the plan failed), `failed`
    /// (the render or encode failed) or `not-reached`.
    mux: &'static str,
}

// ---------------------------------------------------------------------------
// Render

/// `ffmpeg required: …` with how to install it.
fn ffmpeg_missing(detail: &str) -> String {
    format!(
        "ffmpeg required: {detail}. Contour renders MP4 with the ffmpeg installed on this machine and never downloads one. Install it (macOS: `brew install ffmpeg`; Windows: `winget install Gyan.FFmpeg`; Linux: your package manager, e.g. `apt install ffmpeg`) or set {FFMPEG_PATH_ENV} to its path, then render again."
    )
}

/// `Chrome required: …`: the renderer found no browser (decisions #2).
fn chrome_missing(detail: &str) -> String {
    format!(
        "Chrome required: {detail}. Contour captures the frames with the Chrome, Edge or Chromium installed on this machine and never downloads one. Install one or set OD_CHROME_PATH, then render again."
    )
}

/// Runs one `hyperframes-html` task: render, write the MP4, attach it to
/// the run, keep three review frames. A failure is logged with the run
/// and the time spent, so the daemon log counts failed renders too.
///
/// # Errors
/// A message for the agent: what is missing in the composition, or which
/// tool (ffmpeg, Chrome) to install.
pub async fn render(
    state: &AppState,
    renderer: &dyn Renderer,
    env: &EnvLookup,
    task: &TaskRecord,
) -> Result<Value, String> {
    let started = Instant::now();
    let mut audio = AudioLog {
        policy: AudioPolicy::None.as_str(),
        tracks: 0,
        mux: "not-reached",
    };
    let result = render_task(state, renderer, env, task, started, &mut audio).await;
    if let Err(error) = &result {
        let run_id = task
            .request
            .get("runId")
            .and_then(Value::as_str)
            .unwrap_or("");
        tracing::warn!(
            task = %task.id,
            run_id,
            project = %task.project_id,
            render_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            audio_policy = audio.policy,
            audio_tracks = audio.tracks,
            audio_mux = audio.mux,
            error = %error,
            "od media generate hyperframes-html failed"
        );
    }
    result
}

/// How long the review frames of an earlier render are kept: a render
/// still running (another task of the project) has written none yet.
const REVIEW_KEEP: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Removes the review folders under `dir` older than [`REVIEW_KEEP`],
/// except `keep`.
fn prune_reviews(dir: &std::path::Path, keep: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > REVIEW_KEEP);
        if path != keep && path.is_dir() && old {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

async fn render_task(
    state: &AppState,
    renderer: &dyn Renderer,
    env: &EnvLookup,
    task: &TaskRecord,
    started: Instant,
    audio_log: &mut AudioLog,
) -> Result<Value, String> {
    let checked = check(&task.request)?;
    let req: MotionRequest =
        serde_json::from_value(task.request.clone()).map_err(|e| e.to_string())?;
    let project_id = task.project_id.clone();
    let entry_path = format!("{}/index.html", checked.dir);
    let (pid, dir, entry_name) = (project_id.clone(), checked.dir.clone(), entry_path.clone());
    let (html, scripts, comp_dir) = state
        .blocking(move |s| {
            let html = String::from_utf8_lossy(&s.read_file(&pid, &entry_name)?.1).into_owned();
            let comp_dir = s.project_root(&s.get_project(&pid)?)?.dir.join(&dir);
            // Each local script's text, or `None` when it is not there.
            let scripts = local_scripts(&html)
                .into_iter()
                .map(|src| {
                    let text = script_path(&dir, &src)
                        .and_then(|p| s.read_file(&pid, &p).ok())
                        .map(|(_, b)| String::from_utf8_lossy(&b).into_owned());
                    (src, text)
                })
                .collect::<Vec<_>>();
            Ok((html, scripts, comp_dir))
        })
        .await
        .map_err(|e| {
            format!(
                "{entry_path}: {}. Run `media scaffold --composition-dir {}` first.",
                e.message, checked.dir
            )
        })?;
    // A script the page cannot load leaves the timeline unregistered and
    // the film standing still: refuse it by name.
    if let Some((src, _)) = scripts.iter().find(|(_, text)| text.is_none()) {
        return Err(format!(
            "index.html loads {src}, which is not in the project. Contour bundles no GSAP or other library and downloads nothing: add the file next to index.html or write the motion without it, then render again"
        ));
    }
    let scripts: Vec<String> = scripts.into_iter().filter_map(|(_, t)| t).collect();
    let (comp, poster) = check_composition(&html, &scripts)?;
    #[allow(clippy::cast_precision_loss)]
    let seconds = comp.duration_ms.unwrap_or(0) as f64 / 1000.0;
    // Sound is checked before minutes of capture: a required track that
    // is missing writes no MP4.
    audio_log.policy = hyperframes::audio_policy(&html).as_str();
    let plan_html = html.clone();
    let plan = tokio::task::spawn_blocking(move || audio_plan(&comp_dir, &plan_html, seconds))
        .await
        .map_err(|e| e.to_string())?
        .inspect_err(|_| audio_log.mux = "refused")?;
    audio_log.tracks = plan.tracks.len();
    audio_log.mux = "failed";
    let fps = req.fps.unwrap_or(comp.fps);
    let ffmpeg = find_ffmpeg_with(env(FFMPEG_PATH_ENV).map(Into::into))
        .map_err(|e| ffmpeg_missing(&e.to_string()))?;
    let mut base = ExportRequest::default();
    base.file_name = Some(entry_path.clone());
    let entry = crate::routes::export::resolve_entry(state, project_id.clone(), &base)
        .await
        .map_err(|e| e.message().to_owned())?;
    let keep = [seconds * 0.1, poster, seconds * 0.95];
    let (bytes, frames, review) = render_mp4(
        state,
        renderer,
        &entry,
        &mut base,
        &comp,
        fps,
        &ffmpeg,
        &keep,
        true,
        &plan.tracks,
    )
    .await
    .map_err(|e| {
        // Only a missing tool says what to install; a failed render keeps
        // its own message.
        let m = e.message();
        match (e.unavailable(), m.contains("ffmpeg")) {
            (true, true) => ffmpeg_missing(m),
            (true, false) => chrome_missing(m),
            (false, _) => m.to_owned(),
        }
    })?;
    let task_id = task.id.clone();
    let (pid, output, video) = (
        project_id.clone(),
        checked.output.clone(),
        declares_video(&html),
    );
    let render_ms = started.elapsed().as_millis();
    let (file, edited, abs, review_paths) = state
        .blocking(move |s| {
            let edited = s.read_file(&pid, &output).is_ok();
            let file = s.write_file(&pid, &output, &bytes, true, None)?;
            let project = s.get_project(&pid)?;
            let root = s.project_root(&project)?;
            if video {
                let mut meta = project.metadata.clone().unwrap_or_default();
                if meta.entry_file.as_deref() != Some(file.name.as_str()) {
                    meta.entry_file = Some(file.name.clone());
                    s.update_project(
                        &pid,
                        UpdateProjectRequest {
                            metadata: Some(Some(meta)),
                            ..UpdateProjectRequest::default()
                        },
                    )?;
                }
            }
            // Review frames live in the data folder, never in the user's
            // project (an imported folder takes no hidden files).
            // One folder per task: two renders of a project never share one.
            let project_reviews = s.data_dir().root().join("media-review").join(&pid);
            let review_dir = project_reviews.join(&task_id);
            std::fs::create_dir_all(&review_dir)?;
            prune_reviews(&project_reviews, &review_dir);
            let mut paths = Vec::new();
            for (name, png) in ["opening", "poster", "ending"].iter().zip(&review) {
                let path = review_dir.join(format!("{name}.png"));
                std::fs::write(&path, png)?;
                paths.push(path.to_string_lossy().into_owned());
            }
            let abs = root.dir.join(&file.name).to_string_lossy().into_owned();
            Ok((file, edited, abs, paths))
        })
        .await
        .map_err(|e| e.message)?;
    #[allow(clippy::cast_precision_loss)]
    let duration_sec = f64::from(frames.count) / f64::from(frames.fps.max(1));
    audio_log.mux = if plan.tracks.is_empty() {
        "none"
    } else {
        "mixed"
    };
    let sound = match (plan.tracks.len(), plan.policy) {
        (0, AudioPolicy::Silent) => "silent by design".to_owned(),
        (0, _) => "no audio".to_owned(),
        (1, _) => "AAC, 1 audio track mixed".to_owned(),
        (n, _) => format!("AAC, {n} audio tracks mixed"),
    };
    let summary = format!(
        "Rendered {} ({:.2} s, {} fps, {}×{}, h264 yuv420p, {sound}) from {}",
        file.name, duration_sec, frames.fps, frames.width, frames.height, entry_path
    );
    if let Some(run_id) = req.run_id.as_deref().filter(|r| !r.is_empty()) {
        attach_to_run(state, run_id, &project_id, &task.id, &abs, edited, &summary);
    }
    tracing::info!(
        task = %task.id,
        run_id = req.run_id.as_deref().unwrap_or(""),
        project = %project_id,
        output = %file.name,
        frames = frames.count,
        render_ms = u64::try_from(render_ms).unwrap_or(u64::MAX),
        edited,
        audio_policy = audio_log.policy,
        audio_tracks = audio_log.tracks,
        audio_mux = audio_log.mux,
        "od media generate hyperframes-html done"
    );
    Ok(json!({
        "file": file,
        "name": file.name,
        "path": abs,
        "model": MODEL,
        "surface": "video",
        "provider": PROVIDER,
        "providerNote": summary,
        "frames": frames.count,
        "fps": frames.fps,
        "width": frames.width,
        "height": frames.height,
        "durationSec": duration_sec,
        "posterTime": poster,
        "audioPolicy": plan.policy.as_str(),
        "audioTracks": plan.tracks.len(),
        "reviewFrames": review_paths,
        "edited": edited,
        "renderMs": u64::try_from(render_ms).unwrap_or(u64::MAX),
        "runId": req.run_id,
    }))
}

/// Adds the MP4 to the live run's turn: a Write (or an Edit of an earlier
/// render) and its result, as if the agent had written it.
fn attach_to_run(
    state: &AppState,
    run_id: &str,
    project_id: &str,
    task_id: &str,
    abs: &str,
    edited: bool,
    summary: &str,
) {
    let Some(run) = state.runs.registry.get(run_id) else {
        tracing::warn!(run = run_id, "media output: no such run");
        return;
    };
    if run.meta().project_id.as_deref() != Some(project_id) {
        tracing::warn!(
            run = run_id,
            "media output: the run belongs to another project"
        );
        return;
    }
    // An ended run takes no events (`Run::emit`): nothing to guard here.
    let id = format!("media-{task_id}");
    run.emit_json(
        sse::AGENT,
        &AgentEvent::ToolUse {
            id: id.clone(),
            name: if edited { "Edit" } else { "Write" }.to_owned(),
            input: json!({ "file_path": abs, "description": format!("od media generate --model {MODEL}") }),
        },
    );
    run.emit_json(
        sse::AGENT,
        &AgentEvent::ToolResult {
            tool_use_id: id,
            content: summary.to_owned(),
            is_error: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_the_request() {
        let ok = check(&json!({"surface": "video", "model": MODEL,
            "compositionDir": "motion-source/film/", "output": "film.mp4"}))
        .unwrap();
        assert_eq!(
            ok,
            Checked {
                dir: "motion-source/film".into(),
                output: "film.mp4".into()
            }
        );
        let default_out =
            check(&json!({"surface": "video", "model": MODEL, "compositionDir": "a/promo"}))
                .unwrap();
        assert_eq!(default_out.output, "promo.mp4");
        for (body, want) in [
            (
                json!({"surface": "video", "model": MODEL}),
                "--composition-dir",
            ),
            (
                json!({"surface": "image", "model": MODEL, "compositionDir": "a"}),
                "--surface video",
            ),
            (
                json!({"surface": "video", "model": MODEL, "compositionDir": "../x"}),
                "inside the project",
            ),
            (
                json!({"surface": "video", "model": MODEL, "compositionDir": "a", "output": "a.gif"}),
                ".mp4",
            ),
            (
                json!({"surface": "video", "model": MODEL, "compositionDir": "a", "output": "/tmp/a.mp4"}),
                "inside the project",
            ),
            (
                json!({"surface": "video", "model": MODEL, "compositionDir": "a", "fps": 0}),
                "--fps",
            ),
        ] {
            let err = check(&body).unwrap_err();
            assert!(err.contains(want), "{body}: {err}");
        }
    }

    #[test]
    fn the_scaffold_is_a_valid_composition() {
        let html = scaffold_html(1280, 720, 24, 3.0);
        let (comp, poster) = check_composition(&html, &[]).unwrap();
        assert_eq!(
            (comp.id.as_str(), comp.width, comp.height, comp.fps),
            ("main", 1280, 720, 24)
        );
        assert_eq!(comp.duration_ms, Some(3000));
        assert!((poster - 1.5).abs() < 1e-9);
        assert!(declares_video(&html));
    }

    #[test]
    fn script_paths_resolve_inside_the_project() {
        assert_eq!(
            script_path("m/film", "./gsap.min.js").as_deref(),
            Some("m/film/gsap.min.js")
        );
        assert_eq!(
            script_path("m/film", "js/a.js?v=2#x").as_deref(),
            Some("m/film/js/a.js")
        );
        assert_eq!(
            script_path("m/film", "../lib/a.js").as_deref(),
            Some("m/lib/a.js")
        );
        assert_eq!(script_path("m", "../../a.js"), None);
        assert_eq!(script_path("m", "/a.js"), None);
    }

    #[test]
    fn composition_errors_name_the_missing_field() {
        let root = r#"<div data-composition-id="main" data-width="64" data-height="36""#;
        let tl = "<script>window.__timelines = { main: tl };</script>";
        let no_duration = format!("{root}></div>{tl}");
        let err = check_composition(&no_duration, &[]).unwrap_err();
        assert!(err.contains("data-duration"), "{err}");

        let no_timeline = format!(r#"{root} data-duration="2"></div>"#);
        let err = check_composition(&no_timeline, &[]).unwrap_err();
        assert!(err.contains("window.__timelines.main"), "{err}");
        // Registered in a local script instead: fine.
        let with_script =
            format!(r#"{root} data-duration="2"></div><script src="main.js"></script>"#);
        assert_eq!(local_scripts(&with_script), ["main.js"]);
        assert!(check_composition(&with_script, &["window.__timelines.main = tl".into()]).is_ok());
        assert!(local_scripts(r#"<script src="https://cdn.example/g.js"></script>"#).is_empty());

        let late_poster = format!(r#"{root} data-duration="2" data-poster-time="2"></div>{tl}"#);
        let err = check_composition(&late_poster, &[]).unwrap_err();
        assert!(err.contains("data-poster-time"), "{err}");

        let err = check_composition("<p>no root</p>", &[]).unwrap_err();
        assert!(err.contains("data-composition-id"), "{err}");
    }

    #[test]
    fn reads_the_motion_output_meta() {
        assert!(declares_video(
            r#"<meta content='video' name='od-motion-output'>"#
        ));
        assert!(!declares_video(
            r#"<meta name="od-motion-output" content="interactive">"#
        ));
        assert!(!declares_video("<title>x</title>"));
    }

    #[test]
    fn missing_tool_messages_say_what_to_install() {
        let m = ffmpeg_missing("OD_FFMPEG_PATH points to /nope, which does not exist");
        assert!(m.starts_with("ffmpeg required"), "{m}");
        assert!(
            m.contains("brew install ffmpeg") && m.contains(FFMPEG_PATH_ENV),
            "{m}"
        );
        let c = chrome_missing("this export needs Chrome, Edge or Chromium");
        assert!(c.starts_with("Chrome required"), "{c}");
    }

    #[test]
    fn audio_plan_places_tracks_in_film_time() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["m.wav", "late.wav", "sub/a.wav", "sub/b.wav", "sub/c.wav"] {
            std::fs::write(dir.join(f), b"RIFF").unwrap();
        }
        std::fs::write(
            dir.join("sub/s.html"),
            r#"<template><audio src="a.wav" data-start="1" data-duration="5"></audio>
<audio src="b.wav"></audio><audio src="c.wav" data-start="3"></audio></template>"#,
        )
        .unwrap();
        let html = r#"<audio src="m.wav" data-volume="0.3"></audio>
<audio src="late.wav" data-start="10"></audio>
<div data-composition-src="sub/s.html" data-start="4" data-duration="3"></div>"#;
        let plan = audio_plan(dir, html, 10.0).unwrap();
        let got: Vec<_> = plan
            .tracks
            .iter()
            .map(|t| {
                let name = t.path.file_name().unwrap().to_string_lossy().into_owned();
                (name, t.start, t.duration, t.volume)
            })
            .collect();
        assert_eq!(
            got,
            [
                // late.wav starts at the film's end: left out.
                ("m.wav".to_owned(), 0.0, None, 0.3),
                // Cut where the 3 s sub-composition ends.
                ("a.wav".to_owned(), 5.0, Some(2.0), 1.0),
                ("b.wav".to_owned(), 4.0, Some(3.0), 1.0),
                // c.wav starts when the sub-composition is over: left out.
            ]
        );
        assert_eq!(plan.policy, AudioPolicy::None);
    }

    #[test]
    fn percent_encoded_sources_name_their_file() {
        assert_eq!(percent_decode("my%20song.wav"), "my song.wav");
        assert_eq!(percent_decode("a%2Fb%zz%4"), "a/b%zz%4");
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("my song.wav"), b"RIFF").unwrap();
        let plan = audio_plan(tmp.path(), r#"<audio src="my%20song.wav"></audio>"#, 5.0).unwrap();
        assert_eq!(plan.tracks.len(), 1);
    }

    #[test]
    fn a_sub_composition_without_audio_never_fails_the_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("quiet.html"),
            "<template><p>no sound</p></template>",
        )
        .unwrap();
        std::fs::write(
            dir.join("loud.html"),
            r#"<template><audio src="a.wav"></audio></template>"#,
        )
        .unwrap();
        std::fs::write(dir.join("a.wav"), b"RIFF").unwrap();
        // Clip references, URLs and missing files: skipped, the film is silent.
        let html = r#"<div data-composition-src="quiet.html" data-start="intro + 2"></div>
<div data-composition-src="https://cdn.example/x.html"></div>
<div data-composition-src="gone.html" data-start="el-1"></div>"#;
        let plan = audio_plan(dir, html, 5.0).unwrap();
        assert!(plan.tracks.is_empty());
        // A film that requires sound still fails on what it cannot read.
        let required = format!(r#"<meta name="od-motion-audio" content="required">{html}"#);
        let err = audio_plan(dir, &required, 5.0).unwrap_err();
        assert!(err.contains("local sub-compositions only"), "{err}");
        // A sub-composition with a track needs numeric timing.
        let err = audio_plan(
            dir,
            r#"<div data-composition-src="loud.html" data-start="intro + 2"></div>"#,
            5.0,
        )
        .unwrap_err();
        assert!(
            err.contains("loud.html") && err.contains("<audio> tracks"),
            "{err}"
        );
    }
}
