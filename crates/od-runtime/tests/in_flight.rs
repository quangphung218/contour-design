//! `tool_in_flight`: the file a Claude `Write` is writing, named while its
//! arguments still stream. Built on the real 2.1.259 recording, its one
//! `Write` grown to ~10 KB of content in small fragments (a real 10 KB write
//! arrives in about a thousand `input_json_delta` frames).

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use od_contracts::AgentEvent;
use od_runtime::parse::{ClaudeParser, IN_FLIGHT_STEP_BYTES, StreamParser};
use serde_json::{Value, json};
use std::path::Path;

const RECORDING: &str =
    "tests/fixtures/claude-cli-recordings/claude-2.1.259-partial-single-turn.jsonl";
const PATH: &str = "/private/tmp/claude-501/-Users-elian-Documents-open-design/bff58f5e-18bb-4b58-96e7-8180846e980a/scratchpad/w107/cwd/alpha.html";

fn frames() -> Vec<Value> {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(RECORDING))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn partial(frame: &Value) -> Option<&str> {
    frame
        .pointer("/event/delta/partial_json")
        .and_then(Value::as_str)
}

/// ~10 KB of page, with quotes and newlines to escape.
fn page() -> String {
    let mut html = String::from("<!doctype html>\n<html><body>\n");
    let mut i = 0;
    while html.len() < 10 * 1024 {
        html.push_str(&format!("<p class=\"row\">Row {i} of the café menu</p>\n"));
        i += 1;
    }
    html.push_str("</body></html>\n");
    html
}

/// The recording with its `Write` content replaced by [`page`], sent in
/// `chunk`-byte fragments after the recorded path fragments.
fn grown(chunk: usize) -> (Vec<String>, String) {
    let frames = frames();
    let page = page();
    let first_content = frames
        .iter()
        .position(|f| partial(f).is_some_and(|p| p.contains("\"content\"")))
        .unwrap();
    let close = frames
        .iter()
        .position(|f| partial(f) == Some("\"}"))
        .unwrap();
    let template = &frames[first_content];
    let body = serde_json::to_string(&page).unwrap();
    // `", "content": "` then the escaped body (without its own quotes), `"}`.
    let mut args = String::from("\", \"content\": ");
    args.push_str(&body[..body.len() - 1]);
    let mut fragments = Vec::new();
    let mut rest = args.as_str();
    while !rest.is_empty() {
        let mut end = chunk.min(rest.len());
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        fragments.push(&rest[..end]);
        rest = &rest[end..];
    }
    let mut lines: Vec<String> = frames[..first_content]
        .iter()
        .map(Value::to_string)
        .collect();
    for fragment in fragments {
        let mut f = template.clone();
        f["event"]["delta"]["partial_json"] = json!(fragment);
        lines.push(f.to_string());
    }
    lines.extend(frames[close..].iter().map(Value::to_string));
    (lines, page)
}

/// Each line's events, with the arguments' byte count after that line.
fn run(lines: &[String]) -> Vec<(usize, Vec<AgentEvent>)> {
    let mut parser = ClaudeParser::default();
    let mut bytes = 0;
    lines
        .iter()
        .map(|line| {
            let frame: Value = serde_json::from_str(line).unwrap();
            if let Some(p) = partial(&frame) {
                bytes += p.len();
            }
            let mut out = Vec::new();
            parser.line(line, &mut out);
            (bytes, out)
        })
        .collect()
}

fn in_flight(events: &[AgentEvent]) -> Vec<(&str, &str, &str, u64)> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolInFlight {
                id,
                name,
                path,
                bytes,
            } => Some((id.as_str(), name.as_str(), path.as_str(), *bytes)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_10_kb_write_names_its_file_within_the_first_kilobyte() {
    let (lines, page) = grown(40);
    let steps = run(&lines);
    let first = steps
        .iter()
        .find(|(_, events)| !in_flight(events).is_empty())
        .expect("a tool_in_flight");
    // The UI gets the name before 1 KB of arguments have arrived.
    assert!(first.0 < 1024, "named only after {} bytes", first.0);
    let all: Vec<_> = steps.iter().flat_map(|(_, e)| e.clone()).collect();
    let rows = in_flight(&all);
    assert_eq!(rows[0].0, "toolu_01TCNVTdkEMcTY46oBndnpKA");
    assert_eq!(rows[0].1, "Write");
    assert!(rows.iter().all(|r| r.2 == PATH), "{rows:?}");
    // Then one update per kilobyte of arguments, growing: not one per frame.
    let bytes: Vec<u64> = rows.iter().map(|r| r.3).collect();
    assert!(
        bytes
            .windows(2)
            .all(|w| w[1] - w[0] >= IN_FLIGHT_STEP_BYTES as u64),
        "{bytes:?}"
    );
    let total: u64 = steps.last().unwrap().0 as u64;
    assert!(
        rows.len() >= 10 && rows.len() as u64 <= total / IN_FLIGHT_STEP_BYTES as u64 + 1,
        "{} rows",
        rows.len()
    );
    // `tool_input_delta` still comes per fragment, and the call still ends
    // in one `tool_use` carrying the whole page.
    let deltas = all
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolInputDelta { .. }))
        .count();
    assert!(deltas > 250, "{deltas} deltas");
    let uses: Vec<&Value> = all
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolUse { input, .. } => Some(input),
            _ => None,
        })
        .collect();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0]["content"], json!(page));
    // The last update comes before the call's `tool_use`.
    let last_row = all
        .iter()
        .rposition(|e| matches!(e, AgentEvent::ToolInFlight { .. }))
        .unwrap();
    let tool_use = all
        .iter()
        .position(|e| matches!(e, AgentEvent::ToolUse { .. }))
        .unwrap();
    assert!(last_row < tool_use);
}

#[test]
fn the_name_comes_on_the_fragment_that_closes_the_path() {
    // Recorded fragments: the path ends inside the one that opens `content`.
    let lines: Vec<String> = frames().iter().map(Value::to_string).collect();
    let steps = run(&lines);
    let named: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, (_, e))| !in_flight(e).is_empty())
        .map(|(i, _)| i)
        .collect();
    let closing = lines
        .iter()
        .position(|l| l.contains(r#"\", \"content\": "#))
        .unwrap();
    assert_eq!(named, [closing]);
}

#[test]
fn other_tools_never_say_they_are_writing() {
    let frames = frames();
    let lines: Vec<String> = frames
        .iter()
        .map(|f| {
            let mut f = f.clone();
            if f.pointer("/event/content_block/name") == Some(&json!("Write")) {
                f["event"]["content_block"]["name"] = json!("Bash");
            }
            f.to_string()
        })
        .collect();
    let all: Vec<_> = run(&lines).into_iter().flat_map(|(_, e)| e).collect();
    assert!(in_flight(&all).is_empty());
}
