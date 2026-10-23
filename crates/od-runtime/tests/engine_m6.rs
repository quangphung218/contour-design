//! M6 runtimes through the real engine (spec 5.7 M6): a mock ACP agent
//! over stdio, a plain-text CLI whose `<artifact>` becomes a file, and BYOK
//! against fake SSE servers on loopback. No real agent, no network beyond
//! 127.0.0.1.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use od_contracts::{FailureCategory, RunStatus, sse};
use od_runs::{RunEvent, RunMeta, Runs};
use od_runtime::byok::{ByokConfig, ByokProtocol};
use od_runtime::def::{BYOK, DSH, KIMI, QWEN, RuntimeDef};
use od_runtime::{EngineConfig, LaunchSpec, PermissionMode, RunEngine, RunOutcome};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    engine: RunEngine,
    runs: Runs,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let mocks = tmp.path().join("mocks");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&mocks).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let mut config = EngineConfig::from_env().with_path_prefix(&mocks);
        config.kill_grace = Duration::from_millis(500);
        config.retry_base = Duration::from_millis(10);
        // A hang shows up as a failed run within seconds, not a stuck test.
        config.idle_timeout = Some(Duration::from_secs(5));
        let runs = Runs::new(tmp.path().join("runs"));
        Self {
            engine: RunEngine::new(config),
            tmp,
            mocks,
            project,
            runs,
        }
    }

    fn log(&self) -> PathBuf {
        self.tmp.path().join("mock.log")
    }

    fn spec(&self, id: &str, def: &'static RuntimeDef, extra_env: &[(&str, &str)]) -> LaunchSpec {
        let (run, _) = self
            .runs
            .create_or_reuse(RunMeta {
                run_id: id.into(),
                agent_id: def.id.into(),
                ..RunMeta::default()
            })
            .unwrap();
        let mut env: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), self.engine.config().path.clone()),
            ("MOCK_LOG".into(), self.log().into()),
        ];
        env.extend(extra_env.iter().map(|(k, v)| ((*k).into(), (*v).into())));
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

    /// Mock log lines: JSON frames as their method (`<response>` for a
    /// reply), other lines verbatim.
    fn log_entries(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(|l| match serde_json::from_str::<Value>(l) {
                Ok(v) => v["method"].as_str().unwrap_or("<response>").to_owned(),
                Err(_) => l.to_owned(),
            })
            .collect()
    }

    fn log_frames(&self) -> Vec<Value> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
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

fn agent_events<'a>(events: &'a [Arc<RunEvent>], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|e| e.event == sse::AGENT && e.data["type"] == kind)
        .map(|e| &e.data)
        .collect()
}

fn agent_text(events: &[Arc<RunEvent>]) -> String {
    agent_events(events, "text_delta")
        .iter()
        .filter_map(|d| d["delta"].as_str())
        .collect()
}

/// A scripted ACP agent. Ids are fixed because the client numbers its
/// requests 1 (initialize), 2 (session/new), 3 (session/prompt).
/// `MOCK_MODE`: `ok` answers and waits for EOF; `linger` answers and never
/// exits on its own; `cancel` streams one chunk and ignores
/// `session/cancel`. `MOCK_PERM_KIND` is the kind of the permission asked.
const ACP_MOCK: &str = r#"
case "$1" in --version) echo "kimi 9.9.9"; exit 0;; esac
log="$MOCK_LOG"
trap 'echo SIGTERM >> "$log"; exit 143' TERM
say() { printf '%s\n' "$1"; }
read -r l; printf "%s\n" "$l" >> "$log"
say '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}'
read -r l; printf "%s\n" "$l" >> "$log"
say '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-1"}}'
read -r l; printf "%s\n" "$l" >> "$log"
if [ "$MOCK_MODE" = cancel ]; then
  say '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}'
  read -r l; printf "%s\n" "$l" >> "$log"
  sleep 30 & wait $!
  exit 0
fi
say '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call","toolCallId":"w1","title":"Write index.html","kind":"edit","status":"pending","locations":[{"path":"index.html"}]}}}'
say "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"w1\",\"kind\":\"${MOCK_PERM_KIND:-edit}\",\"title\":\"Write index.html\"},\"options\":[{\"optionId\":\"once\",\"kind\":\"allow_once\"},{\"optionId\":\"always\",\"kind\":\"allow_always\"},{\"optionId\":\"no\",\"kind\":\"reject_once\"}]}}"
read -r l; printf "%s\n" "$l" >> "$log"
printf '<h1>ACP</h1>' > index.html
say '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"w1","status":"completed"}}}'
say '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Wrote index.html."}}}}'
say '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"inputTokens":10,"outputTokens":5}}}'
if [ "$MOCK_MODE" = linger ]; then
  sleep 30 & wait $!
  exit 0
fi
while read -r l; do printf "%s\n" "$l" >> "$log"; done
echo EOF >> "$log"
"#;

#[tokio::test]
async fn acp_mock_agent_runs_the_whole_turn() {
    let h = Harness::new();
    write_mock(&h.mocks, "kimi", ACP_MOCK);
    let spec = h.spec("acp-ok", &KIMI, &[("MOCK_MODE", "ok")]);
    let run = spec.run.clone();
    let outcome: RunOutcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert_eq!(outcome.session_id.as_deref(), Some("sess-1"));
    assert_eq!(outcome.text, "Wrote index.html.");

    // initialize → session/new → session/prompt → permission reply, then
    // stdin closes (EOF) so the agent exits by itself.
    assert_eq!(
        h.log_entries(),
        [
            "initialize",
            "session/new",
            "session/prompt",
            "<response>",
            "EOF"
        ]
    );
    let frames = h.log_frames();
    assert_eq!(
        frames[1]["params"]["cwd"],
        h.project.to_string_lossy().as_ref()
    );
    assert_eq!(frames[3]["id"], 7);
    assert_eq!(frames[3]["result"]["outcome"]["optionId"], "always");

    let events = events_of(&run).await;
    let end = run.end_payload().unwrap();
    let index = h.project.join("index.html").to_string_lossy().into_owned();
    assert_eq!(end.artifact_paths, [index]);
    assert!(
        agent_events(&events, "status")
            .iter()
            .any(|s| s["label"] == "permission")
    );
}

#[tokio::test]
async fn acp_ask_mode_rejects_commands() {
    let h = Harness::new();
    write_mock(&h.mocks, "kimi", ACP_MOCK);
    let mut spec = h.spec(
        "acp-ask",
        &KIMI,
        &[("MOCK_MODE", "ok"), ("MOCK_PERM_KIND", "execute")],
    );
    spec.permission = PermissionMode::Ask;
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded);
    let frames = h.log_frames();
    assert_eq!(frames[3]["result"]["outcome"]["optionId"], "no");
}

#[tokio::test]
async fn acp_agent_that_lingers_is_stopped_after_completion() {
    let h = Harness::new();
    write_mock(&h.mocks, "kimi", ACP_MOCK);
    let spec = h.spec("acp-linger", &KIMI, &[("MOCK_MODE", "linger")]);
    let started = Instant::now();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(h.log_entries().last().map(String::as_str), Some("SIGTERM"));
}

#[tokio::test]
async fn acp_cancel_sends_session_cancel_before_any_signal() {
    let h = Harness::new();
    write_mock(&h.mocks, "kimi", ACP_MOCK);
    let spec = h.spec("acp-cancel", &KIMI, &[("MOCK_MODE", "cancel")]);
    let run = spec.run.clone();
    let handle = h.engine.launch(spec);
    let mut sub = run.subscribe(0);
    while let Some(ev) = sub.next().await {
        if ev.event == sse::AGENT && ev.data["type"] == "text_delta" {
            break;
        }
    }
    assert!(h.engine.cancel("acp-cancel"));
    let outcome = handle.await.unwrap();
    assert_eq!(outcome.status, RunStatus::Canceled);
    assert_eq!(
        h.log_entries(),
        [
            "initialize",
            "session/new",
            "session/prompt",
            "session/cancel",
            "SIGTERM"
        ]
    );
    let frames = h.log_frames();
    assert_eq!(frames[3]["params"]["sessionId"], "sess-1");
    assert!(
        frames[3].get("id").is_none(),
        "session/cancel is a notification"
    );
}

/// A scripted `dsh --stdio`: ready, read execute, stream (a mid-run
/// `usage` included), then wait for `cancel`, which it ignores.
const DSH_MOCK: &str = r#"
case "$1" in --version) echo "0.1.2-rc.1"; exit 0;; esac
log="$MOCK_LOG"
trap 'echo SIGTERM >> "$log"; exit 143' TERM
say() { printf '%s\n' "$1"; }
say '{"v":1,"type":"ready","runtime":"open-design","protocol_version":1,"plugin_version":"0.1.2-rc.1","capabilities":{"session_resume":true,"session_cancel":true,"structured_events":true}}'
read -r l; printf '%s\n' "$l" >> "$log"
rid=$(printf '%s' "$l" | sed 's/.*"request_id":"\([^"]*\)".*/\1/')
say "{\"v\":1,\"type\":\"session\",\"request_id\":\"$rid\",\"session_id\":\"d1\",\"resumed\":false}"
say "{\"v\":1,\"type\":\"usage\",\"request_id\":\"$rid\",\"provider\":\"p\",\"model\":\"m\",\"input_tokens\":1,\"output_tokens\":1}"
say "{\"v\":1,\"type\":\"text\",\"request_id\":\"$rid\",\"content\":\"working\"}"
if read -r l; then printf '%s\n' "$l" >> "$log"; else echo EOF >> "$log"; fi
sleep 30 & wait $!
"#;

#[tokio::test]
async fn dsh_cancel_is_sent_on_the_open_stdin_before_any_signal() {
    let h = Harness::new();
    write_mock(&h.mocks, "dsh", DSH_MOCK);
    let spec = h.spec("dsh-cancel", &DSH, &[]);
    let run = spec.run.clone();
    let handle = h.engine.launch(spec);
    let mut sub = run.subscribe(0);
    while let Some(ev) = sub.next().await {
        if ev.event == sse::AGENT && ev.data["type"] == "text_delta" {
            break;
        }
    }
    assert!(h.engine.cancel("dsh-cancel"));
    let outcome = handle.await.unwrap();
    assert_eq!(outcome.status, RunStatus::Canceled);
    let log = std::fs::read_to_string(h.log()).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3, "{log}");
    let execute: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(execute["type"], "execute");
    let cancel: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(cancel["type"], "cancel");
    assert_eq!(cancel["request_id"], execute["request_id"]);
    assert_eq!(lines[2], "SIGTERM");
}

#[tokio::test]
async fn plain_text_artifacts_become_project_files() {
    let h = Harness::new();
    write_mock(
        &h.mocks,
        "qwen",
        r#"echo "$*" > "$MOCK_LOG"
cat > /dev/null
cat <<'ANSWER'
Here is the page.
<artifact identifier="index" type="text/html" title="Landing">
<!doctype html><h1>Plain</h1>
</artifact>
<artifact title="Theme" type="text/css">body{}</artifact>
ANSWER
"#,
    );
    let spec = h.spec("plain", &QWEN, &[]);
    let run = spec.run.clone();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded);
    assert_eq!(std::fs::read_to_string(h.log()).unwrap().trim(), "--yolo");
    assert_eq!(
        std::fs::read_to_string(h.project.join("index.html")).unwrap(),
        "\n<!doctype html><h1>Plain</h1>\n"
    );
    assert_eq!(
        std::fs::read_to_string(h.project.join("theme.css")).unwrap(),
        "body{}"
    );
    let end = run.end_payload().unwrap();
    assert_eq!(end.artifact_paths.len(), 2);
    let events = events_of(&run).await;
    let writes: Vec<&Value> = agent_events(&events, "tool_use");
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0]["name"], "Write");
}

// ------------------------------------------------------------------ BYOK

/// Serves one canned response per connection on 127.0.0.1 and returns the
/// raw requests it received.
async fn fake_provider(
    responses: Vec<(&'static str, String)>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).into_owned();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let len = text[..head_end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            requests.push(String::from_utf8_lossy(&buf).into_owned());
            let content_type = if status.starts_with("200") {
                "text/event-stream"
            } else {
                "application/json"
            };
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n{body}"
            );
            sock.write_all(response.as_bytes()).await.unwrap();
            sock.shutdown().await.unwrap();
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

fn sse(events: &[(&str, &str)]) -> String {
    events
        .iter()
        .map(|(name, data)| format!("event: {name}\ndata: {data}\n\n"))
        .collect()
}

fn request_body(raw: &str) -> Value {
    serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

fn anthropic_answer() -> String {
    sse(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_1","type":"message","usage":{"input_tokens":120}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Here.\n<artifact identifier=\"index\" type=\"text/html\" title=\"T\">"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<!doctype html><h1>BYOK</h1>"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"</artifact>"}}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        (
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ])
}

fn byok_spec(h: &Harness, id: &str, config: ByokConfig) -> LaunchSpec {
    let mut spec = h.spec(id, &BYOK, &[]);
    spec.byok = Some(config);
    spec
}

#[tokio::test]
async fn byok_anthropic_streams_into_one_artifact_file() {
    let h = Harness::new();
    let (base, server) = fake_provider(vec![("200 OK", anthropic_answer())]).await;
    let spec = byok_spec(
        &h,
        "byok-a",
        ByokConfig {
            protocol: ByokProtocol::Anthropic,
            api_key: "sk-ant-test-0000".into(),
            base_url: Some(base),
            model: None,
            max_tokens: None,
        },
    );
    let run = spec.run.clone();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");

    let requests = server.await.unwrap();
    let raw = &requests[0];
    assert!(raw.starts_with("POST /v1/messages HTTP/1.1"), "{raw}");
    let lower = raw.to_ascii_lowercase();
    assert!(lower.contains("x-api-key: sk-ant-test-0000"));
    assert!(lower.contains("anthropic-version: "));
    let body = request_body(raw);
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["stream"], true);
    assert!(body["system"].as_str().unwrap().contains("<artifact"));
    assert_eq!(body["messages"][0]["content"], "Build a landing page");

    assert_eq!(
        std::fs::read_to_string(h.project.join("index.html")).unwrap(),
        "<!doctype html><h1>BYOK</h1>"
    );
    let end = run.end_payload().unwrap();
    assert_eq!(
        end.artifact_paths,
        [h.project.join("index.html").to_string_lossy().into_owned()]
    );
    let events = events_of(&run).await;
    assert!(agent_text(&events).starts_with("Here."));
    let usage = agent_events(&events, "usage");
    assert_eq!(usage[0]["usage"]["output_tokens"], 42);
    assert_eq!(usage[0]["usage"]["input_tokens"], 120);
    // The key never reaches the event log.
    for ev in &events {
        assert!(!ev.data.to_string().contains("sk-ant-test-0000"));
    }
}

#[tokio::test]
async fn byok_openai_compatible_streams_chat_completions() {
    let h = Harness::new();
    let chunks = [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"thinking"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"<artifact identifier=\"index\" type=\"text/html\">"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"<p>OAI</p></artifact>"},"finish_reason":"stop"}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}}"#,
        "[DONE]",
    ];
    let body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    let (base, server) = fake_provider(vec![("200 OK", body)]).await;
    let spec = byok_spec(
        &h,
        "byok-o",
        ByokConfig {
            protocol: ByokProtocol::OpenAi,
            api_key: "sk-oai-test".into(),
            base_url: Some(format!("{base}/v1")),
            model: Some("gpt-test".into()),
            max_tokens: None,
        },
    );
    let run = spec.run.clone();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    let raw = &server.await.unwrap()[0];
    assert!(
        raw.starts_with("POST /v1/chat/completions HTTP/1.1"),
        "{raw}"
    );
    assert!(
        raw.to_ascii_lowercase()
            .contains("authorization: bearer sk-oai-test")
    );
    let body = request_body(raw);
    assert_eq!(body["model"], "gpt-test");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(
        std::fs::read_to_string(h.project.join("index.html")).unwrap(),
        "<p>OAI</p>"
    );
    let events = events_of(&run).await;
    assert_eq!(agent_events(&events, "thinking_delta").len(), 1);
    assert_eq!(
        agent_events(&events, "usage")[0]["usage"]["output_tokens"],
        3
    );
}

#[tokio::test]
async fn byok_rejected_key_fails_without_retry() {
    let h = Harness::new();
    let (base, server) = fake_provider(vec![(
        "401 Unauthorized",
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#
            .to_owned(),
    )])
    .await;
    let spec = byok_spec(
        &h,
        "byok-401",
        ByokConfig {
            protocol: ByokProtocol::Anthropic,
            api_key: "sk-bad".into(),
            base_url: Some(base),
            model: None,
            max_tokens: None,
        },
    );
    let run = spec.run.clone();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.failure_category, Some(FailureCategory::AgentError));
    assert_eq!(server.await.unwrap().len(), 1);
    let events = events_of(&run).await;
    let errors: Vec<&Arc<RunEvent>> = events.iter().filter(|e| e.event == sse::ERROR).collect();
    assert_eq!(errors.len(), 1);
    let message = errors[0].data["message"].as_str().unwrap();
    assert!(message.contains("rejected the API key"), "{message}");
    assert!(message.contains("invalid x-api-key"));
    assert!(!h.project.join("index.html").exists());
}

#[tokio::test]
async fn byok_transient_error_is_retried_once() {
    let h = Harness::new();
    let (base, server) = fake_provider(vec![
        (
            "529 Overloaded",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
                .to_owned(),
        ),
        ("200 OK", anthropic_answer()),
    ])
    .await;
    let spec = byok_spec(
        &h,
        "byok-retry",
        ByokConfig {
            protocol: ByokProtocol::Anthropic,
            api_key: "k".into(),
            base_url: Some(base),
            model: Some("claude-sonnet-5-5".into()),
            max_tokens: Some(1000),
        },
    );
    let run = spec.run.clone();
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Succeeded, "{outcome:?}");
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(request_body(&requests[1])["model"], "claude-sonnet-5-5");
    assert_eq!(request_body(&requests[1])["max_tokens"], 1000);
    let events = events_of(&run).await;
    assert!(events.iter().any(|e| e.event == sse::RUN_RETRY_ATTEMPTED));
    assert!(
        !events.iter().any(|e| e.event == sse::ERROR),
        "a retried attempt's error is not shown"
    );
}

#[tokio::test]
async fn byok_without_a_key_fails_fast() {
    let h = Harness::new();
    let spec = h.spec("byok-none", &BYOK, &[]);
    let outcome = h.engine.launch(spec).await.unwrap();
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.failure_category, Some(FailureCategory::SpawnFailed));
}
