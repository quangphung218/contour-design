//! The BYOK key never follows a redirect to another host.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use od_runtime::byok::{ByokConfig, ByokProtocol, ByokRequest, stream};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves one canned response per connection; records request heads.
async fn server(response: String) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8192];
            let n = conn.read(&mut buf).await.unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_lowercase());
            let _ = conn.write_all(response.as_bytes()).await;
            let _ = conn.shutdown().await;
        }
    });
    (base, seen)
}

#[tokio::test]
async fn api_key_is_not_sent_to_a_redirect_target() {
    let (other, other_seen) =
        server("HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into()).await;
    let (base, _) = server(format!(
        "HTTP/1.1 307 Temporary Redirect\r\nlocation: {other}/v1/messages\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    ))
    .await;
    for protocol in [
        ByokProtocol::Anthropic,
        ByokProtocol::OpenAi,
        ByokProtocol::Azure,
    ] {
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let _ = stream(
            ByokRequest {
                config: ByokConfig {
                    protocol,
                    api_key: "fake-probe-key-0000".into(),
                    base_url: Some(base.clone()),
                    model: None,
                    max_tokens: None,
                    api_version: None,
                },
                model: None,
                prompt: "hi".into(),
            },
            tx,
        )
        .await;
    }
    let leaked: Vec<String> = other_seen
        .lock()
        .unwrap()
        .iter()
        .filter(|head| head.contains("fake-probe-key-0000"))
        .cloned()
        .collect();
    assert!(
        leaked.is_empty(),
        "key sent to the redirect target: {leaked:?}"
    );
}
