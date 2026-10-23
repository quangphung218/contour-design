//! Track W2-media routes through the real handlers: HyperFrames MP4 (fake
//! frame renderer, real ffmpeg), live artifacts, plugins, media
//! generation against a loopback fake provider.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_contracts::CreateProjectRequest;
use od_export::hyperframes::frame_count;
use od_export::image::solid_png;
use od_export::renderer::frame_file_name;
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
use od_server::routes::media::{Deps, routes_with};
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// Writes solid frames whose red channel encodes the frame time; records
/// the options and target size it was called with.
#[derive(Default)]
struct Frames {
    seen: Mutex<Vec<(FrameOptions, u32, u32)>>,
}

impl Renderer for Frames {
    fn render_slides<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(async { Err(RenderError::Unsupported("slides")) })
    }
    fn render_frames<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: FrameOptions,
        out_dir: &'a Path,
    ) -> BoxFuture<'a, RenderResult<FrameSet>> {
        Box::pin(async move {
            self.seen
                .lock()
                .unwrap()
                .push((options, target.width, target.height));
            let count = u32::try_from(frame_count(options.duration_ms, options.fps)).unwrap();
            std::fs::create_dir_all(out_dir).unwrap();
            for i in 0..count {
                let png = solid_png(
                    target.width,
                    target.height,
                    [u8::try_from(i * 4).unwrap(), 0, 0],
                )
                .unwrap();
                std::fs::write(out_dir.join(frame_file_name(i)), png).unwrap();
            }
            Ok(FrameSet {
                count,
                fps: options.fps,
                width: target.width,
                height: target.height,
            })
        })
    }
    fn print_pdf<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(RenderError::Unsupported("pdf")) })
    }
    fn capture_page<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async { Err(RenderError::Unsupported("capture")) })
    }
}

pub struct Harness {
    pub tmp: tempfile::TempDir,
    pub state: AppState,
    frames: Arc<Frames>,
    app: Router,
}

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let content = content_root();
    let state = AppState {
        store: Arc::new(Store::open(DataDir::new(&data)).unwrap()),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(&content))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: content,
            daemon_url: None,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    };
    let frames = Arc::new(Frames::default());
    let deps = Deps {
        renderer: Arc::clone(&frames) as Arc<dyn Renderer>,
        // Never pick up a real key from the machine running the tests.
        env: Arc::new(|_: &str| None),
        limits: od_media::Limits {
            fal_poll_interval: std::time::Duration::from_millis(10),
            fal_max_poll: std::time::Duration::from_secs(5),
        },
    };
    let app = routes_with(deps).with_state(state.clone());
    Harness {
        tmp,
        state,
        frames,
        app,
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("json: {}", String::from_utf8_lossy(&self.body)))
    }
}

impl Harness {
    fn project(&self, name: &str) -> String {
        let (p, _) = self
            .state
            .store
            .create_project(CreateProjectRequest {
                name: name.to_owned(),
                ..CreateProjectRequest::default()
            })
            .unwrap();
        p.id
    }

    fn file(&self, project: &str, name: &str, body: impl AsRef<[u8]>) {
        self.state
            .store
            .write_file(project, name, body.as_ref(), true, None)
            .unwrap();
    }

    fn project_dir(&self, project: &str) -> PathBuf {
        let p = self.state.store.get_project(project).unwrap();
        self.state.store.project_root(&p).unwrap().dir
    }

    async fn send(&self, req: Request<Body>) -> Reply {
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    async fn post(&self, uri: &str, body: Value) -> Reply {
        self.post_with(uri, body, None).await
    }

    async fn post_with(&self, uri: &str, body: Value, bearer: Option<&str>) -> Reply {
        let mut req = Request::post(uri).header(header::CONTENT_TYPE, "application/json");
        if let Some(t) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        self.send(req.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn put(&self, uri: &str, body: Value) -> Reply {
        self.send(
            Request::put(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// HyperFrames MP4

const COMPOSITION: &str = r#"<!doctype html><html><body>
<div data-composition-id="main" data-width="321" data-height="181" data-fps="30" data-start="0" data-duration="2"></div>
<script>window.__timelines = { main: { seek() {} } };</script></body></html>"#;

fn ffprobe(mp4: &Path) -> Value {
    let ffmpeg = od_render_cdp::ffmpeg::find_ffmpeg().expect("ffmpeg is required");
    let out = std::process::Command::new(ffmpeg.with_file_name("ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,pix_fmt,width,height,nb_read_frames",
            "-of",
            "json",
        ])
        .arg(mp4)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["streams"][0].clone()
}

#[tokio::test]
async fn mp4_export_encodes_the_composition() {
    let h = harness();
    let id = h.project("Motion");
    h.file(&id, "comp/index.html", COMPOSITION);

    let res = h
        .post(
            &format!("/api/projects/{id}/export/mp4"),
            json!({ "fileName": "comp/index.html", "title": "Promo" }),
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    assert_eq!(res.headers[header::CONTENT_TYPE], "video/mp4");
    assert!(
        res.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("Promo.mp4")
    );
    let seen = h.frames.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![(
            FrameOptions {
                fps: 30,
                duration_ms: 2000
            },
            321,
            181
        )]
    );
    let mp4 = h.tmp.path().join("a.mp4");
    std::fs::write(&mp4, &res.body).unwrap();
    let s = ffprobe(&mp4);
    assert_eq!(s["codec_name"], "h264", "{s}");
    assert_eq!(s["pix_fmt"], "yuv420p", "{s}");
    assert_eq!(s["nb_read_frames"], "60", "{s}");
    assert_eq!(
        (s["width"].as_u64(), s["height"].as_u64()),
        (Some(322), Some(182))
    );

    // Saved into the project instead of downloaded.
    let res = h
        .post(
            &format!("/api/projects/{id}/export/mp4"),
            json!({ "fileName": "comp/index.html", "output": "renders/promo.mp4", "fps": 15 }),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let v = res.json();
    assert_eq!(v["file"]["name"], "renders/promo.mp4");
    assert_eq!(v["frames"], 30);
    let saved = h.project_dir(&id).join("renders/promo.mp4");
    assert_eq!(ffprobe(&saved)["nb_read_frames"], "30");
    // No scratch folders left behind.
    let scratch = h.state.store.data_dir().root().join("export-render");
    assert_eq!(std::fs::read_dir(scratch).unwrap().count(), 0);
}

#[tokio::test]
async fn mp4_export_rejects_non_compositions() {
    let h = harness();
    let id = h.project("Motion");
    h.file(&id, "plain.html", "<html><body>hi</body></html>");
    let res = h
        .post(
            &format!("/api/projects/{id}/export/mp4"),
            json!({ "fileName": "plain.html" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        res.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("data-composition-id")
    );
    let res = h
        .post(
            &format!("/api/projects/{id}/export/mp4"),
            json!({ "fileName": "plain.html", "output": "x.mov" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(h.frames.seen.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Live artifacts

fn live_input() -> Value {
    json!({
        "title": "Sales board",
        "document": {
            "dataJson": { "total": 1 },
            "sourceJson": {
                "type": "daemon_tool",
                "toolName": "project_files.read_json",
                "input": { "path": "sales.json" },
                "outputMapping": { "dataPaths": [{ "from": "json.total", "to": "total" }] },
                "refreshPermission": "manual_refresh_granted_for_read_only"
            }
        }
    })
}

#[tokio::test]
async fn live_artifacts_over_http() {
    let h = harness();
    let id = h.project("Live");

    // <script> is refused before anything is stored.
    let res = h
        .post(
            "/api/tools/live-artifacts/create",
            json!({ "projectId": id, "input": live_input(), "templateHtml": "<p>{{data.total}}</p><script>alert(1)</script>" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        res.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("script")
    );
    assert_eq!(
        h.get(&format!("/api/live-artifacts?projectId={id}"))
            .await
            .json()["artifacts"],
        json!([])
    );

    // A run's tool token decides the project and stamps the run id.
    let token = h
        .state
        .runs
        .tokens
        .issue("run-7", Some(&id), std::time::Duration::ZERO);
    let res = h
        .post_with(
            "/api/tools/live-artifacts/create",
            json!({ "input": live_input(), "templateHtml": "<p>Total: {{data.total}}</p>" }),
            Some(&token),
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    let artifact = res.json()["artifact"].clone();
    let aid = artifact["id"].as_str().unwrap().to_owned();
    assert_eq!(artifact["createdByRunId"], "run-7");
    assert_eq!(artifact["projectId"], id.as_str());
    // A token for one project cannot name another.
    let res = h
        .post_with(
            "/api/tools/live-artifacts/create",
            json!({ "projectId": "other", "input": live_input() }),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);

    let preview = h
        .get(&format!("/api/live-artifacts/{aid}/preview?projectId={id}"))
        .await;
    assert_eq!(preview.status, StatusCode::OK);
    assert_eq!(String::from_utf8_lossy(&preview.body), "<p>Total: 1</p>");
    let csp = preview.headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    // No scripts, and no loads off the daemon: `<img src="https://…?{{data}}">`
    // must not carry data out.
    for directive in [
        "sandbox;",
        "default-src 'none'",
        "img-src 'self' data: blob:",
        "script-src 'none'",
        "connect-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
    ] {
        assert!(csp.contains(directive), "{directive} in {csp}");
    }

    h.file(&id, "sales.json", r#"{"total": 1234}"#);
    let res = h
        .post(
            &format!("/api/live-artifacts/{aid}/refresh?projectId={id}"),
            json!({}),
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    assert_eq!(res.json()["refresh"]["status"], "succeeded");
    let dir = h.project_dir(&id).join(".live-artifacts").join(&aid);
    let data: Value =
        serde_json::from_slice(&std::fs::read(dir.join("data.json")).unwrap()).unwrap();
    assert_eq!(data, json!({ "total": 1234 }));
    let preview = h
        .get(&format!("/api/live-artifacts/{aid}/preview?projectId={id}"))
        .await;
    assert_eq!(String::from_utf8_lossy(&preview.body), "<p>Total: 1234</p>");
    let log = h
        .get(&format!(
            "/api/live-artifacts/{aid}/refreshes?projectId={id}"
        ))
        .await
        .json();
    assert_eq!(log["refreshes"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(dir.join("refreshes.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    // Tool refresh + update through the agent family.
    let res = h
        .post_with(
            "/api/tools/live-artifacts/refresh",
            json!({ "artifactId": aid }),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let res = h
        .post_with(
            "/api/tools/live-artifacts/update",
            json!({ "artifactId": aid, "input": { "title": "Sales" }, "templateHtml": "<iframe></iframe>" }),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    let list = h
        .get(&format!("/api/tools/live-artifacts/list?projectId={id}"))
        .await
        .json();
    assert_eq!(list["artifacts"][0]["title"], "Sales board");
    assert_eq!(list["artifacts"][0]["refreshStatus"], "succeeded");
    // The file API still refuses the reserved folder.
    assert!(h.state.store.read_file(&id, ".live-artifacts/x").is_err());
}

// ---------------------------------------------------------------------------
// Plugins

#[tokio::test]
async fn bundled_plugins_list_info_and_apply() {
    let h = harness();
    let all = h.get("/api/plugins").await.json();
    let ids: Vec<&str> = all["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"od-new-generation") && ids.contains(&"todo-write"));
    assert!(!ids.contains(&"od-default"), "hidden by default");
    let with_hidden = h
        .get("/api/plugins?includeHidden=1&kind=scenario")
        .await
        .json();
    let scenarios = with_hidden["plugins"].as_array().unwrap();
    assert_eq!(scenarios.len(), 10);
    assert!(scenarios.iter().all(|p| p["kind"] == "scenario"));

    let info = h.get("/api/plugins/od-media-generation").await;
    assert_eq!(info.status, StatusCode::OK);
    let info = info.json();
    assert!(info["plugin"]["body"].as_str().unwrap().len() > 100);
    assert_eq!(info["plugin"]["source"], "bundled");
    assert_eq!(
        h.get("/api/plugins/nope").await.status,
        StatusCode::NOT_FOUND
    );

    let res = h
        .post(
            "/api/plugins/od-new-generation/apply",
            json!({ "inputs": {} }),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    let res = h
        .post(
            "/api/plugins/od-new-generation/apply",
            json!({ "inputs": { "artifactKind": "dashboard", "audience": "ops", "topic": "latency" }, "locale": "vi" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let v = res.json();
    assert!(
        v["query"]
            .as_str()
            .unwrap()
            .starts_with("Generate a dashboard for ops on latency.")
    );
    assert_eq!(v["projectMetadata"]["taskKind"], "new-generation");
    let block = v["appliedPlugin"]["promptBlock"].as_str().unwrap();
    assert!(block.contains("## Plugin inputs") && block.contains("- **topic**: latency"));
}

// ---------------------------------------------------------------------------
// Media generation (loopback fake provider) and prompt templates

/// A fake OpenAI Images + ElevenLabs endpoint; records the auth header.
async fn fake_provider() -> (String, Arc<Mutex<Vec<String>>>) {
    use axum::routing::post;
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let s1 = Arc::clone(&seen);
    let s2 = Arc::clone(&seen);
    let app = Router::new()
        .route(
            "/v1/images/generations",
            post(move |h: axum::http::HeaderMap| async move {
                s1.lock().unwrap().push(h["authorization"].to_str().unwrap().to_owned());
                let png = solid_png(2, 2, [1, 2, 3]).unwrap();
                use base64::Engine;
                axum::Json(json!({ "data": [{ "b64_json": base64::engine::general_purpose::STANDARD.encode(png) }] }))
            }),
        )
        .route(
            "/v1/text-to-speech/{voice}",
            post(move |h: axum::http::HeaderMap| async move {
                s2.lock().unwrap().push(h["xi-api-key"].to_str().unwrap().to_owned());
                b"ID3fake".to_vec()
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, seen)
}

#[tokio::test]
async fn media_config_and_generation_write_into_the_project() {
    let h = harness();
    let id = h.project("Media");
    let (base, seen) = fake_provider().await;

    let cfg = h.get("/api/media/config").await.json();
    assert_eq!(cfg["providers"]["openai"]["configured"], false);
    let res = h
        .post(
            &format!("/api/projects/{id}/media/generate"),
            json!({ "surface": "image", "model": "gpt-image-1", "prompt": "a red square" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(
        res.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no openai API key")
    );
    assert!(seen.lock().unwrap().is_empty());

    let res = h
        .put(
            "/api/media/config",
            json!({ "providers": {
                "openai": { "apiKey": "sk-user-9876", "baseUrl": format!("{base}/v1") },
                "elevenlabs": { "apiKey": "el-user", "baseUrl": base },
            } }),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let masked = res.json();
    assert_eq!(masked["providers"]["openai"]["apiKeyTail"], "9876");
    assert!(!masked.to_string().contains("sk-user"));
    let on_disk =
        std::fs::read_to_string(h.state.store.data_dir().root().join("media-config.json")).unwrap();
    assert!(on_disk.contains("sk-user-9876"));

    let res = h
        .post(
            &format!("/api/projects/{id}/media/generate"),
            json!({ "surface": "image", "model": "gpt-image-1", "prompt": "a red square", "output": "assets/hero.webp" }),
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    let v = res.json();
    assert_eq!(v["name"], "assets/hero.png");
    assert_eq!(v["provider"], "openai");
    assert!(
        v["providerNote"]
            .as_str()
            .unwrap()
            .starts_with("openai/gpt-image-1")
    );
    let written = std::fs::read(h.project_dir(&id).join("assets/hero.png")).unwrap();
    assert!(written.starts_with(b"\x89PNG"));
    assert_eq!(seen.lock().unwrap().as_slice(), ["Bearer sk-user-9876"]);

    // The agent's tool route: project from the run token.
    let token = h
        .state
        .runs
        .tokens
        .issue("run-3", Some(&id), std::time::Duration::ZERO);
    let res = h
        .post_with(
            "/api/tools/media/generate",
            json!({ "surface": "audio", "model": "elevenlabs-v3", "prompt": "Chào anh", "output": "voice/intro" }),
            Some(&token),
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    assert_eq!(res.json()["name"], "voice/intro.mp3");
    assert_eq!(
        std::fs::read(h.project_dir(&id).join("voice/intro.mp3")).unwrap(),
        b"ID3fake"
    );

    let models = h.get("/api/media/models").await.json();
    let configured: Vec<(&str, bool)> = models["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap(),
                p["configured"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        configured,
        [("openai", true), ("fal", false), ("elevenlabs", true)]
    );
    assert!(models["models"].as_array().unwrap().len() >= 15);
    let res = h
        .post(
            "/api/projects/missing/media/generate",
            json!({ "surface": "image", "model": "gpt-image-1", "prompt": "x" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(
        seen.lock().unwrap().len(),
        2,
        "no provider call for a missing project"
    );
}

#[tokio::test]
async fn prompt_templates_carry_attribution() {
    let h = harness();
    let all = h.get("/api/prompt-templates").await.json();
    let list = all["promptTemplates"].as_array().unwrap();
    assert_eq!(
        list.len(),
        104,
        "106 upstream minus 2 without a clear license"
    );
    for t in list {
        assert!(t.get("prompt").is_none(), "list omits the prompt text");
        assert!(t.get("previewImageUrl").is_none() && t.get("previewVideoUrl").is_none());
        let license = t["source"]["license"].as_str().unwrap();
        assert!(
            ["CC-BY-4.0", "Apache-2.0", "MIT"].contains(&license),
            "{license}"
        );
    }
    assert!(
        !list
            .iter()
            .any(|t| t["id"] == "profile-avatar-seedream-high-fashion-dreamy-portrait")
    );
    let videos = h.get("/api/prompt-templates?surface=video").await.json();
    assert_eq!(videos["promptTemplates"].as_array().unwrap().len(), 58);
    let one = h
        .get("/api/prompt-templates/image/3d-stone-staircase-evolution-infographic")
        .await
        .json();
    assert!(one["promptTemplate"]["prompt"].as_str().unwrap().len() > 20);
    assert_eq!(
        h.get("/api/prompt-templates/image/..%2F..%2Fsecret")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
