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
use od_server::routes::media::routes_with;
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
    };
    let frames = Arc::new(Frames::default());
    let app = routes_with(Arc::clone(&frames) as Arc<dyn Renderer>).with_state(state.clone());
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
    assert!(
        preview.headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );

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
