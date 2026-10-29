//! Run engine end to end through HTTP + SSE with mock CLIs (spec 5.7 M2).
//! No test calls a real `claude` or `codex`: shell mocks sit first on PATH.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // helpers of the `sh` mock tests

mod common;

use common::*;
use od_contracts::AgentEvent;
use od_runtime::parse::{ClaudeParser, parse_all};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn claude_hello(log: &std::path::Path) -> String {
    let log = log.display();
    format!(
        r#"{PROBES}
echo "$*" >> "{log}/argv.log"
{PARSE_SESSION}
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"hello"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
cat >/dev/null
"#
    )
}

fn argv_lines(d: &Daemon) -> Vec<String> {
    std::fs::read_to_string(d.mocks.join("argv.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn flag_value(argv: &str, flag: &str) -> Option<String> {
    let parts: Vec<&str> = argv.split_whitespace().collect();
    parts
        .windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].to_owned())
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn second_turn_resumes_and_a_model_change_does_not() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_hello(&d.mocks));
    let (project, conversation) = d.project().await;
    let body = |msg: &str, model: &str| {
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": msg, "model": model})
    };

    let first = d.start(body("make a landing page", "sonnet")).await;
    assert!(!first.resumed);
    let events = d.events(&first.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded");
    d.settled_messages(&project, &conversation).await;

    let second = d.start(body("make it blue", "sonnet")).await;
    assert!(second.resumed);
    assert_eq!(
        end_of(&d.events(&second.run_id, None).await)["status"],
        "succeeded"
    );
    d.settled_messages(&project, &conversation).await;

    let third = d.start(body("now in opus", "opus")).await;
    assert!(!third.resumed);
    assert_eq!(
        end_of(&d.events(&third.run_id, None).await)["status"],
        "succeeded"
    );
    let messages = d.settled_messages(&project, &conversation).await;

    let argv = argv_lines(&d);
    assert_eq!(argv.len(), 3, "{argv:?}");
    let first_session = flag_value(&argv[0], "--session-id").unwrap();
    assert!(flag_value(&argv[0], "--resume").is_none());
    assert_eq!(
        flag_value(&argv[1], "--resume"),
        Some(first_session.clone())
    );
    assert!(flag_value(&argv[1], "--session-id").is_none());
    assert!(flag_value(&argv[2], "--resume").is_none());
    let third_session = flag_value(&argv[2], "--session-id").unwrap();
    assert_ne!(third_session, first_session);
    assert_eq!(flag_value(&argv[2], "--model").as_deref(), Some("opus"));
    for a in &argv {
        assert!(a.contains("--include-partial-messages"), "{a}");
        assert_eq!(
            flag_value(a, "--permission-mode").as_deref(),
            Some("bypassPermissions")
        );
    }

    // Turn 2 continues the session: no instructions, no transcript.
    // Turn 3 starts fresh: instructions plus the transcript so far.
    let prompts: Vec<Value> = std::fs::read_to_string(d.mocks.join("prompts.log"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let text = |i: usize| {
        prompts[i]["message"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(text(0).starts_with("# Instructions (read first)"));
    assert_eq!(text(1), "# User request\n\nmake it blue");
    assert!(text(2).starts_with("# Instructions (read first)"));
    assert!(
        text(2).contains(
            "# Conversation so far\n\n## user\nmake a landing page\n\n## assistant\nhello"
        )
    );

    // Messages persisted with their events, folded from batches.
    assert_eq!(messages.len(), 6);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "hello");
    assert_eq!(messages[1]["runStatus"], "succeeded");
    assert!(
        messages[1]["events"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    );
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn sse_reconnect_with_last_event_id_has_no_gap_or_duplicate() {
    let d = daemon().await;
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
IFS= read -r line
i=0
while [ $i -lt 60 ]; do
  printf '{{"type":"assistant","message":{{"id":"m%s","content":[{{"type":"text","text":"t%s"}}]}}}}\n' $i $i
  i=$((i+1))
  sleep 0.01
done
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    // Disconnect at k while the run is live…
    let k = 12;
    let mut first = d.stream(&run.run_id, None).await;
    let mut seen = Vec::new();
    while let Some(ev) = first.next().await {
        seen.push(ev.id);
        if ev.id == k {
            break;
        }
    }
    drop(first);
    // …reconnect with Last-Event-ID: k.
    let rest = d.events(&run.run_id, Some(k)).await;
    assert_eq!(rest.first().map(|e| e.id), Some(k + 1));
    seen.extend(rest.iter().map(|e| e.id));
    let last = rest.last().unwrap().id;
    assert_eq!(seen, (1..=last).collect::<Vec<_>>());
    assert_eq!(end_of(&rest)["status"], "succeeded");
    let texts: Vec<String> = agent_payloads(&d.events(&run.run_id, None).await)
        .iter()
        .filter(|p| p["type"] == "text_delta")
        .map(|p| p["delta"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(texts.len(), 60);

    // `?after=N` is the same cursor; a finished run replays and closes.
    let res = d
        .http
        .get(format!(
            "{}/api/runs/{}/events?after={}",
            d.base,
            run.run_id,
            last - 2
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.headers()["cache-control"].to_str().unwrap(),
        "no-cache, no-transform"
    );
    assert_eq!(res.headers()["x-accel-buffering"].to_str().unwrap(), "no");
    let tail = d.events(&run.run_id, Some(last - 2)).await;
    assert_eq!(
        tail.iter().map(|e| e.id).collect::<Vec<_>>(),
        [last - 1, last]
    );
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn cancel_kills_the_whole_tree_within_3_5_s() {
    let d = daemon().await;
    let pids = d.mocks.join("pids");
    // Ignores SIGTERM (so does everything it spawns): the engine must
    // escalate to SIGKILL on the whole process group.
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
trap '' TERM
IFS= read -r line
sleep 300 &
echo $! >> "{pids}"
sh -c 'sleep 300 & echo $! >> "{pids}"; wait' &
echo $! >> "{pids}"
echo $$ >> "{pids}"
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
wait
"#,
            pids = pids.display()
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let mut reader = d.stream(&run.run_id, None).await;
    // Wait until the agent is up and has spawned its children.
    while let Some(ev) = reader.next().await {
        if ev.event == "agent" && ev.data["type"] == "status" {
            break;
        }
    }
    for _ in 0..100 {
        if std::fs::read_to_string(&pids)
            .unwrap_or_default()
            .lines()
            .count()
            >= 4
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let pids: Vec<u32> = std::fs::read_to_string(&pids)
        .unwrap()
        .lines()
        .map(|l| l.trim().parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 4);
    assert!(pids.iter().all(|p| pid_alive(*p)));

    let started = Instant::now();
    let res = d
        .post(&format!("/api/runs/{}/cancel", run.run_id), &json!({}))
        .await;
    assert_eq!(res.status().as_u16(), 200);
    // A canceled run takes no more input, even before it has exited.
    let res = d
        .post(
            &format!("/api/runs/{}/steer", run.run_id),
            &json!({"message": "x"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 409);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "RUN_TERMINAL");
    let mut end = None;
    while let Some(ev) = reader.next().await {
        if ev.event == "end" {
            end = Some(ev.data);
        }
    }
    while pids.iter().any(|p| pid_alive(*p)) && started.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let elapsed = started.elapsed();
    assert!(
        pids.iter().all(|p| !pid_alive(*p)),
        "survivors after {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_millis(3500),
        "tree died after {elapsed:?}"
    );
    assert_eq!(end.unwrap()["status"], "canceled");
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn steer_reaches_open_stdin_and_is_409_once_it_closed() {
    let d = daemon().await;
    // Turn 1 waits for a steer line, echoes it, ends the turn, then lingers
    // after stdin closes so the run is still alive.
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
printf '{{"type":"stream_event","event":{{"type":"message_start","message":{{"id":"m0"}}}}}}\n'
printf '{{"type":"stream_event","event":{{"type":"message_delta","delta":{{"stop_reason":"tool_use"}}}}}}\n'
IFS= read -r steer
case "$steer" in *"also add a footer"*) t="steered";; *) t="other";; esac
printf '{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"text","text":"%s"}}]}}}}\n' "$t"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
sleep 2
"#
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let mut reader = d.stream(&run.run_id, None).await;
    // A `tool_use` turn boundary must leave stdin open.
    while let Some(ev) = reader.next().await {
        if ev.event == "agent" && ev.data["type"] == "turn_end" {
            assert_eq!(ev.data["stopReason"], "tool_use");
            break;
        }
    }
    let steer = |msg: &'static str| {
        let d = &d;
        let id = run.run_id.clone();
        async move {
            d.post(&format!("/api/runs/{id}/steer"), &json!({"message": msg}))
                .await
        }
    };
    assert_eq!(steer("also add a footer").await.status().as_u16(), 202);
    let mut saw = false;
    while let Some(ev) = reader.next().await {
        if ev.event == "agent" && ev.data["type"] == "text_delta" {
            assert_eq!(ev.data["delta"], "steered");
        }
        if ev.event == "agent" && ev.data["type"] == "usage" {
            saw = true;
            break;
        }
    }
    assert!(saw);
    // The turn ended, the engine closed stdin; the process still runs.
    let res = steer("one more thing").await;
    assert_eq!(res.status().as_u16(), 409);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "STDIN_CLOSED");
    assert_eq!(body["error"]["message"], "stdin_closed");
    while reader.next().await.is_some() {}
    // After the end: run_terminal.
    let res = steer("too late").await;
    assert_eq!(res.status().as_u16(), 409);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "RUN_TERMINAL");
}

fn codex_mock(d: &Daemon) -> String {
    let fixture = fixture("codex/turn-exec-json.jsonl");
    format!(
        r#"{PROBES}
echo "$*" >> "{log}/argv.log"
cat > "{log}/codex-prompt.txt"
case "$(cat "{log}/codex-prompt.txt")" in *linger*) sleep 1;; esac
cat "{fixture}"
"#,
        log = d.mocks.display(),
        fixture = fixture.display()
    )
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn codex_command_execution_streams_as_bash_tool_use_and_result() {
    let d = daemon().await;
    write_mock(&d.mocks, "codex", &codex_mock(&d));
    let (project, conversation) = d.project().await;
    let run = d
        .start(json!({"agentId": "codex", "projectId": project, "message": "run echo hi"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded");
    let agent = agent_payloads(&events);
    let use_idx = agent
        .iter()
        .position(|p| p["type"] == "tool_use" && p["name"] == "Bash")
        .unwrap();
    assert_eq!(agent[use_idx]["id"], "item_3");
    assert_eq!(agent[use_idx]["input"]["command"], "/bin/zsh -lc 'echo hi'");
    let result_idx = agent
        .iter()
        .position(|p| p["type"] == "tool_result" && p["toolUseId"] == "item_3")
        .unwrap();
    assert!(use_idx < result_idx);
    assert_eq!(agent[result_idx]["content"], "hi\n");
    assert_eq!(agent[result_idx]["isError"], false);
    // Prompt went in on stdin (then EOF).
    let prompt = std::fs::read_to_string(d.mocks.join("codex-prompt.txt")).unwrap();
    assert!(prompt.ends_with("# User request\n\nrun echo hi"));
    // The captured thread id resumes the next turn: `exec resume … <id>`.
    d.settled_messages(&project, &conversation).await;
    let next = d
        .start(
            json!({"agentId": "codex", "projectId": project, "conversationId": conversation,
                      "message": "again"}),
        )
        .await;
    assert!(next.resumed);
    d.events(&next.run_id, None).await;
    let argv = argv_lines(&d);
    assert!(argv[0].starts_with("exec --json --skip-git-repo-check --sandbox workspace-write"));
    assert!(argv[1].starts_with("exec resume --json"));
    assert!(argv[1].ends_with("01a04341-7b7e-7750-aeb4-721b12024ac2"));
    // Codex has no steering channel, even while it runs.
    let live = d
        .start(
            json!({"agentId": "codex", "projectId": project, "conversationId": conversation,
                      "message": "linger"}),
        )
        .await;
    let res = d
        .post(
            &format!("/api/runs/{}/steer", live.run_id),
            &json!({"message": "x"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 409);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "RUNTIME_UNSUPPORTED");
    d.events(&live.run_id, None).await;
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn reused_client_request_id_returns_the_same_run() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_hello(&d.mocks));
    let (project, _) = d.project().await;
    let body = json!({"agentId": "claude", "projectId": project, "message": "hi",
                      "clientRequestId": "req-42"});
    let first = d.start(body.clone()).await;
    assert!(!first.reused);
    let again = d.start(body).await;
    assert!(again.reused);
    assert_eq!(again.run_id, first.run_id);
    assert_eq!(again.assistant_message_id, first.assistant_message_id);
    // Same key, different request: conflict.
    let res = d
        .post(
            "/api/runs",
            &json!({"agentId": "claude", "projectId": project, "message": "other",
                    "clientRequestId": "req-42"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 409);
    let err: Value = res.json().await.unwrap();
    assert_eq!(err["error"]["code"], "IDEMPOTENCY_CONFLICT");
    d.events(&first.run_id, None).await;
    // Exactly one agent process ran.
    assert_eq!(argv_lines(&d).len(), 1);
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn recorded_claude_stream_reaches_sse_as_the_golden_events() {
    let d = daemon().await;
    let rec = fixture("claude-cli-recordings/claude-2.1.259-partial-two-turns.jsonl");
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            "{PROBES}\nIFS= read -r line\ncat \"{}\"\ncat >/dev/null\n",
            rec.display()
        ),
    );
    let (project, conversation) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(events[0].event, "start");
    assert_eq!(events[0].data["protocolVersion"], 1);
    let expected: Vec<Value> = parse_all(
        &mut ClaudeParser::default(),
        &std::fs::read_to_string(rec).unwrap(),
    )
    .into_iter()
    .filter(|e| !matches!(e, AgentEvent::Error { .. }))
    .map(|e| serde_json::to_value(e).unwrap())
    .collect();
    assert_eq!(agent_payloads(&events), expected);
    let end = end_of(&events);
    assert_eq!(end["status"], "succeeded");
    assert!(
        end["artifactPaths"][0]
            .as_str()
            .unwrap()
            .ends_with("alpha.html")
    );
    // The stored message keeps the events minus live-only input deltas.
    let messages = d.settled_messages(&project, &conversation).await;
    let stored = messages[1]["events"].as_array().unwrap();
    assert!(stored.iter().any(|e| e["type"] == "tool_use"));
    assert!(stored.iter().all(|e| e["type"] != "tool_input_delta"));
    assert!(
        agent_payloads(&events)
            .iter()
            .any(|e| e["type"] == "tool_input_delta")
    );
    assert_eq!(
        messages[1]["content"],
        "DONE1<artifact type=\"text/html\" title=\"Beta\">\n<!doctype html><html><body><h1>Beta</h1></body></html>\n</artifact>"
    );
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn transient_failure_without_output_is_retried_once() {
    let d = daemon().await;
    let count = d.mocks.join("count");
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
n=$(cat "{c}" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "{c}"
if [ $n -eq 1 ]; then echo "API Error: 429 Too Many Requests" >&2; exit 1; fi
IFS= read -r line
printf '{{"type":"assistant","message":{{"id":"m","content":[{{"type":"text","text":"ok"}}]}}}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#,
            c = count.display()
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    let retry = events
        .iter()
        .find(|e| e.event == "run_retry_attempted")
        .unwrap();
    assert_eq!(retry.data["retry_attempt_index"], 1);
    assert_eq!(retry.data["failure_category"], "rate_limit");
    assert!(
        events.iter().all(|e| e.event != "error"),
        "retried error must stay hidden"
    );
    assert_eq!(end_of(&events)["status"], "succeeded");
    assert_eq!(std::fs::read_to_string(count).unwrap().trim(), "2");
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn failed_resume_silently_reseeds_a_fresh_session() {
    let d = daemon().await;
    let log = d.mocks.display().to_string();
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
echo "$*" >> "{log}/argv.log"
{PARSE_SESSION}
case "$*" in *--resume*)
  echo "No conversation found with session ID: $sid" >&2; exit 1;;
esac
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"hello"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#
        ),
    );
    let (project, conversation) = d.project().await;
    let body = |m: &str| json!({"agentId": "claude", "projectId": project, "conversationId": conversation, "message": m});
    let first = d.start(body("one")).await;
    d.events(&first.run_id, None).await;
    d.settled_messages(&project, &conversation).await;
    let second = d.start(body("two")).await;
    assert!(second.resumed);
    let events = d.events(&second.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded");
    assert!(events.iter().all(|e| e.event != "error"));
    let argv = argv_lines(&d);
    assert_eq!(argv.len(), 3);
    assert!(flag_value(&argv[1], "--resume").is_some());
    assert!(flag_value(&argv[2], "--session-id").is_some());
    // The fresh session got the full transcript.
    let prompts = std::fs::read_to_string(d.mocks.join("prompts.log")).unwrap();
    let last: Value = serde_json::from_str(prompts.lines().last().unwrap()).unwrap();
    let text = last["message"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("## user\none") && text.ends_with("# User request\n\ntwo"));
    // And the stored session now points at it.
    d.settled_messages(&project, &conversation).await;
    let stored = d
        .state
        .store
        .get_agent_session(&conversation, "claude")
        .unwrap()
        .unwrap();
    assert_eq!(
        Some(stored.session_id),
        flag_value(&argv[2], "--session-id")
    );
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn silent_agent_times_out() {
    let d = daemon_with(Options {
        idle_timeout: Some(Duration::from_millis(300)),
        kill_grace: Duration::from_millis(500),
        ..Options::default()
    })
    .await;
    write_mock(
        &d.mocks,
        "claude",
        &format!("{PROBES}\nIFS= read -r line\nsleep 30\n"),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    let end = end_of(&events);
    assert_eq!(end["status"], "failed");
    assert_eq!(end["failureCategory"], "timeout");
    assert!(events.iter().any(|e| e.event == "error"));
    // A timeout is not transient: no retry.
    assert!(events.iter().all(|e| e.event != "run_retry_attempted"));
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn agent_env_permission_mode_and_skill_staging() {
    let content = tempfile::tempdir().unwrap();
    let skill = content.path().join("skills/mock-deck");
    std::fs::create_dir_all(skill.join("assets")).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: mock-deck\ndescription: A test skill.\n---\n\nRead assets/template.html first.\n",
    )
    .unwrap();
    std::fs::write(skill.join("assets/template.html"), "<html></html>").unwrap();
    let d = daemon_with(Options {
        content_root: Some(content.path().to_path_buf()),
        extra_env: vec![("OD_API_TOKEN".into(), "do-not-leak".into())],
        ..Options::default()
    })
    .await;
    std::fs::write(
        d.data.join("app-config.json"),
        r#"{"agent_permission_mode": "ask"}"#,
    )
    .unwrap();
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
echo "$*" >> "{log}/argv.log"
env | grep -E '^(OD_|CLAUDE_CODE)' | sort > "{log}/env.txt"
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#,
            log = d.mocks.display()
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(
            json!({"agentId": "claude", "projectId": project, "message": "go",
                      "skillId": "mock-deck"}),
        )
        .await;
    assert_eq!(
        end_of(&d.events(&run.run_id, None).await)["status"],
        "succeeded"
    );
    let argv = argv_lines(&d);
    assert_eq!(
        flag_value(&argv[0], "--permission-mode").as_deref(),
        Some("acceptEdits")
    );
    let env = std::fs::read_to_string(d.mocks.join("env.txt")).unwrap();
    assert!(!env.contains("OD_API_TOKEN"), "{env}");
    for key in [
        "OD_DATA_DIR=",
        "OD_DAEMON_URL=http://127.0.0.1:",
        "OD_TOOL_TOKEN=odt_",
        "OD_PROJECT_DIR=",
        "OD_BIN=",
        "CLAUDE_CODE_ENABLE_TODO_TOOLS=1",
    ] {
        assert!(env.contains(key), "missing {key} in {env}");
    }
    assert!(env.contains(&format!("OD_PROJECT_ID={project}")));
    // Staged as a real copy under the project, named like the preamble.
    let staged = d.data.join("projects").join(&project).join(".od-skills");
    let entry = std::fs::read_dir(&staged).unwrap().next().unwrap().unwrap();
    assert!(
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("mock-deck-")
    );
    assert!(!entry.path().is_symlink());
    assert!(entry.path().join("assets/template.html").is_file());
    let prompt = std::fs::read_to_string(d.mocks.join("prompts.log")).unwrap();
    assert!(prompt.contains("## Active skill — mock-deck"));
    assert!(prompt.contains(".od-skills/mock-deck-"));
    // The run's tool token was revoked when it ended.
    let token = env
        .lines()
        .find_map(|l| l.strip_prefix("OD_TOOL_TOKEN="))
        .unwrap()
        .to_owned();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        d.state
            .runs
            .tokens
            .authorize(&token, "/api/tools/x")
            .is_none()
    );
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn agents_endpoint_detects_mocks_and_missing_binaries() {
    let d = daemon().await;
    // Nothing installed yet: no agent found for setup.
    let none = d.get_json("/api/agents").await;
    assert!(
        none["agents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["available"] == false),
        "{none}"
    );
    let steps = d.get_json("/api/usage").await["setup"]["steps"].clone();
    assert_eq!(steps.as_array().unwrap().len(), 1, "{steps}");
    write_mock(&d.mocks, "claude", &claude_hello(&d.mocks));
    let body = d.get_json("/api/agents").await;
    let agents = body["agents"].as_array().unwrap();
    let claude = agents.iter().find(|a| a["id"] == "claude").unwrap();
    assert_eq!(claude["available"], true);
    assert_eq!(claude["version"], "mock 1.0");
    assert_eq!(
        claude["capabilities"],
        json!(["--include-partial-messages"])
    );
    assert_eq!(claude["supportsSteer"], true);
    let codex = agents.iter().find(|a| a["id"] == "codex").unwrap();
    assert_eq!(codex["supportsSteer"], false);
    // Finding an agent is setup step 2 (Settings ▸ About ▸ Your usage).
    let steps = d.get_json("/api/usage").await["setup"]["steps"].clone();
    assert_eq!(steps[1]["step"], "agent_found", "{steps}");
}

/// Onboarding ▸ Check again in the app: an agent installed after start in a
/// folder only the login shell's new `PATH` names (a new nvm Node) is not
/// found by a plain detection, and is found once rescan asks the shell.
#[cfg(unix)]
#[tokio::test]
async fn rescan_asks_the_login_shell_and_finds_a_new_agent() {
    let shell_dir = tempfile::tempdir().unwrap();
    let late = shell_dir.path().join("nvm-v24-bin");
    std::fs::create_dir_all(&late).unwrap();
    let shell = shell_dir.path().join("fake-shell");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nPATH={}:/usr/bin\nexport PATH\nshift\neval \"$1\"\n",
            late.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A new binary's first exec can take seconds on a loaded Mac (it scans
    // it); run it once so the rescan's 3 s login shell limit is not that.
    std::process::Command::new(&shell)
        .args(["-ilc", "true"])
        .status()
        .unwrap();
    let d = daemon_with(Options {
        login_shell: Some(shell),
        ..Options::default()
    })
    .await;
    let claude = |v: &Value| {
        v["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "claude")
            .cloned()
            .unwrap()
    };
    write_mock(&late, "claude", &claude_hello(&d.mocks));
    let before = d.get_json("/api/agents").await;
    assert_eq!(
        claude(&before)["available"],
        false,
        "found without a rescan"
    );
    let res = d.post("/api/agents/rescan", &json!({})).await;
    assert_eq!(res.status(), 200);
    let after: Value = res.json().await.unwrap();
    assert_eq!(claude(&after)["available"], true, "{after}");
    assert!(
        claude(&after)["path"]
            .as_str()
            .unwrap()
            .starts_with(late.to_str().unwrap())
    );
}

#[tokio::test]
async fn unknown_agent_and_run_are_rejected() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    let res = d
        .post(
            "/api/runs",
            &json!({"agentId": "nope", "projectId": project, "message": "x"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 400);
    let res = d
        .http
        .get(format!("{}/api/runs/missing/events", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 404);
    let res = d.post("/api/runs/missing/cancel", &json!({})).await;
    assert_eq!(res.status().as_u16(), 404);
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn conversation_that_moved_on_is_not_resumed() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_hello(&d.mocks));
    let (project, conversation) = d.project().await;
    let body = |m: &str| json!({"agentId": "claude", "projectId": project, "conversationId": conversation, "message": m});
    let first = d.start(body("one")).await;
    d.events(&first.run_id, None).await;
    d.settled_messages(&project, &conversation).await;
    // Another reply lands after the session's last message.
    let res = d
        .http
        .put(format!(
            "{}/api/projects/{project}/conversations/{conversation}/messages/other-1",
            d.base
        ))
        .json(&json!({"role": "assistant", "content": "from another agent"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let second = d.start(body("two")).await;
    assert!(!second.resumed);
    d.events(&second.run_id, None).await;
    let argv = argv_lines(&d);
    assert!(flag_value(&argv[1], "--resume").is_none());
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn retry_happens_at_most_once() {
    let d = daemon().await;
    let count = d.mocks.join("count");
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
n=$(cat "{c}" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "{c}"
echo "read ECONNRESET" >&2; exit 1
"#,
            c = count.display()
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    let retries = events
        .iter()
        .filter(|e| e.event == "run_retry_attempted")
        .count();
    assert_eq!(retries, 1);
    let end = end_of(&events);
    assert_eq!(end["status"], "failed");
    assert_eq!(end["failureCategory"], "transient");
    assert_eq!(std::fs::read_to_string(count).unwrap().trim(), "2");
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn failure_after_visible_output_is_not_retried() {
    let d = daemon().await;
    let count = d.mocks.join("count");
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
n=$(cat "{c}" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "{c}"
IFS= read -r line
printf '{{"type":"assistant","message":{{"id":"m","content":[{{"type":"text","text":"partial"}}]}}}}\n'
echo "read ECONNRESET" >&2; exit 1
"#,
            c = count.display()
        ),
    );
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert!(events.iter().all(|e| e.event != "run_retry_attempted"));
    assert_eq!(end_of(&events)["status"], "failed");
    assert_eq!(std::fs::read_to_string(count).unwrap().trim(), "1");
}

/// Decision 1: `auto` by default; Settings › "Ask first" (`PUT
/// /api/app-config`) switches Claude to `acceptEdits` and drops Codex's
/// self-approval flag on the next run.
#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn settings_permission_mode_reaches_the_agent_argv() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_hello(&d.mocks));
    write_mock(&d.mocks, "codex", &codex_mock(&d));
    let (project, _) = d.project().await;
    async fn run(d: &Daemon, project: &str, agent: &str) {
        let body = json!({"agentId": agent, "projectId": project, "message": "go"});
        let run = d.start(body).await;
        let end = end_of(&d.events(&run.run_id, None).await).clone();
        assert_eq!(end["status"], "succeeded", "{agent}: {end}");
    }
    let network = "sandbox_workspace_write.network_access=true";

    run(&d, &project, "claude").await;
    run(&d, &project, "codex").await;
    let res = d
        .http
        .put(format!("{}/api/app-config", d.base))
        .json(&json!({"agentPermissionMode": "ask"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    run(&d, &project, "claude").await;
    run(&d, &project, "codex").await;

    let argv = argv_lines(&d);
    assert_eq!(argv.len(), 4, "{argv:?}");
    assert_eq!(
        flag_value(&argv[0], "--permission-mode").as_deref(),
        Some("bypassPermissions")
    );
    assert!(argv[1].contains(network), "{}", argv[1]);
    assert_eq!(
        flag_value(&argv[2], "--permission-mode").as_deref(),
        Some("acceptEdits")
    );
    assert!(!argv[3].contains(network), "{}", argv[3]);
}
