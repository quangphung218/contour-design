//! What a `POST /api/runs` body turns into: skills staged once each, and
//! idempotency keyed on everything the agent sees.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // helpers of the `sh` mock tests

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

#[cfg(unix)] // the mock agent is a `sh` script
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
    let (project, conversation) = d.project().await;
    let run = d
        .start(
            json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
            "message": "\n  A tea shop landing page\nWarm colours, a menu and opening hours."}),
        )
        .await;
    end_of(&d.events(&run.run_id, None).await);
    // The list reads the run, not the chat (it is polled every few
    // seconds): a message rewritten afterwards does not change it. The run
    // keeps only the first non-empty line of what was asked.
    let messages = d.settled_messages(&project, &conversation).await;
    let user = messages.iter().find(|m| m["role"] == "user").unwrap();
    let res = d
        .http
        .put(format!(
            "{}/api/projects/{project}/conversations/{conversation}/messages/{}",
            d.base,
            user["id"].as_str().unwrap()
        ))
        .json(&json!({"role": "user", "content": "rewritten"}))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "{}", res.status());
    let runs: Value = d.get_json("/api/runs").await;
    let listed = runs["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == run.run_id.as_str())
        .unwrap();
    assert_eq!(listed["prompt"], "A tea shop landing page");
    let one: Value = d.get_json(&format!("/api/runs/{}", run.run_id)).await;
    assert_eq!(one["run"]["prompt"], "A tea shop landing page");
}

/// Recent projects (spec L6): a rename or a scheduled run (no `sentAt`)
/// keeps the order; a send moves the project up at the time it was sent,
/// so a queued message sent late does not pass a later send elsewhere.
#[tokio::test]
async fn only_a_send_moves_a_project_up_recent_projects() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (first, _) = d.project().await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (second, _) = d.project().await;
    let order = || async {
        let list: Value = d.get_json("/api/projects").await;
        list["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(order().await, [second.clone(), first.clone()]);

    let res = d
        .http
        .patch(format!("{}/api/projects/{first}", d.base))
        .json(&json!({"name": "Renamed"}))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "{}", res.status());
    let run = |project: &str, sent_at: Option<i64>| {
        let mut body = json!({"agentId": "claude", "projectId": project, "message": "go"});
        if let Some(at) = sent_at {
            body["sentAt"] = json!(at);
        }
        body
    };
    let queued_at = od_store::now_ms();
    let scheduled = d.start(run(&first, None)).await;
    end_of(&d.events(&scheduled.run_id, None).await);
    assert_eq!(
        order().await,
        [second.clone(), first.clone()],
        "a rename and a run nobody sent keep the order"
    );

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let sent = d.start(run(&first, Some(od_store::now_ms()))).await;
    end_of(&d.events(&sent.run_id, None).await);
    assert_eq!(order().await, [first.clone(), second.clone()]);

    // Queued in `second` before the send above, sent only now.
    let late = d.start(run(&second, Some(queued_at))).await;
    end_of(&d.events(&late.run_id, None).await);
    assert_eq!(order().await, [first, second]);
}
