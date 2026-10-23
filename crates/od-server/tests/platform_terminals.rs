//! Terminal tabs over WebSocket (spec 1.5): a real shell in the project
//! folder, and every network input that must be refused.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn open(d: &common::Daemon, project: &str) -> Value {
    let res = d
        .post(
            &format!("/api/projects/{project}/terminals"),
            &json!({"cols": 90, "rows": 20}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    res.json().await.unwrap()
}

fn ws_url(d: &common::Daemon, tid: &str) -> String {
    format!(
        "{}/api/terminals/{tid}/ws",
        d.base.replace("http://", "ws://")
    )
}

async fn connect(d: &common::Daemon, tid: &str, origin: Option<&str>) -> Result<Ws, u16> {
    let mut req = ws_url(d, tid).into_client_request().unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert(header::ORIGIN, o.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(res)) => Err(res.status().as_u16()),
        Err(e) => panic!("ws error: {e}"),
    }
}

/// Reads frames until the decoded text contains `needle`.
async fn read_until(ws: &mut Ws, seen: &mut String, needle: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !seen.contains(needle) {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, ws.next()).await {
            Ok(Some(Ok(Message::Binary(b)))) => seen.push_str(&String::from_utf8_lossy(&b)),
            Ok(Some(Ok(Message::Text(t)))) => seen.push_str(&t),
            Ok(Some(Ok(_))) => {}
            _ => break,
        }
    }
    assert!(seen.contains(needle), "never saw {needle:?} in {seen:?}");
}

#[tokio::test]
async fn type_echo_reconnect_and_close() {
    let d = common::daemon().await;
    let (project, _) = d.project().await;
    let info = open(&d, &project).await;
    let tid = info["id"].as_str().unwrap().to_owned();
    assert_eq!(info["status"], "running");
    assert_eq!(
        (info["cols"].as_u64(), info["rows"].as_u64()),
        (Some(90), Some(20))
    );
    let dir = std::fs::canonicalize(info["cwd"].as_str().unwrap()).unwrap();
    assert!(
        dir.starts_with(std::fs::canonicalize(&d.data).unwrap()),
        "cwd is the project folder"
    );

    let mut ws = connect(&d, &tid, Some(&d.base)).await.unwrap();
    let mut seen = String::new();
    ws.send(Message::Text(
        json!({"type": "input", "data": "echo w3-$((20+22))\r"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    read_until(&mut ws, &mut seen, "w3-42").await;
    ws.send(Message::Text(
        json!({"type": "resize", "cols": 120, "rows": 40})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    // Binary frames are typed as-is; unknown text frames are ignored.
    ws.send(Message::Text("{\"type\":\"bogus\"}".into()))
        .await
        .unwrap();
    ws.send(Message::Binary(b"echo bin-$((1+1))\r".to_vec().into()))
        .await
        .unwrap();
    read_until(&mut ws, &mut seen, "bin-2").await;
    drop(ws);

    // Reattach (page reload): the scrollback replays.
    let mut again = connect(&d, &tid, None).await.unwrap();
    let mut replay = String::new();
    read_until(&mut again, &mut replay, "w3-42").await;
    let list = d
        .get_json(&format!("/api/projects/{project}/terminals"))
        .await;
    assert_eq!(list["terminals"][0]["cols"], 120);

    let res = d
        .http
        .delete(format!("{}/api/terminals/{tid}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    read_until(&mut again, &mut replay, "\"type\":\"exit\"").await;
    let res = d
        .http
        .delete(format!("{}/api/terminals/{tid}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(connect(&d, &tid, None).await.err(), Some(404));
}

#[tokio::test]
async fn foreign_origins_hosts_and_bodies_are_refused() {
    let d = common::daemon().await;
    let (project, _) = d.project().await;
    let tid = open(&d, &project).await["id"].as_str().unwrap().to_owned();
    let port = d.base.rsplit(':').next().unwrap().to_owned();

    // WebSocket upgrades from pages that are not the app.
    // A loopback page on another port (or the default one) is another site.
    for origin in [
        "https://evil.example",
        "null",
        &format!("http://192.168.1.5:{port}"),
        "http://localhost",
        "http://127.0.0.1",
        "http://localhost:1",
    ] {
        assert_eq!(
            connect(&d, &tid, Some(origin)).await.err(),
            Some(403),
            "{origin}"
        );
    }

    // DNS rebinding and LAN hosts, with and without an Origin.
    let url = format!("{}/api/projects/{project}/terminals", d.base);
    for (host, origin) in [
        ("evil.example", None),
        ("192.168.1.5", None),
        ("192.168.1.5", Some(format!("http://192.168.1.5:{port}"))),
    ] {
        let mut req = d
            .http
            .post(&url)
            .header(header::HOST, format!("{host}:{port}"))
            .json(&json!({}));
        if let Some(o) = &origin {
            req = req.header(header::ORIGIN, o);
        }
        assert_eq!(
            req.send().await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{host} {origin:?}"
        );
    }
    let res = d
        .http
        .post(&url)
        .header(header::ORIGIN, "https://evil.example")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Bad bodies and unknown ids.
    for body in [r#"{"cols":"wide"}"#, r#"{"shell":"/bin/bash"}"#, "not json"] {
        let res = d
            .http
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    let res = d.post("/api/projects/nope/terminals", &json!({})).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(connect(&d, "nope", None).await.err(), Some(404));

    // Not a loopback peer (or no peer info at all): refused even with a
    // loopback Host header.
    for peer in [Some("10.0.0.2:5000"), Some("192.168.1.9:5000"), None] {
        let mut req = Request::post(format!("/api/projects/{project}/terminals"))
            .header(header::HOST, format!("127.0.0.1:{port}"))
            .body(Body::empty())
            .unwrap();
        if let Some(p) = peer {
            req.extensions_mut()
                .insert(ConnectInfo(p.parse::<SocketAddr>().unwrap()));
        }
        let res = od_server::router(d.state.clone())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{peer:?}");
    }

    // A web origin allowed for the API (`OD_ALLOWED_ORIGINS`) still may not
    // open terminals: these routes take loopback origins only.
    let mut state = d.state.clone();
    let mut config = (*state.config).clone();
    config.allowed_origins = vec!["https://studio.example".to_owned()];
    state.config = std::sync::Arc::new(config);
    let req = |origin: &str| {
        let mut req = Request::get(format!("/api/projects/{project}/terminals"))
            .header(header::HOST, format!("127.0.0.1:{port}"))
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()));
        req
    };
    let res = od_server::router(state.clone())
        .oneshot(req("https://studio.example"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = od_server::router(state)
        .oneshot(req(&format!("http://127.0.0.1:{port}")))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "the app's own origin still works"
    );
    let res = d
        .http
        .delete(format!("{}/api/terminals/{tid}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn deleting_a_project_closes_its_terminals() {
    let d = common::daemon().await;
    let (project, _) = d.project().await;
    let (other, _) = d.project().await;
    let mine = open(&d, &project).await;
    let kept = open(&d, &other).await;
    let res = d
        .http
        .delete(format!("{}/api/projects/{project}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let tid = mine["id"].as_str().unwrap();
    assert_eq!(connect(&d, tid, None).await.err(), Some(404));
    let list = d
        .get_json(&format!("/api/projects/{other}/terminals"))
        .await;
    assert_eq!(list["terminals"][0]["id"], kept["id"]);
}
