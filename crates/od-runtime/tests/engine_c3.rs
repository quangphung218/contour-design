//! Track C3 runtimes through the real engine with mock CLIs on `PATH`:
//! the prompt on argv (Aider) and its size limit, in a temp file (Grok
//! Build), as one Antigravity stdin event (with the model written to agy's
//! settings), and over Pi's RPC (resume included). No real agent, no
//! network.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use od_contracts::{FailureCategory, RunStatus, sse};
use od_runs::{RunEvent, RunMeta, Runs};
use od_runtime::def::{AIDER, ANTIGRAVITY, GROK_BUILD, MAX_PROMPT_ARG_BYTES, PI, QODER, RuntimeDef};
use od_runtime::{EngineConfig, LaunchSpec, PermissionMode, RunEngine};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn write_mock(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Harness {
    tmp: tempfile::TempDir,
    mocks: PathBuf,
    project: PathBuf,
    home: PathBuf,
    engine: RunEngine,
    runs: Runs,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let mocks = tmp.path().join("mocks");
        let project = tmp.path().join("project");
        let home = tmp.path().join("home");
        for d in [&mocks, &project, &home] {
            std::fs::create_dir_all(d).unwrap();
        }
        let mut config = EngineConfig::from_env().with_path_prefix(&mocks);
        config.kill_grace = Duration::from_millis(500);
        config.retry_base = Duration::from_millis(10);
        config.idle_timeout = Some(Duration::from_secs(5));
        let runs = Runs::new(tmp.path().join("runs"));
        Self {
            engine: RunEngine::new(config),
            tmp,
            mocks,
            project,
            home,
            runs,
        }
    }

    fn log(&self) -> PathBuf {
        self.tmp.path().join("mock.log")
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(self.log()).unwrap_or_default()
    }

    fn spec(&self, id: &str, def: &'static RuntimeDef) -> LaunchSpec {
        let (run, _) = self
            .runs
            .create_or_reuse(RunMeta {
                run_id: id.into(),
                agent_id: def.id.into(),
                ..RunMeta::default()
            })
            .unwrap();
        let env: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), self.engine.config().path.clone()),
            ("MOCK_LOG".into(), self.log().into()),
            ("HOME".into(), self.home.clone().into()),
        ];
        LaunchSpec {
            run,
            def,
            cwd: self.project.clone(),
            prompt: "Build a landing page".into(),
            fallback_prompt: None,
            model: None,
            reasoning: None,
            service_tier: None,
            bin: None,
            extra_dirs: Vec::new(),
            resume_session_id: None,
            permission: PermissionMode::Auto,
            env,
            project_id: None,
            byok: None,
            mcp: od_runtime::McpLaunch::default(),
        }
    }
}

async fn events_of(run: &Arc<od_runs::Run>) -> Vec<Arc<RunEvent>> {
    let mut sub = run.subscribe(0);
    let mut out = Vec::new();
    while let Some(ev) = sub.next().await {
        out.push(ev);
    }
    out
}

/// Logs every argv value on its own line, then answers with an artifact.
const ARGV_MOCK: &str = r#"
for a in "$@"; do printf 'ARG:%s\n' "$a" >> "$MOCK_LOG"; done
if read -r l; then echo "STDIN:$l" >> "$MOCK_LOG"; else echo "STDIN:EOF" >> "$MOCK_LOG"; fi
echo 'Here it is.'
echo '<artifact identifier="landing" type="text/html" title="Landing">'
echo '<!doctype html><h1>Aider</h1>'
echo '</artifact>'
"#;

#[tokio::test]
async fn aider_gets_the_prompt_on_argv_and_its_artifact_becomes_a_file() {
    let h = Harness::new();
    write_mock(&h.mocks, "aider", ARGV_MOCK);
    let spec = h.spec("aider-ok", &AIDER);
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    let log = h.log_text();
    let args: Vec<&str> = log.lines().filter_map(|l| l.strip_prefix("ARG:")).collect();
    assert_eq!(&args[args.len() - 2..], ["--message", "Build a landing page"]);
    assert!(args.contains(&"--yes-always"));
    assert!(log.contains("STDIN:EOF"), "stdin is closed at once: {log}");
    let files: Vec<_> = std::fs::read_dir(&h.project)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        files.iter().any(|f| f.ends_with(".html")),
        "artifact written: {files:?}"
    );
}

#[tokio::test]
async fn an_argv_prompt_over_the_limit_is_refused_before_spawn() {
    let h = Harness::new();
    write_mock(&h.mocks, "aider", ARGV_MOCK);
    let mut spec = h.spec("aider-big", &AIDER);
    spec.prompt = "x".repeat(MAX_PROMPT_ARG_BYTES + 1);
    let run = spec.run.clone();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.failure_category, Some(FailureCategory::SpawnFailed));
    assert!(h.log_text().is_empty(), "the CLI never ran");
    let events = events_of(&run).await;
    let error = events.iter().find(|e| e.event == sse::ERROR).unwrap();
    assert!(
        error.data["message"]
            .as_str()
            .unwrap()
            .contains("on the command line"),
        "{:?}",
        error.data
    );
    // Exactly at the limit still runs.
    let mut spec = h.spec("aider-edge", &AIDER);
    spec.prompt = "x".repeat(MAX_PROMPT_ARG_BYTES);
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
}

/// Copies the `--prompt-file` it was given into the log, with its mode.
const PROMPT_FILE_MOCK: &str = r#"
while [ $# -gt 0 ]; do
  case "$1" in --prompt-file) f="$2"; shift;; esac
  shift
done
echo "FILE:$f" >> "$MOCK_LOG"
echo "MODE:$(stat -f '%Lp' "$f" 2>/dev/null || stat -c '%a' "$f")" >> "$MOCK_LOG"
cat "$f" >> "$MOCK_LOG"; echo >> "$MOCK_LOG"
echo 'Done.'
"#;

#[tokio::test]
async fn grok_build_reads_a_private_prompt_file_that_is_removed_after() {
    let h = Harness::new();
    write_mock(&h.mocks, "grok", PROMPT_FILE_MOCK);
    let spec = h.spec("grok-ok", &GROK_BUILD);
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert_eq!(outcome.text, "Done.\n");
    let log = h.log_text();
    let file = log
        .lines()
        .find_map(|l| l.strip_prefix("FILE:"))
        .unwrap()
        .to_owned();
    assert!(log.contains("MODE:600"), "{log}");
    assert!(log.contains("Build a landing page"), "{log}");
    assert!(!Path::new(&file).exists(), "the prompt file is removed");
}

/// Reads the one stdin event, logs the model agy would read, writes the
/// model line to `--log-file`, prints a recorded success stream.
const AGY_MOCK: &str = r#"
while [ $# -gt 0 ]; do
  case "$1" in --log-file) lf="$2"; shift;; esac
  printf 'ARG:%s\n' "$1" >> "$MOCK_LOG"
  shift
done
IFS= read -r l; printf 'STDIN:%s\n' "$l" >> "$MOCK_LOG"
if read -r more; then echo "MORE:$more" >> "$MOCK_LOG"; else echo "STDIN:EOF" >> "$MOCK_LOG"; fi
m=$(sed -n 's/.*"model": *"\([^"]*\)".*/\1/p' "$HOME/.gemini/antigravity-cli/settings.json" 2>/dev/null)
echo "MODEL:$m" >> "$MOCK_LOG"
echo "LOG:$lf" >> "$MOCK_LOG"
printf 'Propagating selected model override to backend: label="%s"\n' "$m" >> "$lf"
echo '{"event":"init","conversation_id":"agy-1","init":{"cwd":"/p"}}'
echo '{"event":"step_update","step_update":{"conversation_id":"agy-1","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"Hello from agy"}}'
echo '{"event":"result","result":{"conversation_id":"agy-1","status":"SUCCESS","response":"Hello from agy","duration_seconds":1.5,"usage":{"input_tokens":3,"output_tokens":4}}}'
"#;

#[tokio::test]
async fn antigravity_gets_one_stdin_event_and_its_model_through_settings() {
    let h = Harness::new();
    write_mock(&h.mocks, "agy", AGY_MOCK);
    let settings = od_runtime::antigravity::settings_path(&h.home);
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
    let mut spec = h.spec("agy-ok", &ANTIGRAVITY);
    spec.model = Some("Gemini 3.1 Pro (High)".into());
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert_eq!(outcome.text, "Hello from agy");
    let log = h.log_text();
    let stdin: Value = serde_json::from_str(
        log.lines()
            .find_map(|l| l.strip_prefix("STDIN:{"))
            .map(|rest| format!("{{{rest}"))
            .unwrap()
            .as_str(),
    )
    .unwrap();
    assert_eq!(stdin, serde_json::json!({"event": "user", "message": {"content": "Build a landing page"}}));
    assert!(log.contains("STDIN:EOF"), "EOF after the one event: {log}");
    assert!(log.contains("MODEL:Gemini 3.1 Pro (High)"), "{log}");
    assert!(!log.contains("ARG:--model"), "agy has no --model");
    let kept: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(kept["theme"], "dark", "other settings are kept");
    let agy_log = log.lines().find_map(|l| l.strip_prefix("LOG:")).unwrap();
    assert!(!Path::new(agy_log).exists(), "the agent log is removed");

    // The default model leaves settings alone.
    std::fs::write(&settings, r#"{"model":"Mine"}"#).unwrap();
    let mut spec = h.spec("agy-default", &ANTIGRAVITY);
    spec.model = Some("default".into());
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    let kept: Value = serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(kept["model"], "Mine");
}

/// A scripted `pi --mode rpc`: logs each command, answers `switch_session`
/// (rejecting `/s/gone.jsonl`), `prompt` and `get_state`, then streams a
/// turn and waits for EOF.
const PI_MOCK: &str = r#"
case "$1" in --version) echo "0.70.2"; exit 0;; esac
say() { printf '%s\n' "$1"; }
while IFS= read -r l; do
  printf '%s\n' "$l" >> "$MOCK_LOG"
  id=$(printf '%s' "$l" | sed 's/.*"id":\([0-9]*\).*/\1/')
  case "$l" in
    *'"switch_session"'*)
      case "$l" in
        *gone*) say "{\"type\":\"response\",\"id\":$id,\"command\":\"switch_session\",\"success\":false,\"error\":\"session not found\"}";;
        *) say "{\"type\":\"response\",\"id\":$id,\"command\":\"switch_session\",\"success\":true}";;
      esac;;
    *'"prompt"'*)
      say "{\"type\":\"response\",\"id\":$id,\"command\":\"prompt\",\"success\":true}";;
    *'"get_state"'*)
      say "{\"type\":\"response\",\"id\":$id,\"command\":\"get_state\",\"success\":true,\"data\":{\"sessionFile\":\"$HOME/.pi/s1.jsonl\"}}"
      say '{"type":"agent_start"}'
      say '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Hi from pi"}}'
      say '{"type":"turn_end","message":{"stopReason":"stop","usage":{"input":1,"output":2}}}'
      say '{"type":"agent_end","messages":[]}';;
  esac
done
echo EOF >> "$MOCK_LOG"
"#;

#[tokio::test]
async fn pi_rpc_turn_captures_its_session_and_resumes_it() {
    let h = Harness::new();
    write_mock(&h.mocks, "pi", PI_MOCK);
    let outcome = h.engine.launch(h.spec("pi-1", &PI)).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert_eq!(outcome.text, "Hi from pi");
    let session = h.home.join(".pi/s1.jsonl").to_string_lossy().into_owned();
    assert_eq!(outcome.session_id.as_deref(), Some(session.as_str()));
    let log = h.log_text();
    let kinds: Vec<String> = log
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l)
                .map_or_else(|_| l.to_owned(), |v| v["type"].as_str().unwrap().to_owned())
        })
        .collect();
    assert_eq!(kinds, ["prompt", "get_state", "EOF"], "stdin closes after agent_end");

    std::fs::remove_file(h.log()).unwrap();
    let mut spec = h.spec("pi-2", &PI);
    spec.resume_session_id = Some(session.clone());
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert!(outcome.resumed);
    let first: Value = serde_json::from_str(h.log_text().lines().next().unwrap()).unwrap();
    assert_eq!(first["type"], "switch_session");
    assert_eq!(first["sessionPath"], session.as_str());

    // A session Pi no longer has: start over with the full transcript.
    std::fs::remove_file(h.log()).unwrap();
    let mut spec = h.spec("pi-3", &PI);
    spec.resume_session_id = Some("/s/gone.jsonl".into());
    spec.fallback_prompt = Some("FULL TRANSCRIPT".into());
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert!(outcome.resume_failed);
    assert!(!outcome.resumed);
    assert!(h.log_text().contains("\"message\":\"FULL TRANSCRIPT\""));
}

#[tokio::test]
async fn a_qoder_error_result_fails_the_run() {
    let h = Harness::new();
    write_mock(
        &h.mocks,
        "qodercli",
        r#"
cat > /dev/null
echo '{"type":"system","subtype":"init","model":"lite","session_id":"q1"}'
echo '{"type":"result","is_error":true,"error":"Not logged in. Run qodercli login.","usage":null}'
exit 0
"#,
    );
    let outcome = h.engine.launch(h.spec("qoder-err", &QODER)).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Failed);
}
