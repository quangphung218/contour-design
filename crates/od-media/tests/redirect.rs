//! Provider keys never follow a redirect to another host; downloads (no
//! key) still may.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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
async fn provider_client_does_not_carry_keys_across_hosts() {
    let (other, other_seen) =
        server("HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into()).await;
    let (base, _) = server(format!(
        "HTTP/1.1 307 Temporary Redirect\r\nlocation: {other}/v1/text-to-speech/x\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    ))
    .await;
    let http = od_media::provider::http_client().unwrap();
    let _ = http
        .post(format!("{base}/v1/text-to-speech/x"))
        .header("xi-api-key", "fake-probe-key-0000")
        .body("{}")
        .send()
        .await;
    let seen = other_seen.lock().unwrap();
    assert!(
        seen.iter()
            .all(|head| !head.contains("fake-probe-key-0000")),
        "{seen:?}"
    );
}
