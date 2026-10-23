//! UI routes: app-config, folder import, static bundle.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_server::routes::ui::routes_with_dist;
use od_server::{AppState, ServerConfig, router};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
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

#[tokio::test]
async fn app_config_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let app = router(state(tmp.path()));

    let (s, b) = call(&app, "GET", "/api/app-config", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(json_of(&b), json!({}));

    let patch = json!({"agentId": "claude", "model": "opus", "agentPermissionMode": "ask"});
    let (s, b) = call(&app, "PUT", "/api/app-config", Some(patch.clone())).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(json_of(&b), patch);

    let (s, _) = call(
        &app,
        "PUT",
        "/api/app-config",
        Some(json!({"agentPermissionMode": "never"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (_, b) = call(&app, "PUT", "/api/app-config", Some(json!({"model": null}))).await;
    assert_eq!(
        json_of(&b),
        json!({"agentId": "claude", "agentPermissionMode": "ask"})
    );
    // Persisted on disk, readable by the run engine.
    let on_disk = std::fs::read(tmp.path().join("data/app-config.json")).unwrap();
    assert_eq!(json_of(&on_disk)["agentPermissionMode"], "ask");
}

#[tokio::test]
async fn import_folder_points_at_folder() {
    // `tempdir()` names start with `.tmp`, a hidden segment import refuses.
    let tmp = tempfile::Builder::new().prefix("od-ui-").tempdir().unwrap();
    let app = router(state(tmp.path()));
    let folder = tmp.path().join("my-site");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("index.html"), "<h1>hi</h1>").unwrap();

    let (s, b) = call(
        &app,
        "POST",
        "/api/import/folder",
        Some(json!({"baseDir": folder})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    let v = json_of(&b);
    assert_eq!(v["project"]["name"], "my-site");
    let id = v["project"]["id"].as_str().unwrap().to_owned();

    let (s, b) = call(
        &app,
        "GET",
        &format!("/api/projects/{id}/raw/index.html"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b, b"<h1>hi</h1>");

    let hidden = tmp.path().join(".fake-hidden");
    std::fs::create_dir_all(hidden.join("inside")).unwrap();
    let linked_hidden = tmp.path().join("visible-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&hidden, &linked_hidden).unwrap();
    for bad in [
        json!({"baseDir": "relative/dir"}),
        json!({"baseDir": "."}),
        json!({"baseDir": tmp.path().join("missing")}),
        json!({"baseDir": tmp.path().join("data")}),
        json!({"baseDir": "/"}),
        json!({"baseDir": hidden}),
        json!({"baseDir": hidden.join("inside")}),
        json!({"baseDir": linked_hidden}),
        json!({"baseDir": std::env::var("HOME").unwrap()}),
    ] {
        let (s, _) = call(&app, "POST", "/api/import/folder", Some(bad.clone())).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn static_bundle_and_spa_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let dist = tmp.path().join("dist");
    std::fs::create_dir(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<!doctype html>app").unwrap();
    std::fs::write(dist.join("od-ui-abc_bg.wasm"), b"\0asm").unwrap();
    let app = Router::new()
        .merge(routes_with_dist(dist))
        .with_state(state(tmp.path()));

    let res = app
        .clone()
        .oneshot(
            Request::get("/od-ui-abc_bg.wasm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/wasm");

    // Client-side routes get index.html.
    let (s, b) = call(&app, "GET", "/projects/abc", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b, b"<!doctype html>app");

    // Unknown API routes stay JSON 404s, not the SPA.
    let (s, b) = call(&app, "GET", "/api/nope", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(json_of(&b)["error"]["code"], "NOT_FOUND");

    let (s, _) = call(&app, "GET", "/../secret", None).await;
    assert_eq!(s, StatusCode::OK, "traversal falls back to index.html");
}

#[tokio::test]
async fn missing_bundle_explains_how_to_build() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Router::new()
        .merge(routes_with_dist(tmp.path().join("nope")))
        .with_state(state(tmp.path()));
    let (s, b) = call(&app, "GET", "/", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(String::from_utf8_lossy(&b).contains("trunk build"));
}
