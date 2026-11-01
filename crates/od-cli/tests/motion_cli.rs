//! Motion through the CLI an agent calls: `"$OD_NODE_BIN" "$OD_BIN" media
//! scaffold|generate|wait` against the real `od` daemon, started as a
//! process so each test gives it its own `OD_FFMPEG_PATH` and
//! `OD_CHROME_PATH`. The render test needs the Chrome and ffmpeg (with
//! ffprobe) installed, like the MP4 export tests (`OD_SKIP_CHROME_TESTS=1`
//! skips it).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use serde_json::Value;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const OD: &str = env!("CARGO_BIN_EXE_od");

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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(env: &[(&str, &str)]) -> Daemon {
    // The free port can be taken before the daemon binds it: retry.
    for _ in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let port = free_port();
        let child = Command::new(OD)
            .args(["daemon", "start", "--port", &port.to_string()])
            .env("OD_DATA_DIR", &data)
            .env("OD_RESOURCE_ROOT", tmp.path().join("resources"))
            .env("RUST_LOG", "warn")
            .env_remove("OD_LOG_FILE")
            .env_remove("OD_FFMPEG_PATH")
            .env_remove("OD_CHROME_PATH")
            .envs(env.iter().copied())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut d = Daemon {
            child,
            url: format!("http://127.0.0.1:{port}"),
            data,
            _tmp: tmp,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if od(&d, &["daemon", "status"]).status.success() {
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

/// `"$OD_NODE_BIN" "$OD_BIN" args…` as the skill writes it: the binary
/// gets its own path as first argument. `OD_DAEMON_URL` as an agent has it.
fn od(d: &Daemon, args: &[&str]) -> Output {
    Command::new(OD)
        .arg(OD)
        .args(args)
        .env("OD_DAEMON_URL", &d.url)
        .env_remove("OD_RUN_ID")
        .env_remove("OD_PROJECT_ID")
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

fn project(d: &Daemon) -> String {
    let out = od(
        d,
        &[
            "--json", "project", "create", "--name", "Film", "--kind", "video",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    v["project"]["id"].as_str().unwrap().to_owned()
}

fn project_dir(d: &Daemon, id: &str) -> PathBuf {
    d.data.join("projects").join(id)
}

fn scaffold(d: &Daemon, pid: &str) -> Output {
    od(
        d,
        &[
            "media",
            "scaffold",
            "--project",
            pid,
            "--composition-dir",
            "motion-source/film",
            "--width",
            "320",
            "--height",
            "180",
            "--duration",
            "2",
        ],
    )
}

fn generate(d: &Daemon, pid: &str) -> Output {
    od(
        d,
        &[
            "media",
            "generate",
            "--project",
            pid,
            "--surface",
            "video",
            "--model",
            "hyperframes-html",
            "--composition-dir",
            "motion-source/film",
            "--output",
            "film.mp4",
            "--timeout",
            "240",
        ],
    )
}

fn ffprobe(mp4: &Path) -> Value {
    let ffmpeg = od_render_cdp::ffmpeg::find_ffmpeg().expect("ffmpeg is required");
    let out = Command::new(ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }))
    .args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-count_frames",
        "-show_entries",
        "stream=codec_name,pix_fmt,r_frame_rate,nb_read_frames:format=duration",
        "-of",
        "json",
    ])
    .arg(mp4)
    .output()
    .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_agent_calling_form_reaches_od() {
    let d = start(&[]);
    // `od <od> …` drops the repeated path.
    let out = od(&d, &["media", "models"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("video\thyperframes-html\thyperframes"),
        "{}",
        text(&out)
    );
}

#[test]
fn scaffold_never_overwrites_a_source() {
    let d = start(&[]);
    let pid = project(&d);
    let out = scaffold(&d, &pid);
    assert!(out.status.success(), "{}", text(&out));
    let dir = project_dir(&d, &pid).join("motion-source/film");
    for f in ["index.html", "hyperframes.json", "meta.json"] {
        assert!(dir.join(f).is_file(), "{f} missing");
    }
    let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
    assert!(html.contains(r#"data-width="320""#) && html.contains(r#"data-duration="2""#));
    assert!(html.contains("window.__timelines.main"));

    std::fs::write(dir.join("index.html"), "my film").unwrap();
    std::fs::remove_file(dir.join("meta.json")).unwrap();
    let out = scaffold(&d, &pid);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("nothing was written"), "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(dir.join("index.html")).unwrap(),
        "my film"
    );
    assert!(!dir.join("meta.json").exists());
}

#[test]
fn without_ffmpeg_generate_says_to_install_it() {
    let d = start(&[("OD_FFMPEG_PATH", "/nonexistent/ffmpeg")]);
    let pid = project(&d);
    assert!(scaffold(&d, &pid).status.success());
    let out = generate(&d, &pid);
    assert!(!out.status.success(), "{}", text(&out));
    let all = text(&out);
    assert!(all.contains("ffmpeg required"), "{all}");
    assert!(all.contains("brew install ffmpeg"), "{all}");
    assert!(all.contains("OD_FFMPEG_PATH"), "{all}");
    assert!(!project_dir(&d, &pid).join("film.mp4").exists());
}

#[test]
fn without_chrome_generate_says_chrome_required() {
    // An existing file passes the ffmpeg lookup; the render stops at Chrome.
    let d = start(&[
        ("OD_FFMPEG_PATH", OD),
        ("OD_CHROME_PATH", "/nonexistent/chrome"),
    ]);
    let pid = project(&d);
    assert!(scaffold(&d, &pid).status.success());
    let out = generate(&d, &pid);
    assert!(!out.status.success(), "{}", text(&out));
    let all = text(&out);
    assert!(all.contains("Chrome required"), "{all}");
    assert!(all.contains("OD_CHROME_PATH"), "{all}");
    assert!(!project_dir(&d, &pid).join("film.mp4").exists());
}

#[test]
fn a_broken_composition_names_the_missing_field() {
    let d = start(&[]);
    let pid = project(&d);
    assert!(scaffold(&d, &pid).status.success());
    let index = project_dir(&d, &pid).join("motion-source/film/index.html");
    let html = std::fs::read_to_string(&index).unwrap();
    std::fs::write(&index, html.replace(r#" data-duration="2""#, "")).unwrap();
    let out = generate(&d, &pid);
    assert!(!out.status.success());
    assert!(text(&out).contains("data-duration"), "{}", text(&out));

    std::fs::write(&index, html.replace("window.__timelines", "window.other")).unwrap();
    let out = generate(&d, &pid);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("window.__timelines.main"),
        "{}",
        text(&out)
    );
    assert!(!project_dir(&d, &pid).join("film.mp4").exists());

    // Not a composition folder at all.
    let out = od(
        &d,
        &[
            "media",
            "generate",
            "--project",
            &pid,
            "--surface",
            "video",
            "--model",
            "hyperframes-html",
            "--composition-dir",
            "nowhere",
        ],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("media scaffold"), "{}", text(&out));
}

#[test]
fn scaffold_then_generate_renders_the_film() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let d = start(&[]);
    let pid = project(&d);
    assert!(scaffold(&d, &pid).status.success());
    let out = generate(&d, &pid);
    assert!(out.status.success(), "{}", text(&out));
    let all = text(&out);
    assert!(all.contains("\tdone\tfilm.mp4"), "{all}");
    assert!(all.contains("h264 yuv420p"), "{all}");
    let frames: Vec<&str> = all
        .lines()
        .filter_map(|l| l.strip_prefix("review frame\t"))
        .collect();
    assert_eq!(frames.len(), 3, "{all}");
    for f in &frames {
        assert!(std::fs::read(f).unwrap().starts_with(b"\x89PNG"), "{f}");
    }
    let mp4 = project_dir(&d, &pid).join("film.mp4");
    let probe = ffprobe(&mp4);
    let s = &probe["streams"][0];
    assert_eq!(s["codec_name"], "h264", "{probe}");
    assert_eq!(s["pix_fmt"], "yuv420p", "{probe}");
    assert_eq!(s["r_frame_rate"], "30/1", "{probe}");
    assert_eq!(s["nb_read_frames"], "60", "{probe}");
    let secs: f64 = probe["format"]["duration"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((secs - 2.0).abs() < 0.05, "{probe}");

    // Re-render into the same output: replaced, reported as an edit.
    let before = std::fs::metadata(&mp4).unwrap().modified().unwrap();
    let out = od(
        &d,
        &[
            "--json",
            "media",
            "generate",
            "--project",
            &pid,
            "--surface",
            "video",
            "--model",
            "hyperframes-html",
            "--composition-dir",
            "motion-source/film",
            "--output",
            "film.mp4",
            "--fps",
            "15",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    let task: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(task["result"]["edited"], true, "{task}");
    assert!(std::fs::metadata(&mp4).unwrap().modified().unwrap() >= before);
    assert_eq!(ffprobe(&mp4)["streams"][0]["nb_read_frames"], "30");

    // The scaffold declares a video film: the MP4 is the project's entry.
    let out = od(&d, &["--json", "project", "info", &pid]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["project"]["metadata"]["entryFile"], "film.mp4", "{v}");
}
