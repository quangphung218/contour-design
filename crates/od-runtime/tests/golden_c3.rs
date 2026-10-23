//! Golden tests for the track C3 runtimes: Qoder, Pi, Antigravity, Amp,
//! Codebuddy, MiMo, and the plain-text CLIs (Aider, DeepSeek TUI, Grok
//! Build, AtomCode). Antigravity's fixtures are the original app's agy
//! 1.3.1 recordings (`tests/fixtures/antigravity-stream-json`); the others
//! are hand-written in each CLI's wire format as its original parser reads
//! it (`qoder-stream.ts`, `pi-rpc/events.ts`, `json-event-stream.ts`).
//!
//! Regenerate with `OD_UPDATE_GOLDEN=1 cargo test`. The asserts next to
//! each golden say what the snapshot must contain. Every adapter's argv is
//! pinned too, in both permission modes.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use od_contracts::AgentEvent;
use od_runtime::PermissionMode;
use od_runtime::def::{self, ArgvInput, PromptInput, RuntimeDef, SessionIdSource, StreamFormat};
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

/// Wall-clock fields (Pi's ttft and duration) are zeroed for the snapshot.
fn snapshot(events: &[AgentEvent]) -> String {
    events
        .iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap();
            for key in ["ttftMs", "durationMs"] {
                if v.get(key).is_some_and(Value::is_number) {
                    v[key] = json!(0);
                }
            }
            v.to_string() + "\n"
        })
        .collect()
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
    check_lines(&golden.join(format!("{name}.events.jsonl")), &snapshot(events));
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

fn thinking(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ThinkingDelta { delta } => Some(delta.as_str()),
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

fn tool_names(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolUse { name, .. } => Some(name.clone()),
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
fn qoder_stream_json() {
    let (events, writes) = replay(
        StreamFormat::QoderStreamJson,
        &ctx(),
        "qoder/turn-stream-json.jsonl",
    );
    check_golden("qoder-turn-stream-json", &events, &writes);
    assert_eq!(session_ids(&events), ["qd-sess-1"]);
    assert_eq!(thinking(&events), "Plan the hero first. Then the footer.");
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ThinkingStart))
            .count(),
        1
    );
    // A string `content` is text too; multibyte text survives.
    assert_eq!(text(&events), "Writing index.html. Done — ✓ 完成.");
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::Usage { is_error: false, cost_usd: Some(c), duration_ms: Some(d), .. }
            if (*c - 0.0042).abs() < 1e-9 && (*d - 4210.0).abs() < 1e-9)));
    // The `user` echo and the non-JSON line stay raw.
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Raw { .. }))
            .count(),
        2
    );
    assert!(errors(&events).is_empty());
}

#[test]
fn qoder_errors() {
    let (events, writes) = replay(
        StreamFormat::QoderStreamJson,
        &ctx(),
        "qoder/turn-error.jsonl",
    );
    check_golden("qoder-turn-error", &events, &writes);
    assert_eq!(
        errors(&events),
        ["quota exceeded", "Qoder account quota exceeded"]
    );
    assert!(matches!(
        events.last(),
        Some(AgentEvent::Error { terminal: true, .. })
    ));
}

#[test]
fn pi_rpc_turn() {
    let (events, writes) = replay(StreamFormat::PiRpc, &ctx(), "pi/turn-rpc.jsonl");
    check_golden("pi-turn-rpc", &events, &writes);
    let sent = frames(&writes);
    assert_eq!(sent[0], json!({"id": 1, "type": "prompt", "message": "Build a landing page"}));
    // get_state once the prompt is accepted; dialogs answered, notify not.
    assert_eq!(sent[1], json!({"id": 2, "type": "get_state"}));
    assert_eq!(sent[2]["type"], "extension_ui_response");
    assert_eq!((sent[2]["id"].clone(), sent[2]["confirmed"].clone()), (json!("ui-1"), json!(true)));
    assert_eq!((sent[3]["id"].clone(), sent[3]["value"].clone()), (json!("ui-2"), json!("Keep")));
    assert_eq!(sent.len(), 4);
    assert_eq!(text(&events), "Writing index.html.", "nothing after agent_end");
    assert_eq!(thinking(&events), "Hero first.");
    assert_eq!(
        tool_results(&events),
        [("call_1".into(), false), ("call_2".into(), true)]
    );
    assert_eq!(
        session_ids(&events),
        ["/home/u/.pi/agent/sessions/proj/2026-10-24.jsonl"]
    );
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::Usage { usage: Some(u), cost_usd: Some(c), .. }
            if u["cached_read_tokens"] == 300 && u["total_tokens"] == 1320 && (*c - 0.0031).abs() < 1e-9)));
    assert!(errors(&events).is_empty());
    let mut parser = parser_for(StreamFormat::PiRpc, &ctx());
    let mut out = Vec::new();
    for line in std::fs::read_to_string(fixtures().join("pi/turn-rpc.jsonl"))
        .unwrap()
        .lines()
    {
        parser.line(line, &mut out);
    }
    assert!(parser.completed());
    assert_eq!(parser.abort_line(), None, "nothing to abort after agent_end");
}

/// A resumed turn loads the stored session first and sends the prompt only
/// once Pi accepts it; a rejected session fails with the resume marker.
#[test]
fn pi_resume_waits_for_the_parent_session() {
    let resumed = SessionContext {
        resume_session_id: Some("/s/old.jsonl".to_owned()),
        ..ctx()
    };
    let mut parser = parser_for(StreamFormat::PiRpc, &resumed);
    let first = frames(&parser.take_writes());
    assert_eq!(
        first,
        [json!({"id": 1, "type": "switch_session", "sessionPath": "/s/old.jsonl"})]
    );
    let mut out = Vec::new();
    parser.line(r#"{"type":"response","id":1,"success":true}"#, &mut out);
    let next = frames(&parser.take_writes());
    assert_eq!(next[0]["type"], "prompt");
    assert_eq!(next[0]["id"], 2);

    let mut rejected = parser_for(StreamFormat::PiRpc, &resumed);
    rejected.take_writes();
    let mut out = Vec::new();
    rejected.line(
        r#"{"type":"response","id":1,"success":false,"error":"no such session"}"#,
        &mut out,
    );
    assert!(rejected.take_writes().is_empty(), "no prompt after a rejected session");
    assert!(rejected.completed());
    assert!(out.iter().any(|e| matches!(e,
        AgentEvent::Error { code: Some(c), message, .. }
            if c == od_runtime::parse::PI_PARENT_SESSION_FAILED && message.contains("no such session"))));
    assert!(
        def::PI
            .resume_failure_markers
            .contains(&od_runtime::parse::PI_PARENT_SESSION_FAILED)
    );

    // Cancel before the end sends `abort` once.
    let mut live = parser_for(StreamFormat::PiRpc, &ctx());
    live.take_writes();
    let abort: Value = serde_json::from_str(live.abort_line().unwrap().trim_end()).unwrap();
    assert_eq!(abort["type"], "abort");
    assert_eq!(live.abort_line(), None);
    let mut out = Vec::new();
    live.finish(&mut out);
    assert!(errors(&out).is_empty(), "an aborted run is not an early exit");
}

#[test]
fn antigravity_docs_success() {
    let (events, writes) = replay(
        StreamFormat::AntigravityJson,
        &ctx(),
        "antigravity/docs-success.stdout.ndjson",
    );
    check_golden("antigravity-docs-success", &events, &writes);
    assert_eq!(
        session_ids(&events),
        ["c3b66b04-872b-4fbe-a3a4-058a026ef20a"]
    );
    // Text came from the step; the result's copy is not repeated.
    assert_eq!(text(&events), "Git rebase...");
    assert_eq!(tool_names(&events), ["run_command"]);
    assert_eq!(
        tool_results(&events),
        [("edb1c8c1-50ba-4f3f-87eb-412d0e9d47c3:4".into(), false)]
    );
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::Usage { usage: Some(u), duration_ms: Some(d), .. }
            if u["thought_tokens"] == 551 && u["cached_read_tokens"] == 8113 && (*d - 6880.0).abs() < 1e-9)));
    assert!(errors(&events).is_empty());
}

#[test]
fn antigravity_recorded_failures() {
    let (events, writes) = replay(
        StreamFormat::AntigravityJson,
        &ctx(),
        "antigravity/quota-exhausted.stdout.ndjson",
    );
    check_golden("antigravity-quota-exhausted", &events, &writes);
    assert_eq!(
        errors(&events),
        ["Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 148h43m22s."]
    );
    let (events, writes) = replay(
        StreamFormat::AntigravityJson,
        &ctx(),
        "antigravity/signed-out.stdout.ndjson",
    );
    check_golden("antigravity-signed-out", &events, &writes);
    assert_eq!(errors(&events), ["authentication failed or timed out"]);

    // The Gemini JSONL branch some agy-compatible builds print.
    let mut parser = parser_for(StreamFormat::AntigravityJson, &ctx());
    let mut out = Vec::new();
    for line in [
        r#"{"type":"init","model":"gemini-2.5-pro"}"#,
        r#"{"type":"message","role":"assistant","content":"Hi"}"#,
        r#"{"type":"tool_use","tool_id":"g1","tool_name":"write_file","parameters":"{\"file_path\":\"a.html\"}"}"#,
        r#"{"type":"tool_result","tool_id":"g1","status":"error","output":"denied"}"#,
        r#"{"type":"error","severity":"warning","message":"slow"}"#,
        r#"{"type":"result","stats":{"input_tokens":3,"output_tokens":4,"duration_ms":9}}"#,
    ] {
        parser.line(line, &mut out);
    }
    assert_eq!(text(&out), "Hi");
    assert_eq!(tool_results(&out), [("g1".into(), true)]);
    assert!(errors(&out).is_empty(), "a warning is a status");
    assert!(out.iter().any(|e| matches!(e, AgentEvent::Usage { usage: Some(u), .. } if u["output_tokens"] == 4)));
}

#[test]
fn amp_and_codebuddy_speak_claude_stream_json() {
    let (events, writes) = replay(
        def::AMP.stream_format,
        &ctx(),
        "amp/turn-stream-json.jsonl",
    );
    check_golden("amp-turn-stream-json", &events, &writes);
    assert_eq!(text(&events), "Creating the page.Done.");
    assert_eq!(tool_results(&events), [("toolu_1".into(), false)]);
    assert!(errors(&events).is_empty());

    let (events, writes) = replay(
        def::CODEBUDDY.stream_format,
        &ctx(),
        "codebuddy/turn-stream-json.jsonl",
    );
    check_golden("codebuddy-turn-stream-json", &events, &writes);
    assert_eq!(text(&events), "Writing index.html.");
    assert_eq!(thinking(&events), "Small page.");
    assert_eq!(tool_names(&events), ["Write"]);
    assert!(errors(&events).is_empty());
}

#[test]
fn mimo_speaks_opencode_json() {
    let (events, writes) = replay(def::MIMO.stream_format, &ctx(), "mimo/turn-run-json.jsonl");
    check_golden("mimo-turn-run-json", &events, &writes);
    assert_eq!(text(&events), "Writing the page.");
    assert_eq!(tool_names(&events), ["Write"]);
    assert_eq!(tool_results(&events), [("call_m1".into(), false)]);
}

#[test]
fn plain_clis_stream_text() {
    for d in [&def::AIDER, &def::DEEPSEEK_TUI, &def::GROK_BUILD, &def::ATOMCODE] {
        assert_eq!(d.stream_format, StreamFormat::Plain, "{}", d.id);
        let (events, writes) = replay(d.stream_format, &ctx(), "plain-cli/aider-message.txt");
        assert!(text(&events).contains("<artifact identifier=\"landing\""), "{}", d.id);
        if d.id == "aider" {
            check_golden("plain-cli-aider-message", &events, &writes);
        }
    }
    let arts = od_runtime::artifact::extract(&text(
        &replay(StreamFormat::Plain, &ctx(), "plain-cli/aider-message.txt").0,
    ));
    assert_eq!(arts.len(), 1, "the answer's artifact becomes a file");
}

fn argv(d: &RuntimeDef, input: &ArgvInput<'_>) -> String {
    d.build_args(input).join(" ")
}

#[test]
fn every_adapter_argv_in_both_modes() {
    let cwd = Path::new("/proj");
    let dirs = [PathBuf::from("/skills")];
    let file = Path::new("/tmp/od-prompt.md");
    let log = Path::new("/tmp/agy.log");
    let all_caps = [
        def::CLAUDE_PARTIAL_FLAG.to_owned(),
        def::CODEBUDDY_ADD_DIR_FLAG.to_owned(),
        def::ANTIGRAVITY_SKIP_PERMISSIONS_FLAG.to_owned(),
    ];
    let base = ArgvInput {
        cwd: Some(cwd),
        extra_dirs: &dirs,
        capabilities: &all_caps,
        prompt: Some("make it"),
        prompt_file: Some(file),
        log_file: Some(log),
        ..ArgvInput::default()
    };
    let ask = ArgvInput {
        permission: PermissionMode::Ask,
        ..base.clone()
    };
    let with = |model: &'static str, reasoning: Option<&'static str>| ArgvInput {
        model: Some(model),
        reasoning,
        ..base.clone()
    };

    assert_eq!(argv(&def::AMP, &base), "-x --stream-json --dangerously-allow-all");
    assert_eq!(argv(&def::AMP, &ask), "-x --stream-json");
    assert_eq!(
        argv(&def::AMP, &with("deep", None)),
        "-x --stream-json --dangerously-allow-all --mode deep"
    );
    assert!(!argv(&def::AMP, &with("gpt-5", None)).contains("--mode"), "only Amp modes");

    let fresh = ArgvInput {
        new_session_id: Some("s-new"),
        ..with("glm-4.6", Some("high"))
    };
    assert_eq!(
        argv(&def::CODEBUDDY, &fresh),
        "-p --input-format stream-json --output-format stream-json --verbose \
         --include-partial-messages --model glm-4.6 --effort high --add-dir /skills \
         --session-id s-new --permission-mode bypassPermissions"
    );
    let resumed = ArgvInput {
        resume_session_id: Some("s-old"),
        new_session_id: Some("ignored"),
        capabilities: &[],
        permission: PermissionMode::Ask,
        ..base.clone()
    };
    assert_eq!(
        argv(&def::CODEBUDDY, &resumed),
        "-p --input-format stream-json --output-format stream-json --verbose \
         --resume s-old --permission-mode acceptEdits"
    );

    assert_eq!(
        argv(&def::QODER, &with("performance", None)),
        "-p --output-format stream-json --yolo -w /proj --model performance --add-dir /skills"
    );
    assert_eq!(
        argv(&def::QODER, &ask),
        "-p --output-format stream-json -w /proj --add-dir /skills"
    );

    assert_eq!(argv(&def::MIMO, &base), "run --format json");
    assert_eq!(argv(&def::MIMO, &with("mimo-v2", None)), "run --format json --model mimo-v2");

    assert_eq!(argv(&def::PI, &base), "--mode rpc");
    assert_eq!(
        argv(&def::PI, &with("anthropic/claude-sonnet-4-5", Some("high"))),
        "--mode rpc --model anthropic/claude-sonnet-4-5 --thinking high"
    );

    assert_eq!(
        argv(&def::AIDER, &with("sonnet", None)),
        "--yes-always --no-pretty --no-git --no-auto-commits --no-suggest-shell-commands \
         --no-show-model-warnings --model sonnet --message make it"
    );
    assert!(!argv(&def::AIDER, &ask).contains("--yes-always"));
    assert_eq!(
        def::AIDER.build_args(&base).last().map(String::as_str),
        Some("make it"),
        "the prompt is one argv value"
    );

    assert_eq!(
        argv(&def::DEEPSEEK_TUI, &with("deepseek-v4-pro", None)),
        "exec --auto --model deepseek-v4-pro make it"
    );
    assert_eq!(argv(&def::DEEPSEEK_TUI, &ask), "exec make it");

    assert_eq!(
        argv(&def::GROK_BUILD, &with("grok-4.20-reasoning", Some("high"))),
        "--prompt-file /tmp/od-prompt.md --no-plan --always-approve \
         --model grok-4.20-reasoning --effort high"
    );
    assert_eq!(
        argv(&def::GROK_BUILD, &with("grok-4.3", Some("high"))),
        "--prompt-file /tmp/od-prompt.md --no-plan --always-approve --model grok-4.3",
        "effort only on reasoning models"
    );
    assert_eq!(argv(&def::GROK_BUILD, &ask), "--prompt-file /tmp/od-prompt.md --no-plan");

    assert_eq!(
        argv(&def::ATOMCODE, &with("glm-5.2", None)),
        "--prompt-file /tmp/od-prompt.md -y --model glm-5.2"
    );
    assert_eq!(argv(&def::ATOMCODE, &ask), "--prompt-file /tmp/od-prompt.md");

    assert_eq!(
        argv(&def::ANTIGRAVITY, &with("Gemini 3.1 Pro (High)", None)),
        "--log-file /tmp/agy.log --dangerously-skip-permissions \
         --input-format stream-json --output-format stream-json",
        "agy takes no --model"
    );
    assert_eq!(
        argv(&def::ANTIGRAVITY, &ask),
        "--log-file /tmp/agy.log --input-format stream-json --output-format stream-json"
    );
    let undetected = ArgvInput {
        capabilities: &[],
        ..base.clone()
    };
    assert!(!argv(&def::ANTIGRAVITY, &undetected).contains("skip-permissions"));
}

#[test]
fn adapters_are_wired_like_the_original() {
    use def::McpInjection;
    let find = |id: &str| def::find(id).unwrap_or_else(|| panic!("{id} not registered"));
    for id in [
        "aider",
        "amp",
        "pi",
        "qoder",
        "mimo",
        "codebuddy",
        "atomcode",
        "grok-build",
        "antigravity",
        "deepseek",
    ] {
        assert!(def::BUILTIN.iter().any(|d| d.id == id), "{id} not detected");
        find(id);
    }
    assert_eq!(find("codebuddy").fallback_bins, ["cbc"]);
    assert_eq!(find("deepseek").fallback_bins, ["codewhale"]);
    assert_eq!(find("qoder").bin, "qodercli");
    assert_eq!(find("antigravity").bin, "agy");
    assert_eq!(find("grok-build").bin, "grok");
    assert!(find("codebuddy").supports_steer());
    assert_eq!(find("codebuddy").session, SessionIdSource::Minted);
    assert_eq!(find("pi").session, SessionIdSource::Captured);
    assert_eq!(find("amp").prompt_input, PromptInput::TextStdin);
    assert_eq!(find("antigravity").prompt_input, PromptInput::AgyStdin);
    for id in ["aider", "deepseek"] {
        assert_eq!(find(id).prompt_input, PromptInput::Argv);
    }
    for id in ["grok-build", "atomcode"] {
        assert_eq!(find(id).prompt_input, PromptInput::File);
    }
    assert_eq!(find("codebuddy").mcp, McpInjection::ClaudeConfig);
    assert_eq!(
        find("mimo").mcp,
        McpInjection::OpenCodeEnv("MIMOCODE_CONFIG_CONTENT")
    );
    assert_eq!(find("amp").mcp, McpInjection::None, "amp takes no --mcp-config");
    assert_eq!(
        od_runtime::engine::agy_user_line("hi"),
        "{\"event\":\"user\",\"message\":{\"content\":\"hi\"}}\n"
    );
}
