//! One id follows one piece of work through `daemon.log`: every line a
//! failed export logs carries its request id, a run's lines carry the id
//! of the request that started it, and the diagnostics put those lines
//! first. The log is the real one ([`od_server::logs::init`]), so the test
//! reads what a user would attach.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use futures_util::future::BoxFuture;
use od_content::{ContentRoots, ContentStore};
use od_contracts::CreateProjectRequest;
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, RenderError, RenderResult, RenderTarget, Renderer,
    Slides,
};
use od_server::request_id::HEADER;
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;

/// The log every test here writes to (one global subscriber per binary),
/// and a lock so each test reads only its own lines.
static LOG: LazyLock<(tempfile::TempDir, tokio::sync::Mutex<()>)> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().unwrap();
    od_server::logs::init(Some(tmp.path()));
    (tmp, tokio::sync::Mutex::new(()))
});

fn log_root() -> &'static Path {
    LOG.0.path()
}

fn log_path() -> PathBuf {
    od_server::logs::dir(log_root()).join(od_server::logs::LOG_FILE)
}

fn log_len() -> usize {
    std::fs::read(log_path()).map_or(0, |b| b.len())
}

/// Lines written since `from` (a byte offset).
fn lines_since(from: usize) -> Vec<String> {
    let bytes = std::fs::read(log_path()).unwrap();
    String::from_utf8_lossy(&bytes[from..])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A renderer that fails as a Chrome that crashed does, after logging
/// from a task of its own (as the CDP handler does).
struct Crashing;

impl Crashing {
    async fn fail() -> RenderError {
        tokio::spawn(tracing::Instrument::in_current_span(async {
            tracing::warn!("fake renderer: lost the connection to the browser");
        }))
        .await
        .unwrap();
        RenderError::Failed("the browser quit while printing".into())
    }
}

impl Renderer for Crashing {
    fn render_slides<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(async { Err(Self::fail().await) })
    }
    fn render_frames<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: FrameOptions,
        _: &'a Path,
    ) -> BoxFuture<'a, RenderResult<od_export::FrameSet>> {
        Box::pin(async { Err(Self::fail().await) })
    }
    fn print_pdf<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(Self::fail().await) })
    }
    fn capture_page<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(Self::fail().await) })
    }
}

fn export_app(data: &Path, content: &Path) -> (AppState, Router) {
    let state = AppState {
        store: Arc::new(Store::open(DataDir::new(data)).unwrap()),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(content))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: content.to_path_buf(),
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
    // As `od_server::router` wraps every route.
    let app = Router::new()
        .merge(od_server::routes::export::routes_with(Arc::new(Crashing)))
        .layer(axum::middleware::from_fn(od_server::request_id::tag))
        .with_state(state.clone());
    (state, app)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_line_of_a_failed_export_carries_its_request_id() {
    let _one = LOG.1.lock().await;
    let tmp = tempfile::tempdir().unwrap();
    let (state, app) = export_app(&tmp.path().join("data"), &tmp.path().join("content"));
    let project = state
        .store
        .create_project(CreateProjectRequest {
            name: "Deck".into(),
            ..CreateProjectRequest::default()
        })
        .unwrap()
        .0
        .id;
    state
        .store
        .write_file(
            &project,
            "index.html",
            b"<h1>secret design words</h1>",
            false,
            None,
        )
        .unwrap();

    let from = log_len();
    let res = app
        .oneshot(
            Request::post(format!("/api/projects/{project}/export/pdf"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"fileName":"index.html"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 502);
    let id = res.headers()[HEADER].to_str().unwrap().to_owned();
    assert_eq!(id.len(), od_server::request_id::ID_LEN);

    let lines = lines_since(from);
    for want in [
        "fake renderer: lost the connection",
        "the browser quit while printing",
        "request failed",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(want)),
            "no line has {want:?}: {lines:#?}"
        );
    }
    for line in &lines {
        assert!(line.contains(&format!("request_id={id}")), "{line}");
        assert!(!line.contains("secret design words"), "{line}");
    }
    // The failed request's lines lead the diagnostics, even with no id given.
    assert_eq!(od_server::request_id::last_failed().as_deref(), Some(&*id));
    let id_asked = od_server::routes::manage::diagnostics::lookup_id(None);
    let report =
        od_server::routes::manage::diagnostics::log_report(log_root(), id_asked.as_deref());
    assert_eq!(report.id.as_deref(), Some(&*id));
    let joined = report.joined();
    assert!(
        joined.starts_with(&format!("# Lines with {id}\n")),
        "{joined}"
    );
    assert!(report.matched.contains("the browser quit while printing"));
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_runs_lines_carry_its_run_id_and_the_request_that_started_it() {
    let _one = LOG.1.lock().await;
    let d = common::daemon().await;
    common::write_mock(
        &d.mocks,
        "claude",
        &format!("{}echo 'agent says no' >&2\nexit 3\n", common::PROBES),
    );
    let (project, conversation) = d.project().await;
    let from = log_len();
    let res = d
        .post(
            "/api/runs",
            &serde_json::json!({"agentId": "claude", "projectId": project,
                "conversationId": conversation, "message": "make a page"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 202);
    let request_id = res.headers()[HEADER].to_str().unwrap().to_owned();
    let run: od_contracts::CreateRunResponse = res.json().await.unwrap();
    let events = d.events(&run.run_id, None).await;
    assert!(events.iter().any(|e| e.event == "end"), "{events:?}");

    let lines: Vec<String> = lines_since(from)
        .into_iter()
        .filter(|l| l.contains(&run.run_id))
        .collect();
    assert!(!lines.is_empty(), "the run logged nothing with its id");
    // The engine's own lines sit in the run's span, not only name it.
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&format!("run{{run_id={}", run.run_id)) && l.contains("od_runtime")),
        "{lines:#?}"
    );
    for line in &lines {
        assert!(line.contains(&format!("request_id={request_id}")), "{line}");
        assert!(
            !line.contains("make a page"),
            "the prompt reached the log: {line}"
        );
    }
}
