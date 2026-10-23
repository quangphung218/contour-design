//! Track W3-content through the real daemon: a design system written by a
//! mock agent from a brief, the atom catalogue and plugin pipelines,
//! queued media generation (limit, cancel, restore after restart) and the
//! editable PPTX route. No real agent, provider or browser is called.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use axum::Router;
use base64::Engine;
use common::{Daemon, Options, PROBES, daemon_with, write_mock};
use futures_util::future::BoxFuture;
use od_export::image::solid_png;
use od_export::pptx_editable::{Element, Paragraph, Rect, SlideLayout, TextRun};
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
use od_media::tasks::{TaskPersist, TaskRecord, TaskStatus};
use od_server::routes::w3::{Deps, media_tasks::SqlitePersist, routes_with};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

async fn daemon() -> Daemon {
    daemon_with(Options {
        content_root: Some(content_root()),
        ..Options::default()
    })
    .await
}

/// Mock `claude` that writes the package files the prompt asks for.
/// `$1`-style switches come from the brief: `no-tokens` skips tokens.css,
/// `crash` exits 1.
fn design_agent(d: &Daemon) {
    let mut design =
        String::from("# Tea House\\n\\n> Category: Food\\n\\nWarm tea shop palette.\\n\\n");
    for s in od_content::design_system_gen::SECTIONS {
        design.push_str(&format!("## {s}\\n\\nText.\\n\\n"));
    }
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r##"{PROBES}
IFS= read -r line
case "$line" in *crash*) echo boom >&2; exit 1;; esac
case "$line" in *"# Design system brief"*) ;; *) echo "not a design system prompt" >&2; exit 1;; esac
printf '%b' "{design}" > DESIGN.md
case "$line" in *no-tokens*) ;; *)
printf ':root {{ --bg: #f6efe2; --surface: #fff; --fg: #1d1b16; --accent: #2f6b3a; --font-display: Georgia; --font-body: system-ui; }}\n' > tokens.css
;; esac
echo scratch > notes.txt
printf '{{"type":"system","subtype":"init","session_id":"s1","model":"mock"}}\n'
printf '{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"text","text":"Wrote the design system."}}]}}}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
"##
        ),
    );
}

async fn wait_job(d: &Daemon, id: &str) -> Value {
    for _ in 0..200 {
        let job = d.get_json(&format!("/api/design-systems/jobs/{id}")).await["job"].clone();
        if job["status"] != "running" {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("design system job {id} did not finish");
}

#[tokio::test]
async fn design_system_from_a_brief_lists_next_to_bundled() {
    let d = daemon().await;
    design_agent(&d);
    let bundled = d.get_json("/api/design-systems").await["designSystems"]
        .as_array()
        .unwrap()
        .len();
    assert!(bundled > 100);

    let res = d
        .post(
            "/api/design-systems/generate",
            &json!({"name": "Tea House", "brief": "A calm tea shop.", "category": "Food"}),
        )
        .await;
    assert_eq!(res.status(), 202);
    let job: Value = res.json().await.unwrap();
    let job = &job["job"];
    assert_eq!(job["status"], "running");
    let run_id = job["runId"].as_str().unwrap().to_owned();
    let done = wait_job(&d, job["id"].as_str().unwrap()).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["designSystemId"], "user:tea-house");

    // The run streamed like any other.
    let events = d.events(&run_id, None).await;
    assert_eq!(common::end_of(&events)["status"], "succeeded");

    let list = d.get_json("/api/design-systems").await["designSystems"].clone();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), bundled + 1);
    let mine = list.iter().find(|s| s["id"] == "user:tea-house").unwrap();
    assert_eq!(mine["title"], "Tea House");
    assert_eq!(mine["category"], "Food");
    assert_eq!(mine["source"], "user");
    let one = d.get_json("/api/design-systems/user:tea-house").await;
    assert!(one.to_string().contains("Warm tea shop palette"), "{one}");
    let dir = d.data.join("design-systems/tea-house");
    assert!(dir.join("manifest.json").is_file() && dir.join("tokens.css").is_file());
    assert!(
        !dir.join("notes.txt").exists(),
        "only package files are kept"
    );
    assert!(
        std::fs::read_dir(d.data.join("design-system-jobs"))
            .unwrap()
            .next()
            .is_none(),
        "staging folders are removed"
    );

    // A second one with the same name gets its own folder.
    let res: Value = d
        .post(
            "/api/design-systems/generate",
            &json!({"name": "Tea House", "brief": "Again."}),
        )
        .await
        .json()
        .await
        .unwrap();
    let again = wait_job(&d, res["job"]["id"].as_str().unwrap()).await;
    assert_eq!(again["designSystemId"], "user:tea-house-2");
    let jobs = d.get_json("/api/design-systems/jobs").await["jobs"].clone();
    assert_eq!(jobs.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn design_system_generation_failures() {
    let d = daemon().await;
    design_agent(&d);
    for (brief, why) in [("no-tokens please", "tokens.css"), ("crash now", "failed")] {
        let res: Value = d
            .post(
                "/api/design-systems/generate",
                &json!({"name": "Broken", "brief": brief}),
            )
            .await
            .json()
            .await
            .unwrap();
        let job = wait_job(&d, res["job"]["id"].as_str().unwrap()).await;
        assert_eq!(job["status"], "failed");
        assert!(job["error"].as_str().unwrap().contains(why), "{job}");
    }
    assert!(!d.data.join("design-systems/broken").exists());
    let res = d
        .post(
            "/api/design-systems/generate",
            &json!({"name": " ", "brief": "x"}),
        )
        .await;
    assert_eq!(res.status(), 400);
    let res = d
        .post(
            "/api/design-systems/generate",
            &json!({"name": "A", "brief": "x", "agentId": "nope"}),
        )
        .await;
    assert_eq!(res.status(), 400);
    assert_eq!(
        d.get_json("/api/design-systems/jobs/nope").await["error"]["code"],
        "NOT_FOUND"
    );
}

#[tokio::test]
async fn atoms_and_plugin_pipelines() {
    let d = daemon().await;
    let atoms = d.get_json("/api/atoms").await["atoms"].clone();
    assert_eq!(atoms.as_array().unwrap().len(), 22);
    assert_eq!(atoms[0]["id"], "discovery-question-form");
    let p = d.get_json("/api/plugins/od-new-generation/pipeline").await;
    let stages = p["pipeline"]["stages"].as_array().unwrap();
    let ids: Vec<&str> = stages.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["discovery", "plan", "generate", "critique"]);
    assert_eq!(stages[3]["repeat"], true);
    assert_eq!(p["maxIterations"], 10);
    // The prompt block a run gets lists the stages.
    let applied: Value = d
        .post(
            "/api/plugins/od-new-generation/apply",
            &json!({"inputs": {"artifactKind": "deck", "audience": "VCs", "topic": "tea"}}),
        )
        .await
        .json()
        .await
        .unwrap();
    let block = applied["appliedPlugin"]["promptBlock"].as_str().unwrap();
    assert!(block.contains("## Plugin pipeline"), "{block}");
    assert!(block.contains(
        "4. **critique**: `critique-theater` (repeat until `critique.score>=4 || iterations>=3`)"
    ));
    let missing = d.get_json("/api/plugins/nope/pipeline").await;
    assert_eq!(missing["error"]["code"], "NOT_FOUND");
}

// ---------------------------------------------------------------------------
// Media tasks and editable PPTX, through `routes_with` and fakes.

/// Returns fixed layouts; counts how often it was asked.
#[derive(Default)]
struct FakeLayouts;

impl Renderer for FakeLayouts {
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
        _: &'a Path,
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
    fn extract_layouts<'a>(
        &'a self,
        target: &'a RenderTarget,
    ) -> BoxFuture<'a, RenderResult<Vec<SlideLayout>>> {
        Box::pin(async move {
            // The renderer gets the bundled copy, not the project file.
            assert!(
                std::fs::read_to_string(&target.html_path)
                    .unwrap()
                    .contains("Deck")
            );
            let slide = |t: &str| SlideLayout {
                width: 1920.0,
                height: 1080.0,
                background: Some("#ffffff".into()),
                elements: vec![Element::Text {
                    rect: Rect {
                        x: 10.0,
                        y: 10.0,
                        w: 500.0,
                        h: 100.0,
                    },
                    paragraphs: vec![Paragraph {
                        align: None,
                        line_height: None,
                        runs: vec![TextRun {
                            text: t.into(),
                            size: 40.0,
                            color: None,
                            bold: false,
                            italic: false,
                            underline: false,
                            font: None,
                        }],
                    }],
                    valign: None,
                }],
            };
            Ok(vec![slide("One"), slide("Two"), slide("Three")])
        })
    }
}

/// Fake OpenAI images endpoint; `slow` prompts take 400 ms.
async fn fake_provider() -> String {
    use axum::routing::post;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/v1/images/generations",
        post(|axum::Json(body): axum::Json<Value>| async move {
            if body["prompt"].as_str().unwrap_or("").contains("slow") {
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
            let png = solid_png(2, 2, [1, 2, 3]).unwrap();
            axum::Json(json!({ "data": [{ "b64_json": base64::engine::general_purpose::STANDARD.encode(png) }] }))
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn deps(renderer: Arc<dyn Renderer>, concurrency: usize) -> Deps {
    Deps {
        renderer,
        env: Arc::new(|_: &str| None),
        limits: od_media::Limits::default(),
        media_concurrency: concurrency,
    }
}

/// Serves `routes_with` on a loopback port; returns its base URL.
async fn serve(d: &Daemon, deps: Deps) -> String {
    let app = routes_with(&d.state, deps).with_state(d.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn call(d: &Daemon, method: &str, url: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = d.http.request(method.parse().unwrap(), url);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn media_tasks_queue_cancel_and_restore() {
    let d = daemon().await;
    let provider = fake_provider().await;
    std::fs::write(
        d.data.join("media-config.json"),
        json!({"providers": {"openai": {"apiKey": "sk-test", "baseUrl": format!("{provider}/v1")}}}).to_string(),
    )
    .unwrap();
    let (project, _) = d.project().await;
    let base = serve(&d, deps(Arc::new(FakeLayouts), 1)).await;
    let submit = |prompt: &str, output: &str| json!({"surface": "image", "model": "gpt-image-1", "prompt": prompt, "output": output});

    let (s, bad) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/{project}/media/tasks"),
        Some(json!({"surface": "image"})),
    )
    .await;
    assert_eq!(s, 400, "{bad}");
    let (s, _) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/{project}/media/tasks"),
        Some(submit("x", "a")),
    )
    .await;
    assert_eq!(s, 202);
    let (s, _) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/nope/media/tasks"),
        Some(submit("x", "a")),
    )
    .await;
    assert_eq!(s, 404);

    // One slot: the slow task runs, the next waits and is canceled.
    let (_, slow) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/{project}/media/tasks"),
        Some(submit("slow red", "slow")),
    )
    .await;
    let (_, queued) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/{project}/media/tasks"),
        Some(submit("quick", "quick")),
    )
    .await;
    let slow_id = slow["task"]["id"].as_str().unwrap();
    let queued_id = queued["task"]["id"].as_str().unwrap();
    let (_, c) = call(
        &d,
        "POST",
        &format!("{base}/api/media/tasks/{queued_id}/cancel"),
        None,
    )
    .await;
    assert_eq!(c["task"]["status"], "canceled");
    let (_, done) = call(
        &d,
        "GET",
        &format!("{base}/api/media/tasks/{slow_id}?waitMs=5000"),
        None,
    )
    .await;
    assert_eq!(done["task"]["status"], "done", "{done}");
    assert_eq!(done["task"]["result"]["name"], "slow.png");
    let dir = d
        .state
        .store
        .project_root(&d.state.store.get_project(&project).unwrap())
        .unwrap()
        .dir;
    assert!(dir.join("slow.png").is_file());
    assert!(
        !dir.join("quick.png").exists(),
        "the canceled task never ran"
    );
    let (_, list) = call(
        &d,
        "GET",
        &format!("{base}/api/media/tasks?projectId={project}"),
        None,
    )
    .await;
    let statuses: Vec<&str> = list["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["done", "done", "canceled"]);
    let (s, _) = call(&d, "GET", &format!("{base}/api/media/tasks/nope"), None).await;
    assert_eq!(s, 404);

    // "Restart": records left queued/running by a dead daemon.
    let persist = SqlitePersist::open(Arc::clone(&d.state.store)).unwrap();
    let left = |id: &str, status: TaskStatus, output: &str| TaskRecord {
        id: id.into(),
        project_id: project.clone(),
        status,
        request: submit("again", output),
        result: None,
        error: None,
        created_at: od_store::now_ms() + 1_000,
        started_at: None,
        ended_at: None,
    };
    persist.save(&left("t-queued", TaskStatus::Queued, "restored"));
    persist.save(&left("t-running", TaskStatus::Running, "lost"));
    let base2 = serve(&d, deps(Arc::new(FakeLayouts), 2)).await;
    let (_, r) = call(
        &d,
        "GET",
        &format!("{base2}/api/media/tasks/t-queued?waitMs=5000"),
        None,
    )
    .await;
    assert_eq!(r["task"]["status"], "done", "{r}");
    assert!(dir.join("restored.png").is_file());
    let (_, r) = call(
        &d,
        "GET",
        &format!("{base2}/api/media/tasks/t-running"),
        None,
    )
    .await;
    assert_eq!(r["task"]["status"], "interrupted");
    assert!(
        !dir.join("lost.png").exists(),
        "an interrupted task is not repeated"
    );
    let stored: Vec<(String, TaskStatus)> = persist
        .load_all()
        .into_iter()
        .map(|t| (t.id, t.status))
        .filter(|(id, _)| id.starts_with("t-"))
        .collect();
    assert_eq!(
        stored,
        [
            ("t-queued".into(), TaskStatus::Done),
            ("t-running".into(), TaskStatus::Interrupted)
        ]
    );
}

#[tokio::test]
async fn editable_pptx_route() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    d.state
        .store
        .write_file(
            &project,
            "deck.html",
            b"<!doctype html><h1>Deck</h1>",
            true,
            None,
        )
        .unwrap();
    let base = serve(&d, deps(Arc::new(FakeLayouts), 1)).await;
    let res = d
        .http
        .post(format!(
            "{base}/api/projects/{project}/export/pptx-editable"
        ))
        .json(&json!({"fileName": "deck.html", "title": "Tea deck"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], od_export::pptx::PPTX_MIME);
    assert!(
        res.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("Tea deck.pptx")
    );
    let bytes = res.bytes().await.unwrap();
    assert_eq!(od_export::pptx::count_slides(&bytes).unwrap(), (3, 3));

    // The daemon's default route table carries it too; a renderer without
    // layouts answers 501.
    struct NoLayouts;
    impl Renderer for NoLayouts {
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
            _: &'a Path,
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
    let base = serve(&d, deps(Arc::new(NoLayouts), 1)).await;
    let (s, v) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/{project}/export/pptx-editable"),
        Some(json!({"fileName": "deck.html"})),
    )
    .await;
    assert_eq!(s, 501, "{v}");
    let (s, _) = call(
        &d,
        "POST",
        &format!("{base}/api/projects/{project}/export/pptx-editable"),
        Some(json!({"fileName": "missing.html"})),
    )
    .await;
    assert_eq!(s, 404);
}
