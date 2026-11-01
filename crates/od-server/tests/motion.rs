//! `hyperframes-html` through the real media task queue (fake frame
//! renderer, real ffmpeg): `media/scaffold`, a queued render, the MP4 in
//! the asking run's turn (Write, then Edit on a re-render) and never in a
//! run of another project.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_contracts::CreateProjectRequest;
use od_export::hyperframes::frame_count;
use od_export::image::solid_png;
use od_export::renderer::frame_file_name;
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
use od_runs::{Run, RunMeta};
use od_server::routes::w3::{Deps, routes_with};
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Writes solid frames.
struct Frames;

impl Renderer for Frames {
    fn render_slides<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(async { Err(RenderError::Unsupported("slides")) })
    }

    fn render_frames<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: FrameOptions,
        out_dir: &'a Path,
    ) -> BoxFuture<'a, RenderResult<FrameSet>> {
        Box::pin(async move {
            let count = u32::try_from(frame_count(options.duration_ms, options.fps)).unwrap();
            std::fs::create_dir_all(out_dir).unwrap();
            for i in 0..count {
                let png = solid_png(target.width, target.height, [40, 40, 40]).unwrap();
                std::fs::write(out_dir.join(frame_file_name(i)), png).unwrap();
            }
            Ok(FrameSet {
                count,
                fps: options.fps,
                width: target.width,
                height: target.height,
            })
        })
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
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
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
        renderer: Arc::new(Frames),
        env: Arc::new(|_: &str| None),
        limits: od_media::Limits::default(),
        media_concurrency: 1,
    };
    let app = routes_with(&state, deps).with_state(state.clone());
    Harness {
        _tmp: tmp,
        state,
        app,
    }
}

impl Harness {
    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn task(&self, id: &str) -> Value {
        let req = Request::builder()
            .uri(format!("/api/media/tasks/{id}?waitMs=20000"))
            .body(Body::empty())
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<Value>(&bytes).unwrap()["task"].clone()
    }

    fn project(&self, name: &str) -> String {
        self.state
            .store
            .create_project(CreateProjectRequest {
                name: name.to_owned(),
                ..CreateProjectRequest::default()
            })
            .unwrap()
            .0
            .id
    }

    fn run(&self, id: &str, project: &str) -> Arc<Run> {
        self.state
            .runs
            .registry
            .create_or_reuse(RunMeta {
                run_id: id.to_owned(),
                agent_id: "claude".to_owned(),
                project_id: Some(project.to_owned()),
                ..RunMeta::default()
            })
            .unwrap()
            .0
    }

    /// Renders `dir` of `pid` for `run`; the finished task.
    async fn render(&self, pid: &str, run: &str) -> Value {
        let (status, v) = self
            .post(
                &format!("/api/projects/{pid}/media/tasks"),
                json!({"surface": "video", "model": "hyperframes-html",
                    "compositionDir": "motion-source/film", "output": "film.mp4", "runId": run}),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{v}");
        self.task(v["task"]["id"].as_str().unwrap()).await
    }
}

/// The `agent` events of `run` so far.
async fn agent_events(run: &Arc<Run>) -> Vec<Value> {
    let mut sub = run.subscribe(0);
    let mut out = Vec::new();
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(200), sub.next()).await {
        if ev.event == "agent" {
            out.push(ev.data.clone());
        }
    }
    out
}

#[tokio::test]
async fn a_render_joins_the_asking_run_only() {
    let h = harness();
    let pid = h.project("Film");
    let other = h.project("Other");
    let (status, v) = h
        .post(
            &format!("/api/projects/{pid}/media/scaffold"),
            json!({"compositionDir": "motion-source/film", "width": 64, "height": 36, "duration": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["created"], true);

    // A run id of another project: rendered, but not added to that run.
    let foreign = h.run("run-other", &other);
    let task = h.render(&pid, "run-other").await;
    assert_eq!(task["status"], "done", "{task}");
    assert!(
        agent_events(&foreign).await.is_empty(),
        "nothing joins another project's run"
    );

    // The asking run: the MP4 as an Edit (an earlier render exists).
    let run = h.run("run-1", &pid);
    let task = h.render(&pid, "run-1").await;
    assert_eq!(task["status"], "done", "{task}");
    assert_eq!(task["result"]["edited"], true);
    let events = agent_events(&run).await;
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["type"], "tool_use");
    assert_eq!(events[0]["name"], "Edit");
    let path = events[0]["input"]["file_path"].as_str().unwrap();
    assert!(
        path.ends_with("film.mp4") && Path::new(path).is_absolute(),
        "{path}"
    );
    assert!(Path::new(path).is_file());
    assert_eq!(events[1]["type"], "tool_result");
    assert!(
        events[1]["content"].as_str().unwrap().contains("30 fps"),
        "{events:?}"
    );

    // A first render of a new output is a Write.
    let (status, _) = h
        .post(
            &format!("/api/projects/{pid}/media/tasks"),
            json!({"surface": "video", "model": "hyperframes-html",
                "compositionDir": "motion-source/film", "output": "cut/b.mp4", "runId": "run-1"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut tries = 0;
    loop {
        let events = agent_events(&run).await;
        if events.len() == 4 {
            assert_eq!(events[2]["name"], "Write", "{events:?}");
            break;
        }
        tries += 1;
        assert!(tries < 100, "{events:?}");
    }
}

#[tokio::test]
async fn a_bad_request_is_refused_before_queueing() {
    let h = harness();
    let pid = h.project("Film");
    for (body, want) in [
        (
            json!({"surface": "video", "model": "hyperframes-html"}),
            "--composition-dir",
        ),
        (
            json!({"surface": "video", "model": "hyperframes-html", "compositionDir": "../up"}),
            "inside the project",
        ),
    ] {
        let (status, v) = h
            .post(&format!("/api/projects/{pid}/media/tasks"), body)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains(want),
            "{v}"
        );
    }
}
