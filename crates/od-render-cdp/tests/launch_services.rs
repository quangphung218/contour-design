//! macOS: a browser inside an `.app` bundle starts through LaunchServices,
//! not as our child, so macOS does not charge what it (and Google's
//! updater) does to Contour: that asked for App Management in Contour's
//! name. The "browsers" are scripts inside throwaway bundles that record
//! their arguments and parent pid; most then run the real Chrome.
//!
//! Where `ps` cannot run (inside `sandbox-exec`, how the maintainer's
//! pre-merge check runs `make check`) a browser that is not our child could
//! be neither found nor ended, so it starts as our child there: these tests
//! then check that fallback instead (parent pid = this test).
//! Needs Chrome like `chrome.rs`; `OD_SKIP_CHROME_TESTS=1` skips it.

#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::{CdpRenderer, NO_SANDBOX_ENV, find_chrome};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

fn skip() -> bool {
    std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1")
}

/// Whether a bundled browser goes through LaunchServices here: only where
/// `ps` runs (it does not inside `sandbox-exec`, being setuid root).
fn launch_services() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::process::Command::new("ps")
            .args(["-p", "1", "-o", "pid="])
            .output()
            .is_ok_and(|o| o.status.success());
        if !on {
            eprintln!("no LaunchServices (`ps` cannot run): checking the fallback instead");
        }
        on
    })
}

/// The parent pid a bundled browser records: launchd, or this test on the
/// fallback.
fn bundle_ppid() -> String {
    if launch_services() {
        "ppid=1".to_owned()
    } else {
        format!("ppid={}", std::process::id())
    }
}

/// Kills, when dropped, whatever still runs `exe` or a profile it was
/// started with (`pgrep` works inside `sandbox-exec`, `ps` does not), so a
/// failed test leaves nothing running.
struct Reap {
    exe: PathBuf,
    dir: PathBuf,
}

impl Drop for Reap {
    fn drop(&mut self) {
        let mut needles = vec![self.exe.display().to_string()];
        if let Ok(args) = std::fs::read_to_string(self.dir.join("args.txt")) {
            needles.extend(
                args.lines()
                    .filter(|a| a.starts_with("--user-data-dir="))
                    .map(str::to_owned),
            );
        }
        for needle in needles {
            let _ = std::process::Command::new("pkill")
                .args(["-9", "-f", "--", &needle])
                .status();
        }
    }
}

/// A script at `path` that writes its parent pid and arguments to
/// `<dir>/args.txt`, counts launches in `<dir>/launches.txt`, then runs
/// `body`.
fn script(path: &Path, dir: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\n{{ echo \"ppid=$PPID\"; for a in \"$@\"; do printf '%s\\n' \"$a\"; done; }} > '{}'\necho x >> '{}'\n{body}\n",
            dir.join("args.txt").display(),
            dir.join("launches.txt").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// [`script`] as the main executable of a bundle in `dir` with a name and
/// bundle id of its own: LaunchServices picks an app by its identity, not
/// its path, so `open -a` on one of two bundles alike may start the other.
fn bundled(dir: &Path, body: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "Fake Browser {}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let contents = dir.join(format!("{name}.app/Contents"));
    let exe = contents.join("MacOS").join(&name);
    script(&exe, dir, body);
    let id = name.to_lowercase().replace(' ', "-");
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.contour.test.{id}</string>
<key>CFBundleExecutable</key><string>{name}</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
        ),
    )
    .unwrap();
    exe
}

fn recorded(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("args.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Nothing runs any more with `needle` in its arguments.
fn gone(needle: &str) -> bool {
    let out = std::process::Command::new("pgrep")
        .args(["-f", needle])
        .output()
        .unwrap();
    out.stdout.is_empty()
}

async fn pdf_via(exe: &Path, dir: &Path) -> Vec<u8> {
    let page = dir.join("page.html");
    std::fs::write(&page, "<!doctype html><h1>LaunchServices</h1>").unwrap();
    match common::renderer(exe)
        .print_pdf(&RenderTarget::new(page), DeckMode::Page)
        .await
    {
        Ok(pdf) => pdf,
        // A bundled real Chrome once timed out inside `make check` only;
        // what ran then is the lead.
        Err(e) => panic!("{e}\nprocesses:\n{}", processes()),
    }
}

/// The fake browsers and Chrome profiles running now (empty without `ps`).
fn processes() -> String {
    let out = std::process::Command::new("ps")
        .args(["-A", "-ww", "-o", "pid,ppid,stat,etime,args"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    out.lines()
        .filter(|l| l.contains("Fake Browser") || l.contains("od-chrome-"))
        .map(|l| l.chars().take(300).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The same switches either way, the user-data-dir value aside; only the
/// bundle is started by launchd (parent pid 1), not by us. Without `ps` the
/// bundle too is our child, and the export still works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bundled_browser_is_not_our_child_where_ps_runs() {
    if skip() {
        return;
    }
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    // Not `exec`: the script outlives Chrome and marks that it exited by
    // itself after `Browser.close`, before the profile ends what is left.
    let run = |dir: &Path| {
        format!(
            "'{}' \"$@\"\necho exited > '{}'",
            chrome.display(),
            dir.join("exited").display()
        )
    };

    let direct = tempfile::tempdir().unwrap();
    let wrapper = direct.path().join("recording-chrome");
    script(&wrapper, direct.path(), &run(direct.path()));
    assert!(pdf_via(&wrapper, direct.path()).await.starts_with(b"%PDF"));

    let viabundle = tempfile::tempdir().unwrap();
    let exe = bundled(viabundle.path(), &run(viabundle.path()));
    let _reap = Reap {
        exe: exe.clone(),
        dir: viabundle.path().to_path_buf(),
    };
    assert!(pdf_via(&exe, viabundle.path()).await.starts_with(b"%PDF"));
    for dir in [direct.path(), viabundle.path()] {
        assert!(
            dir.join("exited").exists(),
            "closing killed the browser instead of letting it exit: {}",
            dir.display()
        );
    }

    let normalize = |mut args: Vec<String>| {
        let ppid = args.remove(0);
        for a in &mut args {
            if a.starts_with("--user-data-dir=") {
                *a = "--user-data-dir=…".to_owned();
            }
        }
        args.sort();
        (ppid, args)
    };
    let (direct_ppid, direct_args) = normalize(recorded(direct.path()));
    let (bundle_parent, bundle_args) = normalize(recorded(viabundle.path()));
    assert_eq!(
        direct_ppid,
        format!("ppid={}", std::process::id()),
        "a bare executable is still our child"
    );
    assert_eq!(bundle_parent, bundle_ppid(), "who started the bundle");
    assert_eq!(bundle_args, direct_args, "both ways run Chrome alike");
    assert!(
        gone(&exe.display().to_string()),
        "the browser was left running"
    );
}

/// A bundled browser that exits at once fails within seconds, saying what
/// it printed; one that never opens DevTools is tried twice and ended.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bundled_browser_that_cannot_start_fails_fast_and_is_ended() {
    if std::env::var(NO_SANDBOX_ENV).as_deref() == Ok("1") {
        eprintln!("skipping: {NO_SANDBOX_ENV}=1 is set for this run");
        return;
    }
    let page_dir = tempfile::tempdir().unwrap();
    let page = page_dir.path().join("page.html");
    std::fs::write(&page, "<!doctype html><h1>x</h1>").unwrap();

    for (body, limit, launches) in [
        (
            "echo 'boom: no display' >&2; exit 1",
            Duration::from_secs(10),
            1,
        ),
        // A shell loop, not `exec sleep`: it keeps the profile in its
        // arguments, as a hung Chrome does.
        ("while :; do sleep 1; done", Duration::from_secs(70), 2),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let exe = bundled(dir.path(), body);
        let _reap = Reap {
            exe: exe.clone(),
            dir: dir.path().to_path_buf(),
        };
        let started = Instant::now();
        let err = tokio::time::timeout(
            limit + Duration::from_secs(30),
            CdpRenderer::with_executable(&exe)
                .print_pdf(&RenderTarget::new(page.clone()), DeckMode::Page),
        )
        .await
        .expect("the export hung instead of failing")
        .expect_err("a browser that cannot start cannot render");
        let took = started.elapsed();
        assert!(took <= limit, "{body}: took {took:?}: {err}");
        let msg = err.to_string();
        assert!(msg.contains("cannot start"), "{body}: {msg}");
        if body.contains("boom") {
            assert!(msg.contains("boom: no display"), "says why: {msg}");
        }
        let launched = std::fs::read_to_string(dir.path().join("launches.txt")).unwrap();
        assert_eq!(launched.lines().count(), launches, "{body}: launches");
        let args = recorded(dir.path());
        assert_eq!(args[0], bundle_ppid(), "{body}: who started it");
        assert!(
            !args.iter().any(|a| a.contains("no-sandbox")),
            "the product turned Chrome's sandbox off by itself: {args:?}"
        );
        assert!(
            gone(&exe.display().to_string()),
            "{body}: the browser was left running"
        );
    }
}

/// A bundled browser that ignores `Browser.close` and SIGTERM does not hold
/// the export, and is killed: found through its profile, or as our child
/// on the fallback.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bundled_browser_that_never_exits_does_not_hang_the_export() {
    if skip() {
        return;
    }
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    let dir = tempfile::tempdir().unwrap();
    let exe = bundled(
        dir.path(),
        &format!(
            "trap '' TERM\n'{}' \"$@\" &\nwhile :; do sleep 1; done",
            chrome.display()
        ),
    );
    let _reap = Reap {
        exe: exe.clone(),
        dir: dir.path().to_path_buf(),
    };
    let started = Instant::now();
    let pdf = tokio::time::timeout(Duration::from_secs(90), pdf_via(&exe, dir.path()))
        .await
        .expect("the export waited for the browser forever");
    assert!(pdf.starts_with(b"%PDF"));
    eprintln!("finished in {:?}", started.elapsed());
    assert_eq!(recorded(dir.path())[0], bundle_ppid());
    assert!(
        gone(&exe.display().to_string()),
        "the stuck browser is still running"
    );
}
