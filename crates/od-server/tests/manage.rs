//! Management routes (Projects, Settings, Integrations, Automations) over
//! HTTP against a real daemon with mock agents. The BYOK provider is a
//! local axum server: nothing leaves loopback.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use common::{PROBES, write_mock};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A claude that records its prompt and env, then answers `reply`.
fn claude(record: &std::path::Path, reply: &str) -> String {
    format!(
        r#"{PROBES}
IFS= read -r line
printf '%s\n' "$line" > "{record}.prompt"
printf 'proxy=%s\n' "$ANTHROPIC_BASE_URL" > "{record}.env"
printf '{{"type":"system","subtype":"init","session_id":"s-%s","model":"mock"}}\n' "$$"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"%s"}}]}}}}\n' "$$" '{reply}'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#,
        record = record.display()
    )
}

async fn wait_for<F, Fut>(label: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..400 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {label}");
}

async fn put(d: &common::Daemon, path: &str, body: &Value) -> reqwest::Response {
    d.http
        .put(format!("{}{path}", d.base))
        .json(body)
        .send()
        .await
        .unwrap()
}

async fn run_turn(d: &common::Daemon, project: &str, conversation: &str, message: &str) {
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "conversationId": conversation, "message": message}))
        .await;
    wait_for("run end", || async {
        d.get_json(&format!("/api/runs/{}", run.run_id)).await["run"]["status"] != "running"
    })
    .await;
    d.settled_messages(project, conversation).await;
}

#[tokio::test]
async fn status_search_and_recent_views_follow_the_history() {
    let d = common::daemon().await;
    let rec = d.tmp.path().join("rec");
    write_mock(
        &d.mocks,
        "claude",
        // A printf argument, not the format: dash (Linux's sh) keeps `\"`
        // in a format, so the JSON line broke there.
        &claude(
            &rec,
            r#"Matcha first. <question-form id=\"q\" title=\"Brief\"></question-form>"#,
        ),
    );
    let (asked, conversation) = d.project().await;
    let (idle, _) = d.project().await;

    let statuses = d.get_json("/api/project-status").await;
    assert_eq!(statuses["statuses"][&asked]["value"], "not_started");

    run_turn(&d, &asked, &conversation, "A tea shop").await;
    let statuses = d.get_json("/api/project-status").await;
    assert_eq!(
        statuses["statuses"][&asked]["value"], "awaiting_input",
        "{statuses}"
    );
    assert_eq!(statuses["statuses"][&idle]["value"], "not_started");

    // Answering moves it on; the mock answers with the form again but the
    // newest message then is ours until the reply lands.
    write_mock(&d.mocks, "claude", &claude(&rec, "All done"));
    run_turn(&d, &asked, &conversation, "Answers: green").await;
    let statuses = d.get_json("/api/project-status").await;
    assert_eq!(statuses["statuses"][&asked]["value"], "succeeded");

    let hits = d.get_json("/api/search?q=MATCHA").await["hits"].clone();
    assert_eq!(hits.as_array().unwrap().len(), 1, "{hits}");
    assert_eq!(hits[0]["kind"], "message");
    assert_eq!(hits[0]["projectId"], asked.as_str());
    assert!(hits[0]["snippet"].as_str().unwrap().contains("Matcha"));
    let hits = d.get_json("/api/search?q=run%20test").await["hits"].clone();
    assert_eq!(hits.as_array().unwrap().len(), 2, "both projects by name");
    let hits = d.get_json("/api/search?q=%25").await["hits"].clone();
    assert_eq!(hits.as_array().unwrap().len(), 0, "% is literal");

    for id in [&idle, &asked] {
        let res = d
            .post(&format!("/api/projects/{id}/viewed"), &json!({}))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let views = d.get_json("/api/recently-viewed").await["views"].clone();
    assert_eq!(views[0]["projectId"], asked.as_str());
    assert_eq!(views[1]["projectId"], idle.as_str());
    let res = d.post("/api/projects/nope/viewed", &json!({})).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

async fn import_zip(d: &common::Daemon, name: &str, bytes: Vec<u8>) -> reqwest::Response {
    let boundary = "od-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
         Content-Type: application/zip\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    d.http
        .post(format!("{}/api/import/claude-design", d.base))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn claude_design_zip_becomes_a_project() {
    let d = common::daemon().await;
    let bytes = zip_of(&[
        ("assets/app.css", b"body{}"),
        ("Landing.html", b"<h1>hi</h1>"),
        ("pages/about.html", b"<p>about</p>"),
    ]);
    let res = import_zip(&d, "Tea Shop.zip", bytes).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["entryFile"], "Landing.html");
    assert_eq!(body["project"]["name"], "Tea Shop");
    assert_eq!(body["project"]["metadata"]["importedFrom"], "claude-design");
    let id = body["project"]["id"].as_str().unwrap().to_owned();
    let files = d.get_json(&format!("/api/projects/{id}/files")).await;
    let names: Vec<&str> = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str())
        .collect();
    assert!(
        names.contains(&"assets/app.css") && names.contains(&"pages/about.html"),
        "{names:?}"
    );
    let convs = d
        .get_json(&format!("/api/projects/{id}/conversations"))
        .await;
    assert_eq!(
        convs["conversations"][0]["title"],
        "Imported Claude Design project"
    );

    let before = d.get_json("/api/projects").await["projects"]
        .as_array()
        .unwrap()
        .len();
    for (name, bytes) in [
        ("evil.zip", zip_of(&[("../escape.html", b"x")])),
        ("nohtml.zip", zip_of(&[("a.css", b"x")])),
        ("notzip.zip", b"plain text".to_vec()),
        ("site.tar", zip_of(&[("index.html", b"x")])),
    ] {
        let res = import_zip(&d, name, bytes).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{name}");
    }
    let after = d.get_json("/api/projects").await["projects"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(before, after, "failed imports leave no project");
}

#[tokio::test]
async fn zip_entries_keep_their_names() {
    let d = common::daemon().await;
    let bytes = zip_of(&[
        ("Landing Page.html", b"<img src=\"assets/hero image.png\">"),
        ("assets/hero image.png", b"png"),
        (".git/config", b"x"),
    ]);
    let res = import_zip(&d, "Tea Shop.zip", bytes).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();
    // The entry and relative links name files that exist; hidden folders
    // still land under a visible name.
    assert_eq!(body["entryFile"], "Landing Page.html");
    assert_eq!(
        body["project"]["metadata"]["entryFile"],
        "Landing Page.html"
    );
    assert_eq!(
        body["files"],
        json!(["Landing Page.html", "assets/hero image.png", "_git/config"])
    );
    let id = body["project"]["id"].as_str().unwrap().to_owned();
    let files = d.get_json(&format!("/api/projects/{id}/files")).await;
    let mut names: Vec<&str> = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["Landing Page.html", "_git/config", "assets/hero image.png"]
    );

    // Two entries naming one file (case-insensitive disks) are refused,
    // not merged.
    let before = d.get_json("/api/projects").await["projects"]
        .as_array()
        .unwrap()
        .len();
    let bytes = zip_of(&[("My Page.html", b"<b>a</b>"), ("my page.html", b"<b>b</b>")]);
    let res = import_zip(&d, "dupes.zip", bytes).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    // APFS also folds Unicode forms (é and e + U+0301): the second write
    // finds the file taken. ext4 keeps both names: a new project.
    let bytes = zip_of(&[("caf\u{e9}.html", b"a"), ("cafe\u{301}.html", b"b")]);
    let res = import_zip(&d, "forms.zip", bytes).await;
    let folds = cfg!(target_os = "macos");
    let expected = if folds {
        StatusCode::CONFLICT
    } else {
        StatusCode::OK
    };
    assert_eq!(res.status(), expected);
    let after = d.get_json("/api/projects").await["projects"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after, before + usize::from(!folds));
}

/// A fake OpenAI/Anthropic endpoint on loopback recording the auth it saw.
async fn fake_provider() -> (String, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let log2 = Arc::clone(&seen);
    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |headers: HeaderMap, body: String| {
                let log = Arc::clone(&log);
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    log.lock().unwrap().push(auth.clone());
                    if auth != "Bearer sk-good" {
                        return (
                            StatusCode::UNAUTHORIZED,
                            r#"{"error":{"message":"bad key"}}"#.to_owned(),
                        );
                    }
                    let v: Value = serde_json::from_str(&body).unwrap();
                    assert_eq!(v["max_tokens"], 100);
                    (
                        StatusCode::OK,
                        json!({"choices": [{"message": {"content": "ok"}}]}).to_string(),
                    )
                }
            }),
        )
        .route(
            "/v1/models",
            get(move |headers: HeaderMap| {
                let log = Arc::clone(&log2);
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    log.lock().unwrap().push(auth);
                    json!({"data": [{"id": "m-b"}, {"id": "m-a"}, {"id": "m-a"}]}).to_string()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1"), seen)
}

#[tokio::test]
async fn byok_test_and_models_never_send_the_saved_key_elsewhere() {
    let d = common::daemon().await;
    let (base, seen) = fake_provider().await;
    let (other, other_seen) = fake_provider().await;

    let test = |body: Value| {
        let d = &d;
        async move {
            let res = d.post("/api/test/connection", &body).await;
            assert_eq!(res.status(), StatusCode::OK);
            res.json::<Value>().await.unwrap()
        }
    };
    // Setup counts a passed test only (Settings ▸ About ▸ Your usage).
    let tested = || async {
        d.get_json("/api/usage").await["setup"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["step"] == "agent_tested")
    };
    let r = test(
        json!({"mode": "provider", "protocol": "openai", "baseUrl": base, "apiKey": "sk-bad"}),
    )
    .await;
    assert_eq!(r["kind"], "auth_failed");
    assert_eq!(r["detail"], "bad key");
    assert!(!tested().await, "a failed test is no setup step");
    let r = test(json!({"mode": "provider", "protocol": "openai", "baseUrl": base, "apiKey": "sk-good", "model": "m"})).await;
    assert_eq!(
        (r["ok"].clone(), r["kind"].clone(), r["sample"].clone()),
        (json!(true), json!("success"), json!("ok"))
    );
    assert!(tested().await);
    let r = test(
        json!({"mode": "provider", "protocol": "openai", "baseUrl": "ftp://x", "apiKey": "k"}),
    )
    .await;
    assert_eq!(r["kind"], "invalid_base_url");

    // Saved key: used for the saved base URL only.
    let res = put(
        &d,
        "/api/app-config",
        &json!({"byok": {"protocol": "openai", "apiKey": "sk-good", "baseUrl": base}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let r = test(json!({"mode": "provider", "protocol": "openai", "baseUrl": base})).await;
    assert_eq!(r["ok"], true, "{r}");
    let res = d
        .post(
            "/api/test/connection",
            &json!({"mode": "provider", "protocol": "openai", "baseUrl": other}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(
        other_seen.lock().unwrap().is_empty(),
        "another host never sees the key"
    );
    // Nor another protocol at the same URL (empty URLs mean each
    // protocol's own host).
    let res = d
        .post(
            "/api/test/connection",
            &json!({"mode": "provider", "protocol": "anthropic", "baseUrl": base}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let models = d
        .post(
            "/api/provider/models",
            &json!({"protocol": "openai", "baseUrl": base}),
        )
        .await;
    assert_eq!(models.status(), StatusCode::OK);
    let models: Value = models.json().await.unwrap();
    assert_eq!(models["models"], json!(["m-a", "m-b"]));
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|a| a.starts_with("Bearer sk-"))
    );
}

/// Azure OpenAI over loopback: the test call goes to the deployment URL
/// with its `api-version` and the key in `api-key`, never as a bearer.
#[tokio::test]
async fn byok_azure_tests_a_deployment() {
    let d = common::daemon().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let app = Router::new().route(
        "/openai/deployments/{dep}/chat/completions",
        post(
            move |axum::extract::Path(dep): axum::extract::Path<String>,
                  axum::extract::RawQuery(query): axum::extract::RawQuery,
                  headers: HeaderMap,
                  body: String| {
                let log = Arc::clone(&log);
                async move {
                    let header = |k: &str| {
                        headers
                            .get(k)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned()
                    };
                    log.lock().unwrap().push(format!(
                        "{dep} {} key={} auth={}",
                        query.unwrap_or_default(),
                        header("api-key"),
                        header("authorization")
                    ));
                    let v: Value = serde_json::from_str(&body).unwrap();
                    assert!(v.get("model").is_none(), "the URL names the deployment");
                    if header("api-key") != "az-good" {
                        return (
                            StatusCode::UNAUTHORIZED,
                            r#"{"error":{"message":"bad key"}}"#.to_owned(),
                        );
                    }
                    (
                        StatusCode::OK,
                        json!({"choices": [{"message": {"content": "ok"}}]}).to_string(),
                    )
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let test = |body: Value| {
        let d = &d;
        async move {
            let res = d.post("/api/test/connection", &body).await;
            assert_eq!(res.status(), StatusCode::OK);
            res.json::<Value>().await.unwrap()
        }
    };
    let r = test(json!({"mode": "provider", "protocol": "azure", "baseUrl": base, "apiKey": "az-good", "model": "design", "apiVersion": "2025-01-01-preview"})).await;
    assert_eq!(
        (r["ok"].clone(), r["sample"].clone()),
        (json!(true), json!("ok")),
        "{r}"
    );
    let r = test(json!({"mode": "provider", "protocol": "azure", "baseUrl": base, "apiKey": "az-bad", "model": "design"})).await;
    assert_eq!(r["kind"], "auth_failed");
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "design api-version=2025-01-01-preview key=az-good auth=",
            "design api-version=2024-10-21 key=az-bad auth=",
        ]
    );
    // No endpoint: Azure has no shared host to fall back to.
    let r =
        test(json!({"mode": "provider", "protocol": "azure", "apiKey": "k", "model": "design"}))
            .await;
    assert_eq!(r["kind"], "invalid_base_url");
    // The config saves with its api-version; the model list says why it has none.
    let res = put(
        &d,
        "/api/app-config",
        &json!({"byok": {"protocol": "azure", "apiKey": "az-good", "baseUrl": base, "model": "design", "apiVersion": "2025-01-01-preview"}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cfg = d.get_json("/api/app-config").await;
    assert_eq!(cfg["byok"]["apiVersion"], "2025-01-01-preview");
    // `null` clears it (the form sends that for an emptied field).
    let res = put(
        &d,
        "/api/app-config",
        &json!({"byok": {"protocol": "azure", "baseUrl": base, "model": "design", "apiVersion": null}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cfg = d.get_json("/api/app-config").await;
    assert!(cfg["byok"]["apiVersion"].is_null(), "{cfg}");
    assert_eq!(cfg["byok"]["hasApiKey"], true, "the key stays");
    // Azure without an endpoint is refused: a run could never reach it.
    for no_base in [json!(null), json!(" ")] {
        let res = put(
            &d,
            "/api/app-config",
            &json!({"byok": {"protocol": "azure", "apiKey": "k", "baseUrl": no_base}}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
    let res = d
        .post(
            "/api/provider/models",
            &json!({"protocol": "azure", "baseUrl": base}),
        )
        .await;
    assert_ne!(res.status(), StatusCode::OK);
}

/// A saved agent key belongs to the base URL it was saved with: a new URL
/// without a new key drops it, so the next run never sends it there.
#[tokio::test]
async fn a_new_agent_base_url_drops_the_saved_key() {
    let d = common::daemon().await;
    let key = |env: &Value, agent: &str, k: &str| env["env"][agent][k]["set"] == true;
    let res = put(
        &d,
        "/api/agent-cli-env",
        &json!({"env": {"claude": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:9/a", "ANTHROPIC_API_KEY": "sk-ant-secret-1234"},
                        "codex": {"OPENAI_BASE_URL": "http://127.0.0.1:9/a", "OPENAI_API_KEY": "sk-openai-1234", "CODEX_HOME": "/tmp/c"}}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    // The same URL again keeps the key.
    let res = put(
        &d,
        "/api/agent-cli-env",
        &json!({"env": {"claude": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:9/a"}}}),
    )
    .await;
    assert!(key(
        &res.json().await.unwrap(),
        "claude",
        "ANTHROPIC_API_KEY"
    ));
    for patch in [
        json!({"claude": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:9/b"}}),
        json!({"codex": {"OPENAI_BASE_URL": null}}),
    ] {
        let res = put(&d, "/api/agent-cli-env", &json!({ "env": patch })).await;
        assert_eq!(res.status(), StatusCode::OK);
    }
    let env = d.get_json("/api/agent-cli-env").await;
    assert!(!key(&env, "claude", "ANTHROPIC_API_KEY"), "{env}");
    assert!(!key(&env, "codex", "OPENAI_API_KEY"), "{env}");
    assert_eq!(env["env"]["codex"]["CODEX_HOME"], "/tmp/c");
    let stored = std::fs::read_to_string(d.data.join("agent-cli-env.json")).unwrap();
    assert!(!stored.contains("sk-"), "{stored}");
    // A new URL with its own key keeps that key.
    let res = put(
        &d,
        "/api/agent-cli-env",
        &json!({"env": {"claude": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:9/c", "ANTHROPIC_API_KEY": "sk-ant-new-5678"}}}),
    )
    .await;
    assert!(key(
        &res.json().await.unwrap(),
        "claude",
        "ANTHROPIC_API_KEY"
    ));
}

/// A config dir, Codex home or Codex binary can route the key elsewhere too:
/// changing one drops the saved key like a new base URL does.
#[tokio::test]
async fn a_new_config_dir_home_or_binary_drops_the_saved_key() {
    let d = common::daemon().await;
    let key = |env: &Value, agent: &str, k: &str| env["env"][agent][k]["set"] == true;
    for (agent, routing, secret) in [
        ("claude", "CLAUDE_CONFIG_DIR", "ANTHROPIC_API_KEY"),
        ("codex", "CODEX_HOME", "OPENAI_API_KEY"),
        ("codex", "CODEX_BIN", "CODEX_API_KEY"),
    ] {
        let res = put(
            &d,
            "/api/agent-cli-env",
            &json!({"env": {agent: {routing: "/tmp/a", secret: "sk-secret-1234"}}}),
        )
        .await;
        assert!(key(&res.json().await.unwrap(), agent, secret));
        let res = put(
            &d,
            "/api/agent-cli-env",
            &json!({"env": {agent: {routing: "/tmp/a"}}}),
        )
        .await;
        assert!(key(&res.json().await.unwrap(), agent, secret), "{routing}");
        let res = put(
            &d,
            "/api/agent-cli-env",
            &json!({"env": {agent: {routing: "/tmp/b"}}}),
        )
        .await;
        assert!(!key(&res.json().await.unwrap(), agent, secret), "{routing}");
    }
    let stored = std::fs::read_to_string(d.data.join("agent-cli-env.json")).unwrap();
    assert!(!stored.contains("sk-"), "{stored}");
}

#[tokio::test]
async fn agent_test_runs_the_cli_once_and_settings_reach_runs() {
    let d = common::daemon().await;
    let rec = d.tmp.path().join("rec");
    write_mock(&d.mocks, "claude", &claude(&rec, "ok"));
    // Setup counts a passed test only (Settings ▸ About ▸ Your usage).
    let tested = || async {
        d.get_json("/api/usage").await["setup"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["step"] == "agent_tested")
    };
    let missing: Value = d
        .post(
            "/api/test/connection",
            &json!({"mode": "agent", "agentId": "codex"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(missing["ok"], false);
    assert!(!tested().await, "a failed test is no setup step");

    let r: Value = d
        .post(
            "/api/test/connection",
            &json!({"mode": "agent", "agentId": "claude"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        (r["ok"].clone(), r["sample"].clone()),
        (json!(true), json!("ok")),
        "{r}"
    );
    assert!(tested().await);
    assert!(
        std::fs::read_to_string(rec.with_extension("prompt"))
            .unwrap()
            .contains("Reply with only: ok")
    );
    let r: Value = d
        .post(
            "/api/test/connection",
            &json!({"mode": "agent", "agentId": "codex"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(r["kind"], "agent_not_installed");

    // Advanced env: stored, secrets hinted, applied to the agent.
    let res = put(
        &d,
        "/api/agent-cli-env",
        &json!({"env": {"claude": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:9/proxy", "ANTHROPIC_API_KEY": "sk-ant-secret-1234"}}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let env = d.get_json("/api/agent-cli-env").await;
    assert_eq!(env["env"]["claude"]["ANTHROPIC_API_KEY"]["hint"], "…1234");
    assert!(!env.to_string().contains("sk-ant-secret"));
    let res = put(
        &d,
        "/api/agent-cli-env",
        &json!({"env": {"claude": {"PATH": "/evil"}}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    // A PUT echoing the redacted value keeps the key.
    let res = put(&d, "/api/agent-cli-env", &env).await;
    assert_eq!(res.status(), StatusCode::OK);
    let stored = std::fs::read_to_string(d.data.join("agent-cli-env.json")).unwrap();
    assert!(stored.contains("sk-ant-secret-1234"));

    // Global rules and global memory are in the prompt.
    put(
        &d,
        "/api/app-config",
        &json!({"customInstructions": "Always use tabs."}),
    )
    .await;
    let res = d
        .post(
            "/api/memory",
            &json!({"kind": "rule", "text": "The brand is teal."}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let memory_id = res.json::<Value>().await.unwrap()["memory"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let res = d
        .post("/api/memory", &json!({"kind": "nope", "text": "x"}))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let (project, conversation) = d.project().await;
    run_turn(&d, &project, &conversation, "Make a page").await;
    let prompt = std::fs::read_to_string(rec.with_extension("prompt")).unwrap();
    assert!(
        prompt.contains("Custom instructions (user-level)") && prompt.contains("Always use tabs.")
    );
    assert!(prompt.contains("The brand is teal."));
    let env = std::fs::read_to_string(rec.with_extension("env")).unwrap();
    assert_eq!(env.trim(), "proxy=http://127.0.0.1:9/proxy");
    let res = d
        .http
        .delete(format!("{}/api/memory/{memory_id}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(d.get_json("/api/memory").await["memories"], json!([]));
}

#[tokio::test]
async fn codex_bin_and_service_tier_reach_the_codex_run() {
    let d = common::daemon().await;
    // A codex that is not on PATH: only CODEX_BIN finds it.
    let own = d.tmp.path().join("own-bin");
    std::fs::create_dir_all(&own).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../od-runtime/tests/fixtures/codex/turn-exec-json.jsonl");
    let codex = write_mock(
        &own,
        "codex",
        &format!(
            "{PROBES}\necho \"$*\" >> \"{log}\"\ncat >/dev/null\ncat \"{fixture}\"\n",
            log = own.join("argv.log").display(),
            fixture = fixture.display()
        ),
    );
    let r: Value = d
        .post(
            "/api/test/connection",
            &json!({"mode": "agent", "agentId": "codex"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(r["kind"], "agent_not_installed");
    let res = put(
        &d,
        "/api/agent-cli-env",
        &json!({"env": {"codex": {"CODEX_BIN": codex.to_string_lossy()}}}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let r: Value = d
        .post(
            "/api/test/connection",
            &json!({"mode": "agent", "agentId": "codex"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");

    put(
        &d,
        "/api/app-config",
        &json!({"agentModels": {"codex": {"serviceTier": "priority", "reasoning": "high"}}}),
    )
    .await;
    let (project, conversation) = d.project().await;
    let run = d
        .start(json!({"agentId": "codex", "projectId": project, "conversationId": conversation, "message": "hi"}))
        .await;
    wait_for("codex run", || async {
        d.get_json(&format!("/api/runs/{}", run.run_id)).await["run"]["status"] != "running"
    })
    .await;
    let argv = std::fs::read_to_string(own.join("argv.log")).unwrap();
    let last = argv.lines().last().unwrap();
    assert!(last.contains("service_tier=\"priority\""), "{last}");
    assert!(last.contains("model_reasoning_effort=\"high\""), "{last}");
}

#[tokio::test]
async fn project_locations_hold_new_projects_and_scan_finds_folders() {
    let d = common::daemon().await;
    // Not under the daemon's own temp dir: its `.tmpXXXX` name is hidden.
    let holder = tempfile::Builder::new()
        .prefix("od-work")
        .tempdir()
        .unwrap();
    let base = holder.path().join("work");
    std::fs::create_dir_all(base.join("Old one/.open-design")).unwrap();
    std::fs::write(
        base.join("Old one/.open-design/project.json"),
        json!({"schemaVersion": 1, "id": "x", "name": "Old one", "createdAt": 1, "updatedAt": 1})
            .to_string(),
    )
    .unwrap();
    let base_s = base.canonicalize().unwrap().to_string_lossy().into_owned();

    let res = put(
        &d,
        "/api/project-locations",
        &json!({"locations": [{"name": "Work", "path": base_s}], "defaultId": "work"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let list: Value = res.json().await.unwrap();
    assert_eq!(list["defaultId"], "work");
    assert_eq!(list["locations"][0]["id"], "default");
    assert_eq!(list["locations"][1]["path"], base_s.as_str());
    // A parent of the data directory (not hidden, so only the overlap rule refuses it).
    let data_parent = d.tmp.path().parent().unwrap().canonicalize().unwrap();
    for bad in [
        json!("relative/dir"),
        json!("/no/such/dir"),
        json!(d.data.to_string_lossy()),
        json!(data_parent.to_string_lossy()),
    ] {
        let res = put(
            &d,
            "/api/project-locations",
            &json!({"locations": [{"name": "x", "path": bad}]}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
    }

    let scan: Value = d
        .post("/api/project-locations/scan", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(scan, json!({"imported": 1, "existing": 0}));
    let scan: Value = d
        .post("/api/project-locations/scan", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(scan, json!({"imported": 0, "existing": 1}));

    let (id, _) = d.project().await;
    let p = d.get_json(&format!("/api/projects/{id}")).await;
    let dir = p["project"]["metadata"]["baseDir"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(dir, format!("{base_s}/Run test"));
    assert!(
        std::path::Path::new(&dir)
            .join(".open-design/project.json")
            .is_file()
    );
    let (id2, _) = d.project().await;
    let p2 = d.get_json(&format!("/api/projects/{id2}")).await;
    assert_eq!(
        p2["project"]["metadata"]["baseDir"],
        format!("{base_s}/Run test 2")
    );
}

#[tokio::test]
async fn integrations_info_and_local_diagnostics() {
    let d = common::daemon().await;
    let info = d.get_json("/api/mcp/install-info").await;
    assert_eq!(info["args"][0], "mcp");
    let ids: Vec<&str> = info["clients"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "claude",
            "codex",
            "cursor",
            "kiro",
            "vscode",
            "antigravity",
            "zed",
            "windsurf"
        ]
    );
    let catalog = d.get_json("/api/mcp/catalog").await;
    assert!(catalog["templates"].as_array().unwrap().len() >= 30);
    let res = d.post("/api/mcp/install/emacs", &json!({})).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    put(
        &d,
        "/api/app-config",
        &json!({"byok": {"protocol": "openai", "apiKey": "sk-hidden-123456"}}),
    )
    .await;
    let logs = d.data.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let old: String = (0..300).map(|i| format!("old line {i}\n")).collect();
    std::fs::write(logs.join("daemon.log"), old + concat!(
        "2026-10-26T10:00:00Z  INFO od_server::routes::projects: project not found project=nope\n",
        "2026-10-26T10:00:01Z  WARN od_runtime: provider refused api_key=\"sk-log-secret\" status=401\n",
    ))
    .unwrap();
    let res = d
        .http
        .get(format!("{}/api/diagnostics/export", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "application/zip");
    let bytes = res.bytes().await.unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
        .collect();
    for n in [
        "summary/manifest.json",
        "summary/machine-info.json",
        "app-config.json",
        "agents.json",
        "runs.json",
    ] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
    }
    let mut config = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("app-config.json").unwrap(), &mut config)
        .unwrap();
    assert!(!config.contains("sk-hidden"), "{config}");
    let mut log = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("logs/daemon.log").unwrap(), &mut log).unwrap();
    assert_eq!(log.lines().count(), 200, "{log}");
    assert!(log.contains("project not found project=nope"), "{log}");
    assert!(log.contains("api_key=[redacted] status=401"), "{log}");
    assert!(!log.contains("sk-log-secret"), "{log}");
    assert!(!log.contains("old line 101\n"), "{log}");
}

#[tokio::test]
async fn a_create_each_run_routine_makes_a_project_per_run() {
    let d = common::daemon().await;
    let rec = d.tmp.path().join("rec");
    write_mock(&d.mocks, "claude", &claude(&rec, "routine done"));
    let res = d
        .post(
            "/api/routines",
            &json!({"name": "Digest", "targetMode": "create_each_run", "agentId": "claude",
                    "prompt": "Summarize", "schedule": "CRON_TZ=Asia/Tokyo 0 9 * * 1-5"}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let routine: Value = res.json().await.unwrap();
    let rid = routine["routine"]["id"].as_str().unwrap().to_owned();
    assert_eq!(routine["routine"]["targetMode"], "create_each_run");
    assert!(routine["routine"]["nextRunAt"].is_i64());
    let before = d.get_json("/api/projects").await["projects"]
        .as_array()
        .unwrap()
        .len();
    let res = d
        .post(&format!("/api/routines/{rid}/run"), &json!({}))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    wait_for("routine run", || async {
        let h = d.get_json(&format!("/api/routines/{rid}/runs")).await;
        h["runs"][0]["status"] == "succeeded"
    })
    .await;
    let h = d.get_json(&format!("/api/routines/{rid}/runs")).await;
    let pid = h["runs"][0]["projectId"].as_str().unwrap().to_owned();
    let projects = d.get_json("/api/projects").await["projects"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(projects.len(), before + 1);
    assert!(
        projects.iter().any(
            |p| p["id"] == pid.as_str() && p["name"].as_str().unwrap().starts_with("Digest · ")
        )
    );
    let convs = d
        .get_json(&format!("/api/projects/{pid}/conversations"))
        .await;
    assert_eq!(
        convs["conversations"].as_array().unwrap().len(),
        1,
        "no empty extra conversation"
    );

    // Edit: rename and move to one project.
    let res = d
        .http
        .patch(format!("{}/api/routines/{rid}", d.base))
        .json(&json!({"name": "Digest 2", "targetMode": "reuse", "projectId": pid, "schedule": "15 * * * *"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let r: Value = res.json().await.unwrap();
    assert_eq!(
        (
            r["routine"]["name"].clone(),
            r["routine"]["projectId"].clone()
        ),
        (json!("Digest 2"), json!(pid))
    );
    let res = d
        .http
        .patch(format!("{}/api/routines/{rid}", d.base))
        .json(&json!({"projectId": "missing"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
