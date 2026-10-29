//! Engine and process behaviour with a real mock process (spec 5.7 M2).

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use od_contracts::{FailureCategory, RunStatus, sse};
use od_runs::{RunMeta, Runs};
use od_runtime::def::{CLAUDE, OPENCODE};
use od_runtime::process::{self, SpawnSpec, group_alive, reap_orphan};
use od_runtime::{EngineConfig, LaunchSpec, PermissionMode, RunEngine};
use std::path::Path;
use std::time::Duration;

/// macOS `ps` is setuid root: inside `sandbox-exec` (how the maintainer's pre-merge check runs
/// `make check`) it cannot be executed, so the process-tree cleanup these
/// tests check has nothing to list. They skip there and run everywhere else.
fn ps_unavailable() -> bool {
    std::process::Command::new("ps")
        .args(["-p", "1"])
        .output()
        .is_err()
}

fn write_mock(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn daemon_restart_mid_run_ends_it_failed_and_reaps_the_orphan() {
    if ps_unavailable() {
        eprintln!("skipping: ps cannot run in this sandbox");
        return;
    }
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
            service_tier: None,
            bin: None,
            extra_dirs: Vec::new(),
            resume_session_id: None,
            permission: PermissionMode::Auto,
            env: vec![("PATH".into(), engine.config().path.clone())],
            project_id: None,
            byok: None,
            mcp: od_runtime::McpLaunch::default(),
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
    if ps_unavailable() {
        eprintln!("skipping: ps cannot run in this sandbox");
        return;
    }
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

#[tokio::test]
async fn processes_left_in_the_group_die_with_the_run() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    let pid_file = tmp.path().join("bg.pid");
    // Starts a background child that keeps nothing open, then exits 0.
    write_mock(
        &mocks,
        "claude",
        &format!(
            r#"case "$1" in --version|-p) [ "$2" = "--help" ] && exit 0;; esac
IFS= read -r line
sleep 300 </dev/null >/dev/null 2>&1 &
echo $! > "{pid}"
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
"#,
            pid = pid_file.display()
        ),
    );
    let runs = Runs::new(tmp.path().join("runs"));
    let (run, _) = runs
        .create_or_reuse(RunMeta {
            run_id: "bg".into(),
            agent_id: "claude".into(),
            ..RunMeta::default()
        })
        .unwrap();
    let mut config = EngineConfig::from_env().with_path_prefix(&mocks);
    config.kill_grace = Duration::from_millis(500);
    let engine = RunEngine::new(config);
    let outcome = engine
        .launch(LaunchSpec {
            run: run.clone(),
            def: &CLAUDE,
            cwd: tmp.path().to_path_buf(),
            prompt: "go".into(),
            fallback_prompt: None,
            model: None,
            reasoning: None,
            service_tier: None,
            bin: None,
            extra_dirs: Vec::new(),
            resume_session_id: None,
            permission: PermissionMode::Auto,
            env: vec![("PATH".into(), engine.config().path.clone())],
            project_id: None,
            byok: None,
            mcp: od_runtime::McpLaunch::default(),
        })
        .await
        .unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded);
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .unwrap()
        .success();
    if alive {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
    }
    assert!(
        !alive,
        "the agent's background child {pid} outlived the run"
    );
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(tmp.path().join("runs/bg/state.json")).unwrap())
            .unwrap();
    assert!(state["pgid"].is_null(), "{state}");
}

/// The help probe timeout of the probe tests: long enough that a mock
/// which answers does so on a machine under heavy load (the default 5 s
/// was not, with the load at 35), shorter than the mocks that hang.
const LOADED_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The first help probe times out (a first exec can be slow under load):
/// that run goes without the optional flag, the next one probes again.
#[tokio::test]
async fn a_help_probe_that_timed_out_is_tried_again() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    let (marker, argv) = (tmp.path().join("probed"), tmp.path().join("argv.log"));
    write_mock(
        &mocks,
        "claude",
        &format!(
            r#"if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  if [ ! -f "{marker}" ]; then touch "{marker}"; exec sleep 60; fi
  echo "--include-partial-messages"; exit 0
fi
echo "$@" >> "{argv}"
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
"#,
            marker = marker.display(),
            argv = argv.display()
        ),
    );
    let runs = Runs::new(tmp.path().join("runs"));
    let mut config = EngineConfig::from_env().with_path_prefix(&mocks);
    config.probe_retry_after = Duration::ZERO;
    config.probe_timeout = LOADED_PROBE_TIMEOUT;
    let engine = RunEngine::new(config);
    for id in ["first", "second"] {
        let (run, _) = runs
            .create_or_reuse(RunMeta {
                run_id: id.into(),
                agent_id: "claude".into(),
                ..RunMeta::default()
            })
            .unwrap();
        let outcome = engine
            .launch(LaunchSpec {
                run: run.clone(),
                def: &CLAUDE,
                cwd: tmp.path().to_path_buf(),
                prompt: "go".into(),
                fallback_prompt: None,
                model: None,
                reasoning: None,
                service_tier: None,
                bin: None,
                extra_dirs: Vec::new(),
                resume_session_id: None,
                permission: PermissionMode::Auto,
                env: vec![("PATH".into(), engine.config().path.clone())],
                project_id: None,
                byok: None,
                mcp: od_runtime::McpLaunch::default(),
            })
            .await
            .unwrap();
        assert_eq!(outcome.status, RunStatus::Succeeded, "{id}");
    }
    let lines: Vec<String> = std::fs::read_to_string(&argv)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        !lines[0].contains("--include-partial-messages"),
        "{lines:?}"
    );
    assert!(lines[1].contains("--include-partial-messages"), "{lines:?}");
}

/// A CLI whose `--help` hangs costs one probe timeout, not one per run:
/// the failure is remembered for a while.
#[tokio::test]
async fn a_failed_help_probe_is_not_retried_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    let probes = tmp.path().join("probes.log");
    write_mock(
        &mocks,
        "claude",
        &format!(
            r#"if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then
  echo probe >> "{probes}"; exec sleep 30
fi
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
"#,
            probes = probes.display()
        ),
    );
    let runs = Runs::new(tmp.path().join("runs"));
    // A long probe timeout: a short one can end the probe under load
    // before the mock has even started, and then nothing was probed at all.
    let mut config = EngineConfig::from_env().with_path_prefix(&mocks);
    config.probe_timeout = LOADED_PROBE_TIMEOUT;
    let engine = RunEngine::new(config);
    for id in ["first", "second"] {
        let (run, _) = runs
            .create_or_reuse(RunMeta {
                run_id: id.into(),
                agent_id: "claude".into(),
                ..RunMeta::default()
            })
            .unwrap();
        let outcome = engine
            .launch(LaunchSpec {
                run: run.clone(),
                def: &CLAUDE,
                cwd: tmp.path().to_path_buf(),
                prompt: "go".into(),
                fallback_prompt: None,
                model: None,
                reasoning: None,
                service_tier: None,
                bin: None,
                extra_dirs: Vec::new(),
                resume_session_id: None,
                permission: PermissionMode::Auto,
                env: vec![("PATH".into(), engine.config().path.clone())],
                project_id: None,
                byok: None,
                mcp: od_runtime::McpLaunch::default(),
            })
            .await
            .unwrap();
        assert_eq!(outcome.status, RunStatus::Succeeded, "{id}");
    }
    let probed = std::fs::read_to_string(&probes).unwrap();
    assert_eq!(probed.lines().count(), 1, "probed again at once");
}

/// A first `--version` and help probe that time out (a new binary's first
/// exec can pass the limit under load) are tried once more, so the agent
/// list still shows the version and flags.
#[tokio::test]
async fn detection_tries_a_timed_out_probe_again() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    write_mock(
        &mocks,
        "claude",
        &format!(
            r#"marker="{dir}/probed-$1-$2"
if [ ! -f "$marker" ]; then /usr/bin/touch "$marker"; exec /bin/sleep 6; fi
case "$1" in --version) echo "mock 1.0"; exit 0;; esac
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then echo "--include-partial-messages"; exit 0; fi
"#,
            dir = tmp.path().display()
        ),
    );
    let info = od_runtime::detect::detect(&CLAUDE, mocks.as_os_str(), false).await;
    assert!(info.available);
    assert_eq!(info.version.as_deref(), Some("mock 1.0"));
    assert_eq!(info.capabilities, ["--include-partial-messages"]);
}

/// An OpenCode mock: `help` is its `run --help` text (empty: the probe
/// hangs), `version` its `--version`; a run logs its argv.
fn write_opencode(dir: &Path, help: &str, version: &str, argv: &Path) {
    let help = if help.is_empty() {
        "exec sleep 30".to_owned()
    } else {
        format!("echo '{help}'; exit 0")
    };
    write_mock(
        dir,
        "opencode-cli",
        &format!(
            r#"if [ "$1" = "run" ] && [ "$2" = "--help" ]; then {help}; fi
if [ "$1" = "--version" ]; then echo "{version}"; exit 0; fi
echo "$@" >> "{argv}"
cat > /dev/null
printf '{{"type":"text","sessionID":"ses_1","part":{{"id":"p1","type":"text","text":"ok"}}}}\n'
printf '{{"type":"step_finish","sessionID":"ses_1","part":{{"id":"p2","type":"step-finish","reason":"stop"}}}}\n'
"#,
            argv = argv.display()
        ),
    );
}

async fn run_opencode(engine: &RunEngine, runs: &Runs, id: &str, cwd: &Path) {
    let (run, _) = runs
        .create_or_reuse(RunMeta {
            run_id: id.into(),
            agent_id: "opencode".into(),
            ..RunMeta::default()
        })
        .unwrap();
    engine
        .launch(LaunchSpec {
            run: run.clone(),
            def: &OPENCODE,
            cwd: cwd.to_path_buf(),
            prompt: "go".into(),
            fallback_prompt: None,
            model: None,
            reasoning: None,
            service_tier: None,
            bin: None,
            extra_dirs: Vec::new(),
            resume_session_id: None,
            permission: PermissionMode::Auto,
            env: vec![("PATH".into(), engine.config().path.clone())],
            project_id: None,
            byok: None,
            mcp: od_runtime::McpLaunch::default(),
        })
        .await
        .unwrap();
}

fn logged(argv: &Path) -> Vec<String> {
    std::fs::read_to_string(argv)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// OpenCode upgraded from 1 to 2 while the app runs: the replaced binary
/// is probed again, so the next run drops OpenCode 1's `--dir`.
#[tokio::test]
async fn an_agent_upgraded_in_place_is_probed_again() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    let argv = tmp.path().join("argv.log");
    let runs = Runs::new(tmp.path().join("runs"));
    let engine = RunEngine::new(EngineConfig::from_env().with_path_prefix(&mocks));

    write_opencode(
        &mocks,
        "--dangerously-skip-permissions --dir",
        "1.14.3",
        &argv,
    );
    run_opencode(&engine, &runs, "v1", tmp.path()).await;
    write_opencode(&mocks, "--standalone --auto", "2.0.25", &argv);
    run_opencode(&engine, &runs, "v2", tmp.path()).await;

    let lines = logged(&argv);
    assert!(lines[0].contains("--dir"), "{lines:?}");
    let last = lines.last().unwrap();
    assert!(last.contains("--standalone"), "{lines:?}");
    assert!(!last.contains("--dir"), "{lines:?}");
}

/// The help probe of OpenCode 2 fails (a slow first exec): its version
/// still picks OpenCode 2's argv, not the `--dir` it rejects.
#[tokio::test]
async fn a_failed_help_probe_falls_back_to_the_version() {
    let tmp = tempfile::tempdir().unwrap();
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    let argv = tmp.path().join("argv.log");
    write_opencode(&mocks, "", "2.0.25", &argv);
    let runs = Runs::new(tmp.path().join("runs"));
    let mut config = EngineConfig::from_env().with_path_prefix(&mocks);
    config.probe_timeout = Duration::from_secs(3);
    let engine = RunEngine::new(config);

    run_opencode(&engine, &runs, "first", tmp.path()).await;

    let lines = logged(&argv);
    assert!(lines[0].contains("--standalone"), "{lines:?}");
    assert!(lines[0].contains("--auto"), "{lines:?}");
    assert!(!lines[0].contains("--dir"), "{lines:?}");
}
