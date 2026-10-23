//! M6 runtimes through the daemon: BYOK key storage and runs, ACP session
//! capture and resume, detection of every runtime.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use common::{Daemon, daemon, end_of, write_mock};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn put_config(d: &Daemon, body: &Value) -> (u16, Value) {
    let res = d
        .http
        .put(format!("{}/api/app-config", d.base))
        .json(body)
        .send()
        .await
        .unwrap();
    (res.status().as_u16(), res.json().await.unwrap())
}

#[tokio::test]
async fn byok_key_is_stored_0600_and_never_returned() {
    use std::os::unix::fs::PermissionsExt;
    let d = daemon().await;
    let key = "sk-ant-secret-1234";
    let (status, body) = put_config(
        &d,
        &json!({"byok": {"protocol": "anthropic", "apiKey": key, "model": "claude-opus-5-5"}}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(!body.to_string().contains(key), "{body}");
    assert_eq!(body["byok"]["hasApiKey"], true);
    assert_eq!(body["byok"]["apiKeyHint"], "…1234");

    let path = d.data.join("app-config.json");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(std::fs::read_to_string(&path).unwrap().contains(key));

    // Saving other fields without the key keeps it.
    let (_, body) = put_config(
        &d,
        &json!({"byok": {"protocol": "anthropic", "model": "x"}}),
    )
    .await;
    assert_eq!(body["byok"]["hasApiKey"], true);
    assert_eq!(body["byok"]["model"], "x");
    let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["byok"]["apiKey"], key);
    let got = d.get_json("/api/app-config").await;
    assert!(!got.to_string().contains(key));

    let (status, _) = put_config(&d, &json!({"byok": {"protocol": "gemini"}})).await;
    assert_eq!(status, 400);
    let (status, _) = put_config(&d, &json!({"byok": "sk-raw"})).await;
    assert_eq!(status, 400);
    let (_, body) = put_config(&d, &json!({"byok": null})).await;
    assert!(body.get("byok").is_none());
}

/// A saved BYOK key belongs to the protocol and base URL it was saved with:
/// saving a new one without a key drops it, so the next run never sends it
/// there.
#[tokio::test]
async fn a_new_byok_protocol_or_base_url_drops_the_saved_key() {
    let d = daemon().await;
    let path = d.data.join("app-config.json");
    let save = |byok: Value| {
        let d = &d;
        async move {
            let (status, body) = put_config(d, &json!({ "byok": byok })).await;
            assert_eq!(status, 200, "{body}");
            body["byok"]["hasApiKey"] == true
        }
    };
    let a = "http://127.0.0.1:9/a";
    assert!(save(json!({"protocol": "openai", "baseUrl": a, "apiKey": "sk-a-1234"})).await);
    // The same route, spelled differently, keeps it.
    assert!(
        save(json!({"protocol": "openai-compatible", "baseUrl": "http://127.0.0.1:9/a/ "})).await
    );
    assert!(save(json!({"protocol": "openai", "baseUrl": a, "model": "m"})).await);

    assert!(!save(json!({"protocol": "openai", "baseUrl": "http://127.0.0.1:9/b"})).await);
    assert!(!std::fs::read_to_string(&path).unwrap().contains("sk-"));

    assert!(save(json!({"protocol": "openai", "baseUrl": a, "apiKey": "sk-a-1234"})).await);
    assert!(!save(json!({"protocol": "anthropic", "baseUrl": a})).await);
    assert!(save(json!({"protocol": "anthropic", "apiKey": "sk-ant-5678"})).await);
    assert!(!save(json!({"protocol": "anthropic", "baseUrl": a})).await);
    // A new route with its own key keeps that key.
    assert!(
        save(
            json!({"protocol": "openai", "baseUrl": "http://127.0.0.1:9/c", "apiKey": "sk-c-9999"})
        )
        .await
    );
    let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["byok"]["apiKey"], "sk-c-9999");
}

#[tokio::test]
async fn agents_list_every_runtime_and_byok_follows_the_key() {
    let d = daemon().await;
    write_mock(&d.mocks, "kimi", "echo kimi 1.2.3\n");
    let agents = d.get_json("/api/agents").await;
    let list = agents["agents"].as_array().unwrap();
    let ids: Vec<&str> = list.iter().filter_map(|a| a["id"].as_str()).collect();
    for id in [
        "claude",
        "codex",
        "kimi",
        "hermes",
        "kiro",
        "kilo",
        "vibe",
        "trae-cli",
        "reasonix",
        "devin",
        "opencode",
        "cursor-agent",
        "copilot",
        "deepseek-harness",
        "qwen",
        "byok",
    ] {
        assert!(ids.contains(&id), "{id} missing from {ids:?}");
    }
    let get = |id: &str| list.iter().find(|a| a["id"] == id).unwrap().clone();
    assert_eq!(get("kimi")["available"], true);
    assert_eq!(get("kimi")["version"], "kimi 1.2.3");
    assert_eq!(get("byok")["available"], false);
    assert_eq!(get("cursor-agent")["supportsResume"], false);

    put_config(
        &d,
        &json!({"byok": {"protocol": "openai", "apiKey": "sk-x"}}),
    )
    .await;
    let agents = d.get_json("/api/agents").await;
    let byok = agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "byok")
        .unwrap()
        .clone();
    assert_eq!(byok["available"], true);
}

/// One canned SSE answer on 127.0.0.1; returns the raw request.
async fn fake_openai(body: String) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = sock.read(&mut chunk).await.unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let len = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len || n == 0 {
                    break;
                }
            }
        }
        let head =
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
        sock.write_all(format!("{head}{body}").as_bytes())
            .await
            .unwrap();
        sock.shutdown().await.unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    });
    (format!("http://{addr}/v1"), handle)
}

#[tokio::test]
async fn byok_run_through_the_daemon_writes_index_html() {
    let d = daemon().await;
    let chunks = [
        r#"{"choices":[{"delta":{"content":"<artifact identifier=\"index\" type=\"text/html\">"}}]}"#,
        r#"{"choices":[{"delta":{"content":"<h1>From BYOK</h1></artifact>"},"finish_reason":"stop"}]}"#,
        "[DONE]",
    ];
    let (base, server) =
        fake_openai(chunks.iter().map(|c| format!("data: {c}\n\n")).collect()).await;
    put_config(
        &d,
        &json!({"byok": {"protocol": "openai", "apiKey": "sk-daemon", "baseUrl": base, "model": "m"}}),
    )
    .await;
    let (project, _) = d.project().await;
    let run = d
        .start(json!({"agentId": "byok", "projectId": project, "message": "a hero"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    let end = end_of(&events);
    assert_eq!(end["status"], "succeeded", "{end}");
    let paths = end["artifactPaths"].as_array().unwrap();
    assert!(paths[0].as_str().unwrap().ends_with("/index.html"));
    let raw = server.await.unwrap();
    assert!(
        raw.to_ascii_lowercase()
            .contains("authorization: bearer sk-daemon")
    );
    assert!(
        raw.contains("a hero"),
        "the composed prompt carries the brief"
    );
    let start = events.iter().find(|e| e.event == "start").unwrap();
    assert!(!start.data.to_string().contains("sk-daemon"));
    let file = d
        .http
        .get(format!("{}/api/projects/{project}/raw/index.html", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(file.status().as_u16(), 200);
    assert_eq!(file.text().await.unwrap(), "<h1>From BYOK</h1>");
}

/// Logs every request line, opens or loads `sess-1`, answers the prompt.
const KIMI: &str = r#"
case "$1" in --version) echo "kimi 1.0"; exit 0;; esac
say() { printf '%s\n' "$1"; }
read -r l; printf "%s\n" "$l" >> "$MOCK_LOG"
say '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}'
read -r l; printf "%s\n" "$l" >> "$MOCK_LOG"
say '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-1"}}'
read -r l; printf "%s\n" "$l" >> "$MOCK_LOG"
say '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}}'
say '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
while read -r l; do :; done
"#;

#[tokio::test]
async fn acp_session_is_captured_and_resumed_next_turn() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let d = common::daemon_with(common::Options {
        extra_env: vec![("MOCK_LOG".into(), log.path().to_string_lossy().into_owned())],
        ..common::Options::default()
    })
    .await;
    write_mock(&d.mocks, "kimi", KIMI);
    let (project, conversation) = d.project().await;
    for message in ["first", "second"] {
        let run = d
            .start(json!({"agentId": "kimi", "projectId": project, "conversationId": conversation, "message": message}))
            .await;
        let events = d.events(&run.run_id, None).await;
        assert_eq!(end_of(&events)["status"], "succeeded");
        d.settled_messages(&project, &conversation).await;
    }
    let frames: Vec<Value> = std::fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let methods: Vec<&str> = frames.iter().filter_map(|f| f["method"].as_str()).collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "session/new",
            "session/prompt",
            "initialize",
            "session/load",
            "session/prompt"
        ]
    );
    assert_eq!(frames[4]["params"]["sessionId"], "sess-1");
    let second = frames[5]["params"]["prompt"][0]["text"].as_str().unwrap();
    assert!(second.contains("second"));
    assert!(
        !second.contains("first"),
        "a resumed turn does not resend the transcript"
    );
}
