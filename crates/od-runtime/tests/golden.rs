//! Golden tests: verbatim CLI recordings → normalized events (spec 5.7 M2).
//!
//! Inputs are byte-for-byte stdout of real `claude` 2.1.259 and `codex exec
//! --json` processes, copied from the original app (Apache-2.0, see
//! NOTICE). The expected event streams live next to them as
//! `*.events.jsonl`; regenerate with `OD_UPDATE_GOLDEN=1 cargo test`.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use od_contracts::AgentEvent;
use od_runtime::parse::{ClaudeParser, CodexParser, StreamParser, parse_all};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn recording(name: &str) -> String {
    std::fs::read_to_string(fixtures().join("claude-cli-recordings").join(name)).unwrap()
}

fn frames(name: &str) -> Vec<Value> {
    recording(name)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn to_text(frames: &[Value]) -> String {
    frames
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn claude(text: &str) -> Vec<AgentEvent> {
    parse_all(&mut ClaudeParser::default(), text)
}

fn stop_reasons(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TurnEnd { stop_reason } => Some(stop_reason.clone()),
            _ => None,
        })
        .collect()
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

/// Compares against (or rewrites) `<golden>`.
fn check_golden(golden: &Path, events: &[AgentEvent]) {
    let actual: String = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect();
    if std::env::var_os("OD_UPDATE_GOLDEN").is_some() {
        std::fs::write(golden, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(golden)
        .unwrap_or_else(|_| panic!("missing golden {}", golden.display()));
    for (i, (a, e)) in actual.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, e, "{}: event {i} differs", golden.display());
    }
    assert_eq!(
        actual.lines().count(),
        expected.lines().count(),
        "{}: event count differs",
        golden.display()
    );
}

const CLAUDE_RECORDINGS: [&str; 6] = [
    "claude-2.1.259-partial-two-turns.jsonl",
    "claude-2.1.259-partial-same-turn-echo.jsonl",
    "claude-2.1.259-partial-single-turn.jsonl",
    "claude-2.1.259-partial-forwarded-subagent.jsonl",
    "claude-2.1.259-no-partial-messages.jsonl",
    "claude-2.1.259-no-partial-two-turns.jsonl",
];

#[test]
fn claude_recordings_match_golden_event_streams() {
    for name in CLAUDE_RECORDINGS {
        let events = claude(&recording(name));
        let golden = fixtures()
            .join("golden")
            .join(name.replace(".jsonl", ".events.jsonl"));
        check_golden(&golden, &events);
    }
}

#[test]
fn codex_exec_json_matches_golden_event_stream() {
    let text = std::fs::read_to_string(fixtures().join("codex/turn-exec-json.jsonl")).unwrap();
    let events = parse_all(&mut CodexParser::default(), &text);
    check_golden(
        &fixtures().join("golden/codex-turn-exec-json.events.jsonl"),
        &events,
    );
}

// ---- corpus guards: the recordings really are the 2.1.259 shape ----

#[test]
fn recordings_never_carry_stop_reason_on_the_assistant_wrapper() {
    for name in CLAUDE_RECORDINGS {
        for f in frames(name) {
            if f["type"] == "assistant" {
                assert!(f["message"]["stop_reason"].is_null(), "{name}");
            }
        }
    }
}

#[test]
fn no_partial_recordings_have_no_stream_events() {
    for name in [
        "claude-2.1.259-no-partial-messages.jsonl",
        "claude-2.1.259-no-partial-two-turns.jsonl",
    ] {
        assert!(frames(name).iter().all(|f| f["type"] != "stream_event"));
    }
}

// ---- the turn boundary ----

#[test]
fn turn_end_fires_once_per_assistant_message_from_message_delta() {
    let events = claude(&recording("claude-2.1.259-partial-two-turns.jsonl"));
    assert_eq!(stop_reasons(&events), ["tool_use", "end_turn", "end_turn"]);
}

/// Real frames with each message's stop reason stamped back onto its last
/// assistant wrapper (the pre-2.1 shape); optionally drop `stream_event`.
fn with_legacy_stop_reason(name: &str, keep_stream_events: bool, only_parented: bool) -> String {
    let all = frames(name);
    let mut stop_by_message = std::collections::HashMap::new();
    let mut open: Option<String> = None;
    for f in &all {
        if f["type"] != "stream_event" {
            continue;
        }
        let ev = &f["event"];
        if ev["type"] == "message_start" {
            open = ev["message"]["id"].as_str().map(str::to_owned);
        } else if ev["type"] == "message_delta" {
            if let (Some(id), Some(stop)) = (&open, ev["delta"]["stop_reason"].as_str()) {
                stop_by_message.insert(id.clone(), stop.to_owned());
            }
        }
    }
    for f in &all {
        if f["type"] == "assistant" && !f["parent_tool_use_id"].is_null() {
            if let Some(id) = f["message"]["id"].as_str() {
                stop_by_message
                    .entry(id.to_owned())
                    .or_insert_with(|| "end_turn".to_owned());
            }
        }
    }
    let mut last_index = std::collections::HashMap::new();
    for (i, f) in all.iter().enumerate() {
        if f["type"] == "assistant" {
            if let Some(id) = f["message"]["id"].as_str() {
                last_index.insert(id.to_owned(), i);
            }
        }
    }
    let mut out = Vec::new();
    for (i, f) in all.iter().enumerate() {
        if !keep_stream_events && f["type"] == "stream_event" {
            continue;
        }
        let mut f = f.clone();
        if f["type"] == "assistant" {
            let id = f["message"]["id"].as_str().map(str::to_owned);
            let eligible = !only_parented || !f["parent_tool_use_id"].is_null();
            if let Some(id) = id {
                if eligible && last_index.get(&id) == Some(&i) {
                    if let Some(stop) = stop_by_message.get(&id) {
                        f["message"]["stop_reason"] = Value::String(stop.clone());
                    }
                }
            }
        }
        out.push(f);
    }
    to_text(&out)
}

#[test]
fn hybrid_build_with_both_sources_does_not_double_fire() {
    let hybrid = with_legacy_stop_reason("claude-2.1.259-partial-two-turns.jsonl", true, false);
    assert_eq!(
        stop_reasons(&claude(&hybrid)),
        ["tool_use", "end_turn", "end_turn"]
    );
}

#[test]
fn legacy_wrapper_field_still_works_without_message_delta() {
    let legacy = with_legacy_stop_reason("claude-2.1.259-partial-two-turns.jsonl", false, false);
    assert_eq!(
        stop_reasons(&claude(&legacy)),
        ["tool_use", "end_turn", "end_turn"]
    );
}

#[test]
fn subagent_frames_never_close_the_parent_turn() {
    let name = "claude-2.1.259-partial-forwarded-subagent.jsonl";
    assert!(
        frames(name)
            .iter()
            .any(|f| !f["parent_tool_use_id"].is_null())
    );
    assert_eq!(
        stop_reasons(&claude(&recording(name))),
        ["tool_use", "tool_use", "end_turn"]
    );
    // A message_delta re-parented under a sub-agent is dropped.
    let reparented: Vec<Value> = frames("claude-2.1.259-partial-two-turns.jsonl")
        .into_iter()
        .map(|mut f| {
            if f["type"] == "stream_event" {
                f["parent_tool_use_id"] = Value::String("toolu_child_1".into());
            }
            f
        })
        .collect();
    assert!(stop_reasons(&claude(&to_text(&reparented))).is_empty());
    // So is a legacy wrapper stop_reason under a parent id.
    let legacy_child = with_legacy_stop_reason(name, false, true);
    assert!(stop_reasons(&claude(&legacy_child)).is_empty());
}

#[test]
fn no_partial_stream_terminates_through_usage() {
    let events = claude(&recording("claude-2.1.259-no-partial-messages.jsonl"));
    assert!(stop_reasons(&events).is_empty());
    let usage: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Usage { .. }))
        .collect();
    assert_eq!(usage.len(), 1);
    assert!(matches!(
        usage[0],
        AgentEvent::Usage { stop_reason: Some(s), is_error: false, .. } if s == "end_turn"
    ));
}

#[test]
fn two_turn_text_and_single_tool_call_per_id() {
    for name in [
        "claude-2.1.259-partial-two-turns.jsonl",
        "claude-2.1.259-no-partial-two-turns.jsonl",
    ] {
        let events = claude(&recording(name));
        let t = text(&events);
        assert!(t.contains("DONE1"), "{name}");
        assert!(
            t.contains(r#"<artifact type="text/html" title="Beta">"#),
            "{name}"
        );
        let usage = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Usage { .. }))
            .count();
        assert_eq!(usage, 2, "{name}: one result per user turn");
    }
    let single = claude(&recording("claude-2.1.259-partial-single-turn.jsonl"));
    assert_eq!(text(&single), "DONE1");
    for name in CLAUDE_RECORDINGS {
        let events = claude(&recording(name));
        let mut ids: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolUse { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "{name}: a tool call was emitted twice");
    }
    let writes = single
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolUse { name, .. } if name == "Write"))
        .count();
    assert_eq!(writes, 1);
}

#[test]
fn codex_command_execution_becomes_bash_tool_use_and_result() {
    let text = std::fs::read_to_string(fixtures().join("codex/turn-exec-json.jsonl")).unwrap();
    let events = parse_all(&mut CodexParser::default(), &text);
    let use_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::ToolUse { id, name, .. } if id == "item_3" && name == "Bash"))
        .unwrap();
    let AgentEvent::ToolUse { input, .. } = &events[use_idx] else {
        unreachable!()
    };
    assert_eq!(input["command"], "/bin/zsh -lc 'echo hi'");
    let results: Vec<&AgentEvent> = events
        .iter()
        .filter(
            |e| matches!(e, AgentEvent::ToolResult { tool_use_id, .. } if tool_use_id == "item_3"),
        )
        .collect();
    assert_eq!(results.len(), 1, "exactly one result");
    assert!(matches!(
        results[0],
        AgentEvent::ToolResult { content, is_error: false, .. } if content == "hi\n"
    ));
    let result_idx = events
        .iter()
        .position(|e| std::ptr::eq(e, results[0]))
        .unwrap();
    assert!(use_idx < result_idx);
    // started + completed frames yield one tool_use, not two.
    let bash = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolUse { name, .. } if name == "Bash"))
        .count();
    assert_eq!(bash, 1);
    // thread.started → status{sessionId}; file_change → Write.
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::Status { session_id: Some(s), .. } if s == "01a04341-7b7e-7750-aeb4-721b12024ac2"
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolUse { name, input, .. } if name == "Write" && input["file_path"] == "/WORK/notes.md"
    )));
}

#[test]
fn feeding_line_by_line_equals_whole_recording() {
    // The engine feeds one line at a time; the parser must not depend on
    // seeing the whole stream.
    let rec = recording("claude-2.1.259-partial-two-turns.jsonl");
    let mut parser = ClaudeParser::default();
    let mut out = Vec::new();
    for line in rec.lines() {
        parser.line(line, &mut out);
    }
    parser.finish(&mut out);
    assert_eq!(out, claude(&rec));
}
