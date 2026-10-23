//! Track C3 providers against one loopback fake that speaks each wire
//! protocol: Volcengine, xAI Grok, Nano Banana, ImageRouter, OpenRouter,
//! the Custom Image API, Leonardo, MiniMax, FishAudio, SenseAudio,
//! AIHubMix, OpenAI speech and Tavily, plus placeholders. No real
//! provider, no network beyond 127.0.0.1.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::post;
use base64::Engine;
use od_media::research::{ResearchRequest, search};
use od_media::{ConfigFile, GenerateRequest, Limits, MediaError, Surface, generate};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-png";
const JPG: &[u8] = b"\xFF\xD8\xFF\xE0fake-jpg";
const MP4: &[u8] = b"\0\0\0\x18ftypisomfake-mp4";
const MP3: &[u8] = b"ID3\x04fake-mp3";

#[derive(Default)]
struct Seen {
    /// (method path, headers, body)
    calls: Vec<(String, HashMap<String, String>, Value)>,
    polls: HashMap<String, u32>,
}

type Shared = Arc<Mutex<Seen>>;

#[derive(Clone)]
struct App {
    seen: Shared,
    base: String,
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn record(app: &App, method: &str, uri: &Uri, headers: &HeaderMap, body: &Bytes) -> u32 {
    let h = headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let body = serde_json::from_slice(body).unwrap_or(Value::Null);
    let key = format!("{method} {}", uri.path());
    let mut seen = app.seen.lock().unwrap();
    seen.calls.push((key.clone(), h, body));
    let n = seen.polls.entry(key).or_default();
    *n += 1;
    *n
}

/// Answers every provider route; GETs report "running" once, then done.
async fn handle(
    State(app): State<App>,
    uri: Uri,
    method: axum::http::Method,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let n = record(&app, method.as_str(), &uri, &headers, &body);
    let base = &app.base;
    let path = uri.path().to_owned();
    let auth = headers
        .get("authorization")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let json = |v: Value| axum::Json(v).into_response();
    let raw = |b: &'static [u8]| b.into_response();
    match (method.as_str(), path.as_str()) {
        ("POST", "/oa/audio/speech") => raw(MP3),
        ("POST", "/ark/images/generations") => json(json!({"data": [{"b64_json": b64(PNG)}]})),
        ("POST", "/ark/contents/generations/tasks") => json(json!({"id": "t1"})),
        ("GET", "/ark/contents/generations/tasks/t1") => {
            if n == 1 {
                json(json!({"status": "running"}))
            } else {
                json(
                    json!({"status": "succeeded", "content": {"video_url": format!("{base}/files/v.mp4")}}),
                )
            }
        }
        ("POST", "/ark-bad/contents/generations/tasks") => json(json!({"id": "bad"})),
        ("GET", "/ark-bad/contents/generations/tasks/bad") => {
            json(json!({"status": "failed", "error": {"message": "content policy"}}))
        }
        ("POST", "/xai/images/generations") => json(json!({"data": [{"b64_json": b64(JPG)}]})),
        ("POST", "/xai/videos/generations") => json(json!({"id": "g1", "status": "pending"})),
        ("GET", "/xai/videos/g1") => {
            if n == 1 {
                json(json!({"status": "pending"}))
            } else {
                json(json!({"status": "done", "video": {"url": format!("{base}/files/v.mp4")}}))
            }
        }
        ("POST", "/xai/tts") => raw(MP3),
        ("POST", p) if p.starts_with("/nb/v1beta/models/") => json(
            json!({"candidates": [{"content": {"parts": [{"text": "here"}, {"inlineData": {"mimeType": "image/png", "data": b64(PNG)}}]}}]}),
        ),
        ("POST", "/ir/images/generations") => {
            json(json!({"data": [{"b64_json": format!("data:image/png;base64,{}", b64(PNG))}]}))
        }
        ("POST", "/ir/videos/generations") => {
            json(json!({"data": [{"url": format!("{base}/files/v.mp4")}]}))
        }
        ("POST", "/or/chat/completions") => json(
            json!({"choices": [{"message": {"images": [{"image_url": {"url": format!("data:image/png;base64,{}", b64(PNG))}}]}}]}),
        ),
        ("POST", "/or/videos") => json(
            json!({"id": "j1", "polling_url": format!("{base}/or/videos/j1"), "status": "pending"}),
        ),
        ("GET", "/or/videos/j1") => {
            if n == 1 {
                json(json!({"status": "in_progress"}))
            } else {
                json(
                    json!({"status": "completed", "unsigned_urls": [format!("{base}/files/v.mp4")]}),
                )
            }
        }
        ("POST", "/custom/v1/images/generations") => {
            json(json!({"data": [{"b64_json": b64(PNG)}]}))
        }
        ("POST", "/leo/generations") => json(json!({"sdGenerationJob": {"generationId": "L1"}})),
        ("GET", "/leo/generations/L1") => {
            if n == 1 {
                json(json!({"generations_by_pk": {"status": "PENDING"}}))
            } else {
                json(
                    json!({"generations_by_pk": {"status": "COMPLETE", "generated_images": [{"url": format!("{base}/files/i.jpg")}]}}),
                )
            }
        }
        ("POST", "/mm/t2a_v2") => json(
            json!({"base_resp": {"status_code": 0}, "data": {"audio": hex::encode(MP3)}, "extra_info": {"audio_length": 1234}}),
        ),
        ("POST", "/mmimg/v1/image_generation") => {
            json(json!({"base_resp": {"status_code": 0}, "data": {"image_base64": [b64(PNG)]}}))
        }
        ("POST", "/fish/v1/tts") => raw(MP3),
        ("POST", "/sense/v1/t2a_v2") => {
            json(json!({"base_resp": {"status_code": 1004, "status_msg": "voice not found"}}))
        }
        ("POST", "/sense/v1/image/sync") => {
            let body: Value = serde_json::from_slice(&body).unwrap_or_default();
            // A gateway answering with an asset on another internal host.
            let url = if body["model"] == "senseaudio-image-1.0-260319" {
                "http://127.0.0.2:9/secret".to_owned()
            } else {
                format!("{base}/files/i.png")
            };
            json(json!({"url": url}))
        }
        ("POST", "/ahm/images/generations") => json(json!({"data": [{"b64_json": b64(PNG)}]})),
        ("POST", p) if p.starts_with("/gemini/v1beta/models/") => json(
            json!({"candidates": [{"content": {"parts": [{"inline_data": {"data": b64(PNG)}}]}}]}),
        ),
        ("POST", "/ahm/audio/speech") => raw(MP3),
        ("POST", "/ahm/videos") => json(json!({"id": "a1"})),
        ("GET", "/ahm/videos/a1") => json(json!({"status": "completed"})),
        ("GET", "/ahm/videos/a1/content") => {
            if auth == "Bearer ahm-key" {
                raw(MP4)
            } else {
                StatusCode::UNAUTHORIZED.into_response()
            }
        }
        ("POST", "/tav/search") => {
            let body: Value = serde_json::from_slice(&body).unwrap_or_default();
            if body["query"] == "nothing" {
                json(json!({"results": []}))
            } else {
                json(json!({"answer": "", "results": [
                    {"title": "  Doc  ", "url": "https://a.example/doc", "content": "Alpha", "published_date": "2026-10-01"},
                    {"url": "https://b.example/", "content": "Beta"},
                    {"title": "no url"}
                ]}))
            }
        }
        ("GET", "/files/v.mp4") => raw(MP4),
        ("GET", "/files/i.png") => raw(PNG),
        ("GET", "/files/i.jpg") => raw(JPG),
        _ => (StatusCode::NOT_FOUND, format!("no route {method} {path}")).into_response(),
    }
}

async fn fake() -> (String, Shared) {
    let seen: Shared = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = App {
        seen: Arc::clone(&seen),
        base: base.clone(),
    };
    let router = Router::new()
        .route("/{*rest}", post(handle).get(handle))
        .with_state(app);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, seen)
}

fn limits() -> Limits {
    Limits {
        fal_poll_interval: Duration::from_millis(10),
        fal_max_poll: Duration::from_secs(5),
        poll_interval: Duration::from_millis(10),
        allow_stubs: false,
    }
}

fn config(dir: &std::path::Path, base: &str) -> ConfigFile {
    let file = ConfigFile::in_dir(dir);
    let entry = |path: &str, key: &str| json!({"apiKey": key, "baseUrl": format!("{base}{path}")});
    file.write(&json!({"providers": {
        "openai": entry("/oa", "oa-key"),
        "volcengine": entry("/ark", "ark-key"),
        "grok": entry("/xai", "xai-key"),
        "nanobanana": entry("/nb", "nb-key"),
        "imagerouter": entry("/ir", "ir-key"),
        "openrouter": entry("/or", "or-key"),
        "custom-image": {"baseUrl": format!("{base}/custom/v1"), "model": "my-sdxl"},
        "leonardo": entry("/leo", "leo-key"),
        "minimax": entry("/mm", "mm-key"),
        "fishaudio": entry("/fish", "fish-key"),
        "senseaudio": entry("/sense", "sense-key"),
        "aihubmix": entry("/ahm", "ahm-key"),
        "tavily": entry("/tav", "tav-key"),
    }}))
    .unwrap();
    file
}

fn req(surface: &str, model: &str, extra: Value) -> GenerateRequest {
    let mut v =
        json!({"surface": surface, "model": model, "prompt": "a red fox", "output": "out/clip"});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    serde_json::from_value(v).unwrap()
}

fn call<'a>(seen: &'a Seen, key: &str) -> &'a (String, HashMap<String, String>, Value) {
    seen.calls
        .iter()
        .find(|(k, _, _)| k == key)
        .unwrap_or_else(|| {
            panic!(
                "no call {key}: {:?}",
                seen.calls.iter().map(|c| &c.0).collect::<Vec<_>>()
            )
        })
}

#[tokio::test]
async fn every_provider_speaks_its_protocol() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let file = config(tmp.path(), &base);
    let image_host = format!("{base}/mmimg");
    let lookup = move |k: &str| (k == "OD_MINIMAX_IMAGE_BASE_URL").then(|| image_host.clone());
    let http = od_media::provider::http_client().unwrap();
    let run = |r: GenerateRequest| {
        let (http, file, lookup) = (&http, &file, &lookup);
        async move { generate(http, file, lookup, &r, limits()).await }
    };

    // OpenAI speech: a style note is not a voice.
    let g = run(req(
        "audio",
        "gpt-4o-mini-tts",
        json!({"voice": "warm and slow"}),
    ))
    .await
    .unwrap();
    assert_eq!(
        (g.bytes.as_slice(), g.name.as_str(), g.provider),
        (MP3, "out/clip.mp3", "openai")
    );
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /oa/audio/speech");
        assert_eq!(h["authorization"], "Bearer oa-key");
        assert_eq!(
            (b["voice"].clone(), b["instructions"].clone()),
            (json!("alloy"), json!("warm and slow"))
        );
        assert_eq!(b["input"], "a red fox");
    }

    // Volcengine: Seedream image, Seedance task with prompt flags.
    let g = run(req(
        "image",
        "doubao-seedream-3-0-t2i-250415",
        json!({"aspect": "16:9"}),
    ))
    .await
    .unwrap();
    assert_eq!(g.bytes, PNG);
    let g = run(req(
        "video",
        "doubao-seedance-2-0-260128",
        json!({"aspect": "9:16", "length": 8}),
    ))
    .await
    .unwrap();
    assert_eq!((g.bytes.as_slice(), g.name.as_str()), (MP4, "out/clip.mp4"));
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /ark/contents/generations/tasks");
        assert_eq!(h["authorization"], "Bearer ark-key");
        assert_eq!(
            b["content"][0]["text"],
            "a red fox --resolution 720p --duration 8 --ratio 9:16"
        );
        assert_eq!(
            s.polls["GET /ark/contents/generations/tasks/t1"], 2,
            "polled until succeeded"
        );
        // openaiSizeFor only knows gpt-image-* and dall-e-3 sizes.
        assert_eq!(
            call(&s, "POST /ark/images/generations").2["size"],
            "1024x1024"
        );
    }

    // xAI: JPEG sniffed, async video polled, TTS voice default.
    let g = run(req("image", "grok-imagine-image", json!({})))
        .await
        .unwrap();
    assert_eq!((g.bytes.as_slice(), g.name.as_str()), (JPG, "out/clip.jpg"));
    let g = run(req("video", "grok-imagine-video", json!({"length": 30})))
        .await
        .unwrap();
    assert_eq!(g.bytes, MP4);
    let g = run(req("audio", "grok-tts", json!({}))).await.unwrap();
    assert_eq!(g.bytes, MP3);
    {
        let s = seen.lock().unwrap();
        assert_eq!(
            call(&s, "POST /xai/videos/generations").2["duration"],
            15.0,
            "clamped to 15 s"
        );
        assert_eq!(call(&s, "POST /xai/tts").2["voice_id"], "eve");
    }

    // Nano Banana through a gateway: bearer, not x-goog-api-key.
    let g = run(req(
        "image",
        "gemini-3.1-flash-image-preview",
        json!({"aspect": "4:3"}),
    ))
    .await
    .unwrap();
    assert_eq!(g.bytes, PNG);
    {
        let s = seen.lock().unwrap();
        let (k, h, b) = s
            .calls
            .iter()
            .find(|c| c.0.starts_with("POST /nb/"))
            .unwrap();
        assert_eq!(
            k,
            "POST /nb/v1beta/models/gemini-3.1-flash-image-preview:generateContent"
        );
        assert_eq!(h["authorization"], "Bearer nb-key");
        assert!(!h.contains_key("x-goog-api-key"));
        assert_eq!(b["generationConfig"]["imageConfig"]["aspectRatio"], "4:3");
    }

    // ImageRouter: data-URL b64 image; video by URL.
    let g = run(req("image", "openai/gpt-image-2", json!({})))
        .await
        .unwrap();
    assert_eq!(g.bytes, PNG);
    let g = run(req("video", "google/veo-3.1-lite", json!({})))
        .await
        .unwrap();
    assert_eq!(g.bytes, MP4);
    assert_eq!(
        call(&seen.lock().unwrap(), "POST /ir/videos/generations").2["seconds"],
        "auto"
    );

    // OpenRouter: chat-completions image, video job with resolution suffix.
    let g = run(req(
        "image",
        "openrouter/google/gemini-2.5-flash-image",
        json!({}),
    ))
    .await
    .unwrap();
    assert_eq!(g.bytes, PNG);
    let g = run(req(
        "video",
        "openrouter/bytedance/seedance-2.0:1080p",
        json!({}),
    ))
    .await
    .unwrap();
    assert_eq!(g.bytes, MP4);
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /or/chat/completions");
        assert_eq!(b["model"], "google/gemini-2.5-flash-image");
        assert_eq!(b["modalities"], json!(["image", "text"]));
        assert_eq!(h["x-title"], "OpenDesign");
        let (_, _, v) = call(&s, "POST /or/videos");
        assert_eq!(
            (v["model"].clone(), v["resolution"].clone()),
            (json!("bytedance/seedance-2.0"), json!("1080p"))
        );
        // The CDN download never carries the key.
        let dl = s
            .calls
            .iter()
            .filter(|c| c.0 == "GET /files/v.mp4")
            .next_back()
            .unwrap();
        assert!(!dl.1.contains_key("authorization"));
    }

    // Custom Image API: no key needed, the configured model.
    let g = run(req("image", "custom-image", json!({}))).await.unwrap();
    assert_eq!((g.bytes.as_slice(), g.provider), (PNG, "custom-image"));
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /custom/v1/images/generations");
        assert!(!h.contains_key("authorization"));
        assert_eq!(b["model"], "my-sdxl");
    }

    // Leonardo: platform model id, contrast for Phoenix, polled result.
    let g = run(req("image", "leonardo-phoenix", json!({"aspect": "9:16"})))
        .await
        .unwrap();
    assert_eq!((g.bytes.as_slice(), g.name.as_str()), (JPG, "out/clip.jpg"));
    {
        let s = seen.lock().unwrap();
        let (_, _, b) = call(&s, "POST /leo/generations");
        assert_eq!(b["modelId"], "6b645e3a-d64f-4341-a6d8-7a3690fbf042");
        assert_eq!(
            (
                b["width"].clone(),
                b["height"].clone(),
                b["contrast"].clone()
            ),
            (json!(768), json!(1344), json!(3.5))
        );
    }

    // MiniMax: hex speech; image on its own host.
    let g = run(req("audio", "minimax-tts", json!({}))).await.unwrap();
    assert_eq!(g.bytes, MP3);
    assert!(g.note.contains("1.2s"), "{}", g.note);
    let g = run(req("image", "minimax-image-01", json!({"aspect": "21:9"})))
        .await
        .unwrap();
    assert_eq!(g.bytes, PNG);
    {
        let s = seen.lock().unwrap();
        assert_eq!(call(&s, "POST /mm/t2a_v2").2["model"], "speech-02-turbo");
        assert_eq!(
            call(&s, "POST /mmimg/v1/image_generation").2["aspect_ratio"],
            "21:9"
        );
        assert_eq!(
            call(&s, "POST /mmimg/v1/image_generation").1["authorization"],
            "Bearer mm-key"
        );
    }

    // FishAudio: the voice is a reference id.
    let g = run(req("audio", "fish-speech-2", json!({"voice": "ref-42"})))
        .await
        .unwrap();
    assert_eq!(g.bytes, MP3);
    assert_eq!(
        call(&seen.lock().unwrap(), "POST /fish/v1/tts").2["reference_id"],
        "ref-42"
    );

    // SenseAudio: a 200 with a failing base_resp is an error; image by URL.
    let e = run(req("audio", "senseaudio-tts", json!({})))
        .await
        .unwrap_err();
    assert!(
        e.to_string().contains("api error 1004: voice not found"),
        "{e}"
    );
    let g = run(req("image", "senseaudio-image-2.0-260319", json!({})))
        .await
        .unwrap();
    assert_eq!(g.bytes, PNG);

    // AIHubMix: OpenAI route with the app code, Gemini-native route,
    // speech, and video through /content.
    let g = run(req("image", "aihubmix-gpt-image-1", json!({})))
        .await
        .unwrap();
    assert_eq!(g.bytes, PNG);
    let g = run(req("audio", "aihubmix-tts-1", json!({"voice": "nova"})))
        .await
        .unwrap();
    assert_eq!(g.bytes, MP3);
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /ahm/images/generations");
        assert_eq!(
            (h["app-code"].as_str(), b["model"].clone()),
            ("DMCY9912", json!("gpt-image-1"))
        );
        assert_eq!(call(&s, "POST /ahm/audio/speech").2["voice"], "nova");
    }
}

#[tokio::test]
async fn guards_overrides_and_placeholders() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let file = config(tmp.path(), &base);
    let no_env = |_: &str| None;
    let http = od_media::provider::http_client().unwrap();

    // An asset URL on another internal host is never fetched.
    let e = generate(
        &http,
        &file,
        &no_env,
        &req("image", "senseaudio-image-1.0-260319", json!({})),
        limits(),
    )
    .await
    .unwrap_err();
    assert!(e.to_string().contains("internal address"), "{e}");

    // A failed task surfaces its reason.
    let mut l = limits();
    l.poll_interval = Duration::from_millis(5);
    let e = od_media::extra::render(
        &http,
        od_media::find_model("doubao-seedance-2-0-260128", Surface::Video).unwrap(),
        &od_media::Job {
            surface: Surface::Video,
            model: "x".into(),
            prompt: String::new(),
            aspect: None,
            length: None,
            duration: None,
            voice: None,
        },
        &od_media::Credentials {
            api_key: Some("k".into()),
            base_url: Some(format!("{base}/ark-bad")),
            model: None,
            source: od_media::KeySource::Stored,
        },
        l,
    )
    .await
    .unwrap_err();
    assert!(
        e.to_string()
            .contains("volcengine task failed: content policy"),
        "{e}"
    );

    // A Custom Image API naming an OpenAI model takes it over.
    file.write(&json!({"providers": {
        "openai": {"apiKey": "oa-key", "baseUrl": format!("{base}/oa")},
        "custom-image": {"baseUrl": format!("{base}/custom/v1"), "model": "gpt-image-1"},
    }}))
    .unwrap();
    let g = generate(
        &http,
        &file,
        &no_env,
        &req("image", "gpt-image-1", json!({})),
        limits(),
    )
    .await
    .unwrap();
    assert_eq!(g.provider, "custom-image");
    assert!(
        seen.lock()
            .unwrap()
            .calls
            .iter()
            .all(|c| !c.0.starts_with("POST /oa/images"))
    );

    // No key: an error, or a labelled placeholder with stubs on.
    file.write(&json!({"providers": {}})).unwrap();
    let e = generate(
        &http,
        &file,
        &no_env,
        &req("image", "grok-imagine-image", json!({})),
        limits(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            e,
            MediaError::NoCredential {
                provider: "grok",
                env: "XAI_API_KEY"
            }
        ),
        "{e}"
    );
    let stubs = Limits {
        allow_stubs: true,
        ..limits()
    };
    let g = generate(
        &http,
        &file,
        &no_env,
        &req("image", "grok-imagine-image", json!({})),
        stubs,
    )
    .await
    .unwrap();
    assert!(
        g.note.starts_with("[stub] [grok error → stub]"),
        "{}",
        g.note
    );
    assert_eq!(g.name, "out/clip.png");

    // A model whose provider has no renderer: placeholder only with stubs.
    let e = generate(
        &http,
        &file,
        &no_env,
        &req("video", "kling-2.0", json!({})),
        limits(),
    )
    .await
    .unwrap_err();
    assert!(e.to_string().contains("provider not configured"), "{e}");
    let g = generate(
        &http,
        &file,
        &no_env,
        &req("video", "kling-2.0", json!({})),
        stubs,
    )
    .await
    .unwrap();
    assert_eq!((g.provider, g.name.as_str()), ("kling", "out/clip.mp4"));
    assert!(g.note.contains("integration pending"));
    assert!(od_media::stubs_allowed(&|k: &str| (k
        == "OD_MEDIA_ALLOW_STUBS")
        .then(|| "true".to_owned())));
    assert!(!od_media::stubs_allowed(&no_env));
}

#[tokio::test]
async fn aihubmix_video_and_gemini_routes() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let file = ConfigFile::in_dir(tmp.path());
    // Custom ids (`supportsCustomModel`) through the stored model.
    file.write(&json!({"providers": {"aihubmix": {"apiKey": "ahm-key", "baseUrl": format!("{base}/ahm"), "model": "sora-2"}}}))
        .unwrap();
    let no_env = |_: &str| None;
    let http = od_media::provider::http_client().unwrap();
    let def = od_media::ModelDef {
        id: "aihubmix-video",
        label: "video",
        provider: od_media::Provider::AiHubMix,
        surface: Surface::Video,
        wire: "sora-2",
    };
    let creds = file.credentials(od_media::Provider::AiHubMix, &no_env);
    let job = |surface| od_media::Job {
        surface,
        model: "m".into(),
        prompt: "p".into(),
        aspect: Some("9:16".into()),
        length: Some(10.0),
        duration: None,
        voice: None,
    };
    let r = od_media::extra::render(&http, def, &job(Surface::Video), &creds, limits())
        .await
        .unwrap();
    assert_eq!(r.bytes, MP4);
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /ahm/videos");
        assert_eq!(
            (b["seconds"].clone(), b["size"].clone()),
            (json!("8"), json!("720x1280"))
        );
        assert_eq!(h["app-code"], "DMCY9912");
    }
    let gemini = od_media::ModelDef {
        surface: Surface::Image,
        wire: "gemini-2.5-flash-image",
        ..def
    };
    let creds = od_media::Credentials {
        model: None,
        ..creds
    };
    let r = od_media::extra::render(&http, gemini, &job(Surface::Image), &creds, limits())
        .await
        .unwrap();
    assert_eq!(r.bytes, PNG);
    let s = seen.lock().unwrap();
    let (k, h, b) = s
        .calls
        .iter()
        .find(|c| c.0.starts_with("POST /gemini/"))
        .unwrap();
    assert_eq!(
        k,
        "POST /gemini/v1beta/models/gemini-2.5-flash-image:generateContent"
    );
    assert_eq!(h["x-goog-api-key"], "ahm-key");
    assert_eq!(b["generationConfig"]["imageConfig"]["aspectRatio"], "9:16");
}

#[tokio::test]
async fn tavily_research() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let file = config(tmp.path(), &base);
    let no_env = |_: &str| None;
    let http = od_media::provider::http_client().unwrap();
    let ask = |query: &str, max: Option<f64>| ResearchRequest {
        query: query.to_owned(),
        max_sources: max,
        providers: Vec::new(),
    };
    let found = search(&http, &file, &no_env, &ask("  fox habitats ", Some(50.0)))
        .await
        .unwrap();
    assert_eq!(found.query, "fox habitats");
    assert_eq!(found.sources.len(), 2, "a result without url is dropped");
    assert_eq!(found.sources[0].title, "Doc");
    assert_eq!(found.sources[0].published_at.as_deref(), Some("2026-10-01"));
    assert_eq!(
        found.sources[1].title, "https://b.example/",
        "the url stands in for a title"
    );
    assert!(
        found.summary.starts_with("(No provider summary"),
        "{}",
        found.summary
    );
    {
        let s = seen.lock().unwrap();
        let (_, h, b) = call(&s, "POST /tav/search");
        assert_eq!(h["authorization"], "Bearer tav-key");
        assert_eq!(
            (b["max_results"].clone(), b["search_depth"].clone()),
            (json!(20), json!("basic"))
        );
    }
    let e = search(&http, &file, &no_env, &ask("nothing", None))
        .await
        .unwrap_err();
    assert_eq!((e.status, e.code), (404, "NO_RESEARCH_SOURCES"));
    let e = search(&http, &file, &no_env, &ask("  ", None))
        .await
        .unwrap_err();
    assert_eq!(e.code, "QUERY_REQUIRED");
    let other = ResearchRequest {
        providers: vec!["exa".into()],
        ..ask("q", None)
    };
    assert_eq!(
        search(&http, &file, &no_env, &other)
            .await
            .unwrap_err()
            .code,
        "UNSUPPORTED_RESEARCH_PROVIDER"
    );
    file.write(&json!({"providers": {}})).unwrap();
    assert_eq!(
        search(&http, &file, &no_env, &ask("q", None))
            .await
            .unwrap_err()
            .code,
        "TAVILY_API_KEY_MISSING"
    );
}
