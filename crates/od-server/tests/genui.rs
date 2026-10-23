//! GenUI surfaces (track C4-depth): a plugin run asks its declared and
//! derived surfaces as run events; answers are taken once, remembered at
//! their tier, revocable; a diff-review answer lands in the project.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};

fn claude() -> String {
    format!(
        r#"{PROBES}
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s-%s","model":"mock"}}\n' "$$"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"ok"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#
    )
}

const PLUGIN: &str = r#"{
  "name": "surface-demo",
  "title": "Surface demo",
  "version": "0.1.0",
  "od": {
    "kind": "scenario",
    "taskKind": "code-migration",
    "genui": { "surfaces": [
      { "id": "publish-ok", "kind": "confirmation", "persist": "run",
        "prompt": "Publish the migrated page?" },
      { "id": "figma-auth", "kind": "oauth-prompt", "persist": "project",
        "prompt": "Authorize the design source", "oauth": { "route": "mcp", "mcpServerId": "design-mcp" } },
      { "id": "tone", "kind": "choice", "persist": "conversation",
        "schema": { "type": "object", "properties": { "tone": { "type": "string", "enum": ["calm", "bold"] } } } }
    ]},
    "pipeline": { "stages": [
      { "id": "patch", "atoms": ["patch-edit"] },
      { "id": "review", "atoms": ["diff-review"] }
    ]}
  }
}"#;

fn requested(events: &[SseEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.event == "genui_surface_request")
        .map(|e| e.data["surfaceId"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn plugin_runs_ask_their_surfaces() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude());
    let dir = d.data.join("plugins/surface-demo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("open-design.json"), PLUGIN).unwrap();
    d.state.content.reload();
    let (project, conversation) = d.project().await;
    // The plugin's patch step touched two files.
    let root = d
        .state
        .store
        .project_root(&d.state.store.get_project(&project).unwrap())
        .unwrap()
        .dir;
    std::fs::create_dir_all(root.join("plan/receipts")).unwrap();
    std::fs::write(
        root.join("plan/receipts/step-1.json"),
        r#"{"step":"step-1","files":["src/app.js","src/old.js"],"added":3,"removed":1}"#,
    )
    .unwrap();
    let body = |conv: &str| {
        json!({ "agentId": "claude", "projectId": project, "conversationId": conv,
                "message": "migrate", "pluginId": "surface-demo" })
    };

    let run = d.start(body(&conversation)).await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(
        requested(&events),
        [
            "publish-ok",
            "figma-auth",
            "tone",
            "__auto_diff_review_review"
        ]
    );
    let review = events
        .iter()
        .find(|e| e.data["surfaceId"] == "__auto_diff_review_review")
        .unwrap();
    assert_eq!(review.data["payload"]["kind"], "choice");
    assert_eq!(
        review.data["payload"]["context"]["touchedFiles"],
        json!(["src/app.js", "src/old.js"])
    );
    let listed = d.get_json(&format!("/api/runs/{}/genui", run.run_id)).await;
    let surfaces = listed["surfaces"].as_array().unwrap();
    assert_eq!(surfaces.len(), 4);
    assert!(surfaces.iter().all(|s| s["status"] == "pending"));
    assert_eq!(surfaces[1]["spec"]["oauth"]["mcpServerId"], "design-mcp");

    // Answer: Continue, Authorize, a choice, Accept all.
    let respond = |sid: &str, value: Value| {
        let path = format!("/api/runs/{}/genui/{sid}/respond", run.run_id);
        let d = &d;
        async move { d.post(&path, &json!({ "value": value })).await }
    };
    let res = respond("publish-ok", json!(true)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = respond("publish-ok", json!(false)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "answered once");
    assert_eq!(
        respond("no-such", json!(1)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        respond(
            "figma-auth",
            json!({"authorized": true, "mcpServerId": "design-mcp"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        respond("tone", json!({"tone": "bold"})).await.status(),
        StatusCode::OK
    );
    let res = respond(
        "__auto_diff_review_review",
        json!({"decision": "partial", "accepted_files": ["src/app.js"]}),
    )
    .await;
    let body_partial: Value = res.json().await.unwrap();
    assert_eq!(
        body_partial["diffReviewBridge"]["ok"], false,
        "partial must place every file"
    );
    let one = d
        .get_json(&format!("/api/runs/{}/genui/publish-ok", run.run_id))
        .await;
    assert_eq!(
        (one["status"].as_str(), one["value"].clone()),
        (Some("resolved"), json!(true))
    );

    // Next run in another conversation: the project-tier answer is
    // reused, the others are asked again.
    let other = d
        .post(
            &format!("/api/projects/{project}/conversations"),
            &json!({}),
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
    let other = other["conversation"]["id"].as_str().unwrap().to_owned();
    let run2 = d.start(body(&other)).await;
    let events = d.events(&run2.run_id, None).await;
    assert_eq!(
        requested(&events),
        ["publish-ok", "tone", "__auto_diff_review_review"]
    );
    let cached = events
        .iter()
        .find(|e| e.event == "genui_surface_response")
        .unwrap();
    assert_eq!(
        (
            cached.data["surfaceId"].as_str(),
            cached.data["respondedBy"].as_str()
        ),
        (Some("figma-auth"), Some("cache"))
    );
    let res = d
        .post(
            &format!(
                "/api/runs/{}/genui/__auto_diff_review_review/respond",
                run2.run_id
            ),
            &json!({ "value": {"decision": "accept", "reason": "looks right"} }),
        )
        .await;
    let bridged: Value = res.json().await.unwrap();
    assert_eq!(bridged["diffReviewBridge"]["ok"], true, "{bridged}");
    let decision: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("review/decision.json")).unwrap())
            .unwrap();
    assert_eq!(decision["decision"], "accept");
    assert_eq!(
        decision["accepted_files"],
        json!(["src/app.js", "src/old.js"])
    );
    assert_eq!(decision["reviewer"], "user");

    // Revoke: the third run asks again.
    let inbox = d.get_json(&format!("/api/projects/{project}/genui")).await;
    assert!(
        inbox["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["surfaceId"] == "figma-auth" && s["status"] == "resolved")
    );
    let res = d
        .post(
            &format!("/api/projects/{project}/genui/figma-auth/revoke"),
            &json!({}),
        )
        .await;
    assert_eq!(res.json::<Value>().await.unwrap()["invalidated"], 1);
    let run3 = d.start(body(&other)).await;
    let events = d.events(&run3.run_id, None).await;
    assert!(requested(&events).contains(&"figma-auth".to_owned()));
    // The conversation tier: run 3 shares run 2's conversation, whose
    // tone was never answered, so tone is asked.
    assert!(requested(&events).contains(&"tone".to_owned()));

    // Prefill answers ahead of the next run.
    let res = d
        .post(
            &format!("/api/projects/{project}/genui/prefill"),
            &json!({"snapshotId": "surface-demo", "surfaceId": "figma-auth", "kind": "oauth-prompt", "value": {"authorized": true}}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let run4 = d.start(body(&other)).await;
    let events = d.events(&run4.run_id, None).await;
    assert!(!requested(&events).contains(&"figma-auth".to_owned()));

    // Runs without a plugin ask nothing; unknown runs are 404.
    let plain = d
        .start(json!({ "agentId": "claude", "projectId": project, "conversationId": conversation, "message": "hi" }))
        .await;
    assert!(requested(&d.events(&plain.run_id, None).await).is_empty());
    let res = d
        .http
        .get(format!("{}/api/runs/nope/genui", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = d
        .http
        .get(format!("{}/api/projects/nope/genui", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bundled_code_migration_asks_for_a_diff_review() {
    let content = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let d = daemon_with(Options {
        content_root: Some(content),
        ..Options::default()
    })
    .await;
    write_mock(&d.mocks, "claude", &claude());
    let (project, conversation) = d.project().await;
    let run = d
        .start(
            json!({ "agentId": "claude", "projectId": project, "conversationId": conversation,
                       "message": "go", "pluginId": "od-code-migration",
                       "pluginInputs": { "repoPath": "./legacy", "targetStack": "React" } }),
        )
        .await;
    let ids = requested(&d.events(&run.run_id, None).await);
    assert!(
        ids.iter().any(|i| i.starts_with("__auto_diff_review_")),
        "{ids:?}"
    );
}
