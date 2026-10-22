//! W2-media end to end: `od mcp live-artifacts` driven by a real MCP
//! client over stdio against the real daemon router.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::{ContentRoots, ContentStore};
use od_server::{AppState, RunContext, ServerConfig};
use od_store::{DataDir, Store};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use serde_json::{Map, Value, json};
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

const OD: &str = env!("CARGO_BIN_EXE_od");

/// The real daemon router over a temp data dir, in this process; returns
/// its URL and run context (to mint tool tokens).
async fn real_daemon(tmp: &Path) -> (String, Arc<RunContext>) {
    let data = tmp.join("data");
    let empty = tmp.join("content");
    let store = Store::open(DataDir::new(&data)).unwrap();
    let runs = od_server::open_runs(&store, &od_server::DaemonLock::acquire(&data).unwrap())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let run_ctx = Arc::new(RunContext::new(runs, od_runtime::EngineConfig::from_env()));
    let state = AppState {
        store: Arc::new(store),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(&empty))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: Vec::new(),
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: empty,
            daemon_url: Some(url.clone()),
        }),
        runs: Arc::clone(&run_ctx),
        preview: od_server::Preview::default(),
    };
    let app = od_server::router(state).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, run_ctx)
}

type Client = RunningService<rmcp::RoleClient, ()>;

async fn live_mcp(daemon_url: &str, args: &[&str], token: Option<&str>) -> Client {
    let mut cmd = tokio::process::Command::new(OD);
    cmd.args(["--daemon-url", daemon_url, "mcp", "live-artifacts"])
        .args(args)
        .env_remove("OD_TOOL_TOKEN")
        .env_remove("OD_PROJECT_ID")
        .stderr(Stdio::null());
    if let Some(t) = token {
        cmd.env("OD_TOOL_TOKEN", t);
    }
    ().serve(TokioChildProcess::new(cmd).unwrap())
        .await
        .unwrap()
}

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

fn input() -> Value {
    json!({
        "title": "Release tracker",
        "document": {
            "dataJson": { "done": 0, "items": [] },
            "sourceJson": {
                "type": "local_file",
                "input": { "path": "status.json" },
                "outputMapping": { "dataPaths": [
                    { "from": "json.done", "to": "done" },
                    { "from": "json.items", "to": "items" }
                ] },
                "refreshPermission": "manual_refresh_granted_for_read_only"
            }
        }
    })
}

const TEMPLATE: &str =
    r#"<h1>{{data.done}} done</h1><ul><li data-od-repeat="it in data.items">{{it}}</li></ul>"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_artifacts_mcp_server_over_stdio() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, runs) = real_daemon(tmp.path()).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let project: Value = http
        .post(format!("{url}/api/projects"))
        .json(&json!({ "name": "Live MCP" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pid = project["project"]["id"].as_str().unwrap().to_owned();

    let client = live_mcp(&url, &["--project", &pid], None).await;
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        [
            "live_artifacts_create",
            "live_artifacts_list",
            "live_artifacts_update",
            "live_artifacts_refresh"
        ]
    );

    // A template with <script> is refused.
    let (err, text, _) = call(
        &client,
        "live_artifacts_create",
        json!({ "input": input(), "templateHtml": "<script>x()</script>" }),
    )
    .await;
    assert!(err, "{text}");
    assert!(text.contains("script"), "{text}");

    let (err, text, created) = call(
        &client,
        "live_artifacts_create",
        json!({ "input": input(), "templateHtml": TEMPLATE }),
    )
    .await;
    assert!(!err, "{text}");
    let aid = created["artifact"]["id"].as_str().unwrap().to_owned();

    http.post(format!("{url}/api/projects/{pid}/files"))
        .json(&json!({ "name": "status.json", "content": "{\"done\": 2, \"items\": [\"api\", \"ui\"]}" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let (err, text, refreshed) = call(
        &client,
        "live_artifacts_refresh",
        json!({ "artifactId": aid }),
    )
    .await;
    assert!(!err, "{text}");
    assert_eq!(refreshed["refresh"]["status"], "succeeded");
    let dir = tmp
        .path()
        .join("data/projects")
        .join(&pid)
        .join(".live-artifacts")
        .join(&aid);
    let data: Value =
        serde_json::from_slice(&std::fs::read(dir.join("data.json")).unwrap()).unwrap();
    assert_eq!(data, json!({ "done": 2, "items": ["api", "ui"] }));
    assert_eq!(
        std::fs::read_to_string(dir.join("index.html")).unwrap(),
        "<h1>2 done</h1><ul><li>api</li><li>ui</li></ul>"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("refreshes.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let (_, _, listed) = call(&client, "live_artifacts_list", json!({})).await;
    assert_eq!(listed["artifacts"][0]["id"], aid.as_str());
    client.cancel().await.unwrap();

    // Launched by an agent run: the tool token names the project and run.
    let token = runs
        .tokens
        .issue("run-42", Some(&pid), std::time::Duration::ZERO);
    let client = live_mcp(&url, &[], Some(&token)).await;
    let (err, text, created) = call(
        &client,
        "live_artifacts_create",
        json!({ "input": { "title": "From agent" } }),
    )
    .await;
    assert!(!err, "{text}");
    assert_eq!(created["artifact"]["createdByRunId"], "run-42");
    assert_eq!(created["artifact"]["projectId"], pid.as_str());
    client.cancel().await.unwrap();
}
