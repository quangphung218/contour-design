//! Real PTY sessions with `/bin/sh` (spec 1.5 Terminal).

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use od_term::{Output, TermConfig, TermStatus, Terminals};
use std::time::Duration;

fn terminals(max: usize) -> Terminals {
    Terminals::new(TermConfig {
        shell: Some("/bin/sh".into()),
        max_sessions: max,
        kill_grace: Duration::from_millis(500),
        ..TermConfig::default()
    })
}

/// Collects output until `needle` shows up (or panics after 10 s).
async fn read_until(
    rx: &mut tokio::sync::broadcast::Receiver<Output>,
    seen: &mut String,
    needle: &str,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !seen.contains(needle) {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Ok(Output::Data(d))) => seen.push_str(&String::from_utf8_lossy(&d)),
            Ok(Ok(Output::Exit(_))) | Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
    assert!(seen.contains(needle), "never saw {needle:?} in {seen:?}");
}

fn pid_after(seen: &str, tag: &str) -> i32 {
    let rest = &seen[seen.rfind(tag).unwrap() + tag.len()..];
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

#[tokio::test]
async fn shell_runs_in_the_project_folder_and_echoes() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let terms = terminals(4);
    let s = terms.create("p1", &cwd, Some(100), Some(30)).unwrap();
    let mut att = s.attach();
    let mut seen = String::from_utf8_lossy(&att.scrollback).into_owned();
    s.write(b"echo hello-$((40+2)); pwd\r").unwrap();
    read_until(&mut att.live, &mut seen, "hello-42").await;
    read_until(&mut att.live, &mut seen, cwd.to_str().unwrap()).await;
    let info = s.info();
    assert_eq!(
        (info.cols, info.rows, info.status),
        (100, 30, TermStatus::Running)
    );

    // A second client replays the scrollback.
    let again = s.attach();
    assert!(String::from_utf8_lossy(&again.scrollback).contains("hello-42"));

    s.write(b"exit 3\r").unwrap();
    let mut rx = att.live;
    let exit = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(Output::Exit(e)) = rx.recv().await {
                return e;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(exit.code, 3);
    assert_eq!(s.info().status, TermStatus::Exited);
    assert_eq!(terms.list("p1").len(), 1);
    assert!(terms.close(s.id()).await);
    assert!(terms.list("p1").is_empty());
    assert!(!terms.close(s.id()).await);
}

#[tokio::test]
async fn closing_kills_the_shell_its_foreground_job_and_background_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let terms = terminals(4);
    let s = terms.create("p1", dir.path(), None, None).unwrap();
    let shell = i32::try_from(s.pid().unwrap()).unwrap();
    let mut att = s.attach();
    let mut seen = String::new();
    // `B""G=` in the typed line keeps the echo from matching `BG=`. The
    // foreground job ignores SIGHUP, so only the SIGKILL step ends it.
    s.write(b"set +H; sleep 1001 & echo B\"\"G=$! .\r").unwrap();
    read_until(&mut att.live, &mut seen, "BG=").await;
    read_until(&mut att.live, &mut seen, ".").await;
    s.write(b"sh -c 'trap \"\" HUP; echo F\"\"G=$$ .; exec sleep 1002'\r")
        .unwrap();
    read_until(&mut att.live, &mut seen, "FG=").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (bg, fg) = (pid_after(&seen, "BG="), pid_after(&seen, "FG="));
    assert!(alive(shell) && alive(bg) && alive(fg));

    assert!(terms.close(s.id()).await);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while [shell, bg, fg].iter().any(|&p| alive(p)) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "survivors: shell {} bg {} fg {}",
            alive(shell),
            alive(bg),
            alive(fg)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A background job in a group of its own that ignores SIGHUP: neither
/// the shell's group nor the foreground one, but still the tab's session.
#[tokio::test]
async fn closing_kills_background_jobs_that_ignore_sighup() {
    let dir = tempfile::tempdir().unwrap();
    let terms = terminals(4);
    let s = terms.create("p1", dir.path(), None, None).unwrap();
    let mut att = s.attach();
    let mut seen = String::new();
    s.write(b"set -m; (trap '' HUP; exec sleep 1003) & echo B\"\"H=$! .\r")
        .unwrap();
    read_until(&mut att.live, &mut seen, "BH=").await;
    read_until(&mut att.live, &mut seen, ".").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let bg = pid_after(&seen, "BH=");
    assert!(alive(bg));
    let shell = i32::try_from(s.pid().unwrap()).unwrap();
    assert_ne!(
        nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(bg)))
            .unwrap()
            .as_raw(),
        shell
    );

    assert!(terms.close(s.id()).await);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while alive(bg) {
        if tokio::time::Instant::now() > deadline {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(bg),
                nix::sys::signal::Signal::SIGKILL,
            );
            panic!("the background job outlived its tab");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn limit_and_bad_cwd_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let terms = terminals(1);
    let s = terms.create("p1", dir.path(), None, None).unwrap();
    assert!(matches!(
        terms.create("p1", dir.path(), None, None),
        Err(od_term::TermError::Limit(1))
    ));
    assert!(matches!(
        terms.create("p1", &dir.path().join("missing"), None, None),
        Err(od_term::TermError::BadCwd(_))
    ));
    terms.close_all().await;
    assert!(s.exit_status().is_some());
    terms.create("p1", dir.path(), None, None).unwrap();
    terms.close_all().await;
}
