//! Engine and process behaviour with a real mock process (spec 5.7 M2).

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use od_contracts::{FailureCategory, RunStatus, sse};
use od_runs::{RunMeta, Runs};
use od_runtime::def::CLAUDE;
use od_runtime::process::{self, SpawnSpec, group_alive, reap_orphan};
use od_runtime::{EngineConfig, LaunchSpec, PermissionMode, RunEngine};
use std::path::Path;
use std::time::Duration;

fn write_mock(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn daemon_restart_mid_run_ends_it_failed_and_reaps_the_orphan() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    write_mock(
        &mocks,
        "claude",
        r#"case "$1" in --version|-p) [ "$2" = "--help" ] && exit 0;; esac
IFS= read -r line
printf '{"type":"system","subtype":"init","session_id":"s","model":"mock"}\n'
sleep 300
"#,
    );
    let runs_dir = tmp.path().join("runs");
    let pgid;
    {
        let runs = Runs::new(&runs_dir);
        let (run, _) = runs
            .create_or_reuse(RunMeta {
                run_id: "r1".into(),
                agent_id: "claude".into(),
                ..RunMeta::default()
            })
            .unwrap();
        let engine = RunEngine::new(EngineConfig::from_env().with_path_prefix(&mocks));
        let handle = engine.launch(LaunchSpec {
            run: run.clone(),
            def: &CLAUDE,
            cwd: tmp.path().to_path_buf(),
            prompt: "go".into(),
            fallback_prompt: None,
            model: None,
            reasoning: None,
            extra_dirs: Vec::new(),
            resume_session_id: None,
            permission: PermissionMode::Auto,
            env: vec![("PATH".into(), engine.config().path.clone())],
            project_id: None,
            byok: None,
        });
        let mut sub = run.subscribe(0);
        while let Some(ev) = sub.next().await {
            if ev.event == sse::AGENT {
                break;
            }
        }
        let state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(runs_dir.join("r1/state.json")).unwrap())
                .unwrap();
        pgid = i32::try_from(state["pgid"].as_i64().unwrap()).unwrap();
        assert!(group_alive(pgid));
        // The daemon dies: its tasks vanish, the agent keeps running.
        handle.abort();
        let _ = handle.await;
        run.flush_if_due();
    }
    assert!(group_alive(pgid), "the agent outlived the daemon");

    let (runs, report) = Runs::load(&runs_dir).unwrap();
    assert_eq!(report.interrupted.len(), 1);
    let orphan = report.interrupted[0].1.clone().unwrap();
    assert_eq!(orphan.pgid, pgid);
    assert!(reap_orphan(orphan.pgid, orphan.bin.as_deref()));
    assert!(process::wait_group_gone(pgid, Duration::from_secs(3)).await);
    let run = runs.get("r1").unwrap();
    assert_eq!(run.status(), RunStatus::Failed);
    assert_eq!(
        run.end_payload().unwrap().failure_category,
        Some(FailureCategory::DaemonRestart)
    );
    let mut sub = run.subscribe(0);
    let mut names = Vec::new();
    while let Some(ev) = sub.next().await {
        names.push(ev.event.clone());
    }
    assert_eq!(names.first().map(String::as_str), Some(sse::START));
    assert_eq!(&names[names.len() - 2..], [sse::ERROR, sse::END]);
}

#[tokio::test]
async fn reap_orphan_spares_a_group_running_something_else() {
    let tmp = tempfile::tempdir().unwrap();
    let mut child = process::spawn(&SpawnSpec {
        program: "/bin/sleep".into(),
        args: vec!["30".into()],
        cwd: tmp.path().to_path_buf(),
        env: Vec::new(),
    })
    .unwrap();
    let pgid = i32::try_from(child.id().unwrap()).unwrap();
    assert!(!reap_orphan(pgid, Some("/usr/local/bin/claude")));
    assert!(group_alive(pgid));
    assert!(!reap_orphan(pgid, None));
    assert!(reap_orphan(pgid, Some("/bin/sleep")));
    let status = child.wait().await.unwrap();
    assert!(!status.success());
}
