//! Every provider against a loopback fake that speaks its wire protocol
//! (OpenAI Images, fal queue, ElevenLabs). No real provider, no network.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use base64::Engine;
use od_media::{ConfigFile, GenerateRequest, Limits, MediaError, Surface, generate};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-png";
const MP4: &[u8] = b"\0\0\0\x18ftypisomfake-mp4";
const MP3: &[u8] = b"ID3\x04fake-mp3";

#[derive(Default)]
struct Seen {
    /// (path, auth header, body)
    calls: Vec<(String, String, Value)>,
    polls: u32,
}

type Shared = Arc<Mutex<Seen>>;

fn record(seen: &Shared, path: &str, headers: &HeaderMap, body: &Bytes) {
    let auth = headers
        .get("authorization")
        .or_else(|| headers.get("xi-api-key"))
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let body = serde_json::from_slice(body).unwrap_or(Value::Null);
    seen.lock()
        .unwrap()
        .calls
        .push((path.to_owned(), auth, body));
}

async fn fake() -> (String, Shared) {
    let seen: Shared = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let b = base.clone();
    let app = Router::new()
        .route(
            "/v1/images/generations",
            post(|State(s): State<Shared>, h: HeaderMap, body: Bytes| async move {
                record(&s, "/v1/images/generations", &h, &body);
                if h.get("authorization").unwrap() != "Bearer sk-test" {
                    return (StatusCode::UNAUTHORIZED, r#"{"error":{"message":"bad key"}}"#).into_response();
                }
                let b64 = base64::engine::general_purpose::STANDARD.encode(PNG);
                axum::Json(json!({ "data": [{ "b64_json": b64 }] })).into_response()
            }),
        )
        .route(
            "/fal-ai/{*endpoint}",
            post({
                let b = b.clone();
                move |State(s): State<Shared>, Path(ep): Path<String>, h: HeaderMap, body: Bytes| async move {
                    record(&s, &format!("/fal-ai/{ep}"), &h, &body);
                    let id = if ep.starts_with("veo3") { "vid" } else { "img" };
                    axum::Json(json!({
                        "request_id": id,
                        "status_url": format!("{b}/requests/{id}/status"),
                        "response_url": format!("{b}/requests/{id}"),
                    }))
                }
            }),
        )
        .route(
            "/requests/{id}/status",
            get(|State(s): State<Shared>| async move {
                let mut seen = s.lock().unwrap();
                seen.polls += 1;
                let status = if seen.polls % 2 == 1 { "IN_PROGRESS" } else { "COMPLETED" };
                axum::Json(json!({ "status": status }))
            }),
        )
        .route(
            "/requests/{id}",
            get({
                let b = b.clone();
                move |Path(id): Path<String>| async move {
                    if id == "vid" {
                        axum::Json(json!({ "video": { "url": format!("{b}/files/clip.mp4") } }))
                    } else {
                        axum::Json(json!({ "images": [{ "url": format!("{b}/files/img.png") }] }))
                    }
                }
            }),
        )
        .route(
            "/files/{name}",
            get(|Path(name): Path<String>| async move {
                if name.ends_with(".mp4") { MP4.to_vec() } else { PNG.to_vec() }
            }),
        )
        .route(
            "/v1/text-to-speech/{voice}",
            post(|State(s): State<Shared>, Path(v): Path<String>, h: HeaderMap, body: Bytes| async move {
                record(&s, &format!("/v1/text-to-speech/{v}"), &h, &body);
                MP3.to_vec()
            }),
        )
        .route(
            "/v1/sound-generation",
            post(|State(s): State<Shared>, h: HeaderMap, body: Bytes| async move {
                record(&s, "/v1/sound-generation", &h, &body);
                MP3.to_vec()
            }),
        )
        .with_state(Arc::clone(&seen));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, seen)
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn limits() -> Limits {
    Limits {
        fal_poll_interval: Duration::from_millis(10),
        fal_max_poll: Duration::from_secs(5),
    }
}

fn req(v: Value) -> GenerateRequest {
    serde_json::from_value(v).unwrap()
}

#[tokio::test]
async fn providers_speak_their_protocols() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let config = ConfigFile::in_dir(tmp.path());
    config
        .write(&json!({ "providers": {
            "openai": { "apiKey": "sk-test", "baseUrl": format!("{base}/v1") },
            "fal": { "apiKey": "fal-test", "baseUrl": base },
            "elevenlabs": { "apiKey": "el-test", "baseUrl": base },
        } }))
        .unwrap();
    let http = od_media::provider::http_client().unwrap();

    // OpenAI: b64 image, extension from the bytes, size from the aspect.
    let g = generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "image", "model": "gpt-image-1", "prompt": "a fox", "aspect": "16:9", "output": "img/fox.jpg" })),
        limits(),
    )
    .await
    .unwrap();
    assert_eq!((g.name.as_str(), g.bytes.as_slice()), ("img/fox.png", PNG));
    let (path, auth, body) = seen.lock().unwrap().calls.last().cloned().unwrap();
    assert_eq!(path, "/v1/images/generations");
    assert_eq!(auth, "Bearer sk-test");
    assert_eq!(body["model"], "gpt-image-1");
    assert_eq!(body["size"], "1792x1024");
    assert_eq!(body["prompt"], "a fox");

    // fal image: submit, poll until COMPLETED, fetch result, download.
    let g = generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "image", "model": "flux-dev-fal", "prompt": "a hill" })),
        limits(),
    )
    .await
    .unwrap();
    assert_eq!(g.bytes, PNG);
    assert!(
        g.name.starts_with("media/flux-dev-fal-") && g.name.ends_with(".png"),
        "{}",
        g.name
    );
    let (path, auth, body) = seen.lock().unwrap().calls.last().cloned().unwrap();
    assert_eq!(
        (path.as_str(), auth.as_str()),
        ("/fal-ai/flux/dev", "Key fal-test")
    );
    assert_eq!(body["image_size"], "square_hd");
    assert!(seen.lock().unwrap().polls >= 2);

    // fal video: Veo snaps the duration to a bucket; result is an MP4.
    let g = generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "video", "model": "veo-3-fal", "prompt": "waves", "length": 7.5, "output": "clip" })),
        limits(),
    )
    .await
    .unwrap();
    assert_eq!((g.name.as_str(), g.bytes.as_slice()), ("clip.mp4", MP4));
    let (_, _, body) = seen.lock().unwrap().calls.last().cloned().unwrap();
    assert_eq!(body["duration"], "8s");

    // ElevenLabs speech and sound effects.
    let g = generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "audio", "model": "elevenlabs-v3", "prompt": "Xin chào", "voice": "v 1" })),
        limits(),
    )
    .await
    .unwrap();
    assert_eq!(g.bytes, MP3);
    assert!(g.name.ends_with(".mp3"));
    let (path, auth, body) = seen.lock().unwrap().calls.last().cloned().unwrap();
    assert_eq!(
        (path.as_str(), auth.as_str()),
        ("/v1/text-to-speech/v 1", "el-test")
    );
    assert_eq!(body["model_id"], "eleven_v3");
    assert_eq!(body["text"], "Xin chào");
    generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "audio", "model": "elevenlabs-sfx", "prompt": "door creak", "duration": 99 })),
        limits(),
    )
    .await
    .unwrap();
    let (path, _, body) = seen.lock().unwrap().calls.last().cloned().unwrap();
    assert_eq!(path, "/v1/sound-generation");
    assert_eq!(body["duration_seconds"], 30.0);
}

#[tokio::test]
async fn failures_are_reported_without_calling_out() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let config = ConfigFile::in_dir(tmp.path());
    let http = od_media::provider::http_client().unwrap();
    let image = req(json!({ "surface": "image", "model": "gpt-image-1", "prompt": "x" }));

    // No key anywhere: refused before any request.
    let err = generate(&http, &config, &no_env, &image, limits())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            MediaError::NoCredential {
                provider: "openai",
                ..
            }
        ),
        "{err}"
    );
    assert!(seen.lock().unwrap().calls.is_empty());

    // Wrong key: the upstream status is surfaced.
    config
        .write(&json!({ "providers": { "openai": { "apiKey": "sk-wrong", "baseUrl": format!("{base}/v1") } } }))
        .unwrap();
    let err = generate(&http, &config, &no_env, &image, limits())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("openai 401"), "{err}");

    let err = generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "video", "model": "gpt-image-1" })),
        limits(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, MediaError::BadRequest(_)), "{err}");
    let err = generate(
        &http,
        &config,
        &no_env,
        &req(json!({ "surface": "image", "model": "midjourney" })),
        limits(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("unknown media model"));
    assert_eq!(
        Surface::Image,
        req(json!({ "surface": "image", "model": "m" })).surface
    );
}
