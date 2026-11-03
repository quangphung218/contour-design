//! Reference videos for Motion: what a film the user attached is made of,
//! and how close a render comes to it. Local files only (decisions #2):
//! a link is refused with what to do instead, ffmpeg and ffprobe are the
//! ones already installed, and every input is opened through ffmpeg's
//! `file` protocol alone, so no container can make it reach the network.
//!
//! - [`build`] (`od media reference`, and `POST /api/projects/:id/media/
//!   reference` when the CLI names a project) writes a reference folder:
//!   `facts.json`, one frame per scene in `scenes/`, an even grid of frames
//!   in `grid/`, `contact-sheet.png` of the grid, and `audio.m4a` when the
//!   film has sound.
//! - [`compare`] (`od media compare`) scores two films at the same points
//!   of their length, 0–1 (SSIM of small grey frames).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use od_render_cdp::ffmpeg::{FFMPEG_PATH_ENV, find_ffmpeg};
use od_store::paths::validate_project_path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path as FsPath, PathBuf};
use std::process::{Command, Output, Stdio};

/// Largest reference file read.
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Longest reference film read, in seconds.
pub const MAX_SECONDS: f64 = 180.0;
/// Most frames in `grid/` (about one per second below that).
pub const MAX_GRID: usize = 60;
/// Most frames in `scenes/`; `facts.json` still lists every cut.
pub const MAX_SCENE_FRAMES: usize = 60;
/// Scene-change score (0–1) above which a frame starts a new scene.
pub const SCENE_THRESHOLD: f64 = 0.3;
/// Points [`compare`] scores when not told otherwise.
pub const DEFAULT_POINTS: usize = 8;
/// Most points [`compare`] scores.
pub const MAX_POINTS: usize = 60;
/// Size of the grey frames [`compare`] scores.
const CMP_W: usize = 160;
const CMP_H: usize = 90;

/// The installed ffmpeg and the ffprobe next to it.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// `ffmpeg required: …` with how to install it.
fn ffmpeg_missing(detail: &str) -> String {
    format!(
        "ffmpeg required: {detail}. Contour reads reference videos with the ffmpeg and ffprobe installed on this machine and never downloads them. Install ffmpeg (macOS: `brew install ffmpeg`; Windows: `winget install Gyan.FFmpeg`; Linux: your package manager, e.g. `apt install ffmpeg`) or set {FFMPEG_PATH_ENV} to its path, then run the command again."
    )
}

/// Finds ffmpeg as the MP4 render does, and ffprobe beside it.
///
/// # Errors
/// What to install, when either is missing.
pub fn find_tools() -> Result<Tools, String> {
    let ffmpeg = find_ffmpeg().map_err(|e| ffmpeg_missing(&e.to_string()))?;
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    let ffprobe = ffmpeg.with_file_name(name);
    if !ffprobe.is_file() {
        return Err(ffmpeg_missing(&format!(
            "no {name} next to {}",
            ffmpeg.display()
        )));
    }
    Ok(Tools { ffmpeg, ffprobe })
}

/// Refuses a link: Contour reads files the user attached, never fetches.
///
/// # Errors
/// What to do instead, for an `http(s)://` (or any `scheme://`) argument.
pub fn refuse_link(raw: &str) -> Result<(), String> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.contains("://") {
        return Err(format!(
            "{raw} is a link. Contour does not download videos: save the file on this machine, attach it to the project (or pass its path), then run the command again."
        ));
    }
    Ok(())
}

/// What ffprobe says about a film.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
}

/// `path` as an ffmpeg input that can only be a local file.
fn input(path: &FsPath) -> Vec<String> {
    vec![
        "-protocol_whitelist".into(),
        "file".into(),
        "-i".into(),
        format!("file:{}", path.display()),
    ]
}

fn run(cmd: &mut Command, what: &str) -> Result<Output, String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| ffmpeg_missing(&format!("cannot start {what}: {e}")))?;
    if out.status.success() {
        return Ok(out);
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: String = err.lines().rev().take(4).collect::<Vec<_>>().join(" / ");
    Err(format!("{what} failed: {}", tail.trim()))
}

/// `"30000/1001"` → 29.97.
fn rate(raw: &str) -> Option<f64> {
    let (n, d) = raw.split_once('/').unwrap_or((raw, "1"));
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then(|| n / d)
}

/// Duration, size, frame rate and sound of `video`.
///
/// # Errors
/// ffprobe failed, or the file has no video stream.
pub fn probe(tools: &Tools, video: &FsPath) -> Result<Probe, String> {
    let mut cmd = Command::new(&tools.ffprobe);
    cmd.args(["-v", "error", "-protocol_whitelist", "file"])
        .args([
            "-show_entries",
            "format=duration:stream=codec_type,width,height,duration,avg_frame_rate,r_frame_rate:stream_side_data=rotation:stream_tags=rotate",
            "-of",
            "json",
        ])
        .arg(format!("file:{}", video.display()));
    let out = run(&mut cmd, "ffprobe")?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let not_video = || format!("{} has no video stream", video.display());
    let vs = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(not_video)?;
    let dim = |k: &str| {
        vs[k]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    let fps = vs["avg_frame_rate"]
        .as_str()
        .and_then(rate)
        .or_else(|| vs["r_frame_rate"].as_str().and_then(rate))
        .unwrap_or(0.0);
    // The picture's length: sound that runs on past it would put the grid,
    // the scenes and the compare points where there is no frame. Some
    // containers (Matroska, WebM) give only the file's.
    let secs = |d: &Value| {
        d.as_str()
            .and_then(|d| d.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d > 0.0)
    };
    let duration = secs(&vs["duration"])
        .or_else(|| secs(&v["format"]["duration"]))
        .ok_or_else(|| format!("ffprobe gives no duration for {}", video.display()))?;
    // A phone's portrait clip is stored landscape and turned on playback;
    // ffmpeg turns the frames it writes, so the facts follow the picture.
    let (width, height) = if quarter_turn(vs) {
        (dim("height"), dim("width"))
    } else {
        (dim("width"), dim("height"))
    };
    Ok(Probe {
        duration,
        width,
        height,
        fps: (fps * 1000.0).round() / 1000.0,
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
    })
}

/// Whether a video stream plays turned by 90° or 270°: its display matrix
/// (`side_data_list[].rotation`), else the older `rotate` tag.
fn quarter_turn(stream: &Value) -> bool {
    let side = stream["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["rotation"].as_f64());
    let tag = || {
        stream["tags"]["rotate"]
            .as_str()?
            .trim()
            .parse::<f64>()
            .ok()
    };
    side.or_else(tag).is_some_and(|r| {
        #[allow(clippy::cast_possible_truncation)] // a whole number of degrees
        let r = (r.round() as i64).rem_euclid(360);
        r == 90 || r == 270
    })
}

/// A readable local video within the limits: its size and probe.
///
/// # Errors
/// Missing file, over [`MAX_BYTES`] or [`MAX_SECONDS`], not a video.
pub fn check(tools: &Tools, video: &FsPath) -> Result<(u64, Probe), String> {
    let meta =
        std::fs::metadata(video).map_err(|e| format!("cannot read {}: {e}", video.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", video.display()));
    }
    let size = meta.len();
    if size > MAX_BYTES {
        return Err(format!(
            "{} is {:.0} MB; reference videos can be at most {} MB. Trim or compress it, then run the command again.",
            video.display(),
            mb(size),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let p = probe(tools, video)?;
    if p.duration > MAX_SECONDS {
        return Err(format!(
            "{} is {:.1} s long; reference videos can be at most {MAX_SECONDS:.0} s. Cut the part to follow, then run the command again.",
            video.display(),
            p.duration
        ));
    }
    Ok((size, p))
}

#[allow(clippy::cast_precision_loss)] // sizes ≤ a few GB
fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Times (seconds) where a new scene starts, past 0.
///
/// # Errors
/// ffmpeg failed.
pub fn scene_cuts(tools: &Tools, video: &FsPath) -> Result<Vec<f64>, String> {
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-hide_banner", "-nostats", "-nostdin"])
        .args(input(video))
        .args([
            "-an",
            "-vf",
            &format!("scale=320:-2,select='gt(scene,{SCENE_THRESHOLD})',showinfo"),
            "-f",
            "null",
            "-",
        ]);
    let out = run(&mut cmd, "ffmpeg scene detection")?;
    Ok(parse_showinfo(&String::from_utf8_lossy(&out.stderr)))
}

/// The `pts_time:` of each `showinfo` line.
fn parse_showinfo(stderr: &str) -> Vec<f64> {
    let mut cuts: Vec<f64> = stderr
        .lines()
        .filter(|l| l.contains("Parsed_showinfo"))
        .filter_map(|l| {
            let rest = l.split("pts_time:").nth(1)?;
            rest.split_whitespace().next()?.parse::<f64>().ok()
        })
        .filter(|t| t.is_finite() && *t > 0.0)
        .map(|t| (t * 1000.0).round() / 1000.0)
        .collect();
    cuts.dedup();
    cuts
}

/// Writes the frame at `t` of `video` to `png`, at most `max_w` wide.
fn frame(tools: &Tools, video: &FsPath, t: f64, max_w: u32, png: &FsPath) -> Result<(), String> {
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-nostdin", "-y"])
        .args(["-ss", &format!("{t:.3}")])
        .args(input(video))
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale='min({max_w},iw)':-2"),
        ])
        .arg(png);
    run(&mut cmd, "ffmpeg frame")?;
    if png.is_file() {
        Ok(())
    } else {
        Err(format!("ffmpeg wrote no frame at {t:.2} s"))
    }
}

/// One frame of the reference folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    /// Seconds into the film.
    pub time: f64,
    /// Path inside the reference folder.
    pub frame: String,
}

/// One scene: from `start` to `end`, its frame taken in the middle.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub start: f64,
    pub end: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Shot>,
}

/// `facts.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub source: String,
    pub size_bytes: u64,
    #[serde(flatten)]
    pub probe: Probe,
    /// Seconds where a new scene starts (0 not included).
    pub scene_cuts: Vec<f64>,
    pub scenes: Vec<Scene>,
    pub grid: Vec<Shot>,
    pub contact_sheet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Even times over `duration`, about one a second, at most [`MAX_GRID`],
/// each in the middle of its slot.
#[must_use]
pub fn grid_times(duration: f64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // ≤ 180
    let n = (duration.round() as usize).clamp(1, MAX_GRID);
    #[allow(clippy::cast_precision_loss)]
    let step = duration / n as f64;
    #[allow(clippy::cast_precision_loss)]
    (0..n).map(|i| round3((i as f64 + 0.5) * step)).collect()
}

/// Scenes from the cuts, each frame in the middle of its scene; frames for
/// the first [`MAX_SCENE_FRAMES`] only.
fn scenes_of(cuts: &[f64], duration: f64) -> Vec<Scene> {
    let starts = std::iter::once(0.0).chain(cuts.iter().copied().filter(|c| *c < duration));
    let ends = cuts
        .iter()
        .copied()
        .filter(|c| *c < duration)
        .chain(std::iter::once(duration));
    starts
        .zip(ends)
        .enumerate()
        .map(|(i, (start, end))| Scene {
            start: round3(start),
            end: round3(end),
            frame: (i < MAX_SCENE_FRAMES).then(|| Shot {
                time: round3((start + end) / 2.0),
                frame: format!("scenes/{:03}.png", i + 1),
            }),
        })
        .collect()
}

/// File that marks a folder `od media reference` made.
pub const REFERENCE_MARK: &str = ".od-reference";
/// File that marks a folder `od media compare --out` made.
pub const COMPARE_MARK: &str = ".od-compare";

/// Takes `dir` for a command's output: a folder with its `mark` is its
/// own from an earlier run; a missing or empty one is made and marked. Any
/// other folder holds someone's files, which are never removed or
/// replaced.
///
/// # Errors
/// The folder holds files without the mark; I/O.
fn claim(dir: &FsPath, mark: &str) -> Result<(), String> {
    let io = |e: std::io::Error| format!("cannot write {}: {e}", dir.display());
    if dir.join(mark).is_file() {
        return Ok(());
    }
    let used = match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_some(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(io(e)),
    };
    if used {
        return Err(format!(
            "{} already holds files this command did not write; Contour never removes or replaces them. Choose a new or empty folder with --out.",
            dir.display()
        ));
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    std::fs::write(
        dir.join(mark),
        "Written by Contour. Its frames here are replaced on each run.\n",
    )
    .map_err(io)
}

/// Empties a folder of the `NNN.png` frames an earlier run wrote there.
/// Only for a folder [`claim`] took.
fn clear_frames(dir: &FsPath) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let ours = name
            .strip_suffix(".png")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
        if ours {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

/// Writes the reference folder of `video` into `out` and returns its facts.
///
/// # Errors
/// A link, a missing or too large or too long file, ffmpeg missing or
/// failing, I/O.
pub fn build(tools: &Tools, video: &FsPath, out: &FsPath) -> Result<Facts, String> {
    refuse_link(&video.to_string_lossy())?;
    let (size, probe) = check(tools, video)?;
    let io = |e: std::io::Error| format!("cannot write {}: {e}", out.display());
    claim(out, REFERENCE_MARK)?;
    // Nothing of the last run may outlive a run that fails half way:
    // facts.json is written last, once every file it names is there.
    for f in ["facts.json", "contact-sheet.png", "audio.m4a"] {
        std::fs::remove_file(out.join(f))
            .or_else(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(e),
            })
            .map_err(io)?;
    }
    clear_frames(&out.join("scenes")).map_err(io)?;
    clear_frames(&out.join("grid")).map_err(io)?;

    let cuts = scene_cuts(tools, video)?;
    let scenes = scenes_of(&cuts, probe.duration);
    for shot in scenes.iter().filter_map(|s| s.frame.as_ref()) {
        frame(tools, video, shot.time, 1280, &out.join(&shot.frame))?;
    }
    let grid: Vec<Shot> = grid_times(probe.duration)
        .into_iter()
        .enumerate()
        .map(|(i, time)| Shot {
            time,
            frame: format!("grid/{:03}.png", i + 1),
        })
        .collect();
    for shot in &grid {
        frame(tools, video, shot.time, 640, &out.join(&shot.frame))?;
    }
    contact_sheet(tools, out, grid.len())?;
    let audio = if probe.has_audio {
        let mut cmd = Command::new(&tools.ffmpeg);
        cmd.args(["-hide_banner", "-v", "error", "-nostdin", "-y"])
            .args(input(video))
            .args(["-vn", "-c:a", "aac", "-b:a", "160k"])
            .arg(out.join("audio.m4a"));
        run(&mut cmd, "ffmpeg audio")?;
        Some("audio.m4a".to_owned())
    } else {
        None
    };
    let facts = Facts {
        source: video
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size_bytes: size,
        probe: Probe {
            duration: round3(probe.duration),
            ..probe
        },
        scene_cuts: cuts,
        scenes,
        grid,
        contact_sheet: "contact-sheet.png".to_owned(),
        audio,
    };
    let body = serde_json::to_string_pretty(&facts).map_err(|e| e.to_string())?;
    std::fs::write(out.join("facts.json"), body + "\n").map_err(io)?;
    Ok(facts)
}

/// `contact-sheet.png`: the grid frames in rows of six, in time order.
fn contact_sheet(tools: &Tools, out: &FsPath, n: usize) -> Result<(), String> {
    let cols = n.clamp(1, 6);
    let rows = n.div_ceil(cols);
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-nostdin", "-y"])
        .args(["-protocol_whitelist", "file", "-start_number", "1", "-i"])
        .arg(out.join("grid").join("%03d.png"))
        .args([
            "-vf",
            &format!("scale=320:-2,tile={cols}x{rows}:padding=4:margin=4:color=0x202020"),
            "-frames:v",
            "1",
        ])
        .arg(out.join("contact-sheet.png"));
    run(&mut cmd, "ffmpeg contact sheet").map(|_| ())
}

/// The default reference folder of `video`: `reference/<file stem>`.
#[must_use]
pub fn default_out(video: &FsPath) -> String {
    let stem = video
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches(['-', '.']);
    format!("reference/{}", if safe.is_empty() { "video" } else { safe })
}

// ---------------------------------------------------------------------------
// Compare

/// One scored point.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub index: usize,
    /// Share of each film's length, 0–1.
    pub fraction: f64,
    pub time_a: f64,
    pub time_b: f64,
    /// SSIM of the two frames, 0–1.
    pub score: f64,
    /// Side-by-side frame, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pair: Option<String>,
}

/// What [`compare`] prints.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub a: Probe,
    pub b: Probe,
    pub points: Vec<Point>,
    pub mean: f64,
    pub min: f64,
}

/// The frame at `t`, `CMP_W`×`CMP_H` grey bytes.
fn grey(tools: &Tools, video: &FsPath, t: f64) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-nostdin"])
        .args(["-ss", &format!("{t:.3}")])
        .args(input(video))
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale={CMP_W}:{CMP_H}:flags=area,format=gray"),
            "-f",
            "rawvideo",
            "pipe:1",
        ]);
    let out = run(&mut cmd, "ffmpeg frame")?;
    if out.stdout.len() == CMP_W * CMP_H {
        Ok(out.stdout)
    } else {
        Err(format!(
            "no frame at {t:.2} s of {} ({} bytes)",
            video.display(),
            out.stdout.len()
        ))
    }
}

/// Mean SSIM of two `w`×`h` grey images over 8×8 windows, step 4; 0–1.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn ssim(a: &[u8], b: &[u8], w: usize, h: usize) -> f64 {
    const WIN: usize = 8;
    const STEP: usize = 4;
    let c1 = (0.01_f64 * 255.0).powi(2);
    let c2 = (0.03_f64 * 255.0).powi(2);
    if a.len() < w * h || b.len() < w * h || w < WIN || h < WIN {
        return 0.0;
    }
    let n = (WIN * WIN) as f64;
    let (mut total, mut count) = (0.0, 0usize);
    for y in (0..=h - WIN).step_by(STEP) {
        for x in (0..=w - WIN).step_by(STEP) {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for dy in 0..WIN {
                let row = (y + dy) * w + x;
                for i in row..row + WIN {
                    let (p, q) = (f64::from(a[i]), f64::from(b[i]));
                    sa += p;
                    sb += q;
                    saa += p * p;
                    sbb += q * q;
                    sab += p * q;
                }
            }
            let (ma, mb) = (sa / n, sb / n);
            let va = saa / n - ma * ma;
            let vb = sbb / n - mb * mb;
            let cov = sab / n - ma * mb;
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            count += 1;
        }
    }
    (total / count as f64).clamp(0.0, 1.0)
}

/// Writes the frames of both films at `ta`/`tb` side by side to `png`.
fn pair(
    tools: &Tools,
    a: &FsPath,
    ta: f64,
    b: &FsPath,
    tb: f64,
    png: &FsPath,
) -> Result<(), String> {
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-nostdin", "-y"])
        .args(["-ss", &format!("{ta:.3}")])
        .args(input(a))
        .args(["-ss", &format!("{tb:.3}")])
        .args(input(b))
        .args([
            "-filter_complex",
            "[0:v]scale=-2:360,setsar=1[l];[1:v]scale=-2:360,setsar=1[r];[l][r]hstack=inputs=2",
            "-frames:v",
            "1",
        ])
        .arg(png);
    run(&mut cmd, "ffmpeg pair").map(|_| ())
}

/// Scores `a` against `b` at `points` even shares of each one's length;
/// with `pairs`, also writes `NNN.png` (a left, b right) there.
///
/// # Errors
/// A link, a missing or too large or too long file, ffmpeg missing or
/// failing, `points` out of 1–[`MAX_POINTS`].
pub fn compare(
    tools: &Tools,
    a: &FsPath,
    b: &FsPath,
    points: usize,
    pairs: Option<&FsPath>,
) -> Result<Comparison, String> {
    if !(1..=MAX_POINTS).contains(&points) {
        return Err(format!("--points must be 1–{MAX_POINTS}"));
    }
    refuse_link(&a.to_string_lossy())?;
    refuse_link(&b.to_string_lossy())?;
    let (_, pa) = check(tools, a)?;
    let (_, pb) = check(tools, b)?;
    if let Some(dir) = pairs {
        claim(dir, COMPARE_MARK)?;
        clear_frames(dir).map_err(|e| format!("cannot write {}: {e}", dir.display()))?;
    }
    let mut scored = Vec::with_capacity(points);
    for i in 0..points {
        #[allow(clippy::cast_precision_loss)]
        let fraction = (i as f64 + 0.5) / points as f64;
        let (ta, tb) = (
            round3(fraction * pa.duration),
            round3(fraction * pb.duration),
        );
        let score = ssim(&grey(tools, a, ta)?, &grey(tools, b, tb)?, CMP_W, CMP_H);
        let pair_png = match pairs {
            Some(dir) => {
                let png = dir.join(format!("{:03}.png", i + 1));
                pair(tools, a, ta, b, tb, &png)?;
                Some(png.to_string_lossy().into_owned())
            }
            None => None,
        };
        scored.push(Point {
            index: i + 1,
            fraction: round3(fraction),
            time_a: ta,
            time_b: tb,
            score: round3(score),
            pair: pair_png,
        });
    }
    #[allow(clippy::cast_precision_loss)]
    let mean = scored.iter().map(|p| p.score).sum::<f64>() / scored.len() as f64;
    let min = scored.iter().map(|p| p.score).fold(1.0, f64::min);
    Ok(Comparison {
        a: pa,
        b: pb,
        points: scored,
        mean: round3(mean),
        min,
    })
}

// ---------------------------------------------------------------------------
// Route

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceRequest {
    /// A project path, or an absolute path on this machine.
    video: String,
    /// The reference folder, inside the project; default
    /// `reference/<video name>`.
    #[serde(default)]
    out: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
}

/// `POST /api/projects/:id/media/reference`: [`build`] with project paths.
pub async fn reference(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<ReferenceRequest>,
) -> ApiResult<Json<Value>> {
    refuse_link(&req.video).map_err(ApiErr::bad_request)?;
    let pid = id.clone();
    let root = state
        .blocking(move |s| {
            let project = s.get_project(&pid)?;
            s.project_root(&project)
        })
        .await?
        .dir;
    let video = if FsPath::new(req.video.trim()).is_absolute() {
        PathBuf::from(req.video.trim())
    } else {
        let rel = validate_project_path(req.video.trim()).map_err(|_| {
            ApiErr::bad_request(format!(
                "the video must be a project path or an absolute path, got {:?}",
                req.video
            ))
        })?;
        root.join(rel)
    };
    let out = req
        .out
        .as_deref()
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .map_or_else(|| default_out(&video), str::to_owned);
    let out = validate_project_path(&out).map_err(|_| {
        ApiErr::bad_request(format!(
            "--out must be a folder inside the project, got {out:?}"
        ))
    })?;
    let dest = root.join(&out);
    let video2 = video.clone();
    let facts = tokio::task::spawn_blocking(move || {
        let tools = find_tools()?;
        build(&tools, &video2, &dest)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(ApiErr::bad_request)?;
    tracing::info!(
        project = %id,
        run_id = req.run_id.as_deref().unwrap_or(""),
        out = %out,
        scenes = facts.scenes.len(),
        "od media reference"
    );
    Ok(Json(json!({
        "dir": out,
        "absDir": root.join(&out).to_string_lossy(),
        "facts": facts,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_refused_with_what_to_do() {
        for l in ["https://youtu.be/x", "HTTP://a/b.mp4", "rtmp://h/s"] {
            let e = refuse_link(l).unwrap_err();
            assert!(
                e.contains("does not download") && e.contains("attach"),
                "{e}"
            );
        }
        assert!(refuse_link("clips/ref.mp4").is_ok());
    }

    #[test]
    fn grid_is_about_one_frame_a_second_capped() {
        let g = grid_times(10.0);
        assert_eq!(g.len(), 10);
        assert!((g[0] - 0.5).abs() < 1e-9 && (g[9] - 9.5).abs() < 1e-9);
        assert_eq!(grid_times(0.3).len(), 1);
        assert_eq!(grid_times(180.0).len(), MAX_GRID);
    }

    #[test]
    fn scenes_run_between_cuts() {
        let s = scenes_of(&[4.0, 7.0], 10.0);
        let spans: Vec<(f64, f64)> = s.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(0.0, 4.0), (4.0, 7.0), (7.0, 10.0)]);
        assert_eq!(s[1].frame.as_ref().map(|f| f.time), Some(5.5));
        assert_eq!(
            s[2].frame.as_ref().map(|f| f.frame.as_str()),
            Some("scenes/003.png")
        );
    }

    #[test]
    fn showinfo_times_are_read() {
        let log = "[Parsed_showinfo_2 @ 0x1] n:   0 pts:  120 pts_time:4       duration:1\n\
                   [Parsed_showinfo_2 @ 0x1] n:   1 pts:  210 pts_time:7.0333  duration:1\n\
                   frame=  2 fps=0.0";
        assert_eq!(parse_showinfo(log), vec![4.0, 7.033]);
    }

    #[test]
    fn ssim_is_one_for_the_same_image_and_low_for_another() {
        let (w, h) = (32, 16);
        let a: Vec<u8> = (0..w * h)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        let b: Vec<u8> = a.iter().map(|p| 255 - p).collect();
        assert!((ssim(&a, &a, w, h) - 1.0).abs() < 1e-9);
        assert!(ssim(&a, &b, w, h) < 0.2);
    }

    #[test]
    fn quarter_turns_come_from_side_data_or_the_old_tag() {
        let side = |r: f64| json!({ "side_data_list": [{ "rotation": r }] });
        assert!(quarter_turn(&side(90.0)) && quarter_turn(&side(-90.0)));
        assert!(quarter_turn(&side(270.0)) && !quarter_turn(&side(180.0)));
        assert!(quarter_turn(&json!({ "tags": { "rotate": "90" } })));
        assert!(!quarter_turn(&json!({ "tags": { "rotate": "0" } })));
        assert!(!quarter_turn(&json!({})));
    }

    #[test]
    fn default_out_is_a_safe_project_folder() {
        assert_eq!(
            default_out(FsPath::new("/x/My Ref (1).mp4")),
            "reference/My-Ref--1"
        );
        assert_eq!(default_out(FsPath::new("clip.mov")), "reference/clip");
    }
}
