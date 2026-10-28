//! `od plugin` and `od media` (spec 2.6) against the real daemon router in
//! this process: bundled plugins, a restricted user plugin behind the
//! capability gate (exit 66), folder validation, install and uninstall,
//! and queued media
//! generation against a loopback fake provider.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use base64::Engine;
use od_content::{ContentRoots, ContentStore};
use od_server::{AppState, RunContext, ServerConfig};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::Arc;
use std::time::Duration;

const OD: &str = env!("CARGO_BIN_EXE_od");

struct Daemon {
    url: String,
    state: AppState,
    _tmp: tempfile::TempDir,
}

async fn daemon() -> Daemon {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    // A user plugin that needs `mcp`: restricted, so the gate applies.
    let plugin = data.join("plugins/needs-mcp");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("open-design.json"),
        json!({"name": "needs-mcp", "version": "1.0.0", "od": {"kind": "scenario", "capabilities": ["prompt:inject", "mcp"]}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        plugin.join("SKILL.md"),
        "---\nname: needs-mcp\n---\nUse the MCP server.\n",
    )
    .unwrap();
    let store = Store::open(DataDir::new(&data)).unwrap();
    let runs = od_server::open_runs(&store, &od_server::DaemonLock::acquire(&data).unwrap())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState {
        store: Arc::new(store),
        content: Arc::new(ContentStore::load(
            ContentRoots::bundled(&content).with_user_root(&data),
        )),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: Vec::new(),
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: content,
            daemon_url: Some(url.clone()),
            mcp_daemon_url: true,
        }),
        runs: Arc::new(RunContext::new(runs, od_runtime::EngineConfig::from_env())),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    };
    let app = od_server::router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Daemon {
        url,
        state,
        _tmp: tmp,
    }
}

async fn od(d: &Daemon, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(OD);
    cmd.arg("--daemon-url").arg(&d.url).args(args);
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_commands() {
    let d = daemon().await;
    let out = od(&d, &["plugin", "list", "--kind", "scenario"]).await;
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out)
            .lines()
            .any(|l| l.starts_with("od-new-generation\tscenario\t"))
    );
    assert!(stdout(&out).lines().any(|l| l.starts_with("needs-mcp\t")));

    let out = od(&d, &["plugin", "info", "od-new-generation"]).await;
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("source:       bundled"), "{text}");
    assert!(
        text.contains(
            "stage 4:      critique [critique-theater] until critique.score>=4 || iterations>=3"
        ),
        "{text}"
    );
    assert!(text.contains("input:        topic (required)"));

    let out = od(
        &d,
        &[
            "plugin",
            "apply",
            "od-new-generation",
            "-i",
            "artifactKind=deck",
            "-i",
            "audience=VCs",
            "-i",
            "topic=tea",
        ],
    )
    .await;
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).starts_with("Generate a deck for VCs on tea."));
    assert!(stdout(&out).contains("## Plugin pipeline"));
    let out = od(&d, &["plugin", "apply", "od-new-generation"]).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("missing required plugin inputs"),
        "{}",
        stderr(&out)
    );

    // Restricted user plugin: exit 66 with a JSON error until `mcp` is granted.
    let out = od(&d, &["plugin", "apply", "needs-mcp"]).await;
    assert_eq!(out.status.code(), Some(66), "{}", stderr(&out));
    let err: Value = serde_json::from_str(stderr(&out).trim()).unwrap();
    assert_eq!(err["error"]["code"], "capabilities-required");
    assert_eq!(err["error"]["data"]["missing"], json!(["mcp"]));
    let out = od(
        &d,
        &[
            "plugin",
            "apply",
            "needs-mcp",
            "--grant-caps",
            "fs:read,mcp",
        ],
    )
    .await;
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Use the MCP server."));

    let out = od(&d, &["--json", "plugin", "atoms"]).await;
    let atoms: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(atoms["atoms"].as_array().unwrap().len(), 22);

    // validate: a good folder passes, a broken one exits 1 with reasons.
    let good = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/plugins/_official/scenarios/od-new-generation");
    let out = od(&d, &["plugin", "validate", good.to_str().unwrap()]).await;
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("ok od-new-generation"));
    let bad = tempfile::tempdir().unwrap();
    std::fs::write(
        bad.path().join("open-design.json"),
        json!({"name": "bad", "od": {"kind": "scenario", "pipeline": {"stages": [{"id": "x", "atoms": ["nope"]}]}}}).to_string(),
    )
    .unwrap();
    let out = od(
        &d,
        &["--json", "plugin", "validate", bad.path().to_str().unwrap()],
    )
    .await;
    assert_eq!(out.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert!(
        report["errors"][0]
            .as_str()
            .unwrap()
            .contains("unknown atom"),
        "{report}"
    );

    // install / uninstall a local folder.
    let src = tempfile::tempdir().unwrap();
    let dir = src.path().join("tea-cards");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("open-design.json"),
        json!({"name": "tea-cards", "title": "Tea cards", "od": {"kind": "scenario"}}).to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("SKILL.md"), "---\nname: tea-cards\n---\nCards.\n").unwrap();
    let out = od(&d, &["plugin", "install", dir.to_str().unwrap()]).await;
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "installed tea-cards");
    let out = od(&d, &["plugin", "list"]).await;
    assert!(stdout(&out).lines().any(|l| l.starts_with("tea-cards\t")));
    let out = od(&d, &["plugin", "uninstall", "tea-cards"]).await;
    assert!(out.status.success(), "{}", stderr(&out));
    let out = od(&d, &["plugin", "uninstall", "od-new-generation"]).await;
    assert!(!out.status.success(), "bundled plugins stay");
}

/// Fake OpenAI images endpoint; prompts containing `slow` take 2 s.
async fn fake_provider() -> String {
    use axum::routing::post;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/v1/images/generations",
        post(|axum::Json(body): axum::Json<Value>| async move {
            if body["prompt"].as_str().unwrap_or("").contains("slow") {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let png = od_export::image::solid_png(2, 2, [1, 2, 3]).unwrap();
            axum::Json(json!({ "data": [{ "b64_json": base64::engine::general_purpose::STANDARD.encode(png) }] }))
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_commands() {
    let d = daemon().await;
    let provider = fake_provider().await;
    std::fs::write(
        d.state.store.data_dir().root().join("media-config.json"),
        json!({"providers": {"openai": {"apiKey": "sk-test", "baseUrl": format!("{provider}/v1")}}}).to_string(),
    )
    .unwrap();
    let (project, _) = d
        .state
        .store
        .create_project(od_contracts::CreateProjectRequest {
            name: "Media".into(),
            ..Default::default()
        })
        .unwrap();
    let pid = project.id.as_str();
    let dir = d.state.store.project_root(&project).unwrap().dir;

    let out = od(
        &d,
        &[
            "media",
            "generate",
            "--project",
            pid,
            "--surface",
            "image",
            "--model",
            "gpt-image-1",
            "--prompt",
            "a red square",
            "--output",
            "hero",
        ],
    )
    .await;
    assert!(out.status.success(), "{}", stderr(&out));
    let line = stdout(&out);
    assert!(line.trim_end().ends_with("\tdone\thero.png"), "{line}");
    assert!(dir.join("hero.png").is_file());

    // Queue without waiting, cancel, then `wait` reports the cancel (exit 1).
    let out = od(
        &d,
        &[
            "--json",
            "media",
            "generate",
            "--project",
            pid,
            "--surface",
            "image",
            "--model",
            "gpt-image-1",
            "--prompt",
            "slow one",
            "--output",
            "slow",
            "--no-wait",
        ],
    )
    .await;
    assert!(out.status.success(), "{}", stderr(&out));
    let task: Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = task["id"].as_str().unwrap();
    let out = od(&d, &["media", "cancel", id]).await;
    assert!(stdout(&out).contains("\tcanceled"), "{}", stdout(&out));
    let out = od(&d, &["media", "wait", id]).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("task canceled"), "{}", stderr(&out));
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(
        !dir.join("slow.png").exists(),
        "a canceled task writes nothing"
    );

    let out = od(&d, &["media", "tasks", "--project", pid]).await;
    let lines: Vec<String> = stdout(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    let out = od(
        &d,
        &[
            "media",
            "generate",
            "--project",
            pid,
            "--surface",
            "image",
            "--model",
            "no-such-model",
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unknown media model"),
        "{}",
        stderr(&out)
    );

    let out = od(&d, &["media", "models"]).await;
    assert!(
        stdout(&out).contains("provider\topenai\tkey set"),
        "{}",
        stdout(&out)
    );
    let out = od(&d, &["media", "templates", "--surface", "image"]).await;
    assert!(out.status.success());
    assert!(stdout(&out).lines().count() > 10);
    assert!(stdout(&out).lines().all(|l| l.starts_with("image\t")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lint_command() {
    let d = daemon().await;
    let (project, _) = d
        .state
        .store
        .create_project(od_contracts::CreateProjectRequest {
            name: "Lint".into(),
            ..Default::default()
        })
        .unwrap();
    let pid = project.id.as_str();
    let dir = d.state.store.project_root(&project).unwrap().dir;
    let clean = "<!doctype html><style>:root{--accent:#c96442}</style><section data-od-id=\"hero\"><h1>Tea</h1></section>";
    std::fs::write(dir.join("clean.html"), clean).unwrap();
    std::fs::write(
        dir.join("slop.html"),
        "<style>.a{background:linear-gradient(90deg,#7C3AED,#fff)}</style>",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    std::fs::write(
        dir.join("pages/p2.html"),
        "<section data-od-id=\"a\"></section><section></section>",
    )
    .unwrap();
    std::fs::write(dir.join("notes.txt"), "not html").unwrap();

    let out = od(&d, &["lint", pid, "clean.html"]).await;
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with("clean — 0 findings in 1 file"),
        "{}",
        stdout(&out)
    );

    let out = od(&d, &["lint", pid, "slop.html", "--agent-message"]).await;
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("slop.html: [P0] purple-gradient — "),
        "{text}"
    );
    assert!(text.contains("fix: "), "{text}");
    assert!(
        text.contains("<artifact-lint"),
        "agent block printed: {text}"
    );
    assert_eq!(stderr(&out), "", "the findings are the message");

    // A P2 finding passes the default threshold, fails at --fail-on p2.
    let out = od(&d, &["lint", pid, "pages/p2.html"]).await;
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(stdout(&out).contains("pages/p2.html: [P2] missing-section-anchor"));
    let out = od(&d, &["lint", pid, "pages/p2.html", "--fail-on", "p2"]).await;
    assert_eq!(out.status.code(), Some(1));

    // No file: every HTML file in the project, sorted; other files skipped.
    let out = od(&d, &["--json", "lint", pid, "--fail-on", "none"]).await;
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["failOn"], "none");
    let files: Vec<&str> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["clean.html", "pages/p2.html", "slop.html"]);
    assert_eq!(v["counts"]["p0"], 1);
    assert!(v["counts"]["p2"].as_u64().unwrap() >= 1);
    let out = od(&d, &["lint", pid]).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("in 3 files"), "{}", stdout(&out));

    let out = od(&d, &["lint", pid, "notes.txt"]).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("only HTML files are linted"),
        "{}",
        stderr(&out)
    );
    let out = od(&d, &["lint", "no-such-project"]).await;
    assert!(!out.status.success());
}
