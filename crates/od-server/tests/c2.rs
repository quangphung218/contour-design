//! Track C2-chat-home through HTTP with mock CLIs: Settings ▸ Memory
//! (switch, hooks, MEMORY.md, import, change events), "Import from
//! library", Design Jury ▸ Interrupt, media progress on the run stream and
//! the Report Issue summary. No real agent, no network beyond loopback.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

mod common;

use common::*;
use serde_json::{Value, json};
use std::time::Duration;

/// Logs each prompt line to `prompts.log`, then answers `text`.
fn claude_logging(log: &std::path::Path) -> String {
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

fn last_prompt(d: &Daemon) -> String {
    std::fs::read_to_string(d.mocks.join("prompts.log"))
        .unwrap_or_default()
        .lines()
        .last()
        .unwrap_or_default()
        .to_owned()
}

async fn turn(d: &Daemon, project: &str, conversation: &str, message: &str) -> Vec<SseEvent> {
    let run = d
        .start(json!({"agentId": "claude", "projectId": project,
                      "conversationId": conversation, "message": message}))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(end_of(&events)["status"], "succeeded", "{events:?}");
    events
}

async fn send(d: &Daemon, method: reqwest::Method, path: &str, body: &Value) -> (u16, Value) {
    let res = d
        .http
        .request(method, format!("{}{path}", d.base))
        .json(body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

async fn add_memory(d: &Daemon, kind: &str, text: &str) -> String {
    let (status, v) = send(
        d,
        reqwest::Method::POST,
        "/api/memory",
        &json!({"kind": kind, "text": text}),
    )
    .await;
    assert_eq!(status, 200, "{v}");
    v["memory"]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn memory_switch_hooks_and_index_decide_what_the_prompt_carries() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;

    let cfg = d.get_json("/api/memory/config").await;
    assert_eq!(
        cfg,
        json!({"enabled": true, "chatExtractionEnabled": false, "profileEnabled": true,
               "rewriteEnabled": true, "verifyEnabled": true})
    );
    let teal = add_memory(&d, "user", "Brand colour is teal").await;
    let serif = add_memory(&d, "rule", "Headings are serif").await;
    add_memory(&d, "profile", "Role: product designer").await;

    // No saved index: every entry is linked and injected, hooks too.
    let index = d.get_json("/api/memory/index").await;
    let text = index["index"].as_str().unwrap();
    assert!(text.contains(&format!("({teal}.md) — user")), "{text}");
    assert!(index["rootDir"].as_str().unwrap().ends_with("memory"));
    turn(&d, &project, &conversation, "make a page").await;
    let p = last_prompt(&d);
    assert!(
        p.contains("Brand colour is teal") && p.contains("Role: product designer"),
        "{p}"
    );
    assert!(
        p.contains("## Memory hooks") && p.contains("Self-verify"),
        "{p}"
    );

    // Dropping a line from MEMORY.md turns that fact off; the entry stays.
    let trimmed = od_store::memory_index::without_line(text, &teal);
    let (status, _) = send(
        &d,
        reqwest::Method::PUT,
        "/api/memory/index",
        &json!({"index": trimmed}),
    )
    .await;
    assert_eq!(status, 200);
    let list = d.get_json("/api/memory").await;
    assert_eq!(list["memories"].as_array().unwrap().len(), 3);
    // A new entry is linked into the saved index, so it is injected.
    add_memory(&d, "feedback", "Short copy").await;
    let saved = std::fs::read_to_string(d.data.join("memory/MEMORY.md")).unwrap();
    assert!(
        saved.contains("[Short copy]") && !saved.contains(&teal),
        "{saved}"
    );
    // Deleting an entry drops its line.
    let (status, _) = send(
        &d,
        reqwest::Method::DELETE,
        &format!("/api/memory/{serif}"),
        &json!({}),
    )
    .await;
    assert_eq!(status, 200);
    let saved = std::fs::read_to_string(d.data.join("memory/MEMORY.md")).unwrap();
    assert!(!saved.contains(&serif), "{saved}");

    // Profile hook off: no profile facts. Rules gone: no self-verify.
    let (status, cfg) = send(
        &d,
        reqwest::Method::PATCH,
        "/api/memory/config",
        &json!({"profileEnabled": false}),
    )
    .await;
    assert_eq!(
        (
            status,
            cfg["profileEnabled"].clone(),
            cfg["enabled"].clone()
        ),
        (200, json!(false), json!(true))
    );
    turn(&d, &project, &conversation, "again").await;
    let p = last_prompt(&d);
    assert!(!p.contains("Brand colour is teal"), "unlinked: {p}");
    assert!(!p.contains("Role: product designer"), "profile off: {p}");
    assert!(
        p.contains("Short copy") && p.contains("Rewrite short asks") && !p.contains("Self-verify"),
        "{p}"
    );

    // Master switch off: nothing global, no hooks; project memory stays.
    send(
        &d,
        reqwest::Method::PATCH,
        "/api/memory/config",
        &json!({"enabled": false}),
    )
    .await;
    turn(
        &d,
        &project,
        &conversation,
        "Remember that the logo sits left",
    )
    .await;
    let p = last_prompt(&d);
    assert!(
        !p.contains("Short copy") && !p.contains("## Memory hooks"),
        "{p}"
    );
    assert!(
        p.contains("the logo sits left"),
        "project memory still applies: {p}"
    );

    let (status, _) = send(
        &d,
        reqwest::Method::PUT,
        "/api/memory/index",
        &json!({"index": "x".repeat(70 * 1024)}),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn memory_import_proposes_facts_from_markdown() {
    let d = daemon().await;
    let (status, v) = send(
        &d,
        reqwest::Method::POST,
        "/api/memory/import",
        &json!({"text": "# Notes\n## Rules\n- No emoji icons\n- **Teal** accent\n\nprose\n"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        v["suggestions"],
        json!([{"kind": "rule", "text": "No emoji icons"}, {"kind": "rule", "text": "Teal accent"}])
    );
}

/// Reads `change` events of `/api/memory/events` for `wait`.
async fn memory_changes(reader: &mut SseReader, wait: Duration) -> Vec<Value> {
    let mut out = Vec::new();
    let _ = tokio::time::timeout(wait, async {
        while let Some(ev) = reader.next().await {
            if ev.event == "change" {
                out.push(ev.data);
            }
        }
    })
    .await;
    out
}

#[tokio::test]
async fn chats_teach_memory_and_announce_it() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    let res = d
        .http
        .get(format!("{}/api/memory/events", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let mut reader = SseReader::new(res);
    let first = reader.next().await.unwrap();
    assert_eq!(first.event, "connected");

    // "Learn from chats" off: a preference is not learned; "remember" still
    // goes to the project's memory, announced as such.
    turn(
        &d,
        &project,
        &conversation,
        "I prefer serif headings everywhere",
    )
    .await;
    turn(
        &d,
        &project,
        &conversation,
        "Remember that the brand is calm",
    )
    .await;
    let changes = memory_changes(&mut reader, Duration::from_millis(500)).await;
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(
        (
            changes[0]["kind"].clone(),
            changes[0]["scope"].clone(),
            changes[0]["count"].clone()
        ),
        (json!("extract"), json!("project"), json!(1))
    );
    assert!(
        d.get_json("/api/memory").await["memories"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    send(
        &d,
        reqwest::Method::PATCH,
        "/api/memory/config",
        &json!({"chatExtractionEnabled": true}),
    )
    .await;
    let _config_change = memory_changes(&mut reader, Duration::from_millis(200)).await;
    turn(
        &d,
        &project,
        &conversation,
        "I prefer serif headings everywhere",
    )
    .await;
    // The same fact twice is learned once.
    turn(
        &d,
        &project,
        &conversation,
        "I prefer serif headings everywhere",
    )
    .await;
    let changes = memory_changes(&mut reader, Duration::from_millis(500)).await;
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(
        (changes[0]["scope"].clone(), changes[0]["source"].clone()),
        (json!("global"), json!("heuristic"))
    );
    let list = d.get_json("/api/memory").await;
    assert_eq!(
        list["memories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| (m["kind"].clone(), m["text"].clone()))
            .collect::<Vec<_>>(),
        [(
            json!("feedback"),
            json!("I prefer serif headings everywhere")
        )]
    );
}

fn png() -> Vec<u8> {
    od_export::image::solid_png(2, 2, [9, 9, 9]).unwrap()
}

#[tokio::test]
async fn library_assets_are_copied_into_the_project_and_attached() {
    let d = daemon().await;
    let (source, _) = d.project().await;
    let (target, _) = d.project().await;
    d.state
        .store
        .write_file(&source, "shots/hero.png", &png(), true, None)
        .unwrap();
    d.state
        .store
        .write_file(&source, "notes.md", b"# x", true, None)
        .unwrap();
    d.state
        .store
        .write_file(&target, "own.png", &png(), true, None)
        .unwrap();

    let apply = |assets: Value| json!({"projectId": target, "assets": assets});
    let (status, v) = send(&d, reqwest::Method::POST, "/api/library/assets/apply",
        &apply(json!([{"projectId": source, "path": "shots/hero.png"}, {"projectId": source, "path": "shots/hero.png"}]))).await;
    assert_eq!(status, 200, "{v}");
    let paths: Vec<&str> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["library/hero.png", "library/hero-1.png"],
        "fresh names, never overwritten"
    );
    let copied = std::fs::read(d.data.join(format!("projects/{target}/library/hero.png"))).unwrap();
    assert_eq!(copied, png());

    // An asset of the target project itself is attached in place.
    let (status, v) = send(
        &d,
        reqwest::Method::POST,
        "/api/library/assets/apply",
        &apply(json!([{"projectId": target, "path": "own.png"}])),
    )
    .await;
    assert_eq!(
        (status, v["files"][0]["path"].clone()),
        (200, json!("own.png"))
    );
    assert!(
        !d.data
            .join(format!("projects/{target}/library/own.png"))
            .exists()
    );

    for (body, want) in [
        (
            apply(json!([{"projectId": source, "path": "notes.md"}])),
            400,
        ),
        (apply(json!([])), 400),
        (
            apply(json!([{"projectId": source, "path": "gone.png"}])),
            404,
        ),
        (
            json!({"projectId": "missing", "assets": [{"projectId": source, "path": "shots/hero.png"}]}),
            404,
        ),
    ] {
        let (status, v) = send(
            &d,
            reqwest::Method::POST,
            "/api/library/assets/apply",
            &body,
        )
        .await;
        assert_eq!(status, want, "{body} → {v}");
    }
}

/// A jury that closes round 1, then thinks forever (until interrupted).
fn slow_jury(log: &std::path::Path) -> String {
    let round = [
        r#"<CRITIQUE_RUN version="1" maxRounds="3" threshold="8.0" scale="10"><ROUND n="1">"#,
        r#"<PANELIST role="designer"><NOTES>Draft.</NOTES><ARTIFACT mime="text/html"><![CDATA[<h1 id="draft">Round one</h1>]]></ARTIFACT></PANELIST>"#,
        r#"<PANELIST role="critic" score="6"></PANELIST><PANELIST role="brand" score="7"></PANELIST><PANELIST role="a11y" score="6"></PANELIST><PANELIST role="copy" score="7"></PANELIST>"#,
        r#"<ROUND_END n="1" decision="continue"></ROUND_END></ROUND><ROUND n="2"><PANELIST role="designer"><NOTES>Trying.</NOTES></PANELIST>"#,
    ];
    let lines: String = round
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let line = json!({"type": "assistant", "message": {"id": format!("j{i}"),
                "content": [{"type": "text", "text": text}]}})
            .to_string();
            format!("printf '%s\\n' '{line}'\n")
        })
        .collect();
    format!(
        r#"{PROBES}
{PARSE_SESSION}
IFS= read -r line
printf '%s\n' "$line" >> "{log}/prompts.log"
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
{lines}
sleep 30
"#,
        log = log.display()
    )
}

#[tokio::test]
async fn interrupting_the_jury_stops_the_run_and_ships_the_best_round() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &slow_jury(&d.mocks));
    let (project, conversation) = d.project().await;
    let (other, _) = d.project().await;
    d.http
        .put(format!("{}/api/projects/{project}/critique", d.base))
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    let run = d
        .start(
            json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
                      "message": "poster"}),
        )
        .await;
    let mut reader = d.stream(&run.run_id, None).await;
    // Wait until round 1 closed on the stream.
    let wait = async {
        while let Some(ev) = reader.next().await {
            if ev.event == "agent"
                && ev.data["type"] == "critique"
                && ev.data["report"]["rounds"][0]["closed"] == true
            {
                return;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .unwrap();

    let path = |p: &str| format!("/api/projects/{p}/critique/{}/interrupt", run.run_id);
    let (status, _) = send(&d, reqwest::Method::POST, &path(&other), &json!({})).await;
    assert_eq!(status, 404, "a run of another project");
    let (status, v) = send(&d, reqwest::Method::POST, &path(&project), &json!({})).await;
    assert_eq!(
        (status, v["prevStatus"].clone(), v["accepted"].clone()),
        (202, json!("running"), json!(true))
    );
    let rest = tokio::time::timeout(Duration::from_secs(20), async {
        let mut out = Vec::new();
        while let Some(ev) = reader.next().await {
            out.push(ev);
        }
        out
    })
    .await
    .unwrap();
    assert_eq!(end_of(&rest)["status"], "canceled");
    let (status, v) = send(&d, reqwest::Method::POST, &path(&project), &json!({})).await;
    assert_eq!(
        (status, v["prevStatus"].clone()),
        (202, json!("interrupted")),
        "a repeat is accepted"
    );

    let messages = d.settled_messages(&project, &conversation).await;
    let report = messages[1]["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|e| e["type"] == "critique")
        .unwrap()["report"]
        .clone();
    assert_eq!(
        (report["status"].clone(), report["shipRound"].clone()),
        (json!("interrupted"), json!(1))
    );
    let artifact = report["artifactPath"].as_str().unwrap();
    let html = d
        .http
        .get(format!("{}/api/projects/{project}/raw/{artifact}", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("Round one"), "the best round shipped: {html}");

    // A run that ended on its own cannot be interrupted.
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let done = d
        .start(
            json!({"agentId": "claude", "projectId": project, "conversationId": conversation,
                      "message": "plain"}),
        )
        .await;
    d.events(&done.run_id, None).await;
    let run_id = done.run_id;
    let (status, v) = send(
        &d,
        reqwest::Method::POST,
        &format!("/api/projects/{project}/critique/{run_id}/interrupt"),
        &json!({}),
    )
    .await;
    assert_eq!(
        (status, v["error"]["currentStatus"].clone()),
        (409, json!("succeeded")),
        "{v}"
    );
}

/// A fake OpenAI Images endpoint; a prompt containing "fail" gets a 500.
async fn fake_images() -> String {
    use axum::routing::post;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/v1/images/generations",
        post(|body: String| async move {
            if body.contains("fail") {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "boom".to_owned(),
                )
                    .into_response();
            }
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(png());
            axum::Json(json!({ "data": [{ "b64_json": b64 }] })).into_response()
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

use axum::response::IntoResponse;

#[tokio::test]
async fn media_the_agent_generates_streams_progress_on_its_run() {
    let d = daemon().await;
    let base = fake_images().await;
    let (status, _) = send(
        &d,
        reqwest::Method::PUT,
        "/api/media/config",
        &json!({"providers": {"openai": {"apiKey": "sk-test-1", "baseUrl": format!("{base}/v1")}}}),
    )
    .await;
    assert_eq!(status, 200);
    // The agent calls the media tool twice with its run token.
    let call = |prompt: &str, out: &str| {
        format!(
            "curl -s -X POST \"$OD_DAEMON_URL/api/tools/media/generate\" -H \"authorization: Bearer $OD_TOOL_TOKEN\" -H 'content-type: application/json' -d '{{\"surface\":\"image\",\"model\":\"gpt-image-1\",\"prompt\":\"{prompt}\",\"output\":\"{out}\"}}' >/dev/null\n"
        )
    };
    let body = format!(
        r#"{PROBES}
{PARSE_SESSION}
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"%s","model":"mock"}}\n' "$sid"
{}{}printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"made two"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#,
        call("a red square", "art/one.png"),
        call("please fail", "art/two.png"),
    );
    write_mock(&d.mocks, "claude", &body);
    let (project, conversation) = d.project().await;
    let events = turn(&d, &project, &conversation, "two images").await;
    let media: Vec<Value> = agent_payloads(&events)
        .into_iter()
        .filter(|p| p["type"] == "media")
        .map(|p| p["task"].clone())
        .collect();
    let states: Vec<(String, String)> = media
        .iter()
        .map(|t| {
            (
                t["taskId"].as_str().unwrap().to_owned(),
                t["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(states.len(), 5, "{media:?}"); // running, running+line, done; running, failed
    assert_eq!(states[0].1, "running");
    let first = &media
        .iter()
        .rev()
        .find(|t| t["taskId"] == states[0].0.as_str())
        .unwrap();
    assert_eq!(
        (
            first["status"].clone(),
            first["file"].clone(),
            first["surface"].clone()
        ),
        (json!("done"), json!("art/one.png"), json!("image"))
    );
    assert!(
        first["progress"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "Saved art/one.png")
    );
    let last = media.last().unwrap();
    assert_ne!(last["taskId"], first["taskId"]);
    assert_eq!(last["status"], "failed");
    assert!(last["error"].as_str().unwrap().len() > 3, "{last}");

    // Stored with the turn, so the row survives a reload.
    let messages = d.settled_messages(&project, &conversation).await;
    let stored = messages[1]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "media")
        .count();
    assert_eq!(stored, 5);

    // Outside a run (the Media page) nothing is emitted, and nothing breaks.
    let (status, _) = send(
        &d,
        reqwest::Method::POST,
        &format!("/api/projects/{project}/media/generate"),
        &json!({"surface": "image", "model": "gpt-image-1", "prompt": "plain"}),
    )
    .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn the_issue_summary_lists_facts_without_secrets() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    send(
        &d,
        reqwest::Method::PUT,
        "/api/app-config",
        &json!({"agentId": "claude", "apiKey": "sk-secret-123"}),
    )
    .await;
    turn(&d, &project, &conversation, "hello").await;
    let v = d.get_json("/api/diagnostics/summary").await;
    let text = v["text"].as_str().unwrap();
    assert!(
        text.contains(&format!("- Version: {}", env!("CARGO_PKG_VERSION"))),
        "{text}"
    );
    assert!(
        text.contains("- Agent in use: claude") && text.contains("· claude · succeeded"),
        "{text}"
    );
    assert!(!text.contains("sk-secret"), "{text}");
    assert!(v["logDir"].as_str().unwrap().ends_with("runs"));
}

// Read-only by Unix mode bits.
#[cfg(unix)]
#[tokio::test]
async fn a_memory_index_that_cannot_be_written_does_not_fail_the_run() {
    use std::os::unix::fs::PermissionsExt;
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &claude_logging(&d.mocks));
    let (project, conversation) = d.project().await;
    let patch = json!({"chatExtractionEnabled": true});
    send(&d, reqwest::Method::PATCH, "/api/memory/config", &patch).await;
    let index = json!({"index": "# Memory\n"});
    let (status, _) = send(&d, reqwest::Method::PUT, "/api/memory/index", &index).await;
    assert_eq!(status, 200);
    let dir = d.data.join(od_store::memory_index::MEMORY_DIR);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    turn(
        &d,
        &project,
        &conversation,
        "I prefer serif headings everywhere",
    )
    .await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let memories = d.get_json("/api/memory").await;
    assert_eq!(
        memories["memories"].as_array().unwrap().len(),
        1,
        "{memories}"
    );
}
