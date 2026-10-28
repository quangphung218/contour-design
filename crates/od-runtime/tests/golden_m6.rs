//! Golden tests for the M6 runtimes (spec 5.7 M6): hand-written fixtures in
//! each CLI's wire format, as defined by the original app's parsers and
//! test helpers (`json-event-stream.ts`, `copilot-stream.ts`,
//! `dsh-profile/types.ts`, `acp/session.ts`,
//! `tests/helpers/cursor-agent-stream-shapes.ts`).
//!
//! Each fixture has an `*.events.jsonl` golden (and, for protocol
//! sessions, a `*.writes.jsonl` golden of what the client sent back).
//! Regenerate with `OD_UPDATE_GOLDEN=1 cargo test`. The semantic asserts
//! next to each golden say what the snapshot must contain.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use od_contracts::AgentEvent;
use od_runtime::PermissionMode;
use od_runtime::def::StreamFormat;
use od_runtime::parse::{SessionContext, StreamParser, parser_for};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn ctx() -> SessionContext {
    SessionContext {
        prompt: "Build a landing page".to_owned(),
        cwd: PathBuf::from("/proj"),
        request_id: "req-1".to_owned(),
        ..SessionContext::default()
    }
}

/// Feeds a recording line by line, collecting events and stdin writes.
fn replay(
    format: StreamFormat,
    ctx: &SessionContext,
    file: &str,
) -> (Vec<AgentEvent>, Vec<String>) {
    let text = std::fs::read_to_string(fixtures().join(file)).unwrap();
    let mut parser: Box<dyn StreamParser> = parser_for(format, ctx);
    let mut events = Vec::new();
    let mut writes = parser.take_writes();
    for line in text.lines() {
        parser.line(line, &mut events);
        writes.extend(parser.take_writes());
    }
    parser.finish(&mut events);
    (events, writes)
}

fn check_lines(golden: &Path, actual: &str) {
    if std::env::var_os("OD_UPDATE_GOLDEN").is_some() {
        std::fs::write(golden, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(golden)
        .unwrap_or_else(|_| panic!("missing golden {}", golden.display()));
    for (i, (a, e)) in actual.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, e, "{}: line {i} differs", golden.display());
    }
    assert_eq!(
        actual.lines().count(),
        expected.lines().count(),
        "{}: line count differs",
        golden.display()
    );
}

fn check_golden(name: &str, events: &[AgentEvent], writes: &[String]) {
    let golden = fixtures().join("golden");
    let events: String = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect();
    check_lines(&golden.join(format!("{name}.events.jsonl")), &events);
    if !writes.is_empty() {
        check_lines(
            &golden.join(format!("{name}.writes.jsonl")),
            &writes.concat(),
        );
    }
}

fn text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

fn tool_uses(events: &[AgentEvent]) -> Vec<(String, String, Value)> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolUse { id, name, input } => {
                Some((id.clone(), name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect()
}

fn tool_results(events: &[AgentEvent]) -> Vec<(String, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => Some((tool_use_id.clone(), *is_error)),
            _ => None,
        })
        .collect()
}

fn session_ids(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Status {
                session_id: Some(id),
                ..
            } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn errors(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Error { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

fn frames(writes: &[String]) -> Vec<Value> {
    writes
        .iter()
        .map(|w| {
            assert!(w.ends_with('\n'), "one frame per line: {w:?}");
            serde_json::from_str(w.trim_end()).unwrap()
        })
        .collect()
}

#[test]
fn opencode_run_json() {
    let (events, writes) = replay(
        StreamFormat::OpenCodeJson,
        &ctx(),
        "opencode/turn-run-json.jsonl",
    );
    check_golden("opencode-turn-run-json", &events, &writes);
    assert_eq!(session_ids(&events), ["ses_od1"]);
    assert_eq!(
        text(&events),
        "I'll build the landing page.Done: index.html is ready."
    );
    let uses = tool_uses(&events);
    assert_eq!(uses.len(), 2, "the repeated bash frame is deduplicated");
    assert_eq!(uses[0].1, "Write");
    assert_eq!(uses[0].2["file_path"], "/proj/index.html");
    assert_eq!(
        tool_results(&events),
        [("call_write".into(), false), ("call_bash".into(), true)]
    );
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::Usage { cost_usd: Some(c), usage: Some(u), .. }
            if (*c - 0.0123).abs() < 1e-9 && u["cached_read_tokens"] == 800)));
    assert_eq!(errors(&events), ["Invalid API key"]);
    assert!(matches!(events.last(), Some(AgentEvent::Raw { .. })));
}

/// OpenCode 2: no `callID` (the part `id` is the call's) and `path` for
/// the file (captured from `opencode run --standalone` 2.0.25).
#[test]
fn opencode_2_run_json() {
    let (events, _) = replay(
        StreamFormat::OpenCodeJson,
        &ctx(),
        "opencode/turn-run-json-v2.jsonl",
    );
    assert_eq!(session_ids(&events), ["ses_od2"]);
    assert_eq!(text(&events), "I'll update the page.");
    let uses = tool_uses(&events);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1, "Write");
    assert_eq!(uses[0].2["file_path"], "index.html");
    assert_eq!(
        tool_results(&events),
        [("84d06fd2-dfd7-499c-b36a-5f86466e8cf1".into(), false)]
    );
    assert!(
        !events.iter().any(|e| matches!(e, AgentEvent::Raw { .. })),
        "every frame is understood"
    );
}

#[test]
fn cursor_stream_json() {
    let (events, writes) = replay(
        StreamFormat::CursorStreamJson,
        &ctx(),
        "cursor-agent/turn-stream-json.jsonl",
    );
    check_golden("cursor-agent-turn-stream-json", &events, &writes);
    // Repeated deltas are real content; the final replay adds only ", done."
    assert_eq!(text(&events), "haha, done.");
    assert!(
        session_ids(&events).is_empty(),
        "cursor runs are not resumed"
    );
    let uses = tool_uses(&events);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1, "Edit");
    assert_eq!(uses[0].2["file_path"], "/proj/index.html");
    assert_eq!(tool_results(&events), [("tool_edit_1".into(), false)]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ThinkingDelta { .. }))
    );
    assert!(!events.iter().any(|e| matches!(e, AgentEvent::Raw { .. })));
}

#[test]
fn copilot_output_json() {
    let (events, writes) = replay(
        StreamFormat::CopilotJson,
        &ctx(),
        "copilot/turn-output-json.jsonl",
    );
    check_golden("copilot-turn-output-json", &events, &writes);
    assert_eq!(text(&events), "Creating index.html.");
    assert_eq!(
        tool_results(&events),
        [("tc_1".into(), false), ("tc_2".into(), true)]
    );
    assert!(
        matches!(events.last(), Some(AgentEvent::Usage { stop_reason: Some(r), is_error: false, duration_ms: Some(_), .. }) if r == "completed")
    );
}

#[test]
fn dsh_profile_stdio() {
    let (events, writes) = replay(
        StreamFormat::DshJsonl,
        &ctx(),
        "dsh/turn-profile-stdio.jsonl",
    );
    check_golden("dsh-turn-profile-stdio", &events, &writes);
    let sent = frames(&writes);
    assert_eq!(sent.len(), 1, "one execute after ready");
    assert_eq!(sent[0]["type"], "execute");
    assert_eq!(sent[0]["request_id"], "req-1");
    assert_eq!(sent[0]["prompt"], "Build a landing page");
    assert_eq!(session_ids(&events), ["dsh-sess-1"]);
    assert_eq!(
        text(&events),
        "Writing index.html.",
        "other requests are ignored"
    );
    assert_eq!(tool_uses(&events)[0].2["path"], "index.html");
    assert!(errors(&events).is_empty());
}

#[test]
fn dsh_resume_cancel_and_protocol_errors() {
    let mut c = ctx();
    c.resume_session_id = Some("dsh-old".to_owned());
    c.model = Some("deepseek/deepseek-v4-pro".to_owned());
    let mut s = parser_for(StreamFormat::DshJsonl, &c);
    let mut ev = Vec::new();
    assert!(s.abort_line().is_none(), "nothing to cancel before execute");
    let mut s2 = parser_for(StreamFormat::DshJsonl, &c);
    s2.line(r#"{"v":1,"type":"ready","runtime":"open-design","protocol_version":1,"plugin_version":"x","capabilities":{"session_resume":true,"session_cancel":true,"structured_events":true}}"#, &mut ev);
    let exec = frames(&s2.take_writes());
    assert_eq!(exec[0]["resume_session_id"], "dsh-old");
    assert_eq!(
        exec[0]["model"],
        json!({"provider": "deepseek", "id": "deepseek-v4-pro"})
    );
    let cancel: Value = serde_json::from_str(s2.abort_line().unwrap().trim_end()).unwrap();
    assert_eq!(
        cancel,
        json!({"v": 1, "type": "cancel", "request_id": "req-1"})
    );
    s2.line(r#"{"v":1,"type":"result","request_id":"req-1","status":"failed","session_id":"dsh-old","resume_rejected":true}"#, &mut ev);
    assert!(s2.completed());
    assert!(errors(&ev)[0].contains("could not resume"));

    // Content before `ready` is a protocol violation.
    let mut ev = Vec::new();
    s.line(
        r#"{"v":1,"type":"text","request_id":"req-1","content":"x"}"#,
        &mut ev,
    );
    assert!(s.completed());
    assert_eq!(errors(&ev).len(), 1);
}

#[test]
fn acp_json_rpc_full_turn() {
    let (events, writes) = replay(StreamFormat::Acp, &ctx(), "acp/turn-json-rpc.jsonl");
    check_golden("acp-turn-json-rpc", &events, &writes);
    let sent = frames(&writes);
    let methods: Vec<&str> = sent
        .iter()
        .map(|f| f["method"].as_str().unwrap_or("<response>"))
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "session/new",
            "session/prompt",
            "<response>",
            "<response>"
        ]
    );
    assert_eq!(sent[0]["params"]["protocolVersion"], 1);
    assert_eq!(sent[1]["params"], json!({"cwd": "/proj", "mcpServers": []}));
    assert_eq!(sent[2]["params"]["sessionId"], "sess-kimi-1");
    assert_eq!(
        sent[2]["params"]["prompt"][0]["text"],
        "Build a landing page"
    );
    // Auto mode: the session-wide allow option.
    assert_eq!(sent[3]["id"], 0);
    assert_eq!(
        sent[3]["result"]["outcome"],
        json!({"outcome": "selected", "optionId": "allow_always"})
    );
    // fs/* is not advertised, so it is refused instead of hanging the agent.
    assert_eq!(sent[4]["id"], "fs-1");
    assert_eq!(sent[4]["error"]["code"], -32601);

    assert_eq!(session_ids(&events), ["sess-kimi-1"]);
    assert_eq!(
        text(&events),
        "Creating the page. Done.",
        "cumulative snapshot adds only the suffix"
    );
    let uses = tool_uses(&events);
    let names: Vec<&str> = uses.iter().map(|u| u.1.as_str()).collect();
    assert_eq!(names, ["TodoWrite", "Edit", "Bash"]);
    assert_eq!(uses[1].2["file_path"], "/proj/index.html");
    // The unfinished command is flushed as failed when the prompt ends.
    assert_eq!(
        tool_results(&events),
        [("call_w1".into(), false), ("call_x1".into(), true)]
    );
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::Usage { stop_reason: Some(r), usage: Some(u), .. }
            if r == "end_turn" && u["output_tokens"] == 220)));
    assert!(
        !text(&events).contains("late"),
        "updates after completion are dropped"
    );
}

#[test]
fn acp_resume_model_ask_mode_and_cancel() {
    let mut c = ctx();
    c.resume_session_id = Some("sess-old".to_owned());
    c.model = Some("kimi-k2".to_owned());
    c.permission = PermissionMode::Ask;
    let mut s = parser_for(StreamFormat::Acp, &c);
    let mut ev = Vec::new();
    assert!(s.abort_line().is_none(), "no session yet");
    let mut s = parser_for(StreamFormat::Acp, &c);
    s.take_writes();
    s.line(
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        &mut ev,
    );
    let load = frames(&s.take_writes());
    assert_eq!(load[0]["method"], "session/load");
    assert_eq!(load[0]["params"]["sessionId"], "sess-old");
    // `session/load` answers null: the session is the one asked for.
    s.line(r#"{"jsonrpc":"2.0","id":2,"result":null}"#, &mut ev);
    let set = frames(&s.take_writes());
    assert_eq!(set[0]["method"], "session/set_model");
    assert_eq!(set[0]["params"]["modelId"], "kimi-k2");
    // A rejected model switch still prompts.
    s.line(
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"unknown model"}}"#,
        &mut ev,
    );
    let prompt = frames(&s.take_writes());
    assert_eq!(prompt[0]["method"], "session/prompt");
    assert_eq!(prompt[0]["id"], 4);
    assert_eq!(session_ids(&ev), ["sess-old"]);
    // Ask mode: a shell command is rejected.
    s.line(r#"{"jsonrpc":"2.0","id":"p","method":"session/request_permission","params":{"toolCall":{"toolCallId":"t","kind":"execute","title":"rm -rf"},"options":[{"optionId":"ok","kind":"allow_once"},{"optionId":"no","kind":"reject_once"}]}}"#, &mut ev);
    assert_eq!(
        frames(&s.take_writes())[0]["result"]["outcome"]["optionId"],
        "no"
    );
    let cancel: Value = serde_json::from_str(s.abort_line().unwrap().trim_end()).unwrap();
    assert_eq!(
        cancel,
        json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": "sess-old"}})
    );
    assert!(!s.completed());
    s.line(
        r#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"cancelled"}}"#,
        &mut ev,
    );
    assert!(s.completed());
    let mut tail = Vec::new();
    s.finish(&mut tail);
    assert!(errors(&tail).is_empty());
}

#[test]
fn acp_rpc_error_and_early_exit_fail_the_turn() {
    let mut s = parser_for(StreamFormat::Acp, &ctx());
    let mut ev = Vec::new();
    s.line(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Authentication required","data":{"message":"run kimi login"}}}"#, &mut ev);
    assert!(s.completed());
    assert_eq!(errors(&ev), ["Authentication required: run kimi login"]);

    let mut s = parser_for(StreamFormat::Acp, &ctx());
    let mut ev = Vec::new();
    s.finish(&mut ev);
    assert_eq!(errors(&ev), ["ACP session exited before completion"]);
}

#[test]
fn kiro_turn_end_completes_the_prompt() {
    let mut c = ctx();
    c.acp_turn_end_completes = true;
    let mut s = parser_for(StreamFormat::Acp, &c);
    let mut ev = Vec::new();
    for line in [
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"k"}}"#,
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"turn_end"}}}"#,
    ] {
        s.line(line, &mut ev);
    }
    assert!(s.completed());
}

#[test]
fn plain_text_is_answer_text() {
    let (events, writes) = replay(StreamFormat::Plain, &ctx(), "plain/qwen-answer.txt");
    assert!(writes.is_empty());
    let all = text(&events);
    let original = std::fs::read_to_string(fixtures().join("plain/qwen-answer.txt")).unwrap();
    // CRLF when git checks the fixture out on Windows; the replay reads lines.
    let original = original.replace("\r\n", "\n");
    assert_eq!(all, original);
    let artifacts = od_runtime::artifact::extract(&all);
    let names: Vec<&str> = artifacts.iter().map(|a| a.file_name.as_str()).collect();
    assert_eq!(
        names,
        ["index.html", "theme.css"],
        "the fenced example is skipped"
    );
}
