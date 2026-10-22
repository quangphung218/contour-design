//! HTTP round trips through the real router (spec 5.7, M1 criteria).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use od_content::{ContentRoots, ContentStore};
use od_contracts::{
    CreateProjectRequest, CreateProjectResponse, FileVersionsResponse, ProjectManifest,
    ProjectMetadata, ProjectsResponse,
};
use od_server::{AppState, ServerConfig, router};
use od_store::{DataDir, PROJECT_MANIFEST_PATH, Store};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;

struct Harness {
    _tmp: tempfile::TempDir,
    data: PathBuf,
    state: AppState,
}

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn harness_with(resource_root: &Path) -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let store = Store::open(DataDir::new(&data)).unwrap();
    let content = ContentStore::load(ContentRoots::bundled(resource_root).with_user_root(&data));
    let state = AppState {
        store: Arc::new(store),
        content: Arc::new(content),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![7456],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: resource_root.to_path_buf(),
        }),
    };
    Harness {
        _tmp: tmp,
        data,
        state,
    }
}

/// Small router without the real catalogue (fast).
fn harness() -> Harness {
    let empty = std::env::temp_dir().join("od-empty-content-for-tests");
    harness_with(&empty)
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("json body, got {:?}", String::from_utf8_lossy(&self.body)))
    }
    fn typed<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap()
    }
}

impl Harness {
    async fn send(&self, req: Request<Body>) -> Reply {
        let res = router(self.state.clone()).oneshot(req).await.unwrap();
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

    async fn json(&self, method: &str, uri: &str, body: &Value) -> Reply {
        self.send(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    async fn create_project(&self, name: &str) -> String {
        let r = self
            .json("POST", "/api/projects", &json!({ "name": name }))
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        r.typed::<CreateProjectResponse>().project.id
    }

    async fn write(&self, id: &str, name: &str, content: &str) -> Reply {
        self.json(
            "POST",
            &format!("/api/projects/{id}/files"),
            &json!({ "name": name, "content": content }),
        )
        .await
    }
}

#[tokio::test]
async fn health_and_version() {
    let h = harness();
    let r = h.get("/api/health").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["ok"], true);
    let r = h.get("/api/version").await;
    assert_eq!(r.json()["name"], "open-design-rs");
}

#[tokio::test]
async fn project_crud_round_trip_creates_folder_and_manifest() {
    let h = harness();
    let r = h
        .json(
            "POST",
            "/api/projects",
            &json!({ "name": "  Landing  ", "skillId": "web-prototype", "metadata": { "kind": "prototype", "custom": 1 } }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let created: CreateProjectResponse = r.typed();
    let id = created.project.id.clone();
    assert_eq!(created.project.name, "Landing");
    assert!(!created.conversation_id.is_empty());

    let dir = h.data.join("projects").join(&id);
    assert!(dir.is_dir(), "projects/<id>/ exists");
    let manifest: ProjectManifest =
        serde_json::from_slice(&std::fs::read(dir.join(PROJECT_MANIFEST_PATH)).unwrap()).unwrap();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        (manifest.id.as_str(), manifest.name.as_str()),
        (id.as_str(), "Landing")
    );
    assert_eq!(manifest.skill_id.as_deref(), Some("web-prototype"));

    let list: ProjectsResponse = h.get("/api/projects").await.typed();
    assert_eq!(list.projects.len(), 1);
    let got = h.get(&format!("/api/projects/{id}")).await.json();
    assert_eq!(
        got["project"]["metadata"]["custom"], 1,
        "unknown metadata round-trips"
    );

    let r = h
        .json(
            "PATCH",
            &format!("/api/projects/{id}"),
            &json!({ "name": "Renamed", "skillId": null }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["project"]["skillId"], Value::Null);
    let manifest: ProjectManifest =
        serde_json::from_slice(&std::fs::read(dir.join(PROJECT_MANIFEST_PATH)).unwrap()).unwrap();
    assert_eq!(manifest.name, "Renamed", "manifest follows updates");
    assert_eq!(manifest.skill_id, None);

    let r = h
        .send(
            Request::delete(format!("/api/projects/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(!dir.exists(), "folder removed");
    assert_eq!(
        h.get(&format!("/api/projects/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn create_rejects_base_dir_and_bad_input() {
    let h = harness();
    let r = h
        .json(
            "POST",
            "/api/projects",
            &json!({ "name": "x", "metadata": { "baseDir": "/etc" } }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"]["code"], "BAD_REQUEST");
    let r = h
        .json("POST", "/api/projects", &json!({ "name": "  " }))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .json("POST", "/api/projects", &json!({ "id": "..", "name": "x" }))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .send(
            Request::post("/api/projects")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{not json"))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.json()["error"]["message"].is_string());
}

#[tokio::test]
async fn duplicate_copies_files() {
    let h = harness();
    let id = h.create_project("Source").await;
    assert_eq!(
        h.write(&id, "index.html", "<h1>hi</h1>").await.status,
        StatusCode::OK
    );
    assert_eq!(
        h.write(&id, "assets/a.css", "a{}").await.status,
        StatusCode::OK
    );
    let r = h
        .send(
            Request::post(format!("/api/projects/{id}/duplicate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let body = r.json();
    assert_eq!(body["project"]["name"], "Source Copy");
    let mut copied: Vec<String> = serde_json::from_value(body["copiedFiles"].clone()).unwrap();
    copied.sort();
    assert_eq!(copied, ["assets/a.css", "index.html"]);
    let new_id = body["project"]["id"].as_str().unwrap();
    let r = h
        .get(&format!("/api/projects/{new_id}/files/index.html"))
        .await;
    assert_eq!(r.body, b"<h1>hi</h1>");
    assert!(
        h.data
            .join("projects")
            .join(new_id)
            .join(PROJECT_MANIFEST_PATH)
            .is_file()
    );
}

#[tokio::test]
async fn files_write_list_read_rename_delete_upload() {
    let h = harness();
    let id = h.create_project("Files").await;
    let r = h.write(&id, "pages/my page.html", "<p>x</p>").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.json()["file"]["name"],
        "pages/my-page.html",
        "segments sanitized"
    );
    let r = h
        .json(
            "POST",
            &format!("/api/projects/{id}/files"),
            &json!({ "name": "logo.png", "content": "iVBORw0K", "encoding": "base64" }),
        )
        .await;
    assert_eq!(r.json()["file"]["kind"], "image");
    let r = h
        .json(
            "POST",
            &format!("/api/projects/{id}/files"),
            &json!({ "name": "logo.png", "content": "x", "overwrite": false }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    let list = h.get(&format!("/api/projects/{id}/files")).await;
    assert_eq!(list.headers[header::CACHE_CONTROL], "no-store");
    let names: Vec<String> = list.json()["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"pages/my-page.html".to_owned()));
    assert!(
        !names.iter().any(|n| n.contains(".open-design")),
        "manifest hidden"
    );

    let r = h
        .json(
            "POST",
            &format!("/api/projects/{id}/files/rename"),
            &json!({ "from": "pages/my-page.html", "to": "index.html" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    assert_eq!(
        h.get(&format!("/api/projects/{id}/files/index.html"))
            .await
            .body,
        b"<p>x</p>"
    );
    assert_eq!(
        h.get(&format!("/api/projects/{id}/files/pages/my-page.html"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let r = h
        .send(
            Request::delete(format!("/api/projects/{id}/files/index.html"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = h
        .send(
            Request::delete(format!("/api/projects/{id}/files/index.html"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.json()["error"]["code"], "FILE_NOT_FOUND");

    let boundary = "XBOUNDARY";
    let multipart = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"dir\"\r\n\r\nuploads\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"photo 1.png\"\r\nContent-Type: image/png\r\n\r\nPNGDATA\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"photo 1.png\"\r\nContent-Type: image/png\r\n\r\nPNG2\r\n--{boundary}--\r\n"
    );
    let r = h
        .send(
            Request::post(format!("/api/projects/{id}/upload"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .unwrap(),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "{:?}",
        String::from_utf8_lossy(&r.body)
    );
    let up = r.json();
    assert_eq!(up["files"][0]["name"], "uploads/photo-1.png");
    assert_eq!(
        up["files"][1]["name"], "uploads/photo-1-1.png",
        "no overwrite on upload"
    );
    assert_eq!(up["files"][0]["originalName"], "photo 1.png");
}

/// Spec 5.7: traversal, symlink escape and hidden segments in imported
/// folders all answer 400 or 403 (at least 10 cases).
#[tokio::test]
async fn path_traversal_table() {
    let h = harness();
    let id = h.create_project("Guarded").await;
    let project_dir = h.data.join("projects").join(&id);
    let outside = h.data.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "top secret").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, project_dir.join("link")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), project_dir.join("leak.txt"))
            .unwrap();
        std::os::unix::fs::symlink(outside.join("nope.txt"), project_dir.join("dangling.txt"))
            .unwrap();
    }

    // Imported (folder) project: hidden segments are off limits.
    let import_dir = h.data.join("imported");
    std::fs::create_dir_all(import_dir.join(".ssh")).unwrap();
    std::fs::write(import_dir.join(".ssh/id_rsa"), "KEY").unwrap();
    let (imported, _) = h
        .state
        .store
        .create_project(CreateProjectRequest {
            name: "Imported".to_owned(),
            metadata: Some(ProjectMetadata {
                base_dir: Some(import_dir.to_string_lossy().into_owned()),
                imported_from: Some("folder".to_owned()),
                ..ProjectMetadata::default()
            }),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    let imp = imported.id;

    let reads = [
        format!("/api/projects/{id}/files/..%2F..%2Fapp.sqlite"),
        format!("/api/projects/{id}/files/a/..%2F..%2F..%2Foutside%2Fsecret.txt"),
        format!("/api/projects/{id}/raw/..%2Foutside%2Fsecret.txt"),
        format!("/api/projects/{id}/files/%2Fetc%2Fpasswd"),
        format!("/api/projects/{id}/files/C:%5Cwindows%5Cwin.ini"),
        format!("/api/projects/{id}/files/.file-versions/x/manifest.json"),
        format!("/api/projects/{id}/files/.open-design/project.json"),
        format!("/api/projects/{id}/files/a%00b"),
        format!("/api/projects/{id}/files/..%5C..%5Capp.sqlite"),
        format!("/api/projects/{imp}/files/.ssh/id_rsa"),
        format!("/api/projects/{imp}/raw/.ssh/id_rsa"),
        #[cfg(unix)]
        format!("/api/projects/{id}/files/link/secret.txt"),
        #[cfg(unix)]
        format!("/api/projects/{id}/raw/leak.txt"),
    ];
    for uri in &reads {
        let r = h.get(uri).await;
        assert!(
            r.status == StatusCode::BAD_REQUEST || r.status == StatusCode::FORBIDDEN,
            "{uri} must be 400/403, got {} {:?}",
            r.status,
            String::from_utf8_lossy(&r.body)
        );
        assert!(!String::from_utf8_lossy(&r.body).contains("top secret"));
        assert!(!String::from_utf8_lossy(&r.body).contains("KEY"));
    }

    let writes = [
        (id.as_str(), "../escape.html"),
        (id.as_str(), "/abs.html"),
        (id.as_str(), ".file-versions/x.html"),
        (imp.as_str(), ".ssh/authorized_keys"),
        #[cfg(unix)]
        (id.as_str(), "link/planted.txt"),
        #[cfg(unix)]
        (id.as_str(), "dangling.txt"),
    ];
    for (pid, name) in writes {
        let r = h.write(pid, name, "pwned").await;
        assert!(
            r.status == StatusCode::BAD_REQUEST || r.status == StatusCode::FORBIDDEN,
            "write {name} must be 400/403, got {}",
            r.status
        );
    }
    assert!(!outside.join("planted.txt").exists());
    assert!(!outside.join("nope.txt").exists());

    let r = h
        .json(
            "POST",
            &format!("/api/projects/{id}/files/rename"),
            &json!({ "from": "../outside/secret.txt", "to": "stolen.txt" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .send(
            Request::delete(format!(
                "/api/projects/{id}/files/..%2Foutside%2Fsecret.txt"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(outside.join("secret.txt").exists());
    assert!(reads.len() + writes.len() + 2 >= 10);
}

#[tokio::test]
async fn raw_has_etag_304_and_ranges() {
    let h = harness();
    let id = h.create_project("Raw").await;
    h.write(&id, "index.html", "0123456789").await;
    let uri = format!("/api/projects/{id}/raw/index.html");
    let r = h.get(&uri).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, b"0123456789");
    assert_eq!(r.headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
    let etag = r.headers[header::ETAG].to_str().unwrap().to_owned();
    assert!(etag.starts_with("W/\""));
    assert!(r.headers.contains_key(header::LAST_MODIFIED));

    let r = h
        .send(
            Request::get(&uri)
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_MODIFIED);
    assert!(r.body.is_empty());

    let r = h
        .send(
            Request::get(&uri)
                .header(header::IF_NONE_MATCH, "W/\"stale\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let r = h
        .send(
            Request::get(&uri)
                .header(header::RANGE, "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.body, b"2345");
    assert_eq!(r.headers[header::CONTENT_RANGE], "bytes 2-5/10");

    let r = h
        .send(
            Request::get(&uri)
                .header(header::RANGE, "bytes=50-")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::RANGE_NOT_SATISFIABLE);

    // Sandboxed preview iframe.
    let r = h
        .send(
            Request::get(&uri)
                .header(header::ORIGIN, "null")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    assert_eq!(
        h.get(&format!("/api/projects/{id}/raw/missing.png"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn versions_create_list_restore_bytes_exact() {
    let h = harness();
    let id = h.create_project("Versions").await;
    let v1_content = "<!doctype html>\n<p>première — ✓</p>\r\n";
    h.write(&id, "index.html", v1_content).await;

    // Listing bootstraps a baseline from the current content.
    let r = h
        .get(&format!("/api/projects/{id}/files/index.html/versions"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let listed: FileVersionsResponse = r.typed();
    assert_eq!(listed.versions.len(), 1);
    let v1 = listed.versions[0].id.clone();

    h.write(&id, "index.html", "<p>second</p>").await;
    let r = h
        .json(
            "POST",
            &format!("/api/projects/{id}/files/index.html/versions"),
            &json!({ "label": "after edit" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["version"]["label"], "after edit");
    assert_eq!(r.json()["version"]["version"], 2);

    let r = h
        .get(&format!(
            "/api/projects/{id}/files/index.html/versions/{v1}"
        ))
        .await;
    assert_eq!(r.json()["content"], v1_content);

    let r = h
        .send(
            Request::post(format!(
                "/api/projects/{id}/files/index.html/versions/{v1}/restore"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    assert_eq!(r.json()["version"]["source"], "restore");
    assert_eq!(r.json()["version"]["restoreFromVersionId"], v1.as_str());

    let raw = h.get(&format!("/api/projects/{id}/raw/index.html")).await;
    assert_eq!(
        raw.body,
        v1_content.as_bytes(),
        "restored bytes are identical"
    );

    let listed: FileVersionsResponse = h
        .get(&format!("/api/projects/{id}/files/index.html/versions"))
        .await
        .typed();
    assert_eq!(listed.versions.len(), 3);
    assert!(listed.versions[2].current);

    let r = h
        .send(
            Request::post(format!(
                "/api/projects/{id}/files/index.html/versions/nope/restore"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    h.write(&id, "a.css", "a{}").await;
    let r = h
        .get(&format!("/api/projects/{id}/files/a.css/versions"))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn origin_guard() {
    let h = harness();
    let with_origin = |method: &str, uri: &str, origin: &str, host: &str| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::ORIGIN, origin)
            .header(header::HOST, host)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "name": "x" }).to_string()))
            .unwrap()
    };
    let r = h
        .send(with_origin(
            "POST",
            "/api/projects",
            "https://evil.example",
            "127.0.0.1:7456",
        ))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = h
        .send(with_origin(
            "POST",
            "/api/projects",
            "http://127.0.0.1:7456",
            "127.0.0.1:7456",
        ))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = h
        .send(with_origin(
            "POST",
            "/api/projects",
            "null",
            "127.0.0.1:7456",
        ))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = h
        .send(with_origin(
            "GET",
            "/api/projects",
            "http://localhost",
            "localhost:7456",
        ))
        .await;
    assert_eq!(r.status, StatusCode::OK, "portless loopback GET allowed");
    let r = h
        .send(with_origin(
            "POST",
            "/api/projects",
            "http://localhost",
            "localhost:7456",
        ))
        .await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "portless loopback write refused"
    );
}

#[tokio::test]
async fn content_endpoints_serve_real_catalogue() {
    let h = harness_with(&content_root());
    let skills = h.get("/api/skills").await.json();
    let skills = skills["skills"].as_array().unwrap();
    assert!(skills.len() >= 150);
    assert!(
        skills
            .iter()
            .all(|s| s.get("body").is_none() && s["hasBody"].is_boolean())
    );

    let first = skills[0]["id"].as_str().unwrap();
    let detail = h.get(&format!("/api/skills/{first}")).await.json();
    assert!(detail["body"].as_str().is_some_and(|b| !b.is_empty()));
    assert_eq!(
        h.get("/api/skills/does-not-exist").await.status,
        StatusCode::NOT_FOUND
    );

    let templates = h.get("/api/design-templates").await.json();
    assert!(templates["designTemplates"].as_array().unwrap().len() >= 100);

    let systems = h.get("/api/design-systems").await.json();
    let systems = systems["designSystems"].as_array().unwrap();
    assert!(systems.len() >= 150);
    let ds_id = systems[0]["id"].as_str().unwrap();
    let ds = h.get(&format!("/api/design-systems/{ds_id}")).await.json();
    assert!(ds["body"].as_str().unwrap().contains('#'));
    assert_eq!(ds["designSystem"]["id"], ds_id);

    let craft = h.get("/api/craft").await.json();
    assert!(
        craft["craft"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "typography")
    );
    let typo = h.get("/api/craft/typography").await;
    assert_eq!(typo.status, StatusCode::OK);
    assert_eq!(
        h.get("/api/craft/Bad_Slug").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(h.get("/api/craft/nope").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn token_required_for_remote_peers_when_configured() {
    let mut h = harness();
    let mut config = (*h.state.config).clone();
    config.api_token = Some("s3cret".to_owned());
    h.state.config = Arc::new(config);
    let remote = |auth: Option<&str>| {
        let mut req = Request::get("/api/projects").body(Body::empty()).unwrap();
        req.extensions_mut().insert(axum::extract::ConnectInfo(
            "192.168.1.20:5000".parse::<std::net::SocketAddr>().unwrap(),
        ));
        if let Some(a) = auth {
            req.headers_mut()
                .insert(header::AUTHORIZATION, a.parse().unwrap());
        }
        req
    };
    assert_eq!(h.send(remote(None)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        h.send(remote(Some("Bearer s3cret"))).await.status,
        StatusCode::OK
    );
    assert_eq!(
        h.send(remote(Some("Bearer nope"))).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.get("/api/projects").await.status,
        StatusCode::OK,
        "loopback/no peer info passes"
    );
}
