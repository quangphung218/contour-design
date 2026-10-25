//! What a `POST /api/runs` body turns into: skills staged once each, and
//! idempotency keyed on everything the agent sees.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

mod common;

use common::*;
use serde_json::{Value, json};
use std::path::Path;

fn claude_logging(log: &Path) -> String {
    let log = log.display();
    format!(
        r#"{PROBES}
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
"#
    )
}

#[tokio::test]
async fn a_skill_named_twice_is_in_the_prompt_once() {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let d = daemon_with(Options {
        content_root: Some(content),
        ..Options::default()
    })
    .await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, _) = d.project().await;
    let run = d
        .start(
            json!({"agentId": "claude", "projectId": project, "message": "go",
            "skillIds": ["ad-creative", "agent-browser", "ad-creative"]}),
        )
        .await;
    assert_eq!(
        end_of(&d.events(&run.run_id, None).await)["status"],
        "succeeded"
    );
    let line = std::fs::read_to_string(d.mocks.join("prompts.log")).unwrap();
    let sent: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    let prompt = sent["message"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(prompt.matches("## Active skill").count(), 2);
}

#[tokio::test]
async fn reusing_a_client_request_id_with_other_comments_is_a_conflict() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, _) = d.project().await;
    let body = |note: &str| {
        json!({"agentId": "claude", "projectId": project, "message": "apply",
            "clientRequestId": "same-key",
            "commentAttachments": [{"id": "c1", "order": 1, "filePath": "index.html",
                "elementId": "hero", "selector": "h1", "label": "h1",
                "currentText": "Tea", "comment": note}]})
    };
    let first = d.start(body("make it green")).await;
    let res = d.post("/api/runs", &body("make it red")).await;
    assert_eq!(res.status().as_u16(), 409, "{}", res.text().await.unwrap());
    let again = d.start(body("make it green")).await;
    assert!(again.reused);
    assert_eq!(again.run_id, first.run_id);
}

#[tokio::test]
async fn the_run_list_names_what_was_asked() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "A tea shop landing page"}))
        .await;
    end_of(&d.events(&run.run_id, None).await);
    let runs: Value = d.get_json("/api/runs").await;
    let listed = runs["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == run.run_id.as_str())
        .unwrap();
    assert_eq!(listed["prompt"], "A tea shop landing page");
}
