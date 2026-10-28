//! Track C3 media routes through the real handlers: the catalogue lists the
//! added providers and models, a Settings save keeps every provider, a
//! generation reaches a loopback fake, and `POST /api/research/search`
//! returns Tavily findings or the original's error codes.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_contracts::CreateProjectRequest;
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
use od_server::routes::media::{Deps, routes_with};
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

struct NoRender;

impl Renderer for NoRender {
    fn render_slides<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(async { Err(RenderError::Unsupported("slides")) })
    }
    fn render_frames<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: FrameOptions,
        _: &'a std::path::Path,
    ) -> BoxFuture<'a, RenderResult<FrameSet>> {
        Box::pin(async { Err(RenderError::Unsupported("frames")) })
    }
    fn print_pdf<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(RenderError::Unsupported("pdf")) })
    }
    fn capture_page<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(RenderError::Unsupported("capture")) })
    }
}

struct Harness {
    _tmp: tempfile::TempDir,
    state: AppState,
    app: Router,
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let content = tmp.path().join("no-content");
    let state = AppState {
        store: Arc::new(Store::open(DataDir::new(&data)).unwrap()),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(&content))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: content,
            daemon_url: None,
            mcp_daemon_url: true,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    };
    let deps = Deps {
        renderer: Arc::new(NoRender),
        // Never pick up a real key from the machine running the tests.
        env: Arc::new(|_: &str| None),
        limits: od_media::Limits {
            poll_interval: std::time::Duration::from_millis(10),
            ..od_media::Limits::default()
        },
    };
    let app = routes_with(deps).with_state(state.clone());
    Harness {
        _tmp: tmp,
        state,
        app,
    }
}

impl Harness {
    async fn send(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let res = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

/// A Grok images endpoint and a Tavily search endpoint on 127.0.0.1.
async fn fake() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nfake");
    let app = Router::new()
        .route(
            "/xai/images/generations",
            axum::routing::post(move || async move { axum::Json(json!({"data": [{"b64_json": png}]})) }),
        )
        .route(
            "/tav/search",
            axum::routing::post(|h: axum::http::HeaderMap| async move {
                if h.get("authorization").unwrap() != "Bearer tav-1" {
                    return (StatusCode::UNAUTHORIZED, "bad key").into_response();
                }
                axum::Json(json!({"answer": "Foxes live in dens.", "results": [{"title": "Fox", "url": "https://a.example/fox", "content": "Dens"}]}))
                    .into_response()
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

use axum::response::IntoResponse;

#[tokio::test]
async fn added_providers_are_listed_saved_and_called() {
    let h = harness();
    let (_, models) = h.send("GET", "/api/media/models", None).await;
    let ids: Vec<&str> = models["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    for id in [
        "volcengine",
        "grok",
        "nanobanana",
        "imagerouter",
        "openrouter",
        "custom-image",
        "leonardo",
        "minimax",
        "fishaudio",
        "senseaudio",
        "aihubmix",
        "tavily",
    ] {
        assert!(ids.contains(&id), "{id} missing from {ids:?}");
    }
    let model_ids: Vec<&str> = models["models"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    for id in [
        "grok-imagine-video",
        "minimax-tts",
        "openrouter/google/veo-3.1",
        "leonardo-phoenix",
        "gpt-4o-mini-tts",
    ] {
        assert!(model_ids.contains(&id), "{id} missing");
    }

    let base = fake().await;
    let (status, saved) = h
        .send(
            "PUT",
            "/api/media/config",
            Some(json!({"providers": {
                "grok": {"apiKey": "xai-1234", "baseUrl": format!("{base}/xai")},
                "tavily": {"apiKey": "tav-1", "baseUrl": format!("{base}/tav")},
            }})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["providers"]["grok"]["apiKeyTail"], "1234");
    assert_eq!(saved["providers"]["tavily"]["configured"], true);

    let (project, _) = h
        .state
        .store
        .create_project(CreateProjectRequest {
            name: "Media C3".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    let (status, body) = h
        .send(
            "POST",
            &format!("/api/projects/{}/media/generate", project.id),
            Some(json!({"surface": "image", "model": "grok-imagine-image", "prompt": "fox", "output": "fox"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["name"].as_str(), body["provider"].as_str()),
        (Some("fox.png"), Some("grok"))
    );

    let (status, found) = h
        .send(
            "POST",
            "/api/research/search",
            Some(json!({"query": "fox dens", "maxSources": 3})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["findings"]["summary"], "Foxes live in dens.");
    assert_eq!(
        found["findings"]["sources"][0]["url"],
        "https://a.example/fox"
    );
    assert_eq!(found["findings"]["depth"], "shallow");

    let (status, err) = h
        .send("POST", "/api/research/search", Some(json!({"query": ""})))
        .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("QUERY_REQUIRED"))
    );
    h.send("PUT", "/api/media/config", Some(json!({"providers": {}})))
        .await;
    let (status, err) = h
        .send("POST", "/api/research/search", Some(json!({"query": "q"})))
        .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("TAVILY_API_KEY_MISSING"))
    );
}
