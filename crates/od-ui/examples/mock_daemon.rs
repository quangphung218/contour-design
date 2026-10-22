//! Real `od-server` router plus a scripted stand-in for the run engine, for
//! the UI e2e test (`e2e/run.sh`). No agent is spawned and nothing leaves
//! loopback.
//!
//! Mocked (owned by M2/M4 on `main` later): `GET /api/agents`,
//! `POST /api/runs`, `GET /api/runs/:id/events` (SSE with `Last-Event-ID`
//! replay), `POST /api/runs/:id/cancel`, conversations/messages, and
//! `GET /api/projects/:id/events` (`file-changed`).
//!
//! TODO(M2/M4 merge): those routes will exist in the real router and this
//! example will panic on overlapping routes; drop the mock routes then and
//! drive the e2e through M2's mock CLI instead.
//!
//! Each run writes `index.html` ("Mock page v<N>" for the project's N-th
//! turn) and streams text. A message containing `form` ends with a
//! `<question-form>`; `slow` streams slowly so Stop can be tested.
//!
//! Usage: `cargo run -p od-ui --example mock_daemon -- [port] [data-dir]`

#![allow(clippy::unwrap_used, clippy::expect_used)] // test fixture

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use od_content::{ContentRoots, ContentStore};
use od_server::{AppState, ServerConfig, resolve_resource_root, router};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone)]
struct Frame {
    id: u64,
    name: &'static str,
    data: Value,
}

struct Run {
    frames: Mutex<Vec<Frame>>,
    live: broadcast::Sender<Frame>,
    canceled: AtomicBool,
}

impl Run {
    fn emit(&self, name: &'static str, data: Value) {
        let mut frames = self.frames.lock().unwrap();
        let frame = Frame {
            id: frames.len() as u64 + 1,
            name,
            data,
        };
        frames.push(frame.clone());
        let _ = self.live.send(frame);
    }

    fn ended(&self) -> bool {
        self.frames
            .lock()
            .unwrap()
            .last()
            .is_some_and(|f| f.name == "end")
    }
}

#[derive(Default)]
struct Mock {
    runs: Mutex<HashMap<String, Arc<Run>>>,
    /// Messages per conversation, as the UI reads them back.
    messages: Mutex<HashMap<String, Vec<Value>>>,
    /// First conversation id per project.
    conversations: Mutex<HashMap<String, String>>,
    project_events: Mutex<HashMap<String, broadcast::Sender<Value>>>,
}

impl Mock {
    fn project_tx(&self, project: &str) -> broadcast::Sender<Value> {
        self.project_events
            .lock()
            .unwrap()
            .entry(project.to_owned())
            .or_insert_with(|| broadcast::channel(64).0)
            .clone()
    }
}

#[derive(Clone)]
struct Ctx {
    app: AppState,
    mock: Arc<Mock>,
}

async fn agents() -> Json<Value> {
    Json(json!({"agents": [
        {"id": "mock", "name": "Mock agent", "bin": "mock", "available": true, "version": "1.0.0",
         "capabilities": [], "supportsResume": true, "supportsSteer": false},
        {"id": "codex", "name": "Codex", "bin": "codex", "available": false,
         "error": "not installed", "capabilities": [], "supportsResume": false, "supportsSteer": false}
    ]}))
}

async fn create_run(State(ctx): State<Ctx>, Json(req): Json<Value>) -> Response {
    let project_id = req["projectId"].as_str().unwrap_or_default().to_owned();
    let message = req["message"].as_str().unwrap_or_default().to_owned();
    if ctx.app.store.get_project(&project_id).is_err() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "PROJECT_NOT_FOUND", "message": "no project"}})),
        )
            .into_response();
    }
    let cid = req["conversationId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            ctx.mock
                .conversations
                .lock()
                .unwrap()
                .entry(project_id.clone())
                .or_insert_with(|| uuid::Uuid::new_v4().to_string())
                .clone()
        });
    ctx.mock
        .conversations
        .lock()
        .unwrap()
        .entry(project_id.clone())
        .or_insert_with(|| cid.clone());
    let run_id = uuid::Uuid::new_v4().to_string();
    let assistant_id = uuid::Uuid::new_v4().to_string();
    let turn = {
        let mut all = ctx.mock.messages.lock().unwrap();
        let list = all.entry(cid.clone()).or_default();
        let turn = list.iter().filter(|m| m["role"] == "user").count() + 1;
        list.push(
            json!({"id": uuid::Uuid::new_v4().to_string(), "role": "user", "content": message}),
        );
        list.push(
            json!({"id": assistant_id, "role": "assistant", "content": "", "runId": run_id,
                          "runStatus": "running", "events": []}),
        );
        turn
    };
    let run = Arc::new(Run {
        frames: Mutex::new(Vec::new()),
        live: broadcast::channel(1024).0,
        canceled: AtomicBool::new(false),
    });
    ctx.mock
        .runs
        .lock()
        .unwrap()
        .insert(run_id.clone(), Arc::clone(&run));
    tokio::spawn(script(
        ctx.clone(),
        run,
        run_id.clone(),
        project_id,
        cid.clone(),
        assistant_id.clone(),
        message,
        turn,
    ));
    (
        StatusCode::ACCEPTED,
        Json(json!({"runId": run_id, "conversationId": cid, "assistantMessageId": assistant_id,
                    "clientRequestId": req["clientRequestId"], "reused": false, "resumed": turn > 1})),
    )
        .into_response()
}

#[allow(clippy::too_many_arguments)]
async fn script(
    ctx: Ctx,
    run: Arc<Run>,
    run_id: String,
    project_id: String,
    cid: String,
    assistant_id: String,
    message: String,
    turn: usize,
) {
    let record = |ev: Value| {
        let mut all = ctx.mock.messages.lock().unwrap();
        if let Some(m) = all
            .get_mut(&cid)
            .and_then(|l| l.iter_mut().find(|m| m["id"] == assistant_id.as_str()))
        {
            m["events"].as_array_mut().unwrap().push(ev);
        }
    };
    let agent = |ev: Value| {
        record(ev.clone());
        run.emit("agent", ev);
    };
    let slow = message.contains("slow");
    let pause = Duration::from_millis(if slow { 250 } else { 25 });

    run.emit(
        "start",
        json!({"runId": run_id, "agentId": "mock", "bin": "mock", "protocolVersion": 1,
                             "model": "mock-1", "resumed": turn > 1}),
    );
    agent(json!({"type": "status", "label": "thinking", "model": "mock-1"}));
    let intro = format!(
        "Turn {turn}. Working on: {}",
        message.lines().next().unwrap_or_default()
    );
    let mut words: Vec<String> = intro.split(' ').map(|w| format!("{w} ")).collect();
    if slow {
        words.extend((0..40).map(|i| format!("step{i} ")));
    }
    for w in words {
        if run.canceled.load(Ordering::SeqCst) {
            return finish(&ctx, &run, &cid, &assistant_id, "canceled");
        }
        agent(json!({"type": "text_delta", "delta": w}));
        tokio::time::sleep(pause).await;
    }

    let tool_id = format!("tool-{turn}");
    let path = "index.html".to_owned();
    agent(json!({"type": "tool_use", "id": tool_id, "name": "Write",
                 "input": {"file_path": format!("/mock/projects/{project_id}/{path}"), "content": "…"}}));
    let html = format!(
        "<!doctype html><html><body><h1 id=\"title\">Mock page v{turn}</h1><p>{}</p></body></html>",
        message.replace('<', "&lt;")
    );
    let wrote = {
        let store = Arc::clone(&ctx.app.store);
        let (pid, p) = (project_id.clone(), path.clone());
        tokio::task::spawn_blocking(move || store.write_file(&pid, &p, html.as_bytes(), true, None))
            .await
            .unwrap()
    };
    let _ = ctx
        .mock
        .project_tx(&project_id)
        .send(json!({"path": path, "kind": if turn == 1 { "add" } else { "change" }}));
    agent(
        json!({"type": "tool_result", "toolUseId": tool_id, "content": "ok", "isError": wrote.is_err()}),
    );
    agent(json!({"type": "text_delta", "delta": format!("\n\nWrote `{path}` (v{turn}).")}));

    if message.contains("form") {
        let form = r#"

<question-form id="discovery" title="A few questions">
{"questions": [
  {"id": "platform", "label": "Platform", "type": "radio", "options": ["Mobile", "Desktop"], "required": true},
  {"id": "audience", "label": "Audience", "type": "text", "placeholder": "Who is it for?"}
]}
</question-form>"#;
        for chunk in form.as_bytes().chunks(40) {
            agent(json!({"type": "text_delta", "delta": String::from_utf8_lossy(chunk)}));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    agent(json!({"type": "turn_end", "stopReason": "end_turn"}));
    finish(&ctx, &run, &cid, &assistant_id, "succeeded");
}

fn finish(ctx: &Ctx, run: &Run, cid: &str, assistant_id: &str, status: &str) {
    if run.ended() {
        return;
    }
    if let Some(m) = ctx
        .mock
        .messages
        .lock()
        .unwrap()
        .get_mut(cid)
        .and_then(|l| l.iter_mut().find(|m| m["id"] == assistant_id))
    {
        m["runStatus"] = json!(status);
    }
    run.emit(
        "end",
        json!({"status": status, "code": 0, "terminalAt": 0, "artifactPaths": ["index.html"],
                           "resumable": true, "retryable": false}),
    );
}

async fn run_events(
    State(ctx): State<Ctx>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(run) = ctx.mock.runs.lock().unwrap().get(&id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let after: u64 = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(256);
    tokio::spawn(async move {
        // Subscribe before snapshotting so nothing falls between the two.
        let mut live = run.live.subscribe();
        let backlog: Vec<Frame> = run.frames.lock().unwrap().clone();
        let mut last = after;
        for f in backlog.into_iter().filter(|f| f.id > after) {
            last = f.id;
            let end = f.name == "end";
            if tx.send(Ok(to_event(&f))).await.is_err() || end {
                return;
            }
        }
        while let Ok(f) = live.recv().await {
            if f.id <= last {
                continue;
            }
            last = f.id;
            let end = f.name == "end";
            if tx.send(Ok(to_event(&f))).await.is_err() || end {
                return;
            }
        }
    });
    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(25)))
        .into_response()
}

fn to_event(f: &Frame) -> Event {
    Event::default()
        .id(f.id.to_string())
        .event(f.name)
        .data(f.data.to_string())
}

async fn cancel(State(ctx): State<Ctx>, Path(id): Path<String>) -> Response {
    let Some(run) = ctx.mock.runs.lock().unwrap().get(&id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    run.canceled.store(true, Ordering::SeqCst);
    Json(json!({"ok": true, "status": "running"})).into_response()
}

async fn conversations(State(ctx): State<Ctx>, Path(pid): Path<String>) -> Json<Value> {
    let list: Vec<Value> = ctx
        .mock
        .conversations
        .lock()
        .unwrap()
        .get(&pid)
        .map(|cid| json!({"id": cid, "projectId": pid}))
        .into_iter()
        .collect();
    Json(json!({"conversations": list}))
}

async fn messages(
    State(ctx): State<Ctx>,
    Path((_pid, cid)): Path<(String, String)>,
) -> Json<Value> {
    let list = ctx
        .mock
        .messages
        .lock()
        .unwrap()
        .get(&cid)
        .cloned()
        .unwrap_or_default();
    Json(json!({"messages": list}))
}

async fn project_events(State(ctx): State<Ctx>, Path(pid): Path<String>) -> impl IntoResponse {
    let mut rx = ctx.mock.project_tx(&pid).subscribe();
    let (tx, out) = mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        let _ = tx
            .send(Ok(Event::default().event("ready").data("{}")))
            .await;
        while let Ok(v) = rx.recv().await {
            if tx
                .send(Ok(Event::default()
                    .event("file-changed")
                    .data(v.to_string())))
                .await
                .is_err()
            {
                return;
            }
        }
    });
    Sse::new(ReceiverStream::new(out)).keep_alive(KeepAlive::default())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let data_dir = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("od-mock-{}", std::process::id())));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let local = listener.local_addr()?;
    let resource_root = resolve_resource_root();
    let store = Store::open(DataDir::new(&data_dir))?;
    let content =
        ContentStore::load(ContentRoots::bundled(&resource_root).with_user_root(&data_dir));
    let app = AppState {
        store: Arc::new(store),
        content: Arc::new(content),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![local.port()],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root,
            daemon_url: None,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data_dir.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
        preview: od_server::Preview::default(),
    };
    let ctx = Ctx {
        app: app.clone(),
        mock: Arc::new(Mock::default()),
    };
    let mock_routes = Router::new()
        .route("/api/agents", get(agents))
        .route("/api/runs", post(create_run))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/runs/{id}/cancel", post(cancel))
        .route("/api/projects/{id}/conversations", get(conversations))
        .route(
            "/api/projects/{id}/conversations/{cid}/messages",
            get(messages),
        )
        .route("/api/projects/{id}/events", get(project_events))
        .with_state(ctx);
    let app = mock_routes.merge(router(app));
    println!(
        "mock daemon listening on http://{local} (data: {})",
        data_dir.display()
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
