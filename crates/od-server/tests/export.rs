//! Export and active-context routes through the real handlers, with a fake
//! renderer (the Chrome path is covered in `od-render-cdp/tests/chrome.rs`).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_contracts::CreateProjectRequest;
use od_export::image::{png_size, solid_png};
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, ImageFormat, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
use od_server::routes::export::routes_with;
use od_server::{AppState, ServerConfig};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::io::Read;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// Renders `.slide` count from the bundled HTML it is handed; records it.
#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<String>>,
    /// File name of each page it loaded (Chrome titles a PDF after it).
    names: Mutex<Vec<String>>,
    unavailable: bool,
}

impl Fake {
    fn load(&self, target: &RenderTarget) -> RenderResult<usize> {
        if self.unavailable {
            return Err(RenderError::Unavailable("this export needs Chrome".into()));
        }
        let html = std::fs::read_to_string(&target.html_path).unwrap();
        let name = target.html_path.file_name().unwrap().to_string_lossy();
        self.names.lock().unwrap().push(name.into_owned());
        let slides = html.matches("class=\"slide\"").count();
        self.seen.lock().unwrap().push(html);
        Ok(slides)
    }
}

impl Renderer for Fake {
    fn render_slides<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(async move {
            let n = self.load(target)?;
            if mode == DeckMode::Deck && n == 0 {
                return Err(RenderError::NotADeck);
            }
            let deck = n > 0 && mode != DeckMode::Page;
            let count = if deck { n } else { 2 };
            Ok(Slides {
                deck,
                width: target.width,
                height: target.height,
                images: (0..count)
                    .map(|i| {
                        solid_png(
                            target.width,
                            target.height,
                            [u8::try_from(i * 40).unwrap(), 0, 0],
                        )
                        .unwrap()
                    })
                    .collect(),
            })
        })
    }
    fn render_frames<'a>(
        &'a self,
        _: &'a RenderTarget,
        _: FrameOptions,
        _: &'a std::path::Path,
    ) -> BoxFuture<'a, RenderResult<od_export::FrameSet>> {
        Box::pin(async { Err(RenderError::Unsupported("frames")) })
    }
    fn print_pdf<'a>(
        &'a self,
        target: &'a RenderTarget,
        _: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async move {
            let n = self.load(target)?.max(1);
            let pages: Vec<Vec<u8>> = (0..n).map(|_| solid_png(16, 9, [0; 3]).unwrap()).collect();
            Ok(od_export::pdf::pngs_to_pdf(&pages).unwrap())
        })
    }
    fn capture_page<'a>(
        &'a self,
        target: &'a RenderTarget,
        opts: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(async move {
            self.load(target)?;
            // Chrome answers in the asked format; a WebP header stands in.
            Ok(match opts.format {
                ImageFormat::Webp => b"RIFF\0\0\0\0WEBPVP8 ".to_vec(),
                _ => solid_png(target.width, 2_000, [1, 2, 3]).unwrap(),
            })
        })
    }
}

struct Harness {
    _tmp: tempfile::TempDir,
    state: AppState,
    fake: Arc<Fake>,
    app: Router,
}

fn harness_with(fake: Fake) -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let empty = tmp.path().join("content");
    let state = AppState {
        store: Arc::new(Store::open(DataDir::new(&data)).unwrap()),
        content: Arc::new(ContentStore::load(ContentRoots::bundled(&empty))),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: empty,
            daemon_url: None,
            mcp_daemon_url: true,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    };
    let fake = Arc::new(fake);
    let app = routes_with(Arc::clone(&fake) as Arc<dyn Renderer>).with_state(state.clone());
    Harness {
        _tmp: tmp,
        state,
        fake,
        app,
    }
}

fn harness() -> Harness {
    harness_with(Fake::default())
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("json: {}", String::from_utf8_lossy(&self.body)))
    }
    fn header(&self, name: impl axum::http::header::AsHeaderName) -> &str {
        self.headers.get(name).unwrap().to_str().unwrap()
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
        self.send(
            Request::post(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

const DECK: &str = r#"<!doctype html><html><head><link rel="stylesheet" href="deck.css"></head><body>
<section class="slide"><img src="img/a.png"></section>
<section class="slide"></section>
<section class="slide"></section>
<section class="slide"></section>
<section class="slide"></section>
</body></html>"#;

fn deck_project(h: &Harness) -> String {
    let id = h.project("Bản đồ deck");
    h.file(&id, "index.html", DECK);
    h.file(&id, "deck.css", "body{background:url(img/a.png)}");
    h.file(&id, "img/a.png", solid_png(4, 4, [9, 9, 9]).unwrap());
    id
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let mut names: Vec<String> = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .unwrap()
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn archive_streams_project_and_subfolder() {
    let h = harness();
    let id = deck_project(&h);
    h.file(&id, "docs/readme.md", "# hi");

    let r = h.get(&format!("/api/projects/{id}/archive")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header(header::CONTENT_TYPE), "application/zip");
    assert!(
        r.header(header::CONTENT_DISPOSITION)
            .contains("filename*=UTF-8''B%E1%BA%A3n")
    );
    // The hidden .open-design manifest stays out.
    assert_eq!(
        zip_names(&r.body),
        ["deck.css", "docs/readme.md", "img/a.png", "index.html"]
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&r.body[..])).unwrap();
    let mut html = String::new();
    zip.by_name("index.html")
        .unwrap()
        .read_to_string(&mut html)
        .unwrap();
    assert_eq!(html, DECK);

    let r = h
        .get(&format!("/api/projects/{id}/archive?root=docs"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.header(header::CONTENT_DISPOSITION)
            .starts_with("attachment; filename=\"docs.zip\"")
    );
    assert_eq!(zip_names(&r.body), ["readme.md"]);

    assert_eq!(
        h.get(&format!("/api/projects/{id}/archive?root=nope"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.get(&format!("/api/projects/{id}/archive?root=../x"))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.get("/api/projects/missing/archive").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn html_export_is_self_contained() {
    let h = harness();
    let id = deck_project(&h);
    let r = h
        .post(
            &format!("/api/projects/{id}/export/html"),
            json!({ "fileName": "index.html" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    assert_eq!(r.header(header::CONTENT_TYPE), "text/html; charset=utf-8");
    assert_eq!(
        r.header(header::CONTENT_SECURITY_POLICY),
        "sandbox allow-scripts"
    );
    assert_eq!(r.header("x-open-design-external-dependencies"), "0");
    assert!(
        r.header(header::CONTENT_DISPOSITION)
            .contains("filename=\"index.html\"")
    );
    let html = String::from_utf8(r.body).unwrap();
    assert!(
        !html.contains("img/a.png") && !html.contains("deck.css"),
        "{html}"
    );
    assert!(html.contains(r#"href="data:text/css;charset=utf-8;base64,"#));

    // Without fileName the project's entryFile is used; none set here.
    let r = h
        .post(&format!("/api/projects/{id}/export/html"), json!({}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn html_export_errors_map_to_statuses() {
    let h = harness();
    let id = deck_project(&h);
    // Entry over 2 MiB → 413 with the limit named.
    h.file(
        &id,
        "big.html",
        format!("<p>{}</p>", "x".repeat(2 * 1024 * 1024)),
    );
    let r = h
        .post(
            &format!("/api/projects/{id}/export/html"),
            json!({ "fileName": "big.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(r.json()["error"]["code"], "PAYLOAD_TOO_LARGE");
    assert_eq!(
        r.json()["error"]["details"],
        json!({ "kind": "limit-exceeded", "limit": "entryBytes" })
    );
    // Missing dependency → 422.
    h.file(&id, "broken.html", r#"<img src="gone.png">"#);
    let r = h
        .post(
            &format!("/api/projects/{id}/export/html"),
            json!({ "fileName": "broken.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.json()["error"]["details"]["dependency"], "gone.png");
    // Escaping the project → 400.
    h.file(&id, "escape.html", r#"<img src="../../x.png">"#);
    let r = h
        .post(
            &format!("/api/projects/{id}/export/html"),
            json!({ "fileName": "escape.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // Not HTML → 415; missing file → 404; missing project → 404.
    let r = h
        .post(
            &format!("/api/projects/{id}/export/html"),
            json!({ "fileName": "deck.css" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let r = h
        .post(
            &format!("/api/projects/{id}/export/html"),
            json!({ "fileName": "nope.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = h
        .post(
            "/api/projects/missing/export/html",
            json!({ "fileName": "index.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.json()["error"]["code"], "PROJECT_NOT_FOUND");
}

/// Chrome titles a printed PDF after the page's file name, so the page is
/// rendered under the exported file's name, not a generic `index.html`.
#[tokio::test]
async fn a_pdf_is_rendered_under_its_file_name() {
    let h = harness();
    let id = h.project("Site");
    h.file(&id, "landing.html", "<!doctype html><p>Hi</p>");

    let r = h
        .post(
            &format!("/api/projects/{id}/export/pdf"),
            json!({ "fileName": "landing.html" }),
        )
        .await;

    assert_eq!(
        r.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    assert_eq!(*h.fake.names.lock().unwrap(), ["landing.html"]);
}

#[tokio::test]
async fn visual_exports_assemble_renderer_output() {
    let h = harness();
    let id = deck_project(&h);
    let body = json!({ "fileName": "index.html", "title": "Q3 deck" });

    let r = h
        .post(
            &format!("/api/projects/{id}/export/pdf-image"),
            body.clone(),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    assert_eq!(r.header(header::CONTENT_TYPE), "application/pdf");
    assert!(
        r.header(header::CONTENT_DISPOSITION)
            .contains("filename=\"Q3 deck.pdf\"")
    );
    assert_eq!(
        lopdf::Document::load_mem(&r.body)
            .unwrap()
            .get_pages()
            .len(),
        5
    );

    let r = h
        .post(&format!("/api/projects/{id}/export/pdf"), body.clone())
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        lopdf::Document::load_mem(&r.body)
            .unwrap()
            .get_pages()
            .len(),
        5
    );

    let r = h
        .post(&format!("/api/projects/{id}/export/pptx"), body.clone())
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header(header::CONTENT_TYPE), od_export::pptx::PPTX_MIME);
    assert_eq!(od_export::pptx::count_slides(&r.body).unwrap(), (5, 5));

    let r = h
        .post(
            &format!("/api/projects/{id}/export/image"),
            json!({ "fileName": "index.html", "index": 2 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header(header::CONTENT_TYPE), "image/png");
    assert_eq!(png_size(&r.body), Some((1920, 1080)));
    // No index: the deck stitched top to bottom.
    let r = h
        .post(
            &format!("/api/projects/{id}/export/image"),
            json!({ "fileName": "index.html" }),
        )
        .await;
    assert_eq!(png_size(&r.body), Some((1920, 5 * 1080)));
    let r = h
        .post(
            &format!("/api/projects/{id}/export/image"),
            json!({ "fileName": "index.html", "index": 9 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // The renderer was handed a bundle with nothing left to fetch.
    let seen = h.fake.seen.lock().unwrap();
    assert!(!seen.is_empty());
    assert!(
        seen.iter()
            .all(|html| !html.contains("img/a.png") && !html.contains("\"deck.css\""))
    );
    drop(seen);
    // Scratch folders are removed after each export.
    let scratch = h.state.store.data_dir().root().join("export-render");
    assert_eq!(std::fs::read_dir(scratch).unwrap().count(), 0);
}

#[tokio::test]
async fn pptx_needs_a_deck_and_renderer_errors_are_clear() {
    let h = harness();
    let id = h.project("page");
    h.file(&id, "page.html", "<h1>not a deck</h1>");
    let r = h
        .post(
            &format!("/api/projects/{id}/export/pptx"),
            json!({ "fileName": "page.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not a slide deck")
    );
    // A page renders as a full-page image.
    let r = h
        .post(
            &format!("/api/projects/{id}/export/image"),
            json!({ "fileName": "page.html" }),
        )
        .await;
    assert_eq!(png_size(&r.body), Some((1920, 2_000)));
    // WebP for a page; other formats are refused.
    let r = h
        .post(
            &format!("/api/projects/{id}/export/image"),
            json!({ "fileName": "page.html", "imageFormat": "webp" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header(header::CONTENT_TYPE), "image/webp");
    assert!(r.header(header::CONTENT_DISPOSITION).contains("page.webp"));
    let r = h
        .post(
            &format!("/api/projects/{id}/export/image"),
            json!({ "fileName": "page.html", "imageFormat": "gif" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let h = harness_with(Fake {
        unavailable: true,
        ..Fake::default()
    });
    let id = deck_project(&h);
    let r = h
        .post(
            &format!("/api/projects/{id}/export/pdf-image"),
            json!({ "fileName": "index.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(r.json()["error"]["code"], "UPSTREAM_UNAVAILABLE");
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Chrome")
    );
}

#[tokio::test]
async fn active_context_round_trip() {
    let h = harness();
    let id = h.project("Landing");
    assert_eq!(
        h.get("/api/active").await.json(),
        json!({ "active": false })
    );

    let r = h
        .post(
            "/api/active",
            json!({ "projectId": id, "fileName": "index.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["active"], true);
    let got = h.get("/api/active").await.json();
    assert_eq!(got["projectId"], id.as_str());
    assert_eq!(got["projectName"], "Landing");
    assert_eq!(got["fileName"], "index.html");
    assert!(got["ageMs"].as_i64().unwrap() >= 0);

    assert_eq!(
        h.post("/api/active", json!({ "fileName": "x" }))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.post("/api/active", json!({ "active": false }))
            .await
            .json(),
        json!({ "active": false })
    );
    assert_eq!(
        h.get("/api/active").await.json(),
        json!({ "active": false })
    );
}
