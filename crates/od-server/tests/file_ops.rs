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

// Symlinks are part of what must survive a move, so Unix only.
#[cfg(unix)]
#[tokio::test]
async fn a_moved_folder_keeps_everything_in_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));
    let id = project(
        &app,
        &[("site/index.html", "<h1>hi</h1>"), ("archive/a.md", "# A")],
    )
    .await;
    // What the files list never shows: hidden files, ignored trees, empty
    // folders, symlinks (kept as links, not followed).
    let dir = tmp.path().join("data/projects").join(&id);
    std::fs::write(dir.join("site/.env"), "SECRET=1").unwrap();
    std::fs::create_dir_all(dir.join("site/dist")).unwrap();
    std::fs::write(dir.join("site/dist/app.js"), "go()").unwrap();
    std::fs::create_dir_all(dir.join("site/node_modules/x")).unwrap();
    std::fs::write(dir.join("site/node_modules/x/i.js"), "x").unwrap();
    std::fs::create_dir_all(dir.join("site/empty")).unwrap();
    std::os::unix::fs::symlink("index.html", dir.join("site/link")).unwrap();

    let (s, b) = call(
        &app,
        "POST",
        &format!("/api/projects/{id}/move-folder"),
        Some(json!({"from": "site", "to": "archive/site"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    assert_eq!(json_of(&b), json!({"folder": "archive/site", "files": 1}));
    assert!(!dir.join("site").exists());
    let moved = dir.join("archive/site");
    for (path, text) in [
        ("index.html", "<h1>hi</h1>"),
        (".env", "SECRET=1"),
        ("dist/app.js", "go()"),
        ("node_modules/x/i.js", "x"),
    ] {
        assert_eq!(
            std::fs::read_to_string(moved.join(path)).unwrap(),
            text,
            "{path}"
        );
    }
    assert!(moved.join("empty").is_dir());
    let link = std::fs::symlink_metadata(moved.join("link")).unwrap();
    assert!(link.file_type().is_symlink());
    // The page history moves with it.
    let (s, b) = call(
        &app,
        "GET",
        &format!("/api/projects/{id}/files/archive/site/index.html/versions"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    assert_eq!(
        json_of(&b)["versions"].as_array().unwrap().len(),
        1,
        "{b:?}"
    );

    let uri = format!("/api/projects/{id}/move-folder");
    let mv = |from: &str, to: &str| call(&app, "POST", &uri, Some(json!({"from": from, "to": to})));
    // A taken target, a folder into itself, hidden or missing sources and
    // escapes are refused, and nothing moves.
    std::fs::create_dir_all(dir.join("other")).unwrap();
    assert_eq!(mv("archive/site", "other").await.0, StatusCode::CONFLICT);
    assert_eq!(
        mv("archive", "archive/site/archive").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        mv("archive/site/.env", "x").await.0,
        StatusCode::BAD_REQUEST
    );
    for hidden in [".site", "x/.site"] {
        assert_eq!(
            mv("archive/site", hidden).await.0,
            StatusCode::BAD_REQUEST,
            "{hidden}"
        );
    }
    assert_eq!(mv("archive/a.md", "b.md").await.0, StatusCode::NOT_FOUND);
    assert_eq!(mv("nope", "x").await.0, StatusCode::NOT_FOUND);
    assert_eq!(mv("archive/site", "../up").await.0, StatusCode::BAD_REQUEST);
    std::os::unix::fs::symlink("other", dir.join("alias")).unwrap();
    assert_eq!(mv("alias", "moved-alias").await.0, StatusCode::NOT_FOUND);
    assert!(dir.join("other").is_dir() && moved.join("dist/app.js").is_file());
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

#[tokio::test]
async fn content_is_replaced_at_the_exact_path() {
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));
    let id = project(&app, &[]).await;
    // A name the sanitizer would rewrite, as an agent may write it.
    let dir = tmp.path().join(format!("data/projects/{id}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("My Deck.html"), "<p>1</p>").unwrap();
    std::fs::write(dir.join("Brief notes.md"), "# Old").unwrap();

    let put = |path: &str, body: &str| {
        Request::builder()
            .method("PUT")
            .uri(format!("/api/projects/{id}/content/{path}"))
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let res = app
        .clone()
        .oneshot(put("Brief%20notes.md", "# New"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(dir.join("Brief notes.md")).unwrap(),
        "# New"
    );

    let res = app
        .clone()
        .oneshot(put("My%20Deck.html?label=Speaker%20notes", "<p>2</p>"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(dir.join("My Deck.html")).unwrap(),
        "<p>2</p>"
    );
    assert!(!dir.join("My-Deck.html").exists(), "no sanitized copy");
    let (_, b) = call(
        &app,
        "GET",
        &format!("/api/projects/{id}/files/My%20Deck.html/versions"),
        None,
    )
    .await;
    let text = String::from_utf8_lossy(&b);
    assert!(text.contains("Speaker notes"), "{text}");

    // Only existing files: new ones go through POST files.
    let res = app.clone().oneshot(put("new.md", "x")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(!dir.join("new.md").exists());
    let res = app
        .clone()
        .oneshot(put("..%2Fescape.md", "x"))
        .await
        .unwrap();
    assert!(res.status().is_client_error());
}

#[tokio::test]
async fn office_files_show_their_text() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));
    let id = project(&app, &[]).await;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "ppt/slides/slide1.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"<p:sld><a:p><a:r><a:t>Hello tea</a:t></a:r></a:p></p:sld>")
        .unwrap();
    let pptx = zip.finish().unwrap().into_inner();
    let dir = tmp.path().join(format!("data/projects/{id}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Pitch deck.pptx"), &pptx).unwrap();
    std::fs::write(dir.join("broken.docx"), b"not a zip").unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();

    let (s, b) = call(
        &app,
        "GET",
        &format!("/api/projects/{id}/office-text/Pitch%20deck.pptx"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    assert_eq!(
        json_of(&b),
        json!({"kind": "presentation", "sections": [{"title": "1", "text": "Hello tea"}]})
    );
    for (path, status) in [
        ("broken.docx", StatusCode::UNPROCESSABLE_ENTITY),
        ("notes.txt", StatusCode::BAD_REQUEST),
        ("missing.xlsx", StatusCode::NOT_FOUND),
    ] {
        let (s, _) = call(
            &app,
            "GET",
            &format!("/api/projects/{id}/office-text/{path}"),
            None,
        )
        .await;
        assert_eq!(s, status, "{path}");
    }
}
