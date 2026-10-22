//! Routes behind the wave-2 UI: preview comments (and their trip into the
//! agent prompt), conversation fork, handoff. Mock CLIs only.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

mod common;

use common::*;
use serde_json::{Value, json};

fn claude_echo(log: &std::path::Path) -> String {
    let log = log.display();
    format!(
        r#"{PROBES}
{PARSE_SESSION}
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"done"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#
    )
}

async fn send(
    d: &Daemon,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut req = d.http.request(method, format!("{}{path}", d.base));
    if let Some(body) = body {
        req = req.json(&body);
    }
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
async fn comments_round_trip_and_reach_the_prompt() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_echo(&d.mocks));
    let (project, conversation) = d.project().await;
    let base = format!("/api/projects/{project}/conversations/{conversation}/comments");

    let (status, made) = send(
        &d,
        reqwest::Method::POST,
        &base,
        Some(json!({
            "filePath": "index.html", "selector": "[data-od-id=\"hero\"]", "elementId": "hero",
            "label": "h1", "text": "Tea", "position": {"x": 1, "y": 2, "width": 3, "height": 4},
            "note": "Make it green"
        })),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    let comment = &made["comment"];
    assert_eq!(comment["status"], "attached");
    let id = comment["id"].as_str().unwrap().to_owned();

    let (status, bad) = send(
        &d,
        reqwest::Method::POST,
        &base,
        Some(json!({"filePath": "../x.html", "selector": "h1", "note": "x"})),
    )
    .await;
    assert!(status == 400 || status == 403, "{status} {bad}");

    let listed = d.get_json(&base).await;
    assert_eq!(listed["comments"].as_array().map(Vec::len), Some(1));

    // The turn carries the comment; the agent sees a scoped block.
    let attachment = json!({
        "id": id, "order": 1, "filePath": "index.html", "elementId": "hero",
        "selector": "[data-od-id=\"hero\"]", "label": "h1", "currentText": "Tea",
        "comment": "Make it green"
    });
    let run = d
        .start(
            json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
                      "message": "apply my comments", "commentAttachments": [attachment]}),
        )
        .await;
    assert_eq!(
        end_of(&d.events(&run.run_id, None).await)["status"],
        "succeeded"
    );
    let prompt: Value = serde_json::from_str(
        std::fs::read_to_string(d.mocks.join("prompts.log"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let text = prompt["message"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("apply my comments\n\n<attached-preview-comments>"),
        "{text}"
    );
    assert!(text.contains("selector: [data-od-id=\"hero\"]\n"), "{text}");
    assert!(text.contains("comment: Make it green\n"), "{text}");
    // The stored user message stays what the user typed.
    let messages = d.settled_messages(&project, &conversation).await;
    assert_eq!(messages[0]["content"], "apply my comments");

    let (status, patched) = send(
        &d,
        reqwest::Method::PATCH,
        &format!("{base}/{id}"),
        Some(json!({"status": "applying"})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(patched["comment"]["status"], "applying");
    let (status, _) = send(&d, reqwest::Method::DELETE, &format!("{base}/{id}"), None).await;
    assert_eq!(status, 200);
    let (status, _) = send(&d, reqwest::Method::DELETE, &format!("{base}/{id}"), None).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn fork_copies_history_into_a_new_conversation() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_echo(&d.mocks));
    let (project, conversation) = d.project().await;
    for msg in ["one", "two"] {
        let run = d
            .start(json!({"agentId": "claude", "projectId": project,
                          "conversationId": conversation, "message": msg}))
            .await;
        d.events(&run.run_id, None).await;
        d.settled_messages(&project, &conversation).await;
    }
    let messages = d.settled_messages(&project, &conversation).await;
    let fork_at = messages[1]["id"].as_str().unwrap();

    let (status, forked) = send(
        &d,
        reqwest::Method::POST,
        &format!("/api/projects/{project}/conversations"),
        Some(
            json!({"title": "Fork", "seedFromConversationId": conversation,
                    "forkAfterMessageId": fork_at}),
        ),
    )
    .await;
    assert_eq!(status, 200, "{forked}");
    let cid = forked["conversation"]["id"].as_str().unwrap();
    let copied = d.settled_messages(&project, cid).await;
    let contents: Vec<&str> = copied
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(contents, ["one", "done"]);

    let (status, _) = send(
        &d,
        reqwest::Method::POST,
        &format!("/api/projects/{project}/conversations"),
        Some(json!({"seedFromConversationId": "missing"})),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn handoff_returns_folder_and_prompt() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    let res = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": "index.html", "content": "<h1>x</h1>"}),
        )
        .await;
    assert!(res.status().is_success());
    let body = d
        .get_json(&format!("/api/projects/{project}/handoff"))
        .await;
    let dir = body["dir"].as_str().unwrap();
    assert!(dir.ends_with(&project), "{dir}");
    let prompt = body["prompt"].as_str().unwrap();
    assert!(prompt.contains(&format!("`{dir}`")), "{prompt}");
    assert!(prompt.contains("Start from `index.html`"), "{prompt}");
    assert!(prompt.contains("- `index.html`\n"), "{prompt}");

    let (status, _) = send(
        &d,
        reqwest::Method::POST,
        "/api/projects/nope/handoff/open",
        None,
    )
    .await;
    assert_eq!(status, 404);
}
