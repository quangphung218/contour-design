//! External MCP servers (spec 1.5): config with masked secrets in 0600
//! files, stdio and HTTP tests, the OAuth PKCE flow through the loopback
//! callback against a mock authorization server, refresh, and the
//! `--mcp-config` an agent gets at spawn.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use common::{PROBES, write_mock};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

async fn put_servers(d: &common::Daemon, body: &Value) -> reqwest::Response {
    d.http
        .put(format!("{}/api/mcp/servers", d.base))
        .json(body)
        .send()
        .await
        .unwrap()
}

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A stdio MCP server in sh: answers initialize (id 1) and tools/list (id 2).
fn stdio_server(dir: &std::path::Path) -> std::path::PathBuf {
    write_mock(
        dir,
        "mcp-stdio",
        r#"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*) echo '{"jsonrpc":"2.0","method":"notifications/message","params":{}}'
                      echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"s","version":"1"}}}';;
    *'"tools/list"'*) echo "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"read_$MCP_ROOT\"}]}}";;
  esac
done
"#,
    )
}

#[tokio::test]
async fn config_is_private_masked_and_validated() {
    let d = common::daemon().await;
    let bin = stdio_server(&d.mocks);
    let res = put_servers(
        &d,
        &json!({"servers": [
            {"id": "files", "transport": "stdio", "command": bin, "env": {"MCP_ROOT": "docs", "API_KEY": "sk-secret-1"}},
            {"id": "remote", "transport": "http", "url": "https://mcp.example/mcp", "headers": {"X-Key": "hdr-secret"}}
        ]}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.text().await.unwrap();
    assert!(
        !body.contains("sk-secret-1") && !body.contains("hdr-secret"),
        "{body}"
    );
    let cfg_path = d.data.join("mcp-config.json");
    assert_eq!(mode(&cfg_path), 0o600);
    assert!(
        std::fs::read_to_string(&cfg_path)
            .unwrap()
            .contains("sk-secret-1")
    );

    // Sending the masked list back keeps the secrets.
    let listed = d.get_json("/api/mcp/servers").await;
    assert_eq!(
        listed["servers"][0]["env"]["API_KEY"],
        od_mcp_client::config::MASK
    );
    assert_eq!(listed["servers"][1]["oauth"]["connected"], false);
    let mut servers: Vec<Value> = listed["servers"].as_array().unwrap().clone();
    for s in &mut servers {
        s.as_object_mut().unwrap().remove("oauth");
    }
    assert_eq!(
        put_servers(&d, &json!({"servers": servers})).await.status(),
        StatusCode::OK
    );
    assert!(
        std::fs::read_to_string(&cfg_path)
            .unwrap()
            .contains("sk-secret-1")
    );

    // The stdio server answers through the test button.
    let t: Value = d
        .post("/api/mcp/servers/files/test", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(t["ok"], true, "{t}");
    assert_eq!(t["tools"][0]["name"], "read_docs");

    for bad in [
        json!({"servers": [{"id": "Bad Id", "transport": "stdio", "command": "x"}]}),
        json!({"servers": [{"id": "a", "transport": "stdio"}]}),
        json!({"servers": [{"id": "a", "transport": "stdio", "command": "x", "url": "https://a"}]}),
        json!({"servers": [{"id": "a", "transport": "http", "url": "ftp://a.example"}]}),
        json!({"servers": [{"id": "a", "transport": "http", "url": "https://u:p@a.example/mcp"}]}),
        json!({"servers": [{"id": "a", "transport": "http", "url": "https://a.example", "headers": {"X": "a\r\nInjected: 1"}}]}),
        json!({"servers": [{"id": "a", "transport": "http", "url": "https://a.example", "headers": {"Bad Name": "v"}}]}),
        json!({"servers": [{"id": "a", "transport": "stdio", "command": "x", "env": {"1BAD": "v"}}]}),
        json!({"servers": [{"id": "a", "transport": "stdio", "command": "x"}, {"id": "a", "transport": "stdio", "command": "y"}]}),
        json!({"servers": [{"id": "new", "transport": "stdio", "command": "x", "env": {"K": od_mcp_client::config::MASK}}]}),
        json!({"servers": [{"id": "a", "transport": "stdio", "command": "x", "shell": true}]}),
        json!({"servers": [{"id": "a", "transport": "websocket", "url": "wss://a"}]}),
    ] {
        assert_eq!(
            put_servers(&d, &bad).await.status(),
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    let res = d.post("/api/mcp/servers/nope/test", &json!({})).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Not from the app's own origin: refused.
    let res = d
        .http
        .put(format!("{}/api/mcp/servers", d.base))
        .header(header::ORIGIN, "https://evil.example")
        .json(&json!({"servers": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(
        std::fs::read_to_string(&cfg_path)
            .unwrap()
            .contains("files")
    );
}

/// Mock MCP server + authorization server on one loopback port.
#[derive(Default)]
struct Auth {
    base: String,
    challenge: Option<String>,
    codes: HashMap<String, String>,
    token_calls: Vec<HashMap<String, String>>,
    valid: Vec<String>,
}

type Shared = Arc<Mutex<Auth>>;

async fn mcp_endpoint(State(a): State<Shared>, headers: HeaderMap, body: String) -> Response {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let (ok, base) = {
        let a = a.lock().unwrap();
        (
            a.valid.iter().any(|t| auth == format!("Bearer {t}")),
            a.base.clone(),
        )
    };
    if !ok {
        let challenge =
            format!(r#"Bearer resource_metadata="{base}/.well-known/oauth-protected-resource""#);
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, challenge)],
        )
            .into_response();
    }
    let msg: Value = serde_json::from_str(&body).unwrap();
    match msg["method"].as_str() {
        Some("initialize") => (
            [("mcp-session-id", "sess-1")],
            axum::Json(json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
        )
            .into_response(),
        Some("tools/list") => {
            assert_eq!(headers.get("mcp-session-id").unwrap(), "sess-1");
            let data = json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"tools": [{"name": "make_image"}]}});
            ([(header::CONTENT_TYPE, "text/event-stream")], format!("event: message\ndata: {data}\n\n")).into_response()
        }
        _ => StatusCode::ACCEPTED.into_response(),
    }
}

async fn authorize(State(a): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["client_id"], "cid-1");
    assert_eq!(q["scope"], "images");
    let mut a = a.lock().unwrap();
    assert_eq!(q["resource"], format!("{}/mcp", a.base));
    a.challenge = Some(q["code_challenge"].clone());
    a.codes.insert("code-1".into(), q["redirect_uri"].clone());
    let loc = format!("{}?code=code-1&state={}", q["redirect_uri"], q["state"]);
    (StatusCode::FOUND, [(header::LOCATION, loc)]).into_response()
}

async fn token(State(a): State<Shared>, Form(f): Form<HashMap<String, String>>) -> Response {
    let mut a = a.lock().unwrap();
    a.token_calls.push(f.clone());
    let ok = match f["grant_type"].as_str() {
        "authorization_code" => {
            let verifier_ok = a.challenge.as_deref()
                == Some(
                    &base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .encode(Sha256::digest(f["code_verifier"].as_bytes())),
                );
            let code_ok = a
                .codes
                .remove(&f["code"])
                .is_some_and(|uri| uri == f["redirect_uri"]);
            verifier_ok && code_ok
        }
        "refresh_token" => f["refresh_token"] == "rt-1",
        _ => false,
    };
    if !ok {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_grant"})),
        )
            .into_response();
    }
    let n = a.token_calls.len();
    let access = format!("at-{n}");
    a.valid.push(access.clone());
    axum::Json(json!({"access_token": access, "refresh_token": "rt-1", "expires_in": 3600, "token_type": "Bearer"})).into_response()
}

async fn mock_auth_server() -> (String, Shared) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let shared: Shared = Arc::new(Mutex::new(Auth {
        base: base.clone(),
        ..Auth::default()
    }));
    let b = base.clone();
    let app = axum::Router::new()
        .route("/mcp", post(mcp_endpoint))
        .route(
            "/.well-known/oauth-protected-resource",
            get({
                let b = b.clone();
                move || async move { axum::Json(json!({"resource": format!("{b}/mcp"), "authorization_servers": [b], "scopes_supported": ["images"]})) }
            }),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(move || async move {
                axum::Json(json!({"issuer": b, "authorization_endpoint": format!("{b}/authorize"),
                    "token_endpoint": format!("{b}/token"), "registration_endpoint": format!("{b}/register"),
                    "code_challenge_methods_supported": ["S256"]}))
            }),
        )
        .route(
            "/register",
            post(|axum::Json(r): axum::Json<Value>| async move {
                assert_eq!(r["token_endpoint_auth_method"], "none");
                axum::Json(json!({"client_id": "cid-1"}))
            }),
        )
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state(Arc::clone(&shared));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, shared)
}

#[tokio::test]
async fn oauth_pkce_flow_refresh_and_agent_wiring() {
    let d = common::daemon().await;
    let (base, auth) = mock_auth_server().await;
    let bin = stdio_server(&d.mocks);
    let res = put_servers(
        &d,
        &json!({"servers": [
            {"id": "images", "transport": "http", "url": format!("{base}/mcp"), "auth": "oauth"},
            {"id": "files", "transport": "stdio", "command": bin, "env": {"MCP_ROOT": "docs"}}
        ]}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let t: Value = d
        .post("/api/mcp/servers/images/test", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        (t["ok"].as_bool(), t["needsAuth"].as_bool()),
        (Some(false), Some(true)),
        "{t}"
    );

    let start: Value = d
        .post("/api/mcp/servers/images/oauth/start", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    let authorize_url = start["authorizeUrl"].as_str().unwrap().to_owned();
    assert!(authorize_url.starts_with(&format!("{base}/authorize?")));
    // The browser: the authorization server redirects to the daemon.
    let no_redirect = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .unwrap();
    let res = no_redirect.get(&authorize_url).send().await.unwrap();
    let callback = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
    assert!(
        callback.starts_with(&format!("{}/api/mcp/oauth/callback?", d.base)),
        "{callback}"
    );
    let res = no_redirect.get(&callback).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!res.text().await.unwrap().contains("code-1"));

    // The state is single use; a forged one is refused.
    assert_eq!(
        no_redirect.get(&callback).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let forged = format!("{}/api/mcp/oauth/callback?code=code-1&state=guess", d.base);
    assert_eq!(
        no_redirect.get(&forged).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );

    let tokens_path = d.data.join("mcp-tokens.json");
    assert_eq!(mode(&tokens_path), 0o600);
    let listed = d
        .http
        .get(format!("{}/api/mcp/servers", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        listed.contains("\"connected\":true")
            && !listed.contains("at-1")
            && !listed.contains("rt-1"),
        "{listed}"
    );
    let t: Value = d
        .post("/api/mcp/servers/images/test", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(t["tools"][0]["name"], "make_image", "{t}");

    // Expired: the next use refreshes it.
    let mut stored: Value = serde_json::from_slice(&std::fs::read(&tokens_path).unwrap()).unwrap();
    stored["servers"]["images"]["expiresAt"] = json!(1);
    std::fs::write(&tokens_path, stored.to_string()).unwrap();
    let t: Value = d
        .post("/api/mcp/servers/images/test", &json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(t["ok"], true, "{t}");
    {
        let a = auth.lock().unwrap();
        assert_eq!(a.token_calls.len(), 2);
        assert_eq!(a.token_calls[1]["grant_type"], "refresh_token");
        assert_eq!(a.token_calls[1]["resource"], format!("{base}/mcp"));
    }
    assert!(
        std::fs::read_to_string(&tokens_path)
            .unwrap()
            .contains("at-2")
    );

    // A Claude run gets both servers through a private --mcp-config file
    // that is gone after the run.
    let log = d.tmp.path().join("claude.log");
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r#"{PROBES}
prev=""; for a in "$@"; do [ "$prev" = "--mcp-config" ] && {{ cat "$a" > "{log}.cfg"; stat -f %Lp "$a" > "{log}.mode"; echo "$a" > "{log}.path"; }}; prev="$a"; done
IFS= read -r line
printf '{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"text","text":"ok"}}]}}}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#,
            log = log.display()
        ),
    );
    let (project, conversation) = d.project().await;
    let run = d
        .start(json!({"agentId": "claude", "projectId": project, "conversationId": conversation, "message": "hi"}))
        .await;
    let events = d.events(&run.run_id, None).await;
    assert_eq!(common::end_of(&events)["status"], "succeeded");
    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{}.cfg", log.display())).unwrap())
            .unwrap();
    assert_eq!(
        cfg["mcpServers"]["images"]["headers"]["Authorization"],
        "Bearer at-2"
    );
    assert_eq!(cfg["mcpServers"]["files"]["env"]["MCP_ROOT"], "docs");
    assert_eq!(
        std::fs::read_to_string(format!("{}.mode", log.display()))
            .unwrap()
            .trim(),
        "600"
    );
    let path = std::fs::read_to_string(format!("{}.path", log.display())).unwrap();
    d.settled_messages(&project, &conversation).await;
    assert!(
        !std::path::Path::new(path.trim()).exists(),
        "the run's MCP config was removed"
    );

    // Removing the server drops its token; signing out is idempotent.
    let res = put_servers(&d, &json!({"servers": [{"id": "files", "transport": "stdio", "command": bin, "env": {"MCP_ROOT": "docs"}}]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        !std::fs::read_to_string(&tokens_path)
            .unwrap()
            .contains("at-2")
    );
    let res = d
        .http
        .delete(format!("{}/api/mcp/servers/images/oauth", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
