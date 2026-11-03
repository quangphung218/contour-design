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
    if raw.contains("://") {
        return Err(link_refusal(raw));
    }
    Ok(())
}

/// Refuses, once `resolved` (where `raw` points on disk) turns out not to
/// exist, a link pasted without its scheme: `youtube.com/watch?v=…`,
/// `youtu.be/…`, `www.…`, as share sheets and chat apps copy them.
///
/// # Errors
/// The same refusal as [`refuse_link`].
pub fn refuse_bare_link(raw: &str, resolved: &FsPath) -> Result<(), String> {
    if looks_like_link(raw) && !resolved.exists() {
        return Err(link_refusal(raw));
    }
    Ok(())
}

fn link_refusal(raw: &str) -> String {
    format!(
        "{raw} is a link. Contour does not download videos: save the file on this machine, attach it to the project (or pass its path), then run the command again."
    )
}

/// Endings of a bare host (`vimeo.com`, `youtu.be`) read as a web address
/// with nothing after it. Others (`.mov`, `.mkv`, `.webm` are endings of
/// both) stay file names: a missing `clip.mov` is told as missing.
const WEB_TLDS: &[&str] = &[
    "com", "net", "org", "io", "co", "tv", "be", "me", "ly", "gl", "app", "dev", "info", "vn",
    "us", "uk", "de", "fr", "jp", "ru", "cn", "br", "to", "xyz",
];

/// `www.<…>`, `<name>.<tld>` followed by `/` or `?` (`youtu.be/x`,
/// `vimeo.com/1`), or a bare host with a web ending (`vimeo.com`,
/// `youtube.com`): a web address, not a project path.
fn looks_like_link(raw: &str) -> bool {
    let raw = raw.trim();
    let end = raw.find(['/', '?', '#']).unwrap_or(raw.len());
    let host = raw[..end].to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let is_host = labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        && labels.last().is_some_and(|t| {
            (2..=24).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphabetic())
        });
    is_host
        && (host.starts_with("www.")
            || end < raw.len()
            || labels.last().is_some_and(|t| WEB_TLDS.contains(t)))
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
            "format=format_name,duration:stream=codec_type,width,height,duration,avg_frame_rate,r_frame_rate:stream_side_data=rotation:stream_disposition=attached_pic:stream_tags=rotate,DURATION",
            "-of",
            "json",
        ])
        .arg(format!("file:{}", video.display()))
        .stdin(Stdio::null());
    let out = cmd
        .output()
        .map_err(|e| ffmpeg_missing(&format!("cannot start ffprobe: {e}")))?;
    let name = shown_name(video);
    // ffprobe cannot open it: a text, a PDF, a broken download. What
    // ffprobe says about it means nothing to the user.
    let v = if out.status.success() {
        serde_json::from_slice::<Value>(&out.stdout).ok()
    } else {
        None
    };
    let v = v.ok_or_else(|| {
        format!(
            "{name} is not a video Contour can read. Attach the film itself ({FORMATS}), then run the command again."
        )
    })?;
    read_probe(&name, &v)
}

/// The containers to suggest instead of a file that is not a video.
const FORMATS: &str = "MP4, MOV, MKV or WebM";

/// The file's name, as the user knows it.
fn shown_name(video: &FsPath) -> String {
    video.file_name().map_or_else(
        || video.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// [`probe`] from ffprobe's JSON about the file `name`.
fn read_probe(name: &str, v: &Value) -> Result<Probe, String> {
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    // An image opens as one picture with no length: png_pipe, jpeg_pipe,
    // webp_pipe, image2.
    let format = v["format"]["format_name"].as_str().unwrap_or_default();
    if format.ends_with("_pipe") || format == "image2" {
        return Err(format!(
            "{name} is a still image, not a video. Attach the film itself ({FORMATS}), then run the command again."
        ));
    }
    let not_video = || format!("{name} has no video stream");
    let vs = streams
        .iter()
        // A song's cover art is a one-picture video stream: not a film.
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
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
    // the scenes and the compare points where there is no frame. Matroska
    // and WebM keep it in the stream's DURATION tag instead.
    let secs = |d: &Value| {
        d.as_str()
            .and_then(|d| d.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d > 0.0)
    };
    let duration = secs(&vs["duration"])
        .or_else(|| vs["tags"]["DURATION"].as_str().and_then(clock))
        .or_else(|| secs(&v["format"]["duration"]))
        .ok_or_else(|| {
            format!(
                "{name} gives no length Contour can read, so it cannot be followed as a video. Save it again as an MP4 and attach that, then run the command again."
            )
        })?;
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

/// Seconds in a Matroska `DURATION` tag (`HH:MM:SS.fffffffff`).
fn clock(tag: &str) -> Option<f64> {
    let mut parts = tag.trim().splitn(3, ':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s).filter(|d| d.is_finite() && *d > 0.0)
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

/// Why a run did not get its folder, or failed in it.
#[derive(Debug)]
enum RunErr {
    /// The folder is not this run's: someone's files, or the reference of
    /// another film.
    Taken(String),
    /// Another run is writing the folder: it is left as it is.
    Busy(String),
    Failed(String),
}

impl From<String> for RunErr {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

impl RunErr {
    fn message(self) -> String {
        match self {
            Self::Taken(m) | Self::Busy(m) | Self::Failed(m) => m,
        }
    }
}

/// What a mark file says first; [`compare`] lists the frames it wrote
/// below it.
const MARK_TEXT: &str = "Written by Contour. Each run replaces the frames the last run wrote here (listed in facts.json, or below) and no other file.\n";
/// The line of a reference mark that names the film it was taken for.
const SOURCE_LINE: &str = "source: ";
/// Start of the name a mark is written under before it is put in place.
const MARK_TMP: &str = ".od-mark-";

fn mark_head(source: Option<&str>) -> String {
    match source {
        Some(s) => format!("{MARK_TEXT}{SOURCE_LINE}{s}\n"),
        None => MARK_TEXT.to_owned(),
    }
}

/// The film whose reference `dir` holds: the source its mark names (a
/// [`film_path`], or only the file name in a mark of beta.17 or older),
/// else (a mark from before it named one) the name in its `facts.json`.
/// Compare it with [`same_film`].
fn taken_for(dir: &FsPath) -> Option<String> {
    let mark = std::fs::read_to_string(dir.join(REFERENCE_MARK)).ok()?;
    mark.lines()
        .find_map(|l| l.strip_prefix(SOURCE_LINE))
        .map(str::to_owned)
        .or_else(|| {
            std::fs::read(dir.join("facts.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v["source"].as_str().map(str::to_owned))
        })
}

/// Takes `dir` for a command's output: a folder with its `mark` is its
/// own from an earlier run (for a reference, of the same film `source`); a
/// missing or empty one is made and marked, in one step, so of two runs
/// at once only one gets it. Any other folder holds someone's files, which
/// are never removed or replaced. Returns whether this run made the mark.
///
/// # Errors
/// [`RunErr::Taken`]: the folder holds files without the mark, or the
/// reference of another film. I/O.
fn claim(dir: &FsPath, mark: &str, source: Option<&str>) -> Result<bool, RunErr> {
    let io = |e: std::io::Error| RunErr::Failed(format!("cannot write {}: {e}", dir.display()));
    for _ in 0..3 {
        if dir.join(mark).is_file() {
            let other =
                source.and_then(|want| taken_for(dir).filter(|have| !same_film(have, want)));
            if let Some(have) = other {
                return Err(RunErr::Taken(format!(
                    "{} holds the reference of {}{}; Contour never replaces it. Choose a new folder with --out.",
                    dir.display(),
                    have.rsplit('/').next().unwrap_or(&have),
                    if have.contains('/') {
                        format!(" (at {have} from that folder)")
                    } else {
                        String::new()
                    }
                )));
            }
            return Ok(false);
        }
        let used = match std::fs::read_dir(dir) {
            Ok(entries) => entries.flatten().any(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                // A run's lock outlives its mark by a moment, and stays.
                !name.starts_with(MARK_TMP) && !name.starts_with(RUN_LOCK)
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io(e)),
        };
        if used {
            return Err(RunErr::Taken(format!(
                "{} already holds files this command did not write; Contour never removes or replaces them. Choose a new or empty folder with --out.",
                dir.display()
            )));
        }
        std::fs::create_dir_all(dir).map_err(io)?;
        match place_mark(dir, mark, &mark_head(source)) {
            Ok(()) => return Ok(true),
            // Another run marked it first: look again.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io(e)),
        }
    }
    Err(RunErr::Taken(format!(
        "{} is being taken by another run; run the command again.",
        dir.display()
    )))
}

/// Puts `dir/mark` holding `body` in place in one step: another run never
/// reads it half written. `AlreadyExists` when another run's mark is there.
fn place_mark(dir: &FsPath, mark: &str, body: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        "{MARK_TMP}{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, body)?;
    let placed = std::fs::hard_link(&tmp, dir.join(mark));
    let _ = std::fs::remove_file(&tmp);
    match placed {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
            // No hard links on this disk (FAT, exFAT): make it only if absent.
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dir.join(mark))?;
            f.write_all(body.as_bytes())
        }
        r => r,
    }
}

/// File a run holds an OS lock on while it writes the folder. The lock goes
/// with the run however it ends, a killed one too; so does the file, unless
/// the run was killed or another run had it open just then.
pub const RUN_LOCK: &str = ".od-running";

/// A folder held by one run: an exclusive OS lock on [`RUN_LOCK`] while it
/// lives. A second run into the folder, in this process or another, is
/// refused instead of clearing the frames the first one is writing.
#[derive(Debug)]
struct RunLock {
    path: PathBuf,
    /// Holds the lock; closing it, in a drop or when the run dies, frees it.
    file: Option<std::fs::File>,
}

impl RunLock {
    /// Takes `dir`. A [`RUN_LOCK`] no run holds (left by a run that was
    /// killed, or by an older Contour) is taken as it is.
    ///
    /// # Errors
    /// [`RunErr::Busy`]: another run holds `dir`. I/O.
    fn take(dir: &FsPath) -> Result<Self, RunErr> {
        let io = |e: std::io::Error| RunErr::Failed(format!("cannot write {}: {e}", dir.display()));
        let path = dir.join(RUN_LOCK);
        for _ in 0..3 {
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            // Never removed while another run has it open: see `drop`.
            #[cfg(windows)]
            std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, 0x1 | 0x2);
            let file = match options.open(&path) {
                Ok(f) => f,
                // A run is removing it this moment.
                #[cfg(windows)]
                Err(e) if e.raw_os_error() == Some(32) => continue,
                Err(e) => return Err(io(e)),
            };
            match Self::hold(path.clone(), file) {
                Ok(Some(lock)) => return Ok(lock),
                // The run before removed this one after it was opened: lock
                // the file there now.
                Ok(None) => {}
                Err(fs4::TryLockError::WouldBlock) => break,
                Err(fs4::TryLockError::Error(e)) => return Err(io(e)),
            }
        }
        Err(RunErr::Busy(format!(
            "another run is writing {}; wait for it to finish",
            dir.display()
        )))
    }

    /// Locks `file`, opened at `path`, and writes this process's id in it;
    /// `None` when `path` no longer names it.
    fn hold(path: PathBuf, mut file: std::fs::File) -> Result<Option<Self>, fs4::TryLockError> {
        use std::io::Write as _;
        // Through fs4: `File::try_lock` needs Rust 1.89, newer than ours.
        fs4::FileExt::try_lock(&file)?;
        // Not built otherwise: its drop would remove the file now there.
        if !still_named(&file, &path) {
            return Ok(None);
        }
        // A beta.17 daemon reads the pid, and takes an empty lock over once
        // it is a minute old: with a live pid in it, it sees the folder busy.
        file.set_len(0)
            .and_then(|()| writeln!(file, "{}", std::process::id()))
            .and_then(|()| file.flush())
            .map_err(fs4::TryLockError::Error)?;
        Ok(Some(Self {
            path,
            file: Some(file),
        }))
    }
}

/// Whether `path` still names `file`.
#[cfg(unix)]
fn still_named(file: &std::fs::File, path: &FsPath) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}

/// Always: no run removes the file while another has it open.
#[cfg(not(unix))]
fn still_named(_file: &std::fs::File, _path: &FsPath) -> bool {
    true
}

impl Drop for RunLock {
    /// Removes [`RUN_LOCK`] only while it is still this run's: on unix
    /// before the lock goes (a run that opened it meanwhile sees it gone and
    /// tries again); elsewhere after, which fails while any other run has
    /// it open, since none shares it for removal.
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
        drop(self.file.take());
        #[cfg(not(unix))]
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether `name` is a frame a run writes: `NNN.png` in one of `dirs`
/// (`""`: the folder itself), nothing else.
fn is_frame(name: &str, dirs: &[&str]) -> bool {
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    dirs.contains(&dir)
        && file
            .strip_suffix(".png")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// The mark beta.16 wrote, with no list: every `NNN.png` there was its.
const LEGACY_MARK: &str = "Written by Contour. Its frames here are replaced on each run.\n";

/// The frames a mark lists in `subs` (one per line, added as each is
/// written); for a beta.16 mark, which lists none, every `NNN.png` there.
fn mark_frames(dir: &FsPath, mark: &str, subs: &[&str], legacy: bool) -> Vec<String> {
    let body = std::fs::read_to_string(dir.join(mark)).unwrap_or_default();
    let mut names: Vec<String> = body
        .lines()
        .filter(|l| is_frame(l, subs))
        .map(str::to_owned)
        .collect();
    if legacy && body == LEGACY_MARK {
        for sub in subs {
            let entries = std::fs::read_dir(dir.join(sub))
                .into_iter()
                .flatten()
                .flatten();
            names.extend(entries.filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let name = if sub.is_empty() {
                    name
                } else {
                    format!("{sub}/{name}")
                };
                is_frame(&name, subs).then_some(name)
            }));
        }
    }
    names
}

/// The frames the last reference run wrote in `out`: those its
/// `facts.json` lists and those its mark lists (a run killed before
/// `facts.json` was written). A frame the user saved there under the same
/// kind of name is in neither, so it stays.
fn listed_frames(out: &FsPath) -> Vec<String> {
    let dirs = ["scenes", "grid"];
    let facts = std::fs::read(out.join("facts.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let mut names = mark_frames(out, REFERENCE_MARK, &dirs, facts.is_none());
    if let Some(v) = facts {
        let scenes = v["scenes"].as_array().into_iter().flatten();
        let grid = v["grid"].as_array().into_iter().flatten();
        names.extend(
            scenes
                .filter_map(|s| s["frame"]["frame"].as_str())
                .chain(grid.filter_map(|g| g["frame"].as_str()))
                .filter(|f| is_frame(f, &dirs))
                .map(str::to_owned),
        );
    }
    names
}

/// The pairs the last compare run listed under its mark in `dir`.
fn listed_pairs(dir: &FsPath) -> Vec<String> {
    mark_frames(dir, COMPARE_MARK, &[""], true)
}

/// Adds `name` to the list in `dir`'s `mark`, before the file is written:
/// a run killed half way leaves the next one the list of what to clear.
fn record(dir: &FsPath, mark: &str, name: &str) -> Result<(), String> {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(mark))
        .and_then(|mut f| writeln!(f, "{name}"))
        .map_err(|e| format!("cannot write {}: {e}", dir.display()))
}

/// Refuses to write `name` in `dir` over a file already there: the last
/// run's files are gone by now, so it is someone else's.
fn not_there(dir: &FsPath, name: &str) -> Result<(), String> {
    let path = dir.join(name);
    if path.exists() {
        return Err(format!(
            "{} already exists and this command did not write it; Contour never removes or replaces it. Move it, or choose a new folder with --out.",
            path.display()
        ));
    }
    Ok(())
}

/// Removes the files `names` (inside `dir`); a missing one is fine.
fn remove_listed(dir: &FsPath, names: &[String]) -> std::io::Result<()> {
    for n in names {
        match std::fs::remove_file(dir.join(n)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Writes the reference folder of `video` into `out` and returns its facts.
///
/// # Errors
/// A link, a missing or too large or too long file, a folder that is not
/// this command's, ffmpeg missing or failing, I/O.
pub fn build(tools: &Tools, video: &FsPath, out: &FsPath) -> Result<Facts, String> {
    refuse_link(&video.to_string_lossy())?;
    let (size, probe) = check(tools, video)?;
    let film = film_key(video, out);
    write_reference(tools, video, &film, out, size, probe).map_err(RunErr::message)
}

/// How the mark in `out` names the film at `video`: [`film_path`] of both
/// as the disk knows them. The same however either is written (relative to
/// any working directory, absolute, with `..`, `A/CLIP.MP4` on a disk that
/// ignores case), from the CLI or the daemon, and after the project moves.
fn film_key(video: &FsPath, out: &FsPath) -> String {
    film_path(&real_path(video), &real_path(out))
}

/// `path` absolute, as the disk knows it: canonical up to the last part
/// that exists (a folder a run is about to make does not yet).
fn real_path(path: &FsPath) -> PathBuf {
    use std::path::Component;
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut head = abs.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(head) {
            let mut real = real;
            for c in tail.iter().rev() {
                match c {
                    Component::ParentDir => {
                        real.pop();
                    }
                    Component::Normal(n) => real.push(n),
                    _ => {}
                }
            }
            return real;
        }
        let (Some(parent), Some(last)) = (head.parent(), head.components().next_back()) else {
            return abs;
        };
        tail.push(last);
        head = parent;
    }
}

/// The path from the folder `out` to `video` (both absolute and
/// canonical), `..` where it climbs, `/` between the parts: in
/// `reference/clip`, `../../a/clip.mp4`. It always holds a `/`
/// (`./clip.mp4` beside the mark), so it never reads as a bare name from a
/// mark of beta.17 or older. On another drive (Windows): `video` itself.
fn film_path(video: &FsPath, out: &FsPath) -> String {
    use std::path::Component;
    let v: Vec<Component> = video.components().collect();
    let o: Vec<Component> = out.components().collect();
    let parts: Vec<String> = if v.first() == o.first() {
        let same = v.iter().zip(&o).take_while(|(a, b)| a == b).count();
        std::iter::repeat_n("..".to_owned(), o.len() - same)
            .chain(
                v[same..]
                    .iter()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned()),
            )
            .collect()
    } else {
        let mut s = String::new();
        for c in &v {
            match c {
                // `\\?\C:` (from canonicalize on Windows) reads as `C:`.
                Component::Prefix(p) => match p.kind() {
                    std::path::Prefix::VerbatimDisk(d) | std::path::Prefix::Disk(d) => {
                        s.push(char::from(d.to_ascii_uppercase()));
                        s.push(':');
                    }
                    _ => s.push_str(&p.as_os_str().to_string_lossy()),
                },
                Component::RootDir => s.push('/'),
                _ => {
                    if !s.ends_with('/') {
                        s.push('/');
                    }
                    s.push_str(&c.as_os_str().to_string_lossy());
                }
            }
        }
        return s;
    };
    let s = parts.join("/");
    if s.contains('/') { s } else { format!("./{s}") }
}

/// Whether a folder whose mark names `have` holds the reference of the
/// film at `want` (a [`film_path`]). A mark of beta.17 or older names the
/// film without its folders (no `/`): then the file names are compared.
fn same_film(have: &str, want: &str) -> bool {
    if have.contains('/') {
        have == want
    } else {
        want.rsplit('/').next() == Some(have)
    }
}

/// [`build`] into the default folder of `video` under `base` (the project,
/// or the working directory): the first of [`default_dirs`] that is free or
/// already holds this film's reference. Returns that folder (relative to
/// `base`) and the facts.
///
/// # Errors
/// As [`build`]; no free folder.
pub fn build_default(
    tools: &Tools,
    video: &FsPath,
    base: &FsPath,
) -> Result<(String, Facts), String> {
    refuse_link(&video.to_string_lossy())?;
    let (size, probe) = check(tools, video)?;
    let source = shown_name(video);
    // Named as the disk stores it: `A/CLIP.MP4` on a disk that ignores case
    // is `clip.mp4`, whose folder is `reference/clip`. A link keeps its name.
    let stored = std::fs::canonicalize(video)
        .ok()
        .and_then(|r| r.file_name().map(std::ffi::OsStr::to_os_string))
        .filter(|n| {
            video.file_name().is_some_and(|v| {
                v.to_string_lossy().to_lowercase() == n.to_string_lossy().to_lowercase()
            })
        })
        .map_or_else(|| video.to_path_buf(), |n| video.with_file_name(n));
    for dir in default_dirs(&stored) {
        let out = base.join(&dir);
        let film = film_key(video, &out);
        if taken_for(&out).is_some_and(|have| !same_film(&have, &film)) {
            continue;
        }
        match write_reference(tools, video, &film, &out, size, probe.clone()) {
            Ok(facts) => return Ok((dir, facts)),
            // Someone's files, or another film's reference (maybe one
            // taken a moment ago by a run at the same time): the next.
            Err(RunErr::Taken(_)) => {}
            Err(e @ (RunErr::Busy(_) | RunErr::Failed(_))) => return Err(e.message()),
        }
    }
    Err(format!(
        "no free folder under reference/ for {source}; choose one with --out."
    ))
}

/// The folders tried for `video`: `reference/<file stem>`, then
/// `reference/<stem>-<extension>` (`clip.mkv` beside `clip.mp4`), then
/// `…-2` to `…-99`.
fn default_dirs(video: &FsPath) -> Vec<String> {
    let safe = |s: Option<&std::ffi::OsStr>| {
        let s: String = s
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        s.trim_matches(['-', '.']).to_owned()
    };
    let stem = safe(video.file_stem());
    let stem = if stem.is_empty() {
        "video".to_owned()
    } else {
        stem
    };
    let first = format!("reference/{stem}");
    let ext = safe(video.extension());
    let named = if ext.is_empty() {
        format!("{first}-2")
    } else {
        format!("{first}-{ext}")
    };
    let mut dirs = vec![first, named.clone()];
    dirs.extend((2..100).map(|n| format!("{named}-{n}")));
    dirs.dedup();
    dirs
}

/// [`build`] past the checks. A run that fails takes back the files it
/// wrote and the folders it made, each only while empty: another run, or
/// the user, may have written beside them in the meantime.
fn write_reference(
    tools: &Tools,
    video: &FsPath,
    film: &str,
    out: &FsPath,
    size: u64,
    probe: Probe,
) -> Result<Facts, RunErr> {
    let fresh = fresh_root(out);
    let mut lock = None;
    let res = take_and_fill(tools, video, film, out, size, probe, &mut lock);
    if let (Err(RunErr::Failed(_)), Some(fresh)) = (&res, fresh) {
        remove_empty_dirs(out, &fresh, &["scenes", "grid"], lock);
    }
    res
}

/// The outermost of `dir` and its parents that does not exist yet: what a
/// run that makes `dir` and then fails takes back.
fn fresh_root(dir: &FsPath) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|d| !d.as_os_str().is_empty() && !d.exists())
        .last()
        .map(FsPath::to_path_buf)
}

/// Removes `subs` of `dir` while the run still holds `lock`, then lets it
/// go (its [`RUN_LOCK`] with it) and removes `dir` and its parents up to
/// `fresh`, each only while it is empty. A run that took the folder the
/// moment the lock went has its mark in it by then, or fails to write it.
fn remove_empty_dirs(dir: &FsPath, fresh: &FsPath, subs: &[&str], lock: Option<RunLock>) {
    for sub in subs {
        let _ = std::fs::remove_dir(dir.join(sub));
    }
    drop(lock);
    for d in dir.ancestors() {
        if std::fs::remove_dir(d).is_err() || d == fresh {
            break;
        }
    }
}

/// Takes `out` for the film at `film` (a [`film_path`]), clears the last
/// run's files and writes the new ones.
fn take_and_fill(
    tools: &Tools,
    video: &FsPath,
    film: &str,
    out: &FsPath,
    size: u64,
    probe: Probe,
    lock: &mut Option<RunLock>,
) -> Result<Facts, RunErr> {
    let io = |e: std::io::Error| format!("cannot write {}: {e}", out.display());
    let created = claim(out, REFERENCE_MARK, Some(film))?;
    // Held by the caller until it has taken back the folders it made.
    *lock = Some(RunLock::take(out)?);
    // Only what the last run wrote goes: the frames its facts.json lists,
    // never another file of the same kind of name.
    let mut old = listed_frames(out);
    // Nothing of the last run may outlive a run that fails half way:
    // facts.json is written last, once every file it names is there.
    old.extend(["facts.json", "contact-sheet.png", "audio.m4a"].map(str::to_owned));
    remove_listed(out, &old).map_err(io)?;
    if !created {
        std::fs::write(out.join(REFERENCE_MARK), mark_head(Some(film))).map_err(io)?;
    }
    for sub in ["scenes", "grid"] {
        std::fs::create_dir_all(out.join(sub)).map_err(io)?;
    }
    let mut written = Vec::new();
    let res = fill(tools, video, out, size, probe, &mut written);
    if res.is_err() {
        // A failed run takes back what it wrote: the next one could not
        // tell those frames from the user's.
        let _ = remove_listed(out, &written);
        let _ = if created {
            std::fs::remove_file(out.join(REFERENCE_MARK))
        } else {
            std::fs::write(out.join(REFERENCE_MARK), mark_head(Some(film)))
        };
    }
    Ok(res?)
}

/// [`build`] once `out` is ready; records each file it writes in `written`.
fn fill(
    tools: &Tools,
    video: &FsPath,
    out: &FsPath,
    size: u64,
    probe: Probe,
    written: &mut Vec<String>,
) -> Result<Facts, String> {
    let io = |e: std::io::Error| format!("cannot write {}: {e}", out.display());
    let cuts = scene_cuts(tools, video)?;
    let scenes = scenes_of(&cuts, probe.duration);
    for shot in scenes.iter().filter_map(|s| s.frame.as_ref()) {
        not_there(out, &shot.frame)?;
        record(out, REFERENCE_MARK, &shot.frame)?;
        written.push(shot.frame.clone());
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
        not_there(out, &shot.frame)?;
        record(out, REFERENCE_MARK, &shot.frame)?;
        written.push(shot.frame.clone());
        frame(tools, video, shot.time, 640, &out.join(&shot.frame))?;
    }
    written.push("contact-sheet.png".to_owned());
    contact_sheet(tools, out, grid.len())?;
    let audio = if probe.has_audio {
        written.push("audio.m4a".to_owned());
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
        source: shown_name(video),
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
    let scored = match pairs {
        Some(dir) => {
            let fresh = fresh_root(dir);
            let mut lock = None;
            let res = take_and_pair(tools, (a, &pa), (b, &pb), points, dir, &mut lock);
            // A new folder left with only the mark goes too.
            if let (Err(RunErr::Failed(_)), Some(fresh)) = (&res, fresh) {
                remove_empty_dirs(dir, &fresh, &[], lock);
            }
            res.map_err(RunErr::message)?
        }
        None => score(tools, (a, &pa), (b, &pb), points, None, &mut Vec::new())?,
    };
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

/// [`compare`] with pairs: takes `dir`, clears the last run's pairs and
/// writes the new ones.
fn take_and_pair(
    tools: &Tools,
    a: (&FsPath, &Probe),
    b: (&FsPath, &Probe),
    points: usize,
    dir: &FsPath,
    lock: &mut Option<RunLock>,
) -> Result<Vec<Point>, RunErr> {
    let io = |e: std::io::Error| format!("cannot write {}: {e}", dir.display());
    let created = claim(dir, COMPARE_MARK, None)?;
    // Held by the caller until it has taken back the folder it made.
    *lock = Some(RunLock::take(dir)?);
    // Only the pairs the last run listed under its mark go.
    remove_listed(dir, &listed_pairs(dir)).map_err(io)?;
    std::fs::write(dir.join(COMPARE_MARK), MARK_TEXT).map_err(io)?;
    let mut written = Vec::new();
    let res = score(tools, a, b, points, Some(dir), &mut written);
    if res.is_err() {
        // Each pair was listed under the mark as it was written; a run
        // that fails takes them back, and its mark when it made it.
        let _ = remove_listed(dir, &written);
        let _ = if created {
            std::fs::remove_file(dir.join(COMPARE_MARK))
        } else {
            std::fs::write(dir.join(COMPARE_MARK), MARK_TEXT)
        };
    }
    Ok(res?)
}

/// The points of [`compare`]; records each pair it writes in `written`.
fn score(
    tools: &Tools,
    (a, pa): (&FsPath, &Probe),
    (b, pb): (&FsPath, &Probe),
    points: usize,
    pairs: Option<&FsPath>,
    written: &mut Vec<String>,
) -> Result<Vec<Point>, String> {
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
                let name = format!("{:03}.png", i + 1);
                not_there(dir, &name)?;
                record(dir, COMPARE_MARK, &name)?;
                let png = dir.join(&name);
                written.push(name);
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
    Ok(scored)
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
    refuse_bare_link(&req.video, &video).map_err(ApiErr::bad_request)?;
    let out = match req.out.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
        Some(o) => Some(validate_project_path(o).map_err(|_| {
            ApiErr::bad_request(format!(
                "--out must be a folder inside the project, got {o:?}"
            ))
        })?),
        None => None,
    };
    let base = root.clone();
    let (out, facts) = tokio::task::spawn_blocking(move || {
        let tools = find_tools()?;
        match out {
            Some(out) => build(&tools, &video, &base.join(&out)).map(|f| (out, f)),
            None => build_default(&tools, &video, &base),
        }
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
    fn matroska_duration_tags_read_as_seconds() {
        assert_eq!(clock("00:00:10.000000000"), Some(10.0));
        assert_eq!(clock("01:02:03.5"), Some(3723.5));
        assert_eq!(clock("00:00:00.000000000"), None);
        assert_eq!(clock("10.0"), None);
        assert_eq!(clock("N/A"), None);
    }

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
    fn links_without_a_scheme_are_refused_when_no_such_file_exists() {
        let missing = FsPath::new("/no/such/path");
        for l in [
            "youtube.com/watch?v=dQw4w9WgXcQ",
            "youtu.be/dQw4w9WgXcQ",
            "www.youtube.com/watch?v=x",
            "vimeo.com/123",
            "www.example.org",
            " m.tiktok.com/@a/video/1 ",
            "vimeo.com",
            "youtube.com",
            "YouTu.be",
            " tiktok.com ",
        ] {
            let e = refuse_bare_link(l, missing).unwrap_err();
            assert!(e.contains("is a link") && e.contains("attach"), "{e}");
        }
        for p in [
            "clips/ref.mp4",
            "clip.mov",
            "reference/clip.v2/a.mp4",
            "../a.mp4",
            "/abs/youtube.com/x",
            // A missing film is told as missing, even with an ending that
            // is also a web one.
            "clip.mov",
            "clip.webm",
            "clip.mkv",
            "notes.txt",
            "logo.ai",
            "config.in",
            "libx.so",
        ] {
            assert!(refuse_bare_link(p, missing).is_ok(), "{p}");
        }
        // A real folder of that name is the user's: it is read.
        let tmp = tempfile::tempdir().unwrap();
        assert!(refuse_bare_link("youtu.be/x", tmp.path()).is_ok());
        assert!(refuse_bare_link("vimeo.com", tmp.path()).is_ok());
    }

    #[test]
    fn one_film_has_one_key_however_its_path_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        std::fs::create_dir_all(base.join("a")).unwrap();
        std::fs::write(base.join("a/clip.mp4"), b"x").unwrap();
        // The folder need not exist yet: a run makes it.
        let out = base.join("reference/clip");
        let key = film_key(&base.join("a/clip.mp4"), &out);
        assert_eq!(key, "../../a/clip.mp4");
        assert_eq!(film_key(&base.join("a/../a/./clip.mp4"), &out), key);
        assert_eq!(
            film_key(
                &base.join("a/clip.mp4"),
                &base.join("a/../reference/./clip")
            ),
            key
        );
        // A disk that ignores case: the name as it is stored.
        if base.join("A/CLIP.MP4").exists() {
            assert_eq!(film_key(&base.join("A/CLIP.MP4"), &out), key);
        }
        std::fs::create_dir_all(&out).unwrap();
        assert_eq!(film_key(&base.join("a/clip.mp4"), &out), key);
        // The project moved: the key stays.
        let moved = base.join("moved");
        std::fs::create_dir(&moved).unwrap();
        std::fs::rename(base.join("a"), moved.join("a")).unwrap();
        std::fs::rename(base.join("reference"), moved.join("reference")).unwrap();
        assert_eq!(
            film_key(&moved.join("a/clip.mp4"), &moved.join("reference/clip")),
            key
        );
    }

    #[test]
    fn a_film_is_known_by_its_path_from_the_folder() {
        let out = FsPath::new("/p/reference/clip");
        assert_eq!(
            film_path(FsPath::new("/p/a/clip.mp4"), out),
            "../../a/clip.mp4"
        );
        assert_eq!(film_path(FsPath::new("/p/clip.mp4"), out), "../../clip.mp4");
        assert_eq!(
            film_path(FsPath::new("/p/reference/clip/clip.mp4"), out),
            "./clip.mp4"
        );
        assert_eq!(film_path(FsPath::new("/q.mp4"), out), "../../../q.mp4");
        // Paths are told apart; a name-only mark (beta.17) by the name.
        assert!(same_film("a/clip.mp4", "a/clip.mp4"));
        assert!(!same_film("a/clip.mp4", "b/clip.mp4"));
        assert!(!same_film("./clip.mp4", "a/clip.mp4"));
        assert!(same_film("clip.mp4", "b/clip.mp4"));
        assert!(same_film("clip.mp4", "./clip.mp4"));
        assert!(!same_film("clip.mkv", "b/clip.mp4"));
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
    fn an_image_or_a_file_without_length_is_told_plainly() {
        let image = json!({
            "streams": [{ "codec_type": "video", "codec_name": "png", "width": 64, "height": 64 }],
            "format": { "format_name": "png_pipe" }
        });
        let e = read_probe("anh.png", &image).unwrap_err();
        assert!(
            e.starts_with("anh.png is a still image, not a video"),
            "{e}"
        );
        let no_length = json!({
            "streams": [{ "codec_type": "video", "width": 64, "height": 64 }],
            "format": { "format_name": "mpegts" }
        });
        let e = read_probe("x.ts", &no_length).unwrap_err();
        assert!(
            e.starts_with("x.ts gives no length") && !e.contains("ffmpeg"),
            "{e}"
        );
        let film = json!({
            "streams": [{ "codec_type": "video", "width": 640, "height": 360,
                          "duration": "10.0", "avg_frame_rate": "25/1" }],
            "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10.0" }
        });
        assert_eq!(read_probe("ngang.mp4", &film).unwrap().duration, 10.0);
    }

    #[test]
    fn a_song_with_cover_art_has_no_video_stream() {
        let song = json!({
            "streams": [
                { "codec_type": "audio", "duration": "5.0" },
                { "codec_type": "video", "codec_name": "png", "width": 64, "height": 64,
                  "disposition": { "attached_pic": 1 } }
            ],
            "format": { "format_name": "mp3", "duration": "5.0" }
        });
        let e = read_probe("bai-hat.mp3", &song).unwrap_err();
        assert_eq!(e, "bai-hat.mp3 has no video stream");
    }

    #[test]
    fn only_frame_names_in_their_folders_are_listed_for_removal() {
        let dirs = ["scenes", "grid"];
        assert!(is_frame("scenes/001.png", &dirs) && is_frame("grid/060.png", &dirs));
        for bad in [
            "../001.png",
            "scenes/../../x/001.png",
            "facts.json",
            "scenes/sketch.png",
            "scenes/.png",
            "other/001.png",
            "001.png",
        ] {
            assert!(!is_frame(bad, &dirs), "{bad}");
        }
        assert!(is_frame("004.png", &[""]) && !is_frame("x/004.png", &[""]));

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("facts.json"),
            r#"{"scenes":[{"frame":{"frame":"scenes/001.png"}},{}],
                "grid":[{"frame":"grid/001.png"},{"frame":"../../etc/x.png"}]}"#,
        )
        .unwrap();
        assert_eq!(
            listed_frames(tmp.path()),
            vec!["scenes/001.png", "grid/001.png"]
        );
        assert!(listed_frames(&tmp.path().join("none")).is_empty());
    }

    #[test]
    fn marks_list_what_their_runs_wrote_and_old_marks_all_frames() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("grid")).unwrap();
        for f in ["grid/001.png", "grid/002.png", "001.png", "notes.png"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        std::fs::write(dir.join(REFERENCE_MARK), mark_head(Some("a.mp4"))).unwrap();
        record(dir, REFERENCE_MARK, "grid/001.png").unwrap();
        assert_eq!(listed_frames(dir), ["grid/001.png"]);
        assert_eq!(taken_for(dir).as_deref(), Some("a.mp4"));
        // beta.16 marks list nothing: every NNN.png was theirs.
        std::fs::write(dir.join(REFERENCE_MARK), LEGACY_MARK).unwrap();
        let mut all = listed_frames(dir);
        all.sort();
        assert_eq!(all, ["grid/001.png", "grid/002.png"]);
        std::fs::write(dir.join(COMPARE_MARK), LEGACY_MARK).unwrap();
        assert_eq!(listed_pairs(dir), ["001.png"]);
        std::fs::write(dir.join(COMPARE_MARK), MARK_TEXT).unwrap();
        assert!(listed_pairs(dir).is_empty());
    }

    #[test]
    fn default_dirs_are_safe_project_folders() {
        let d = default_dirs(FsPath::new("/x/My Ref (1).mp4"));
        assert_eq!(d[0], "reference/My-Ref--1");
        assert_eq!(d[1], "reference/My-Ref--1-mp4");
        assert_eq!(d[2], "reference/My-Ref--1-mp4-2");
        assert_eq!(default_dirs(FsPath::new("clip.mov"))[0], "reference/clip");
        let bare = default_dirs(FsPath::new("clip"));
        assert_eq!(bare[..2], ["reference/clip", "reference/clip-2"]);
    }

    #[test]
    fn a_folder_is_taken_for_the_film_its_mark_or_facts_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert_eq!(taken_for(dir), None);
        // A mark from before it named a source: its facts.json does.
        std::fs::write(dir.join(REFERENCE_MARK), MARK_TEXT).unwrap();
        std::fs::write(
            dir.join("facts.json"),
            json!({ "source": "a.mp4" }).to_string(),
        )
        .unwrap();
        assert_eq!(taken_for(dir).as_deref(), Some("a.mp4"));
        std::fs::write(dir.join(REFERENCE_MARK), mark_head(Some("clip.mkv"))).unwrap();
        assert_eq!(taken_for(dir).as_deref(), Some("clip.mkv"));
        // claim: the same film may have it again, another may not.
        assert!(matches!(
            claim(dir, REFERENCE_MARK, Some("clip.mkv")),
            Ok(false)
        ));
        let e = claim(dir, REFERENCE_MARK, Some("clip.mp4")).unwrap_err();
        assert!(
            matches!(&e, RunErr::Taken(m) if m.contains("reference of clip.mkv")),
            "{e:?}"
        );
        // A new folder is made and marked for its film; the mark is in place
        // whole, and no temporary file stays.
        let new = dir.join("reference/clip");
        assert!(matches!(
            claim(&new, REFERENCE_MARK, Some("clip.mp4")),
            Ok(true)
        ));
        assert_eq!(taken_for(&new).as_deref(), Some("clip.mp4"));
        let names: Vec<_> = std::fs::read_dir(&new)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [REFERENCE_MARK]);
    }

    #[test]
    fn a_folder_being_written_is_held_until_its_run_ends() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = RunLock::take(tmp.path()).unwrap();
        // The pid, for a beta.17 daemon that reads it. Read through the
        // holding handle: on Windows the lock keeps other handles from
        // reading (a beta.17 daemon that cannot read it counts it live).
        let mut body = String::new();
        let mut held = lock.file.as_ref().unwrap();
        std::io::Seek::rewind(&mut held).unwrap();
        std::io::Read::read_to_string(&mut held, &mut body).unwrap();
        assert_eq!(body, format!("{}\n", std::process::id()));
        // A second run, in this same process too, is refused.
        let second = RunLock::take(tmp.path()).unwrap_err();
        let want = format!(
            "another run is writing {}; wait for it to finish",
            tmp.path().display()
        );
        assert!(
            matches!(&second, RunErr::Busy(m) if *m == want),
            "{second:?}"
        );
        drop(lock);
        let again = RunLock::take(tmp.path()).unwrap();
        assert!(matches!(RunLock::take(tmp.path()), Err(RunErr::Busy(_))));
        drop(again);
        assert!(!tmp.path().join(RUN_LOCK).exists());
        // A lock only, with no mark: the folder is still free to claim.
        std::fs::write(tmp.path().join(RUN_LOCK), "").unwrap();
        assert!(claim(tmp.path(), COMPARE_MARK, None).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_file_removed_after_it_was_opened_is_not_the_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(RUN_LOCK);
        // Opened by a second run just before the first removed it on release.
        let late = std::fs::File::create(&path).unwrap();
        assert!(still_named(&late, &path));
        std::fs::remove_file(&path).unwrap();
        assert!(!still_named(&late, &path));
        // A third run made a new one there: the second locks that one.
        std::fs::write(&path, "").unwrap();
        assert!(!still_named(&late, &path));
        assert!(RunLock::hold(path.clone(), late).unwrap().is_none());
        assert!(path.is_file(), "the new one was removed");
        let lock = RunLock::take(tmp.path()).unwrap();
        assert!(matches!(RunLock::take(tmp.path()), Err(RunErr::Busy(_))));
        drop(lock);
    }

    #[test]
    fn the_lock_file_of_a_run_that_was_killed_is_taken() {
        let tmp = tempfile::tempdir().unwrap();
        // What a killed run (or an older Contour, with its pid) left: no
        // run holds it.
        std::fs::write(tmp.path().join(RUN_LOCK), "4242\n").unwrap();
        let lock = RunLock::take(tmp.path()).unwrap();
        assert!(matches!(RunLock::take(tmp.path()), Err(RunErr::Busy(_))));
        drop(lock);
        assert!(!tmp.path().join(RUN_LOCK).exists());
    }

    #[test]
    fn files_an_older_lock_left_do_not_make_a_folder_taken() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(format!("{RUN_LOCK}-stale-1-0")), "1\n").unwrap();
        std::fs::write(tmp.path().join(RUN_LOCK), "").unwrap();
        assert!(claim(tmp.path(), REFERENCE_MARK, Some("clip.mp4")).unwrap());
    }

    #[test]
    fn a_failed_run_takes_back_its_new_folders_with_its_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("reference");
        let dir = fresh.join("clip");
        std::fs::create_dir_all(dir.join("scenes")).unwrap();
        std::fs::create_dir_all(dir.join("grid")).unwrap();
        let lock = RunLock::take(&dir).unwrap();
        remove_empty_dirs(&dir, &fresh, &["scenes", "grid"], Some(lock));
        assert!(!fresh.exists());
    }
}
