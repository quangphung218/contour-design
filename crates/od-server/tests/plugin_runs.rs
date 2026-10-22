//! Plugin presets reach the agent (spec 1.4): `pluginId` + `pluginInputs`
//! on `POST /api/runs` put the `## Active plugin` block into the prompt;
//! unknown plugins and missing inputs fail before a run exists. Mock CLI
//! only, no real agent.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

mod common;

use common::*;
use serde_json::{Value, json};
use std::path::Path;

fn claude_logging(log: &Path) -> String {
    let log = log.display();
    format!(
        r#"{PROBES}
{PARSE_SESSION}
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"ok"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
cat >/dev/null
"#
    )
}

#[tokio::test]
async fn applied_plugin_lands_in_the_prompt() {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let d = daemon_with(Options {
        content_root: Some(content),
        ..Options::default()
    })
    .await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    let body = |plugin: &str, inputs: Value| {
        json!({ "agentId": "claude", "projectId": project, "conversationId": conversation,
                "message": "go", "pluginId": plugin, "pluginInputs": inputs })
    };

    let res = d
        .post("/api/runs", &body("no-such-plugin", json!({})))
        .await;
    assert_eq!(res.status(), 404);
    let res = d
        .post(
            "/api/runs",
            &body("od-new-generation", json!({ "topic": "x" })),
        )
        .await;
    assert_eq!(res.status(), 422);
    let err: Value = res.json().await.unwrap();
    let msg = err["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("artifactKind") && msg.contains("audience"),
        "{msg}"
    );
    let runs: Value = d.get_json("/api/runs").await;
    assert!(runs["runs"].as_array().unwrap().is_empty(), "{runs}");

    let run = d
        .start(body(
            "od-new-generation",
            json!({ "artifactKind": "landing page", "audience": "VCs", "topic": "Rust" }),
        ))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded");
    let line = std::fs::read_to_string(d.mocks.join("prompts.log")).unwrap();
    let sent: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    let prompt = sent["message"]["content"][0]["text"].as_str().unwrap();
    for part in [
        "## Active plugin",
        "(`od-new-generation@",
        "The plugin's example brief is: _Generate a landing page for VCs on Rust.",
        "- **audience**: VCs",
        "## Plugin atoms",
        "- `todo-write`",
        "## Plugin instructions",
        // The plugin's craft list reaches the craft block.
        "## Active craft references",
    ] {
        assert!(prompt.contains(part), "missing {part:?}");
    }
    // Block 10 sits after the instructions charter and before the user request.
    let plugin_at = prompt.find("## Active plugin").unwrap();
    assert!(plugin_at < prompt.find("# User request").unwrap());
}
