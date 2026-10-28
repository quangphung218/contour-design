//! Conversation rename and delete (track B3-chat): delete answers with the
//! conversations left, refuses while a run of the conversation is going, and
//! cannot reach another project's conversation. Mock CLI only.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)] // the mock agents are `sh` scripts

mod common;

use common::*;
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn rename_and_delete_conversations() {
    let d = daemon().await;
    // Waits on stdin until cancelled, so the run stays live.
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
sleep 30
"#
        ),
    );
    let (project, first) = d.project().await;
    let (other, _) = d.project().await;
    let conv = |p: &str, c: &str| format!("{}/api/projects/{p}/conversations/{c}", d.base);
    let second: Value = d
        .post(
            &format!("/api/projects/{project}/conversations"),
            &json!({}),
        )
        .await
        .json()
        .await
        .unwrap();
    let second = second["conversation"]["id"].as_str().unwrap().to_owned();

    let renamed: Value = d
        .http
        .patch(conv(&project, &second))
        .json(&json!({ "title": "  Hero ideas  " }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(renamed["conversation"]["title"], "Hero ideas");

    // Another project's id does not reach it.
    let res = d.http.delete(conv(&other, &second)).send().await.unwrap();
    assert_eq!(res.status(), 404);

    // A live run blocks the delete.
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "conversationId": first, "message": "go"}))
        .await;
    let mut reader = d.stream(&run.run_id, None).await;
    while let Some(ev) = reader.next().await {
        if ev.event == "agent" {
            break;
        }
    }
    let res = d.http.delete(conv(&project, &first)).send().await.unwrap();
    assert_eq!(res.status(), 409);
    let res = d
        .post(&format!("/api/runs/{}/cancel", run.run_id), &json!({}))
        .await;
    assert_eq!(res.status(), 200);
    let mut gone = None;
    for _ in 0..200 {
        let res = d.http.delete(conv(&project, &first)).send().await.unwrap();
        if res.status() != 409 {
            gone = Some(res);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let res = gone.expect("delete allowed once the run ended");
    assert_eq!(res.status(), 200);
    let left: Value = res.json().await.unwrap();
    let ids: Vec<&str> = left["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [second.as_str()]);
    let msgs = d
        .http
        .get(format!("{}/messages", conv(&project, &first)))
        .send()
        .await
        .unwrap();
    assert_eq!(msgs.status(), 404);
    let res = d.http.delete(conv(&project, &first)).send().await.unwrap();
    assert_eq!(res.status(), 404);
}

/// Linked folders: the check route says which may be linked, and a run of a
/// project with `metadata.linkedDirs` gets each accepted one as `--add-dir`
/// while a refused one never reaches the agent.
#[tokio::test]
async fn linked_dirs_are_checked_and_reach_the_agent() {
    let d = daemon().await;
    let log = d.mocks.join("args.log");
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
printf '%s\n' "$*" >> "{log}"
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s","model":"mock"}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{{"input_tokens":1}}}}\n'
"#,
            log = log.display()
        ),
    );
    // Not `tempdir()`: its `.tmpXXXX` name is a hidden segment.
    let outside = tempfile::Builder::new().prefix("odlink").tempdir().unwrap();
    let code = outside.path().join("code");
    std::fs::create_dir(&code).unwrap();
    let hidden = outside.path().join(".secret");
    std::fs::create_dir(&hidden).unwrap();
    let canonical = code.canonicalize().unwrap().display().to_string();

    let res: Value = d
        .post(
            "/api/linked-dirs/check",
            &json!({ "paths": [
                code.display().to_string(),
                "relative/dir",
                hidden.display().to_string(),
                d.data.display().to_string(),
                outside.path().join("missing").display().to_string(),
                d.data.join("projects").display().to_string(),
            ] }),
        )
        .await
        .json()
        .await
        .unwrap();
    let dirs = res["dirs"].as_array().unwrap();
    assert_eq!(dirs[0]["path"], canonical.as_str());
    for refused in &dirs[1..] {
        assert!(refused["path"].is_null(), "{refused}");
        assert!(refused["error"].is_string(), "{refused}");
    }
    // Another project's own folder may be referenced.
    let (other, _) = d.project().await;
    let other_dir = d.data.join("projects").join(&other);
    let res: Value = d
        .post(
            "/api/linked-dirs/check",
            &json!({ "paths": [other_dir.display().to_string()] }),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        res["dirs"][0]["path"],
        other_dir
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
            .as_str()
    );
    let too_many: Vec<String> = (0..33).map(|i| format!("/tmp/{i}")).collect();
    let res = d
        .post("/api/linked-dirs/check", &json!({ "paths": too_many }))
        .await;
    assert_eq!(res.status(), 400);

    let created: Value = d
        .post(
            "/api/projects",
            &json!({ "name": "linked", "metadata": { "linkedDirs": [
                code.display().to_string(), hidden.display().to_string()
            ] } }),
        )
        .await
        .json()
        .await
        .unwrap();
    let project = created["project"]["id"].as_str().unwrap();
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded");
    let args = std::fs::read_to_string(&log).unwrap();
    let run_line = args.lines().find(|l| l.contains("stream-json")).unwrap();
    assert!(
        run_line.contains(&format!("--add-dir {canonical}")),
        "{run_line}"
    );
    assert!(!run_line.contains(".secret"), "{run_line}");
}
