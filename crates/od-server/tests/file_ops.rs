//! Files panel routes: folders, a ZIP of selected files, save as template.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_server::{AppState, ServerConfig, router};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

fn state(tmp: &Path) -> AppState {
    let data = tmp.join("data");
    let empty = tmp.join("no-content");
    AppState {
        store: Arc::new(Store::open(DataDir::new(&data)).unwrap()),
        content: Arc::new(ContentStore::load(
            ContentRoots::bundled(&empty).with_user_root(&data),
        )),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: empty,
            daemon_url: None,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    }
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    (
        status,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

/// A project with `files` (name, content) written through the API.
async fn project(app: &Router, files: &[(&str, &str)]) -> String {
    let (s, b) = call(
        app,
        "POST",
        "/api/projects",
        Some(json!({"name": "Tea Shop"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    let id = json_of(&b)["project"]["id"].as_str().unwrap().to_owned();
    for (name, content) in files {
        let (s, b) = call(
            app,
            "POST",
            &format!("/api/projects/{id}/files"),
            Some(json!({"name": name, "content": content})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    }
    id
}

#[tokio::test]
async fn folders_are_created_listed_and_deleted_with_their_files() {
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));
    let id = project(&app, &[("index.html", "<h1>hi</h1>"), ("docs/a.md", "# A")]).await;

    let (s, b) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/folders"),
        Some(json!({"path": "assets/Brand Kit"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    assert_eq!(json_of(&b)["folder"], "assets/Brand-Kit");

    let (_, b) = call(&app, "GET", &format!("/api/projects/{id}/folders"), None).await;
    assert_eq!(
        json_of(&b)["folders"],
        json!(["assets", "assets/Brand-Kit", "docs"])
    );

    // A file name is taken; escapes and app state are refused.
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/folders"),
        Some(json!({"path": "index.html"})),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/folders"),
        Some(json!({"path": "../up"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("/api/projects/{id}/folders/.od"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = call(
        &app,
        "DELETE",
        &format!("/api/projects/{id}/folders/docs"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, b) = call(&app, "GET", &format!("/api/projects/{id}/files"), None).await;
    let listed = json_of(&b);
    let names: Vec<&str> = listed["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(!names.iter().any(|n| n.starts_with("docs/")), "{names:?}");
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("/api/projects/{id}/folders/docs"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_selection_zip_holds_exactly_the_chosen_files() {
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));
    let id = project(
        &app,
        &[
            ("index.html", "<p>1</p>"),
            ("css/a.css", "a{}"),
            ("b.txt", "b"),
        ],
    )
    .await;
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/projects/{id}/archive/selection"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"files": ["css/a.css", "index.html", "index.html"]}).to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let disposition = res.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(disposition.contains("Tea Shop.zip"), "{disposition}");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["css/a.css", "index.html"]);
    let mut css = String::new();
    zip.by_name("css/a.css")
        .unwrap()
        .read_to_string(&mut css)
        .unwrap();
    assert_eq!(css, "a{}");

    for (body, status) in [
        (json!({"files": []}), StatusCode::BAD_REQUEST),
        (json!({"files": ["missing.txt"]}), StatusCode::NOT_FOUND),
        (
            json!({"files": ["../data/app.sqlite"]}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (s, _) = call(
            &app,
            "POST",
            &format!("/api/projects/{id}/archive/selection"),
            Some(body.clone()),
        )
        .await;
        assert_eq!(s, status, "{body}");
    }
}

#[tokio::test]
async fn a_saved_template_shows_in_the_catalogue() {
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));
    let id = project(
        &app,
        &[
            ("index.html", "<link rel=stylesheet href=s.css><h1>Tea</h1>"),
            ("s.css", "h1{color:red}"),
        ],
    )
    .await;
    let save =
        |name: &str| json!({"name": name, "description": "Warm landing", "fileName": "index.html"});
    let (s, b) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/save-template"),
        Some(save("Tea Landing")),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    assert_eq!(json_of(&b)["id"], "my-tea-landing");
    // The same name again gets its own id.
    let (_, b) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/save-template"),
        Some(save("Tea Landing")),
    )
    .await;
    assert_eq!(json_of(&b)["id"], "my-tea-landing-2");

    let dir = tmp.path().join("data/design-templates/my-tea-landing");
    let example = std::fs::read_to_string(dir.join("example.html")).unwrap();
    // One file: the stylesheet is inlined (`h1{color:red}` in base64).
    assert!(
        example.contains("aDF7Y29sb3I6cmVkfQ=="),
        "styles bundled: {example}"
    );

    let (_, b) = call(&app, "GET", "/api/design-templates", None).await;
    let text = String::from_utf8_lossy(&b);
    assert!(text.contains("my-tea-landing"), "{text}");
    assert!(text.contains("Warm landing"), "{text}");

    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/save-template"),
        Some(json!({"name": " ", "fileName": "index.html"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/save-template"),
        Some(json!({"name": "x", "fileName": "nope.html"})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
