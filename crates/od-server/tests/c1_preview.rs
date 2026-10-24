//! Track C1-preview over HTTP: comments with images, manual edits written
//! back to the source (attributes, element HTML, removal, image swap), the
//! source read of one element, and undo/redo through guarded restores.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use common::daemon;
use serde_json::{Value, json};

async fn write(d: &common::Daemon, project: &str, name: &str, content: &str) {
    let res = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": name, "content": content, "overwrite": true}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200, "write {name}");
}

async fn status_and_json(res: reqwest::Response) -> (u16, Value) {
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn comments_carry_project_images() {
    let d = daemon().await;
    let (project, conversation) = d.project().await;
    write(&d, &project, "shots/a.png", "png").await;
    write(&d, &project, "notes.txt", "text").await;
    let path = format!("/api/projects/{project}/conversations/{conversation}/comments");
    let comment = |attachments: Value| {
        json!({
            "filePath": "index.html",
            "selector": "h1",
            "note": "Like this",
            "attachments": attachments,
        })
    };
    let (status, made) =
        status_and_json(d.post(&path, &comment(json!(["shots/a.png"]))).await).await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(
        made["comment"]["attachments"],
        json!([{"path": "shots/a.png", "name": "a.png"}])
    );
    let listed = d.get_json(&path).await;
    assert_eq!(
        listed["comments"][0]["attachments"][0]["path"],
        "shots/a.png"
    );
    for bad in [
        json!(["notes.txt"]),
        json!(["missing.png"]),
        json!(["../a.png"]),
        json!([
            "shots/a.png",
            "shots/a.png",
            "shots/a.png",
            "shots/a.png",
            "shots/a.png"
        ]),
    ] {
        let (status, body) = status_and_json(d.post(&path, &comment(bad.clone())).await).await;
        assert!((400..500).contains(&status), "{bad} → {status} {body}");
    }
    assert_eq!(
        d.get_json(&path).await["comments"].as_array().map(Vec::len),
        Some(1)
    );
}

const PAGE: &str = "<!doctype html><html><head><title>T</title></head><body>\
<h1 id=\"t\" class=\"big\">Hello</h1><section data-od-id=\"hero\"><p>Body</p></section>\
<img src=\"old.png\" alt=\"Old\"></body></html>";
const H1: &str = "html > body:nth-of-type(1) > h1:nth-of-type(1)";
const IMG: &str = "html > body:nth-of-type(1) > img:nth-of-type(1)";
const HERO: &str = "[data-od-id=\"hero\"]";

async fn file(d: &common::Daemon, project: &str) -> String {
    d.http
        .get(format!("{}/api/projects/{project}/raw/index.html", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

async fn edit(d: &common::Daemon, project: &str, edit: Value) -> (u16, Value) {
    status_and_json(
        d.post(
            &format!("/api/projects/{project}/source-edits"),
            &json!({"file": "index.html", "edits": [edit], "label": "Manual edit"}),
        )
        .await,
    )
    .await
}

async fn restore(d: &common::Daemon, project: &str, to: &str, expect: &str) -> (u16, Value) {
    status_and_json(
        d.post(
            &format!("/api/projects/{project}/files/index.html/versions/{to}/restore"),
            &json!({"expectVersionId": expect}),
        )
        .await,
    )
    .await
}

#[tokio::test]
async fn manual_edits_read_and_write_the_source_with_undo() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    write(&d, &project, "index.html", PAGE).await;

    // Read one element as the source has it.
    let (status, el) = status_and_json(
        d.post(
            &format!("/api/projects/{project}/source-element"),
            &json!({"file": "index.html", "selector": H1, "expectTag": "h1", "expectText": "Hello"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{el}");
    assert_eq!(el["outerHtml"], "<h1 id=\"t\" class=\"big\">Hello</h1>");
    assert_eq!(
        el["attributes"],
        json!([{"name": "id", "value": "t"}, {"name": "class", "value": "big"}])
    );
    assert_eq!(el["textOnly"], true);
    let (status, _) = status_and_json(
        d.post(
            &format!("/api/projects/{project}/source-element"),
            &json!({"file": "index.html", "selector": H1, "expectText": "Other"}),
        )
        .await,
    )
    .await;
    assert_eq!(
        status, 409,
        "a source that moved on is not read as the element"
    );

    // Attributes: class changed, id removed.
    let (status, first) = edit(
        &d,
        &project,
        json!({"selector": H1, "expectTag": "h1", "attributes": {"class": "huge", "id": null}}),
    )
    .await;
    assert_eq!(status, 200, "{first}");
    assert!(
        file(&d, &project)
            .await
            .contains("<h1 class=\"huge\">Hello</h1>")
    );
    let before_first = first["previousVersionId"].as_str().unwrap().to_owned();
    let after_first = first["version"]["id"].as_str().unwrap().to_owned();

    // Swap the image: a new project file as its src.
    write(&d, &project, "assets/new.png", "png").await;
    let (status, _) = edit(
        &d,
        &project,
        json!({"selector": IMG, "expectTag": "img", "attributes": {"src": "assets/new.png", "alt": "New"}}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        file(&d, &project)
            .await
            .contains("<img src=\"assets/new.png\" alt=\"New\">")
    );

    // Whole element HTML, then removal.
    let (status, body) = edit(
        &d,
        &project,
        json!({"selector": HERO, "outerHtml": "<div><p>Changed</p></div>"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        file(&d, &project)
            .await
            .contains("<div data-od-id=\"hero\"><p>Changed</p></div>")
    );
    let (status, body) = edit(
        &d,
        &project,
        json!({"selector": HERO, "outerHtml": "<p>a</p><p>b</p>"}),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let (status, removed) = edit(
        &d,
        &project,
        json!({"selector": HERO, "remove": true, "expectTag": "div"}),
    )
    .await;
    assert_eq!(status, 200, "{removed}");
    assert!(!file(&d, &project).await.contains("hero"));
    let (status, _) = edit(
        &d,
        &project,
        json!({"selector": "html > body:nth-of-type(1)", "remove": true}),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = edit(
        &d,
        &project,
        json!({"selector": H1, "attributes": {"data-od-id": "x"}}),
    )
    .await;
    assert_eq!(status, 400);

    // Undo the removal: back to before it, only while the file is as left.
    let before_removal = removed["previousVersionId"].as_str().unwrap().to_owned();
    let after_removal = removed["version"]["id"].as_str().unwrap().to_owned();
    let (status, undone) = restore(&d, &project, &before_removal, &after_removal).await;
    assert_eq!(status, 200, "{undone}");
    assert!(file(&d, &project).await.contains("<p>Changed</p>"));
    // Redo, expecting the undone state.
    let (status, _) = restore(&d, &project, &after_removal, &before_removal).await;
    assert_eq!(status, 200);
    assert!(!file(&d, &project).await.contains("hero"));
    // An undo whose edit is no longer the file's state is refused.
    let (status, body) = restore(&d, &project, &before_first, &after_first).await;
    assert_eq!(status, 409, "{body}");
    assert!(
        !file(&d, &project).await.contains("id=\"t\""),
        "nothing was overwritten"
    );
    let versions = d
        .get_json(&format!(
            "/api/projects/{project}/files/index.html/versions"
        ))
        .await;
    let labels: Vec<&str> = versions["versions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["label"].as_str())
        .collect();
    assert!(
        labels.iter().filter(|l| **l == "Manual edit").count() >= 4,
        "{labels:?}"
    );
}
