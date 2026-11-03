//! `od media reference` and `od media compare` on films made here with
//! ffmpeg's lavfi sources: a 10 s clip with two hard cuts and a sine tone.
//! The tests that read video need the ffmpeg (with ffprobe) installed, like
//! the Motion render tests; without it they say so and skip.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_server::routes::w3::reference::find_tools;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const OD: &str = env!("CARGO_BIN_EXE_od");

fn ffmpeg() -> Option<PathBuf> {
    match find_tools() {
        Ok(t) => Some(t.ffmpeg),
        Err(e) => {
            eprintln!("skipping: no ffmpeg/ffprobe on this machine ({e})");
            None
        }
    }
}

fn ff(ffmpeg: &Path, args: &[&str], out: &Path) {
    let res = Command::new(ffmpeg)
        .args(["-hide_banner", "-v", "error", "-nostdin", "-y"])
        .args(args)
        .arg(out)
        .output()
        .unwrap();
    assert!(
        res.status.success(),
        "{}",
        String::from_utf8_lossy(&res.stderr)
    );
}

/// 10 s, 320×240 at 25 fps: test card 0–4 s, fractal 4–7 s, bars 7–10 s,
/// with a 440 Hz tone. Built-in encoders only (mpeg4, aac).
fn film(ffmpeg: &Path, out: &Path) {
    ff(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "mandelbrot=size=320x240:rate=25",
            "-f",
            "lavfi",
            "-i",
            "smptebars=size=320x240:rate=25:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=10",
            "-filter_complex",
            "[1:v]trim=duration=3,setpts=PTS-STARTPTS[m];[0:v][m][2:v]concat=n=3:v=1:a=0[v]",
            "-map",
            "[v]",
            "-map",
            "3:a",
            "-c:v",
            "mpeg4",
            "-q:v",
            "3",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ],
        out,
    );
}

/// Another 10 s film: nothing in common with [`film`].
fn other_film(ffmpeg: &Path, out: &Path) {
    ff(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=10",
            "-c:v",
            "mpeg4",
            "-q:v",
            "3",
            "-pix_fmt",
            "yuv420p",
        ],
        out,
    );
}

fn od(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(OD)
        .args(args)
        .current_dir(cwd)
        .env_remove("OD_PROJECT_ID")
        .env_remove("OD_RUN_ID")
        .env_remove("OD_FFMPEG_PATH")
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn pngs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| std::fs::read(e.path()).unwrap().starts_with(b"\x89PNG"))
        .count()
}

/// Everything the folder must hold for the 10 s [`film`].
fn assert_reference(dir: &Path) {
    let facts: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("facts.json")).unwrap()).unwrap();
    let secs = facts["duration"].as_f64().unwrap();
    assert!((secs - 10.0).abs() <= 0.2, "{facts}");
    assert_eq!(
        (facts["width"].as_u64(), facts["height"].as_u64()),
        (Some(320), Some(240))
    );
    assert!(
        (facts["fps"].as_f64().unwrap() - 25.0).abs() < 0.01,
        "{facts}"
    );
    assert_eq!(facts["hasAudio"], true, "{facts}");
    let cuts: Vec<f64> = facts["sceneCuts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_f64().unwrap())
        .collect();
    assert!(!cuts.is_empty(), "{facts}");
    // The two cuts of the film, found where they are.
    for want in [4.0, 7.0] {
        assert!(
            cuts.iter().any(|c| (c - want).abs() < 0.2),
            "{want}: {cuts:?}"
        );
    }
    let scenes = facts["scenes"].as_array().unwrap().len();
    assert_eq!(scenes, cuts.len() + 1);
    assert_eq!(pngs(&dir.join("scenes")), scenes);
    let grid = facts["grid"].as_array().unwrap().len();
    assert_eq!(grid, 10, "{facts}");
    assert_eq!(pngs(&dir.join("grid")), grid);
    assert!(
        std::fs::read(dir.join("contact-sheet.png"))
            .unwrap()
            .starts_with(b"\x89PNG")
    );
    assert_eq!(facts["audio"], "audio.m4a");
    assert!(std::fs::metadata(dir.join("audio.m4a")).unwrap().len() > 1000);
}

#[test]
fn reference_writes_the_folder_next_to_the_video() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("my ref.mp4"));
    let out = od(tmp.path(), &["media", "reference", "my ref.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    let all = text(&out);
    assert!(all.contains("10.00 s, 320×240, 25 fps, audio"), "{all}");
    assert_reference(&tmp.path().join("reference/my-ref"));

    // Into a chosen new folder, with --json.
    let dir = tmp.path().join("look");
    let out = od(
        tmp.path(),
        &[
            "--json",
            "media",
            "reference",
            "my ref.mp4",
            "--out",
            "look",
        ],
        &[],
    );
    assert!(out.status.success(), "{}", text(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["dir"], "look");
    assert_eq!(v["facts"]["source"], "my ref.mp4");
    assert_reference(&dir);

    // Another film may not take that folder, even with --out.
    other_film(&ffmpeg, &tmp.path().join("other.mp4"));
    let out = od(
        tmp.path(),
        &[
            "media",
            "reference",
            "other.mp4",
            "--out",
            "reference/my-ref",
        ],
        &[],
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("holds the reference of my ref.mp4"),
        "{}",
        text(&out)
    );
    assert_reference(&tmp.path().join("reference/my-ref"));

    // The film cut to 3 s, run again: the 10 s run's later frames are gone.
    ff(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=3",
            "-c:v",
            "mpeg4",
        ],
        &tmp.path().join("my ref.mp4"),
    );
    let out = od(tmp.path(), &["media", "reference", "my ref.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    let grid = tmp.path().join("reference/my-ref/grid");
    assert_eq!(pngs(&grid), 3);
    assert!(grid.join("003.png").is_file() && !grid.join("004.png").exists());
}

#[test]
fn compare_scores_a_film_against_itself_and_another() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a.mp4"), tmp.path().join("b.mp4"));
    film(&ffmpeg, &a);
    other_film(&ffmpeg, &b);
    let score = |x: &str, y: &str| -> Value {
        let out = od(tmp.path(), &["--json", "media", "compare", x, y], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let same = score("a.mp4", "a.mp4");
    let points = same["points"].as_array().unwrap();
    assert!(points.len() >= 5, "{same}");
    for p in points {
        assert!(p["score"].as_f64().unwrap() >= 0.99, "{same}");
    }
    let diff = score("a.mp4", "b.mp4");
    assert_eq!(diff["points"].as_array().unwrap().len(), points.len());
    let mean = diff["mean"].as_f64().unwrap();
    assert!(mean < 0.6, "{diff}");
    assert!(same["mean"].as_f64().unwrap() - mean > 0.3, "{diff}");

    // The table, with --points and the side-by-side pairs.
    let out = od(
        tmp.path(),
        &[
            "media", "compare", "a.mp4", "b.mp4", "--points", "5", "--out", "pairs",
        ],
        &[],
    );
    assert!(out.status.success(), "{}", text(&out));
    let all = text(&out);
    assert!(all.starts_with("point\ttime a\ttime b\tscore\n"), "{all}");
    // Paths as this platform writes them (`pairs\001.png` on Windows).
    for i in 1..=5 {
        let pair = Path::new("pairs").join(format!("{i:03}.png"));
        let row = all.lines().find(|l| l.starts_with(&format!("{i}\t")));
        assert!(
            row.is_some_and(|l| l.ends_with(&format!("\t{}", pair.display()))),
            "{all}"
        );
    }
    assert!(all.contains("mean\t"), "{all}");
    assert_eq!(pngs(&tmp.path().join("pairs")), 5);
}

#[test]
fn a_portrait_film_is_read_as_it_plays() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let flat = tmp.path().join("flat.mp4");
    other_film(&ffmpeg, &flat);
    // Stored 320×240, shown turned a quarter: a phone's portrait clip.
    ff(
        &ffmpeg,
        &[
            "-display_rotation",
            "90",
            "-i",
            flat.to_str().unwrap(),
            "-c",
            "copy",
        ],
        &tmp.path().join("rot.mp4"),
    );
    let out = od(
        tmp.path(),
        &["--json", "media", "reference", "rot.mp4"],
        &[],
    );
    assert!(out.status.success(), "{}", text(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let f = &v["facts"];
    assert_eq!(
        (f["width"].as_u64(), f["height"].as_u64()),
        (Some(240), Some(320)),
        "{f}"
    );
    // The frames are turned the same way.
    let png = std::fs::read(tmp.path().join("reference/rot/grid/001.png")).unwrap();
    let (w, h) = (
        u32::from_be_bytes(png[16..20].try_into().unwrap()),
        u32::from_be_bytes(png[20..24].try_into().unwrap()),
    );
    assert_eq!((w, h), (240, 320));
}

#[test]
fn sound_that_outlasts_the_picture_does_not_stretch_the_film() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    // 10 s of picture, 13 s of tone: the container says 13 s. Matroska
    // keeps the picture's length in a tag, not in the stream's duration.
    for (name, stem) in [("av.mp4", "av"), ("avk.mkv", "avk")] {
        let file = tmp.path().join(name);
        ff(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=13",
                "-c:v",
                "mpeg4",
                "-q:v",
                "3",
                "-c:a",
                "aac",
            ],
            &file,
        );
        let out = od(
            tmp.path(),
            &[
                "--json",
                "media",
                "reference",
                file.file_name().unwrap().to_str().unwrap(),
            ],
            &[],
        );
        assert!(out.status.success(), "{}", text(&out));
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        let f = &v["facts"];
        assert!((f["duration"].as_f64().unwrap() - 10.0).abs() <= 0.2, "{f}");
        assert_eq!(f["grid"].as_array().unwrap().len(), 10, "{f}");
        assert_eq!(pngs(&tmp.path().join(format!("reference/{stem}/grid"))), 10);
        let out = od(
            tmp.path(),
            &[
                "--json",
                "media",
                "compare",
                file.to_str().unwrap(),
                file.to_str().unwrap(),
            ],
            &[],
        );
        assert!(out.status.success(), "{}", text(&out));
        let c: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(c["min"].as_f64().unwrap() >= 0.99, "{name}: {c}");
    }

    // The 180 s limit is the picture's: 170 s of film under 190 s of sound.
    for name in ["long-sound.mp4", "long-sound.mkv"] {
        ff(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:size=32x32:rate=1:duration=170",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=190",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ],
            &tmp.path().join(name),
        );
        let out = od(
            tmp.path(),
            &["media", "compare", name, name, "--points", "5"],
            &[],
        );
        assert!(out.status.success(), "{name}: {}", text(&out));
    }
}

#[test]
fn out_never_removes_or_replaces_files_it_did_not_write() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    other_film(&ffmpeg, &tmp.path().join("a.mp4"));
    let mine = tmp.path().join("042.png");
    std::fs::write(&mine, b"the user's").unwrap();
    // compare --out . : the folder holds the user's files.
    let out = od(
        tmp.path(),
        &[
            "media", "compare", "a.mp4", "a.mp4", "--points", "2", "--out", ".",
        ],
        &[],
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("did not write"), "{}", text(&out));
    assert_eq!(std::fs::read(&mine).unwrap(), b"the user's");
    assert!(!tmp.path().join("001.png").exists());

    // reference --out into a folder of the user's frames.
    let frames = tmp.path().join("frames");
    std::fs::create_dir_all(frames.join("grid")).unwrap();
    std::fs::write(frames.join("grid/042.png"), b"the user's").unwrap();
    let out = od(
        tmp.path(),
        &["media", "reference", "a.mp4", "--out", "frames"],
        &[],
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("did not write"), "{}", text(&out));
    assert_eq!(
        std::fs::read(frames.join("grid/042.png")).unwrap(),
        b"the user's"
    );
    assert!(!frames.join("facts.json").exists());

    // Its own folder, run twice: the second run clears the first's pairs.
    for points in ["4", "2"] {
        let out = od(
            tmp.path(),
            &[
                "media", "compare", "a.mp4", "a.mp4", "--points", points, "--out", "pairs",
            ],
            &[],
        );
        assert!(out.status.success(), "{}", text(&out));
    }
    assert_eq!(pngs(&tmp.path().join("pairs")), 2);
}

#[test]
fn a_song_with_cover_art_is_not_a_video_and_leaves_no_folder() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    ff(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=1:duration=1",
            "-frames:v",
            "1",
        ],
        &tmp.path().join("cover.png"),
    );
    ff(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=500:duration=5",
            "-i",
            tmp.path().join("cover.png").to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-c:a",
            "aac",
            "-c:v",
            "png",
            "-disposition:v",
            "attached_pic",
        ],
        &tmp.path().join("song.m4a"),
    );
    let out = od(tmp.path(), &["media", "reference", "song.m4a"], &[]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("song.m4a has no video stream"),
        "{}",
        text(&out)
    );
    assert!(!tmp.path().join("reference").exists());

    // A run that fails after taking a new folder leaves none behind.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        film(&ffmpeg, &tmp.path().join("clip.mp4"));
        let tools = tmp.path().join("broken-tools");
        std::fs::create_dir_all(&tools).unwrap();
        let fake = tools.join("ffmpeg");
        std::fs::write(&fake, "#!/bin/sh\necho broken >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(ffmpeg.with_file_name("ffprobe"), tools.join("ffprobe"))
            .unwrap();
        let out = od(
            tmp.path(),
            &["media", "reference", "clip.mp4"],
            &[("OD_FFMPEG_PATH", fake.to_str().unwrap())],
        );
        assert!(!out.status.success(), "{}", text(&out));
        assert!(!tmp.path().join("reference").exists());
    }
}

/// An `ffmpeg` in `dir` that runs `script` (sh) with the real ffprobe beside
/// it; `$REAL` is the real ffmpeg.
#[cfg(unix)]
fn fake_ffmpeg(dir: &Path, real: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let fake = dir.join("ffmpeg");
    let body = format!("#!/bin/sh\nREAL='{}'\n{script}\n", real.display());
    std::fs::write(&fake, body).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(real.with_file_name("ffprobe"), dir.join("ffprobe")).unwrap();
    fake
}

#[cfg(unix)]
#[test]
fn a_failed_run_removes_only_what_it_wrote() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    let fake = fake_ffmpeg(
        &tmp.path().join("slow-broken"),
        &ffmpeg,
        "sleep 3\necho broken >&2\nexit 1",
    );
    // It takes reference/clip (and reference/), then fails 3 s later;
    // meanwhile another file lands in reference/.
    let run = Command::new(OD)
        .args(["media", "reference", "clip.mp4"])
        .current_dir(tmp.path())
        .env_remove("OD_PROJECT_ID")
        .env_remove("OD_RUN_ID")
        .env("OD_FFMPEG_PATH", &fake)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mark = tmp.path().join("reference/clip/.od-reference");
    let start = Instant::now();
    while !mark.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(mark.exists(), "the run never took reference/clip");
    let keep = tmp.path().join("reference/other/keep.txt");
    std::fs::create_dir_all(keep.parent().unwrap()).unwrap();
    std::fs::write(&keep, b"another run's").unwrap();
    let out = run.wait_with_output().unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(std::fs::read(&keep).unwrap(), b"another run's");
    assert!(!tmp.path().join("reference/clip").exists());
}

#[cfg(unix)]
#[test]
fn a_run_killed_half_way_is_cleared_by_the_next() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    // Kills `od` (its parent) when asked for grid/002.png or pair 003.png.
    let fake = fake_ffmpeg(
        &tmp.path().join("killer"),
        &ffmpeg,
        "case \"$*\" in *grid/002.png*|*/003.png*) kill -9 $PPID; exit 1;; esac\nexec \"$REAL\" \"$@\"",
    );
    let killer = [("OD_FFMPEG_PATH", fake.to_str().unwrap())];
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &killer);
    assert!(!out.status.success(), "{}", text(&out));
    let dir = tmp.path().join("reference/clip");
    assert!(dir.join("scenes/001.png").is_file() && !dir.join("facts.json").exists());
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert_reference(&dir);

    let pairs = [
        "media", "compare", "clip.mp4", "clip.mp4", "--points", "4", "--out", "pairs",
    ];
    let out = od(tmp.path(), &pairs, &killer);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(tmp.path().join("pairs/001.png").is_file());
    // Run again with 2 points: the killed run's 001 and 002 are cleared.
    let mut again = pairs;
    again[5] = "2";
    let out = od(tmp.path(), &again, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(pngs(&tmp.path().join("pairs")), 2);
}

#[cfg(unix)]
#[test]
fn a_rerun_that_fails_takes_back_its_frames_in_a_folder_it_had() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    let dir = tmp.path().join("reference/clip");
    std::fs::write(dir.join("beats.md"), b"the user's").unwrap();
    // Frames are written, then the contact sheet fails.
    let fake = fake_ffmpeg(
        &tmp.path().join("no-sheet"),
        &ffmpeg,
        "case \"$*\" in *contact-sheet*|*pairs/002.png*) echo broken >&2; exit 1;; esac\nexec \"$REAL\" \"$@\"",
    );
    let out = od(
        tmp.path(),
        &["media", "reference", "clip.mp4"],
        &[("OD_FFMPEG_PATH", fake.to_str().unwrap())],
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("broken"), "{}", text(&out));
    for sub in ["scenes", "grid"] {
        assert_eq!(
            pngs(&dir.join(sub)),
            0,
            "{sub} keeps the failed run's frames"
        );
    }
    assert!(!dir.join("facts.json").exists());
    assert!(dir.join(".od-reference").is_file());
    assert_eq!(std::fs::read(dir.join("beats.md")).unwrap(), b"the user's");
    // The next run takes the folder again.
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert_reference(&dir);

    // compare --out, run once, then failing at its second pair.
    let mut args = [
        "media", "compare", "clip.mp4", "clip.mp4", "--points", "4", "--out", "pairs",
    ];
    let out = od(tmp.path(), &args, &[]);
    assert!(out.status.success(), "{}", text(&out));
    args[5] = "3";
    let out = od(
        tmp.path(),
        &args,
        &[("OD_FFMPEG_PATH", fake.to_str().unwrap())],
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(
        pngs(&tmp.path().join("pairs")),
        0,
        "the failed run's pairs stay"
    );
    let out = od(tmp.path(), &args, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(pngs(&tmp.path().join("pairs")), 3);
}

#[test]
fn a_file_that_is_not_a_video_is_told_plainly() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.mp4"), "just notes").unwrap();
    std::fs::write(tmp.path().join("brief.pdf"), "%PDF-1.4\n% not really\n").unwrap();
    ff(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=1:duration=1",
            "-frames:v",
            "1",
        ],
        &tmp.path().join("still.png"),
    );
    for (file, says) in [
        ("notes.mp4", "notes.mp4 is not a video"),
        ("brief.pdf", "brief.pdf is not a video"),
        ("still.png", "still.png is a still image, not a video"),
    ] {
        let out = od(tmp.path(), &["media", "reference", file], &[]);
        assert!(!out.status.success(), "{file}");
        let all = text(&out);
        assert!(all.contains(says), "{all}");
        for noise in ["ffprobe", "Invalid data", "moov atom", "file:", "ffmpeg -i"] {
            assert!(!all.contains(noise), "{noise}: {all}");
        }
    }
    assert!(!tmp.path().join("reference").exists());
}

#[test]
fn two_films_with_one_name_each_get_their_own_folder() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    other_film(&ffmpeg, &tmp.path().join("clip.mkv"));
    for v in ["clip.mp4", "clip.mkv", "clip.mp4"] {
        let out = od(tmp.path(), &["media", "reference", v], &[]);
        assert!(out.status.success(), "{}", text(&out));
    }
    let source = |dir: &str| {
        let f = std::fs::read_to_string(tmp.path().join(dir).join("facts.json")).unwrap();
        serde_json::from_str::<Value>(&f).unwrap()["source"].clone()
    };
    assert_eq!(source("reference/clip"), "clip.mp4");
    assert_eq!(source("reference/clip-mkv"), "clip.mkv");
    assert_reference(&tmp.path().join("reference/clip"));
}

#[test]
fn two_films_with_one_name_run_at_once_each_get_their_own_folder() {
    let Some(ffmpeg) = ffmpeg() else { return };
    for round in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        film(&ffmpeg, &tmp.path().join("clip.mp4"));
        other_film(&ffmpeg, &tmp.path().join("clip.mkv"));
        let runs: Vec<Child> = ["clip.mp4", "clip.mkv"]
            .into_iter()
            .map(|v| {
                Command::new(OD)
                    .args(["media", "reference", v])
                    .current_dir(tmp.path())
                    .env_remove("OD_PROJECT_ID")
                    .env_remove("OD_RUN_ID")
                    .env_remove("OD_FFMPEG_PATH")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let mut printed = Vec::new();
        for r in runs {
            let out = r.wait_with_output().unwrap();
            assert!(out.status.success(), "round {round}: {}", text(&out));
            printed.push(text(&out).lines().next().unwrap_or("").to_owned());
        }
        let mut sources = Vec::new();
        for e in std::fs::read_dir(tmp.path().join("reference")).unwrap() {
            let f = std::fs::read_to_string(e.unwrap().path().join("facts.json")).unwrap();
            sources.push(serde_json::from_str::<Value>(&f).unwrap()["source"].clone());
        }
        sources.sort_by_key(ToString::to_string);
        assert_eq!(
            sources,
            ["clip.mkv", "clip.mp4"],
            "round {round}: {printed:?}"
        );
        // Each run printed the folder that holds its own film.
        assert_ne!(printed[0].split(':').next(), printed[1].split(':').next());
    }
}

#[test]
fn a_rerun_with_more_frames_never_writes_over_the_users() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let short = |path: &Path| {
        ff(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=25:duration=3",
                "-c:v",
                "mpeg4",
            ],
            path,
        );
    };
    short(&tmp.path().join("clip.mp4"));
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    let dir = tmp.path().join("reference/clip");
    std::fs::write(dir.join("grid/005.png"), b"the user's").unwrap();
    // The same film, now 10 s: its grid would need 005.png.
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("did not write"), "{}", text(&out));
    assert_eq!(
        std::fs::read(dir.join("grid/005.png")).unwrap(),
        b"the user's"
    );
    // What the failed run wrote is gone; the user's frame is all there is.
    let count = |sub: &str| std::fs::read_dir(dir.join(sub)).unwrap().count();
    assert_eq!((count("grid"), count("scenes")), (1, 0));

    // compare --out: 2 pairs, then the user's 003.png, then 4 pairs.
    for (points, ok) in [("2", true), ("4", false)] {
        if !ok {
            std::fs::write(tmp.path().join("pairs/003.png"), b"the user's").unwrap();
        }
        let out = od(
            tmp.path(),
            &[
                "media", "compare", "clip.mp4", "clip.mp4", "--points", points, "--out", "pairs",
            ],
            &[],
        );
        assert_eq!(out.status.success(), ok, "{}", text(&out));
    }
    assert_eq!(
        std::fs::read(tmp.path().join("pairs/003.png")).unwrap(),
        b"the user's"
    );
    assert!(!tmp.path().join("pairs/001.png").exists());
}

#[test]
fn a_rerun_keeps_frames_the_user_saved_under_a_frame_name() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    let dir = tmp.path().join("reference/clip");
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    std::fs::write(dir.join("scenes/010.png"), b"the user's").unwrap();
    std::fs::write(dir.join("grid/099.png"), b"the user's").unwrap();
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    for f in ["scenes/010.png", "grid/099.png"] {
        assert_eq!(std::fs::read(dir.join(f)).unwrap(), b"the user's", "{f}");
    }

    // compare --out: a rerun clears its own pairs, not the user's 099.png.
    let pairs = tmp.path().join("pairs");
    for (points, mine) in [("4", false), ("2", true)] {
        if mine {
            std::fs::write(pairs.join("099.png"), b"the user's").unwrap();
        }
        let out = od(
            tmp.path(),
            &[
                "media", "compare", "clip.mp4", "clip.mp4", "--points", points, "--out", "pairs",
            ],
            &[],
        );
        assert!(out.status.success(), "{}", text(&out));
    }
    assert_eq!(std::fs::read(pairs.join("099.png")).unwrap(), b"the user's");
    assert!(pairs.join("002.png").is_file() && !pairs.join("003.png").exists());
}

#[test]
fn a_rerun_leaves_no_facts_or_sound_of_the_last_film() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    film(&ffmpeg, &tmp.path().join("clip.mp4"));
    let dir = tmp.path().join("reference/clip");
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(dir.join("audio.m4a").is_file());

    // Same name, now silent: the old sound is gone.
    other_film(&ffmpeg, &tmp.path().join("clip.mp4"));
    let out = od(tmp.path(), &["media", "reference", "clip.mp4"], &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!dir.join("audio.m4a").exists());
    let facts: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("facts.json")).unwrap()).unwrap();
    assert_eq!(facts["hasAudio"], false);
    assert!(facts.get("audio").is_none(), "{facts}");

    // A run that fails half way (ffprobe reads it, ffmpeg breaks) leaves
    // no facts.json describing frames that are gone.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tools = tmp.path().join("broken-tools");
        std::fs::create_dir_all(&tools).unwrap();
        let fake = tools.join("ffmpeg");
        std::fs::write(&fake, "#!/bin/sh\necho broken >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(ffmpeg.with_file_name("ffprobe"), tools.join("ffprobe"))
            .unwrap();
        let out = od(
            tmp.path(),
            &["media", "reference", "clip.mp4"],
            &[("OD_FFMPEG_PATH", fake.to_str().unwrap())],
        );
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("broken"), "{}", text(&out));
        for f in ["facts.json", "contact-sheet.png", "audio.m4a"] {
            assert!(!dir.join(f).exists(), "{f} left behind");
        }
    }
}

#[test]
fn links_are_refused_before_anything_runs() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        vec!["media", "reference", "https://example.com/film.mp4"],
        vec![
            "media",
            "reference",
            "http://example.com/a.mp4",
            "--project",
            "p1",
        ],
        vec!["media", "compare", "a.mp4", "https://example.com/b.mp4"],
        // Pasted without the scheme, as share sheets copy them.
        vec!["media", "reference", "youtube.com/watch?v=dQw4w9WgXcQ"],
        vec!["media", "reference", "www.youtube.com/watch?v=x"],
        vec!["media", "compare", "youtu.be/dQw4w9WgXcQ", "a.mp4"],
    ] {
        // No daemon at this url: the refusal comes first.
        let out = od(
            tmp.path(),
            &args,
            &[("OD_DAEMON_URL", "http://127.0.0.1:0")],
        );
        assert!(!out.status.success(), "{args:?}");
        let all = text(&out);
        assert!(all.contains("is a link"), "{all}");
        assert!(
            all.contains("does not download") && all.contains("attach"),
            "{all}"
        );
    }
    assert!(!tmp.path().join("reference").exists());
}

#[test]
fn too_large_or_too_long_videos_are_refused() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let tmp = tempfile::tempdir().unwrap();
    // Sparse: 201 MB on paper, nothing on disk.
    let big = std::fs::File::create(tmp.path().join("big.mp4")).unwrap();
    big.set_len(201 * 1024 * 1024).unwrap();
    let out = od(tmp.path(), &["media", "reference", "big.mp4"], &[]);
    assert!(!out.status.success());
    assert!(text(&out).contains("at most 200 MB"), "{}", text(&out));

    ff(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=gray:size=32x32:rate=1:duration=181",
            "-c:v",
            "mpeg4",
        ],
        &tmp.path().join("long.mp4"),
    );
    let out = od(tmp.path(), &["media", "reference", "long.mp4"], &[]);
    assert!(!out.status.success());
    let all = text(&out);
    assert!(
        all.contains("181.0 s long") && all.contains("at most 180 s"),
        "{all}"
    );
    // Either side: the render (b) is checked as the reference (a) is.
    ff(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=gray:size=32x32:rate=1:duration=2",
            "-c:v",
            "mpeg4",
        ],
        &tmp.path().join("short.mp4"),
    );
    let out = od(
        tmp.path(),
        &["media", "compare", "short.mp4", "long.mp4"],
        &[],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("at most 180 s"), "{}", text(&out));
    assert!(!tmp.path().join("reference").exists());
}

#[test]
fn without_ffmpeg_it_says_how_to_install_it() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("clip.mp4"), b"x").unwrap();
    let env = [("OD_FFMPEG_PATH", "/nonexistent/ffmpeg")];
    for args in [
        vec!["media", "reference", "clip.mp4"],
        vec!["media", "compare", "clip.mp4", "clip.mp4"],
    ] {
        let out = od(tmp.path(), &args, &env);
        assert!(!out.status.success());
        let all = text(&out);
        assert!(all.contains("ffmpeg required"), "{all}");
        assert!(
            all.contains("brew install ffmpeg") && all.contains("OD_FFMPEG_PATH"),
            "{all}"
        );
    }
}

// ---------------------------------------------------------------------------
// Through the daemon (`--project`, as an agent calls it)

struct Daemon {
    child: Child,
    url: String,
    data: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[path = "support/lock_url.rs"]
mod lock_url;

fn start(env: &[(&str, &str)]) -> Daemon {
    for _ in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let mut child = Command::new(OD)
            .args(["daemon", "start", "--port", "0"])
            .env("OD_DATA_DIR", &data)
            .env("OD_RESOURCE_ROOT", tmp.path().join("resources"))
            .env("RUST_LOG", "warn")
            .env_remove("OD_LOG_FILE")
            .env_remove("OD_FFMPEG_PATH")
            .envs(env.iter().copied())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let Some(url) = lock_url::wait(&data, &mut child) else {
            continue;
        };
        let mut d = Daemon {
            child,
            url,
            data,
            _tmp: tmp,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if client(&d, &["daemon", "status"]).status.success() {
                return d;
            }
            if d.child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    panic!("daemon did not start");
}

/// `od` against the daemon, from a folder that is not the project.
fn client(d: &Daemon, args: &[&str]) -> Output {
    let elsewhere = std::env::temp_dir();
    od(&elsewhere, args, &[("OD_DAEMON_URL", d.url.as_str())])
}

fn project(d: &Daemon) -> (String, PathBuf) {
    let out = client(
        d,
        &[
            "--json", "project", "create", "--name", "Film", "--kind", "video",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["project"]["id"].as_str().unwrap().to_owned();
    let dir = d.data.join("projects").join(&id);
    (id, dir)
}

#[test]
fn with_a_project_the_folder_lands_in_the_project() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let d = start(&[]);
    let (pid, dir) = project(&d);
    std::fs::create_dir_all(dir.join("uploads")).unwrap();
    film(&ffmpeg, &dir.join("uploads/ref.mp4"));
    let out = client(
        &d,
        &["media", "reference", "uploads/ref.mp4", "--project", &pid],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_reference(&dir.join("reference/ref"));
    // The project lists what was written.
    let out = client(&d, &["--json", "files", "list", &pid]);
    assert!(
        text(&out).contains("reference/ref/facts.json"),
        "{}",
        text(&out)
    );

    // --out must stay in the project; a link and an oversized file say why.
    let out = client(
        &d,
        &[
            "media",
            "reference",
            "uploads/ref.mp4",
            "--project",
            &pid,
            "--out",
            "../x",
        ],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("inside the project"), "{}", text(&out));
    let big = std::fs::File::create(dir.join("uploads/big.mp4")).unwrap();
    big.set_len(201 * 1024 * 1024).unwrap();
    let out = client(
        &d,
        &["media", "reference", "uploads/big.mp4", "--project", &pid],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("at most 200 MB"), "{}", text(&out));

    // A link pasted without https:// is a link, not a missing project file.
    let out = client(
        &d,
        &[
            "media",
            "reference",
            "youtu.be/dQw4w9WgXcQ",
            "--project",
            &pid,
        ],
    );
    assert!(!out.status.success());
    let all = text(&out);
    assert!(
        all.contains("is a link") && all.contains("does not download") && !all.contains("os error"),
        "{all}"
    );
}

#[test]
fn with_a_project_and_no_ffmpeg_the_daemon_says_how_to_install_it() {
    let d = start(&[("OD_FFMPEG_PATH", "/nonexistent/ffmpeg")]);
    let (pid, dir) = project(&d);
    std::fs::write(dir.join("clip.mp4"), b"x").unwrap();
    let out = client(&d, &["media", "reference", "clip.mp4", "--project", &pid]);
    assert!(!out.status.success());
    let all = text(&out);
    assert!(
        all.contains("ffmpeg required") && all.contains("brew install ffmpeg"),
        "{all}"
    );
}
