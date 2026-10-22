//! M5 end to end (spec 5.7): `od mcp` driven by a real MCP client over
//! stdio, `od mcp install`, and `od export` against a running daemon.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::extract::{Path as AxPath, State};
use axum::routing::{get, post};
use od_content::{ContentRoots, ContentStore};
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use serde_json::{Map, Value, json};
use std::net::TcpListener as StdListener;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const OD: &str = env!("CARGO_BIN_EXE_od");

fn free_port() -> u16 {
    StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

/// The real daemon router over a temp data dir, in this process.
async fn real_daemon(tmp: &Path) -> String {
    let data = tmp.join("data");
    let empty = tmp.join("content");
    let state = AppState {
        store: Arc::new(Store::open(DataDir::new(&data)).unwrap()),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(&empty))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: Vec::new(),
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: empty,
        }),
    };
    serve(od_server::router(state)).await
}

type Client = RunningService<rmcp::RoleClient, ()>;

async fn mcp(daemon_url: &str) -> Client {
    let mut cmd = tokio::process::Command::new(OD);
    cmd.args(["--daemon-url", daemon_url, "mcp"])
        .stderr(Stdio::null());
    ().serve(TokioChildProcess::new(cmd).unwrap())
        .await
        .unwrap()
}

/// Calls a tool; returns (isError, text blocks joined, structured content).
async fn call(client: &Client, name: &str, args: Value) -> (bool, String, Value) {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    let res = client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(args))
        .await
        .unwrap();
    let v = serde_json::to_value(&res).unwrap();
    let text = v["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (v["isError"] == true, text, v["structuredContent"].clone())
}

async fn ok(client: &Client, name: &str, args: Value) -> Value {
    let (err, text, structured) = call(client, name, args).await;
    assert!(!err, "{name} failed: {text}");
    structured
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_file_tools_and_active_project_over_stdio() {
    let tmp = tempfile::tempdir().unwrap();
    let url = real_daemon(tmp.path()).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let client = mcp(&url).await;

    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 20);
    for name in [
        "list_projects",
        "write_file",
        "get_file",
        "start_run",
        "collect_brief",
    ] {
        assert!(tools.iter().any(|t| t.name == name), "{name} missing");
    }

    let created = ok(&client, "create_project", json!({ "name": "MCP Demo" })).await;
    let id = created["project"]["id"].as_str().unwrap().to_owned();
    let listed = ok(&client, "list_projects", json!({})).await;
    assert_eq!(listed["projects"][0]["id"], id.as_str());

    let wrote = ok(
        &client,
        "write_file",
        json!({ "project": id, "path": "site/index.html", "content": "<h1>one</h1>\n<p>two</p>\n<p>three</p>" }),
    )
    .await;
    assert_eq!(wrote["file"]["name"], "site/index.html");
    // Projects resolve by name too.
    let (err, text, _) = call(
        &client,
        "get_file",
        json!({ "project": "mcp demo", "path": "site/index.html" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(
        text.ends_with("<h1>one</h1>\n<p>two</p>\n<p>three</p>"),
        "{text}"
    );
    let (_, window, _) = call(
        &client,
        "get_file",
        json!({ "project": id, "path": "site/index.html", "offset": 1, "limit": 1 }),
    )
    .await;
    assert!(window.contains("[od:file-window offset=1 returnedLines=1 totalLines=3; call get_file again with offset=2"), "{window}");
    assert!(window.ends_with("<p>two</p>"), "{window}");

    let found = ok(
        &client,
        "search_files",
        json!({ "project": id, "query": "TWO" }),
    )
    .await;
    assert_eq!(
        found["matches"],
        json!([{ "file": "site/index.html", "line": 2, "snippet": "<p>two</p>" }])
    );

    // No active project: a clear error naming the 5-minute expiry.
    let (err, text, _) = call(
        &client,
        "write_file",
        json!({ "path": "x.html", "content": "x" }),
    )
    .await;
    assert!(err);
    assert!(
        text.contains("no active project") && text.contains("5 minutes"),
        "{text}"
    );

    // The UI marks the project active; omitted `project` now uses it.
    http.post(format!("{url}/api/active"))
        .json(&json!({ "projectId": id, "fileName": "site/index.html" }))
        .send()
        .await
        .unwrap();
    let wrote = ok(
        &client,
        "write_file",
        json!({ "path": "notes.md", "content": "# active" }),
    )
    .await;
    assert_eq!(wrote["usedActiveContext"]["projectId"], id.as_str());
    let (err, text, _) = call(&client, "get_file", json!({})).await;
    assert!(!err, "{text}");
    assert!(
        text.starts_with("[od:active-context project=\"MCP Demo\" file=\"site/index.html\"]"),
        "{text}"
    );

    let active = ok(&client, "get_active_context", json!({})).await;
    assert_eq!(active["projectName"], "MCP Demo");
    let focus = client
        .read_resource(ReadResourceRequestParams::new(od_mcp::server::FOCUS_URI))
        .await
        .unwrap();
    let focus = serde_json::to_value(&focus).unwrap();
    assert!(focus["contents"][0]["text"].as_str().unwrap().contains(&id));

    // Clearing the context (what expiry does after 5 minutes) brings the
    // error back.
    http.post(format!("{url}/api/active"))
        .json(&json!({ "active": false }))
        .send()
        .await
        .unwrap();
    let (err, text, _) = call(&client, "get_file", json!({ "path": "notes.md" })).await;
    assert!(err && text.contains("5 minutes"), "{text}");

    let bundle = ok(
        &client,
        "get_artifact",
        json!({ "project": id, "entry": "site/index.html" }),
    )
    .await;
    assert_eq!(bundle["files"][0]["path"], "site/index.html");

    let (err, text, _) = call(
        &client,
        "create_artifact",
        json!({ "project": id, "name": "notes.md", "content": "dup" }),
    )
    .await;
    assert!(err && text.contains("409"), "{text}");
    ok(
        &client,
        "create_artifact",
        json!({ "project": id, "name": "deck.html", "content": "<p>d</p>" }),
    )
    .await;
    let files = ok(&client, "list_files", json!({ "project": id })).await;
    let deck = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "deck.html")
        .unwrap()
        .clone();
    assert_eq!(deck["artifactManifest"]["kind"], "html");

    ok(
        &client,
        "delete_file",
        json!({ "project": id, "path": "deck.html" }),
    )
    .await;
    let (err, text, _) = call(&client, "delete_project", json!({ "project": id })).await;
    assert!(err && text.contains("confirm"), "{text}");
    ok(
        &client,
        "delete_project",
        json!({ "project": id, "confirm": true }),
    )
    .await;
    assert_eq!(
        ok(&client, "list_projects", json!({})).await["projects"],
        json!([])
    );

    client.cancel().await.unwrap();
}

/// A stand-in for the M2 run engine: `/api/agents`, `/api/runs`, SSE.
#[derive(Clone, Default)]
struct FakeRuns {
    bodies: Arc<Mutex<Vec<Value>>>,
    canceled: Arc<Mutex<Vec<String>>>,
}

async fn fake_run_daemon() -> (String, FakeRuns) {
    let fake = FakeRuns::default();
    let router = Router::new()
        .route(
            "/api/projects",
            get(|| async { axum::Json(json!({ "projects": [{ "id": "p1", "name": "Landing" }] })) }),
        )
        .route(
            "/api/projects/{id}",
            get(|AxPath(id): AxPath<String>| async move {
                axum::Json(json!({ "project": { "id": id, "name": "Landing", "metadata": { "entryFile": "index.html" } } }))
            }),
        )
        .route("/api/active", get(|| async { axum::Json(json!({ "active": false })) }))
        .route(
            "/api/agents",
            get(|| async {
                axum::Json(json!({ "agents": [
                    { "id": "gemini", "name": "Gemini", "available": false },
                    { "id": "claude", "name": "Claude Code", "available": true, "models": ["a","b","c","d","e","f","g","h","i","j","k","l"] },
                ] }))
            }),
        )
        .route(
            "/api/runs",
            post(|State(f): State<FakeRuns>, axum::Json(body): axum::Json<Value>| async move {
                f.bodies.lock().unwrap().push(body.clone());
                (
                    axum::http::StatusCode::ACCEPTED,
                    axum::Json(json!({ "runId": "r1", "conversationId": "c1", "assistantMessageId": "m1", "clientRequestId": body["clientRequestId"], "reused": false, "resumed": false })),
                )
            }),
        )
        .route(
            "/api/runs/{id}",
            get(|AxPath(id): AxPath<String>| async move {
                axum::Json(json!({ "run": { "id": id, "status": "succeeded", "agentId": "claude", "projectId": "p1", "createdAt": 1, "lastEventId": 3 } }))
            }),
        )
        .route(
            "/api/runs/{id}/events",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    "id: 1\nevent: start\ndata: {\"runId\":\"r1\"}\n\nid: 2\nevent: agent\ndata: {\"type\":\"text_delta\",\"delta\":\"Made a landing page.\"}\n\nid: 3\nevent: end\ndata: {\"status\":\"succeeded\"}\n\n",
                )
            }),
        )
        .route(
            "/api/runs/{id}/cancel",
            post(|State(f): State<FakeRuns>, AxPath(id): AxPath<String>| async move {
                f.canceled.lock().unwrap().push(id);
                axum::Json(json!({ "ok": true, "status": "canceled" }))
            }),
        )
        .with_state(fake.clone());
    (serve(router).await, fake)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_run_tools_against_a_fake_run_engine() {
    let (url, fake) = fake_run_daemon().await;
    let client = mcp(&url).await;

    let agents = ok(&client, "list_agents", json!({})).await;
    assert_eq!(agents["agents"].as_array().unwrap().len(), 1);
    assert_eq!(agents["agents"][0]["modelsCount"], 12);
    assert_eq!(agents["agents"][0]["models"].as_array().unwrap().len(), 10);

    let started = ok(&client, "start_run", json!({ "project": "landing", "prompt": "A landing page", "skill": "web", "requestId": "req-1" })).await;
    assert_eq!(started["runId"], "r1");
    assert_eq!(started["resolvedProject"]["via"], "exact");
    let body = fake.bodies.lock().unwrap()[0].clone();
    assert_eq!(
        body,
        json!({ "agentId": "claude", "message": "A landing page", "projectId": "p1", "clientRequestId": "req-1", "skillId": "web" })
    );

    let run = ok(&client, "get_run", json!({ "runId": "r1" })).await;
    assert_eq!(run["status"], "succeeded");
    assert_eq!(run["agentMessage"], "Made a landing page.");
    assert_eq!(
        run["previewUrl"],
        format!("{url}/api/projects/p1/raw/index.html")
    );

    ok(&client, "cancel_run", json!({ "runId": "r1" })).await;
    assert_eq!(fake.canceled.lock().unwrap().as_slice(), ["r1"]);

    // The brief pair needs no daemon.
    let brief = ok(
        &client,
        "collect_brief",
        json!({ "artifactType": "presentation" }),
    )
    .await;
    let done = ok(
        &client,
        "confirm_brief",
        json!({ "briefDraftId": brief["briefDraftId"], "nonce": brief["nonce"], "answers": { "length": "5" } }),
    )
    .await;
    assert_eq!(done["answers"]["length"], "5");

    client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_lists_tools_with_the_daemon_down() {
    let url = format!("http://127.0.0.1:{}", free_port());
    let client = mcp(&url).await;
    assert_eq!(client.list_all_tools().await.unwrap().len(), 20);
    let (err, text, _) = call(&client, "list_projects", json!({})).await;
    assert!(err && text.contains("cannot reach"), "{text}");
    let resources = client.list_all_resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// od mcp install

fn od_home(home: &Path, args: &[&str]) -> Output {
    Command::new(OD)
        .args(args)
        .env("HOME", home)
        .env_remove("OD_DAEMON_URL")
        .output()
        .unwrap()
}

#[test]
fn mcp_install_cursor_merges_and_prints_json() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".cursor/mcp.json");
    let od_path = std::fs::canonicalize(OD).unwrap();

    // --print on a fresh home: valid JSON, nothing written.
    let out = od_home(home.path(), &["mcp", "install", "cursor", "--print"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        printed,
        json!({ "mcpServers": { "open-design": { "type": "stdio", "command": od_path, "args": ["mcp"] } } })
    );
    assert!(!config.exists());

    // Real install into a config that has other servers and settings.
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        r#"{"mcpServers":{"github":{"command":"gh-mcp"}},"editor":{"tabSize":2}}"#,
    )
    .unwrap();
    let out = od_home(home.path(), &["mcp", "install", "cursor"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let merged: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(
        merged["mcpServers"]["github"],
        json!({ "command": "gh-mcp" })
    );
    assert_eq!(merged["mcpServers"]["open-design"]["args"], json!(["mcp"]));
    assert_eq!(merged["editor"], json!({ "tabSize": 2 }));

    // A non-default daemon URL is carried into the entry.
    let out = od_home(
        home.path(),
        &[
            "--daemon-url",
            "http://127.0.0.1:9999",
            "--json",
            "mcp",
            "install",
            "cursor",
            "--print",
        ],
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        result["preview"]["mcpServers"]["open-design"]["args"],
        json!(["mcp", "--daemon-url", "http://127.0.0.1:9999"])
    );

    // Uninstall removes only our entry.
    let out = od_home(home.path(), &["mcp", "install", "cursor", "--uninstall"]);
    assert!(out.status.success());
    let left: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(
        left,
        json!({ "mcpServers": { "github": { "command": "gh-mcp" } }, "editor": { "tabSize": 2 } })
    );

    // A broken config is left alone.
    std::fs::write(&config, "{ not json").unwrap();
    let out = od_home(home.path(), &["mcp", "install", "cursor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{ not json");
}

#[test]
fn mcp_install_cli_agents_print_their_commands() {
    let home = tempfile::tempdir().unwrap();
    let od_path = std::fs::canonicalize(OD).unwrap();
    let out = od_home(home.path(), &["mcp", "install", "claude", "--print"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!(
            "would run: claude mcp add --scope user open-design -- {} mcp",
            od_path.display()
        )
    );
    let out = od_home(
        home.path(),
        &[
            "--json",
            "mcp",
            "install",
            "codex",
            "--print",
            "--uninstall",
        ],
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["command"], "codex mcp remove open-design");
    let out = od_home(home.path(), &["mcp", "install", "vim"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("expected one of: claude codex cursor"));
}

// ---------------------------------------------------------------------------
// od export against the real daemon binary

struct Daemon {
    child: Child,
    url: String,
    tmp: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon() -> Daemon {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("resources/skills")).unwrap();
    let port = free_port();
    let child = Command::new(OD)
        .args(["daemon", "start", "--port", &port.to_string()])
        .env("OD_DATA_DIR", tmp.path().join("data"))
        .env("OD_RESOURCE_ROOT", tmp.path().join("resources"))
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let d = Daemon {
        child,
        url: format!("http://127.0.0.1:{port}"),
        tmp,
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if od(&d, &["daemon", "status"]).status.success() {
            return d;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("daemon did not answer on {}", d.url);
}

fn od(d: &Daemon, args: &[&str]) -> Output {
    Command::new(OD)
        .arg("--daemon-url")
        .arg(&d.url)
        .args(args)
        .output()
        .unwrap()
}

fn od_ok(d: &Daemon, args: &[&str]) -> Output {
    let out = od(d, args);
    assert!(
        out.status.success(),
        "od {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

const DECK: &str = r#"<!doctype html><html><head><link rel="stylesheet" href="deck.css"></head><body>
<section class="slide" style="background:#c00"><img src="img/logo.png"></section>
<section class="slide" style="background:#0c0"></section>
<section class="slide" style="background:#00c"></section>
<section class="slide" style="background:#cc0"></section>
<section class="slide" style="background:#c0c"></section>
</body></html>"#;

#[test]
fn export_cli_writes_every_format() {
    let d = start_daemon();
    let created = od_ok(
        &d,
        &[
            "--json",
            "project",
            "create",
            "--name",
            "Export Demo",
            "--kind",
            "deck",
        ],
    );
    let id = serde_json::from_slice::<Value>(&created.stdout).unwrap()["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let work = d.tmp.path();
    let png = od_export::image::solid_png(8, 8, [1, 2, 3]).unwrap();
    std::fs::write(work.join("logo.png"), &png).unwrap();
    od_ok(
        &d,
        &["files", "write", &id, "index.html", "--content", DECK],
    );
    od_ok(
        &d,
        &[
            "files",
            "write",
            &id,
            "deck.css",
            "--content",
            ".slide{width:100vw;height:100vh}",
        ],
    );
    od_ok(
        &d,
        &[
            "files",
            "write",
            &id,
            "img/logo.png",
            "--from",
            work.join("logo.png").to_str().unwrap(),
        ],
    );

    // HTML: self-contained; default name from the server.
    let out = Command::new(OD)
        .current_dir(work)
        .args(["--daemon-url", &d.url, "export", &id, "index.html"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = std::fs::read_to_string(work.join("index.html")).unwrap();
    assert!(
        !html.contains("img/logo.png")
            && !html.contains("\"deck.css\"")
            && html.contains("data:image/png;base64,")
    );

    // ZIP.
    let zip_path = work.join("p.zip");
    od_ok(
        &d,
        &[
            "export",
            &id,
            "--format",
            "zip",
            "--out",
            zip_path.to_str().unwrap(),
        ],
    );
    let mut names: Vec<String> = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap())
        .unwrap()
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["deck.css", "img/logo.png", "index.html"]);

    // Missing entry: a clear error and a non-zero exit.
    let out = od(&d, &["export", &id, "nope.html", "--out", "-"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("404"));

    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping browser exports");
        return;
    }
    // Browser formats go daemon → Chrome: 5 slides → 5 pages / 5 slides.
    let pdf = work.join("deck.pdf");
    od_ok(
        &d,
        &[
            "export",
            &id,
            "index.html",
            "--format",
            "pdf",
            "--out",
            pdf.to_str().unwrap(),
        ],
    );
    assert_eq!(lopdf::Document::load(&pdf).unwrap().get_pages().len(), 5);
    let pptx = work.join("deck.pptx");
    od_ok(
        &d,
        &[
            "export",
            &id,
            "index.html",
            "--format",
            "pptx",
            "--out",
            pptx.to_str().unwrap(),
        ],
    );
    assert_eq!(
        od_export::pptx::count_slides(&std::fs::read(&pptx).unwrap()).unwrap(),
        (5, 5)
    );
    let png_out = work.join("slide.png");
    od_ok(
        &d,
        &[
            "export",
            &id,
            "index.html",
            "--format",
            "image",
            "--index",
            "0",
            "--out",
            png_out.to_str().unwrap(),
        ],
    );
    assert_eq!(
        od_export::image::png_size(&std::fs::read(&png_out).unwrap()),
        Some((1920, 1080))
    );
}
