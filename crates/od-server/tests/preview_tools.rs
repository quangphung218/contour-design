//! Preview tools over HTTP (spec 1.2): lint, edits written back to the
//! source as manual versions, the deliverable check after a run.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use common::{PROBES, daemon, end_of, write_mock};
use serde_json::{Value, json};

async fn write(d: &common::Daemon, project: &str, name: &str, content: &str) {
    let res = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": name, "content": content}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
}

async fn read(d: &common::Daemon, project: &str, name: &str) -> String {
    d.http
        .get(format!("{}/api/projects/{project}/files/{name}", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

async fn status_and_json(res: reqwest::Response) -> (u16, Value) {
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn lint_posted_html_and_project_files() {
    let d = daemon().await;
    let (status, body) = status_and_json(
        d.post(
            "/api/artifacts/lint",
            &json!({"html": "<h3>🚀 Fast</h3><p>Lorem ipsum</p>"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = body["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["emoji-icon", "filler-copy"]);
    assert_eq!(body["findings"][0]["severity"], "P0");
    assert!(
        body["agentMessage"]
            .as_str()
            .unwrap()
            .contains("2 P0 (must fix)")
    );
    let (status, _) =
        status_and_json(d.post("/api/artifacts/lint", &json!({"html": ""})).await).await;
    assert_eq!(status, 400);

    let (project, _) = d.project().await;
    write(&d, &project, "pages/index.html", "<p>10× faster</p>").await;
    write(&d, &project, "a.css", "p{}").await;
    let body = d
        .get_json(&format!("/api/projects/{project}/lint/pages/index.html"))
        .await;
    assert_eq!(body["findings"][0]["id"], "invented-metric");
    let clean = d
        .get_json(&format!("/api/projects/{project}/lint/a.css"))
        .await;
    assert_eq!(clean["error"]["code"], "UNSUPPORTED_MEDIA_TYPE");
    let missing = d
        .http
        .get(format!("{}/api/projects/{project}/lint/nope.html", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status().as_u16(), 404);
}

#[tokio::test]
async fn source_edits_write_back_as_manual_versions() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    let page =
        "<!doctype html><html><body><h1 id=\"title\">Tea</h1><p>Hi <b>x</b></p></body></html>";
    write(&d, &project, "index.html", page).await;
    let h1 = "html > body:nth-of-type(1) > h1:nth-of-type(1)";
    let edits = format!("/api/projects/{project}/source-edits");

    let (status, body) = status_and_json(
        d.post(
            &edits,
            &json!({"file": "index.html", "edits": [
                {"selector": h1, "style": {"color": "rgb(10, 125, 59)"}},
                {"selector": h1, "text": "Green tea", "expectText": "Tea"},
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["version"]["source"], "manual");
    assert_eq!(body["version"]["label"], "Edited in preview");
    assert_eq!(
        read(&d, &project, "index.html").await,
        page.replace(
            "<h1 id=\"title\">Tea</h1>",
            "<h1 id=\"title\" style=\"color: rgb(10, 125, 59)\">Green tea</h1>"
        )
    );
    let history = d
        .get_json(&format!(
            "/api/projects/{project}/files/index.html/versions"
        ))
        .await;
    let versions = history["versions"].as_array().unwrap();
    assert_eq!(versions.last().unwrap()["label"], "Edited in preview");
    assert_eq!(versions.last().unwrap()["current"], true);

    // Refusals leave the file alone.
    let before = read(&d, &project, "index.html").await;
    for (edit, code) in [
        (
            json!({"selector": h1, "text": "X", "expectText": "Tea"}),
            409,
        ),
        (
            json!({"selector": "html > body:nth-of-type(1) > p:nth-of-type(1)", "text": "X"}),
            409,
        ),
        (json!({"selector": "h9", "style": {"color": "red"}}), 409),
        // The path now lands on an element that reads differently.
        (
            json!({"selector": h1, "style": {"color": "red"}, "expectTag": "h1", "expectText": "Coffee"}),
            409,
        ),
        (
            json!({"selector": h1, "style": {"color": "red"}, "expectTag": "p", "expectText": "Green tea"}),
            409,
        ),
        (json!({"selector": h1, "style": {"color": "red;x:y"}}), 400),
        (json!({"selector": h1}), 400),
    ] {
        let (status, body) = status_and_json(
            d.post(&edits, &json!({"file": "index.html", "edits": [edit]}))
                .await,
        )
        .await;
        assert_eq!(status, code, "{edit} → {body}");
    }
    assert_eq!(read(&d, &project, "index.html").await, before);

    // Paths go through the store's checks (M1): no escape, HTML only.
    write(&d, &project, "a.css", "p{}").await;
    for (file, code) in [
        ("../index.html", 400),
        ("/etc/hosts.html", 400),
        ("a.css", 400),
        ("missing.html", 404),
    ] {
        let (status, body) = status_and_json(
            d.post(
                &edits,
                &json!({"file": file, "edits": [{"selector": "h1", "style": {"color": "red"}}]}),
            )
            .await,
        )
        .await;
        assert_eq!(status, code, "{file} → {body}");
    }
    let (status, _) = status_and_json(
        d.post(&edits, &json!({"file": "index.html", "edits": []}))
            .await,
    )
    .await;
    assert_eq!(status, 400);
}

/// Mock claude that writes `index.html` through a Write tool call when the
/// prompt says `write`, and only talks otherwise.
fn writer_mock() -> String {
    format!(
        r#"{PROBES}
IFS= read -r line
case "$line" in *write*)
  printf '{{"type":"assistant","message":{{"id":"w","content":[{{"type":"tool_use","id":"t1","name":"Write","input":{{"file_path":"%s/index.html","content":"x"}}}}]}}}}\n' "$PWD"
  printf '<h1>Hi</h1>' > index.html
  printf '{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}\n'
;; esac
printf '{{"type":"assistant","message":{{"id":"m","content":[{{"type":"text","text":"done"}}]}}}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#
    )
}

#[tokio::test]
async fn deliverable_after_a_run() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &writer_mock());
    let (project, conversation) = d.project().await;
    let run = |msg: &str| json!({"agentId": "claude", "projectId": project, "conversationId": conversation, "message": msg});

    let wrote = d.start(run("write the page")).await;
    assert_eq!(
        end_of(&d.events(&wrote.run_id, None).await)["status"],
        "succeeded"
    );
    d.settled_messages(&project, &conversation).await;
    let body = d
        .get_json(&format!("/api/runs/{}/deliverable", wrote.run_id))
        .await;
    assert_eq!(body["valid"], true, "{body}");
    assert_eq!(body["validation"], "valid");
    assert_eq!(body["entryFile"], "index.html");
    assert_eq!(body["artifactKind"], "html");

    let talked = d.start(run("just talk")).await;
    d.events(&talked.run_id, None).await;
    d.settled_messages(&project, &conversation).await;
    let body = d
        .get_json(&format!("/api/runs/{}/deliverable", talked.run_id))
        .await;
    assert_eq!(body["valid"], false);
    assert_eq!(body["validation"], "no_artifact");

    let res = d
        .http
        .get(format!("{}/api/runs/nope/deliverable", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 404);
}
