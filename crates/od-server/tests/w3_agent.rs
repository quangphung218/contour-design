//! Track W3-agent through HTTP with mock CLIs: session mode, project
//! memory, transcript export, the Design Jury, and comments a dead daemon
//! left `applying`. No real agent, no network.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // helpers of the `sh` mock tests

mod common;

use common::*;
use od_contracts::{CommentStatus, CreateCommentRequest, CreateProjectRequest};
use serde_json::{Value, json};

/// Logs each prompt line to `prompts.log`, then answers `text`.
fn claude_logging(log: &std::path::Path) -> String {
    let log = log.display();
    format!(
        r#"{PROBES}
{PARSE_SESSION}
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
if [ -f "{log}/reply.jsonl" ]; then cat "{log}/reply.jsonl"; else
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"done"}}]}}}}\n' "$$"
fi
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#
    )
}

fn prompts(d: &Daemon) -> Vec<String> {
    std::fs::read_to_string(d.mocks.join("prompts.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

async fn run_turn(d: &Daemon, body: Value) -> Vec<SseEvent> {
    let run = d.start(body).await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded", "{events:?}");
    events
}

async fn patch(d: &Daemon, path: &str, body: &Value) -> reqwest::Response {
    d.http
        .patch(format!("{}{path}", d.base))
        .json(body)
        .send()
        .await
        .unwrap()
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn session_mode_swaps_the_prompt_head_and_sticks_to_the_conversation() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    let body = |msg: &str, mode: Option<&str>| {
        let mut b = json!({"agentId": "claude", "projectId": project,
                           "conversationId": conversation, "message": msg});
        if let Some(mode) = mode {
            b["sessionMode"] = json!(mode);
        }
        b
    };

    run_turn(&d, body("make a page", None)).await;
    d.settled_messages(&project, &conversation).await;
    let res = patch(
        &d,
        &format!("/api/projects/{project}/conversations/{conversation}"),
        &json!({"sessionMode": "chat"}),
    )
    .await;
    assert_eq!(res.status(), 200);
    let changed: Value = res.json().await.unwrap();
    assert_eq!(changed["conversation"]["sessionMode"], "chat");
    run_turn(&d, body("what font is that?", None)).await;
    d.settled_messages(&project, &conversation).await;
    run_turn(&d, body("plan the next screen", Some("plan"))).await;
    d.settled_messages(&project, &conversation).await;

    let p = prompts(&d);
    assert_eq!(p.len(), 3);
    assert!(p[0].contains("# OD core directives") && !p[0].contains("# Ask mode"));
    // A resumed session gets the instructions again because they changed.
    assert!(p[1].contains("# Ask mode"), "{:.300}", p[1]);
    assert!(!p[1].contains("# OD core directives"));
    assert!(p[2].contains("# Plan mode"), "{:.300}", p[2]);
    let list = d
        .get_json(&format!("/api/projects/{project}/conversations"))
        .await;
    assert_eq!(
        list["conversations"][0]["sessionMode"], "plan",
        "request mode sticks"
    );

    let bad = patch(
        &d,
        &format!("/api/projects/{project}/conversations/{conversation}"),
        &json!({"sessionMode": "yolo"}),
    )
    .await;
    assert_eq!(bad.status(), 400);
    let missing = patch(
        &d,
        &format!("/api/projects/{project}/conversations/nope"),
        &json!({"sessionMode": "chat"}),
    )
    .await;
    assert_eq!(missing.status(), 404);
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn memory_reaches_the_agent_and_can_be_forgotten() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    let added = d
        .post(
            &format!("/api/projects/{project}/memory"),
            &json!({"text": "Brand color is teal"}),
        )
        .await;
    assert_eq!(added.status(), 200);
    let empty = d
        .post(
            &format!("/api/projects/{project}/memory"),
            &json!({"text": " "}),
        )
        .await;
    assert_eq!(empty.status(), 400);

    run_turn(
        &d,
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": "Remember that headings use Inter"}),
    )
    .await;
    let p = prompts(&d);
    assert!(p[0].contains("## Project memory"), "{:.200}", p[0]);
    assert!(p[0].contains("- Brand color is teal"));
    assert!(
        p[0].contains("- headings use Inter"),
        "this turn's fact counts already"
    );

    let memory = d.get_json(&format!("/api/projects/{project}/memory")).await;
    let entries = memory["memory"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["source"], "chat");
    let first = entries[0]["id"].as_str().unwrap().to_owned();

    let gone: Value = d
        .http
        .delete(format!("{}/api/projects/{project}/memory/{first}", d.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(gone["memory"].as_array().unwrap().len(), 1);
    let cleared = d
        .http
        .delete(format!("{}/api/projects/{project}/memory", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), 200);
    d.settled_messages(&project, &conversation).await;
    run_turn(
        &d,
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": "again"}),
    )
    .await;
    assert!(!prompts(&d)[1].contains("## Project memory"), "forgotten");
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn transcript_exports_as_markdown_and_json() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    run_turn(
        &d,
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": "make a tea shop page"}),
    )
    .await;
    d.settled_messages(&project, &conversation).await;
    let url = |q: &str| {
        format!(
            "{}/api/projects/{project}/conversations/{conversation}/transcript{q}",
            d.base
        )
    };

    let md = d.http.get(url("")).send().await.unwrap();
    assert_eq!(md.status(), 200);
    let disposition = md.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        disposition.contains("attachment") && disposition.contains(".md"),
        "{disposition}"
    );
    let text = md.text().await.unwrap();
    assert!(text.contains("- Mode: design"), "{text}");
    assert!(text.contains("## User\n\nmake a tea shop page"), "{text}");
    assert!(
        text.contains("## Assistant (claude, succeeded)\n\ndone"),
        "{text}"
    );

    let json_res = d.http.get(url("?format=json")).send().await.unwrap();
    assert_eq!(json_res.status(), 200);
    let v: Value = json_res.json().await.unwrap();
    assert_eq!(v["conversation"]["id"], conversation.as_str());
    assert_eq!(v["messages"].as_array().unwrap().len(), 2);
    assert_eq!(v["messages"][1]["content"], "done");

    assert_eq!(
        d.http
            .get(url("?format=pdf"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let other = format!(
        "{}/api/projects/{project}/conversations/nope/transcript",
        d.base
    );
    assert_eq!(d.http.get(other).send().await.unwrap().status(), 404);
}

/// Stream-json lines whose text, joined, is a two-round jury run.
fn jury_reply() -> String {
    let chunks = [
        r#"<CRITIQUE_RUN version="1" maxRounds="3" threshold="8.0" scale="10"><ROUND n="1">"#,
        r#"<PANELIST role="designer"><NOTES>Draft.</NOTES><ARTIFACT mime="text/html"><![CDATA[<h1>draft</h1>]]></ARTIFACT></PANELIST>"#,
        r#"<PANELIST role="critic" score="6" must_fix="1"><MUST_FIX>Fix contrast.</MUST_FIX></PANELIST>"#,
        r#"<PANELIST role="brand" score="7"></PANELIST><PANELIST role="a11y" score="6"></PANELIST><PANELIST role="copy" score="7"></PANELIST>"#,
        r#"<ROUND_END n="1" decision="continue"></ROUND_END></ROUND><ROUND n="2">"#,
        r#"<PANELIST role="designer"><NOTES>Fixed.</NOTES></PANELIST><PANELIST role="critic" score="9"></PANELIST>"#,
        r#"<PANELIST role="brand" score="8"></PANELIST><PANELIST role="a11y" score="8"></PANELIST><PANELIST role="copy" score="9"></PANELIST>"#,
        r#"<ROUND_END n="2" decision="ship"></ROUND_END></ROUND>"#,
        r#"<SHIP round="2"><ARTIFACT><![CDATA[<!doctype html><h1 id="final">Jury final</h1>]]></ARTIFACT><SUMMARY>Shipped.</SUMMARY></SHIP></CRITIQUE_RUN>"#,
    ];
    chunks
        .iter()
        .enumerate()
        .map(|(i, text)| {
            json!({"type": "assistant", "message": {"id": format!("j{i}"),
                   "content": [{"type": "text", "text": text}]}})
            .to_string()
                + "\n"
        })
        .collect()
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn the_design_jury_streams_scores_and_ships_the_artifact() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    std::fs::write(d.mocks.join("reply.jsonl"), jury_reply()).unwrap();
    let (project, conversation) = d.project().await;
    let off = d
        .get_json(&format!("/api/projects/{project}/critique"))
        .await;
    assert_eq!(off["enabled"], false);
    let on = d
        .http
        .put(format!("{}/api/projects/{project}/critique", d.base))
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(on.status(), 200);
    let meta = d.get_json(&format!("/api/projects/{project}")).await;
    assert_eq!(meta["project"]["metadata"]["critiqueTheaterEnabled"], true);

    let events = run_turn(
        &d,
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": "poster for a jazz night"}),
    )
    .await;
    assert!(prompts(&d)[0].contains("# Critique Theater (active skill: freeform)"));
    let reports: Vec<Value> = agent_payloads(&events)
        .into_iter()
        .filter(|p| p["type"] == "critique")
        .map(|p| p["report"].clone())
        .collect();
    assert!(reports.len() >= 5, "{reports:?}");
    let last = reports.last().unwrap();
    assert_eq!(last["status"], "shipped");
    // 9*.4 + 8*.2 + 8*.2 + 9*.2 = 8.6, recomputed by the daemon.
    assert_eq!(last["composite"], 8.6);
    assert_eq!(last["rounds"][0]["composite"], 6.4);

    let messages = d.settled_messages(&project, &conversation).await;
    let stored = messages[1]["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|e| e["type"] == "critique")
        .unwrap()
        .clone();
    let path = stored["report"]["artifactPath"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        path.starts_with("critique/") && path.ends_with(".html"),
        "{path}"
    );
    let file = d
        .http
        .get(format!("{}/api/projects/{project}/raw/{path}", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(file.contains("Jury final"), "{file}");

    // Off again: no jury block, no reports.
    d.http
        .put(format!("{}/api/projects/{project}/critique", d.base))
        .json(&json!({"enabled": false}))
        .send()
        .await
        .unwrap();
    let events = run_turn(
        &d,
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": "again"}),
    )
    .await;
    assert!(!prompts(&d)[1].contains("Critique Theater"));
    assert!(
        agent_payloads(&events)
            .iter()
            .all(|p| p["type"] != "critique")
    );
}

/// The shipped artifact cannot be written (a plain file is named
/// `critique`): the run still settles, and the report says why it has no
/// file.
#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn a_jury_artifact_that_cannot_be_written_still_settles_the_run() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    std::fs::write(d.mocks.join("reply.jsonl"), jury_reply()).unwrap();
    let (project, conversation) = d.project().await;
    let blocker = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": "critique", "content": "not a folder"}),
        )
        .await;
    assert!(blocker.status().is_success(), "{}", blocker.status());
    d.http
        .put(format!("{}/api/projects/{project}/critique", d.base))
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    run_turn(
        &d,
        json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
               "message": "poster"}),
    )
    .await;
    let messages = d.settled_messages(&project, &conversation).await;
    assert_eq!(messages[1]["runStatus"], "succeeded", "{:?}", messages[1]);
    let stored = messages[1]["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|e| e["type"] == "critique")
        .unwrap()
        .clone();
    assert_eq!(stored["report"]["status"], "shipped", "{stored}");
    assert!(stored["report"].get("artifactPath").is_none(), "{stored}");
    assert!(
        stored["report"]["degradedReason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "{stored}"
    );
}

/// A daemon that died mid-run left its comments `applying`; the next
/// daemon on the folder hands them back to the next turn.
#[tokio::test]
async fn restart_puts_applying_comments_back_to_attached() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let store = od_store::Store::open(od_store::DataDir::new(&data)).unwrap();
    let (project, conversation) = store
        .create_project(CreateProjectRequest {
            name: "p".into(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    let comment = store
        .create_comment(
            &project.id,
            &conversation,
            CreateCommentRequest {
                file_path: "index.html".into(),
                element_id: String::new(),
                selector: "h1".into(),
                label: String::new(),
                text: String::new(),
                position: od_contracts::CommentRect::default(),
                html_hint: String::new(),
                slide_index: None,
                note: "bigger".into(),
                status: Some(CommentStatus::Applying),
                attachments: Vec::new(),
            },
        )
        .unwrap();
    store
        .begin_turn(&od_store::NewTurn {
            conversation_id: &conversation,
            user_message_id: "u1",
            assistant_message_id: "a1",
            content: "apply the comment",
            attachments: &[],
            agent_id: "claude",
            run_id: "r1",
        })
        .unwrap();
    drop(store);

    let store = od_store::Store::open(od_store::DataDir::new(&data)).unwrap();
    let lock = od_server::DaemonLock::acquire(&data).unwrap();
    od_server::open_runs(&store, &lock).await.unwrap();
    let comments = store.list_comments(&project.id, &conversation).unwrap();
    assert_eq!(comments[0].id, comment.id);
    assert_eq!(comments[0].status, CommentStatus::Attached);
    let messages = store.list_messages(&conversation).unwrap();
    assert_eq!(
        messages[1].run_status,
        Some(od_contracts::RunStatus::Failed)
    );
}
