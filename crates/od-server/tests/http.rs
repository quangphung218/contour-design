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
            daemon_url: None,
        }),
        runs: Arc::new(od_server::RunContext::new(
            Arc::new(od_runs::Runs::new(data.join("runs"))),
            od_runtime::EngineConfig::from_env(),
        )),
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
        // macOS/Windows file systems ignore case: reserved names must too.
        format!("/api/projects/{id}/files/.OPEN-DESIGN/project.json"),
        format!("/api/projects/{id}/raw/.Open-Design/project.json"),
        format!("/api/projects/{id}/raw/.File-Versions/x/manifest.json"),
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
    for uri in [
        format!("/api/projects/{id}/files/.OPEN-DESIGN/project.json"),
        format!("/api/projects/{id}/files/.Open-Design/project.json"),
    ] {
        let r = h
            .send(Request::delete(uri.as_str()).body(Body::empty()).unwrap())
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "delete {uri}");
    }
    let r = h
        .json(
            "POST",
            &format!("/api/projects/{id}/files/rename"),
            &json!({ "from": ".OPEN-DESIGN/project.json", "to": "stolen.json" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(
        project_dir.join(PROJECT_MANIFEST_PATH).is_file(),
        "manifest survives"
    );

    // Unicode case folding (review round 2): APFS folds `ſ` to `s` and `ﬁ`
    // to `fi`. On case-sensitive file systems these are just missing files
    // (404); either way they must never reach the reserved folders.
    h.write(&id, "index.html", "<p>history</p>").await;
    let history = std::fs::read_dir(project_dir.join(".file-versions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    let manifest_before = std::fs::read(project_dir.join(PROJECT_MANIFEST_PATH)).unwrap();
    let folded = [
        format!("/api/projects/{id}/raw/.open-de%C5%BFign/project.json"),
        format!("/api/projects/{id}/files/.open-de%C5%BFign/project.json"),
        format!("/api/projects/{id}/raw/.%EF%AC%81le-versions/{history}/manifest.json"),
        format!("/api/projects/{id}/files/.%EF%AC%81le-versions/{history}/manifest.json"),
    ];
    for uri in &folded {
        let r = h.get(uri).await;
        assert_ne!(r.status, StatusCode::OK, "GET {uri}");
        let r = h
            .send(Request::delete(uri.as_str()).body(Body::empty()).unwrap())
            .await;
        assert_ne!(r.status, StatusCode::OK, "DELETE {uri}");
    }
    assert_eq!(
        std::fs::read(project_dir.join(PROJECT_MANIFEST_PATH)).unwrap(),
        manifest_before,
        "manifest intact"
    );
    assert!(
        project_dir
            .join(".file-versions")
            .join(&history)
            .join("manifest.json")
            .is_file()
    );
    assert!(reads.len() + writes.len() + 5 >= 10);
}

/// Review round 1, item 1: same-name uploads in parallel never overwrite.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_name_uploads_keep_every_file() {
    const N: usize = 16;
    let h = harness();
    let id = h.create_project("Race").await;
    let boundary = "RACEBOUNDARY";
    let mut tasks = Vec::new();
    for i in 0..N {
        let state = h.state.clone();
        let uri = format!("/api/projects/{id}/upload");
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"same.png\"\r\nContent-Type: image/png\r\n\r\npayload-{i}\r\n--{boundary}--\r\n"
        );
        tasks.push(tokio::spawn(async move {
            let req = Request::post(uri)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap();
            let res = router(state).oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap())
        }));
    }
    let mut names = std::collections::BTreeSet::new();
    for task in tasks {
        let (status, body) = task.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        names.insert(body["files"][0]["name"].as_str().unwrap().to_owned());
    }
    assert_eq!(names.len(), N, "every upload got its own name: {names:?}");
    let dir = h.data.join("projects").join(&id);
    let mut payloads: Vec<String> = names
        .iter()
        .map(|n| std::fs::read_to_string(dir.join(n)).unwrap())
        .collect();
    payloads.sort();
    let mut expected: Vec<String> = (0..N).map(|i| format!("payload-{i}")).collect();
    expected.sort();
    assert_eq!(payloads, expected, "no upload's bytes were lost");
}

/// Review round 1, item 1: `overwrite: false` is atomic under contention.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_create_only_writes_have_one_winner() {
    const N: usize = 12;
    let h = harness();
    let id = h.create_project("CreateNew").await;
    let mut tasks = Vec::new();
    for i in 0..N {
        let state = h.state.clone();
        let uri = format!("/api/projects/{id}/files");
        tasks.push(tokio::spawn(async move {
            let req = Request::post(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "name": "once.txt", "content": format!("writer-{i}"), "overwrite": false })
                        .to_string(),
                ))
                .unwrap();
            router(state).oneshot(req).await.unwrap().status()
        }));
    }
    let mut ok = 0;
    for task in tasks {
        match task.await.unwrap() {
            StatusCode::OK => ok += 1,
            StatusCode::CONFLICT => {}
            other => panic!("unexpected {other}"),
        }
    }
    assert_eq!(ok, 1, "exactly one create-only write succeeds");
}

/// Review round 1, item 2: restore writes back to the exact file name even
/// when the sanitizer would change it.
#[tokio::test]
async fn restore_targets_unsanitized_file_name() {
    let h = harness();
    let id = h.create_project("Spaces").await;
    let dir = h.data.join("projects").join(&id);
    // Agents write straight to disk with any name.
    std::fs::write(dir.join("My Page.html"), "<p>v1</p>").unwrap();
    let listed: FileVersionsResponse = h
        .get(&format!("/api/projects/{id}/files/My%20Page.html/versions"))
        .await
        .typed();
    let v1 = listed.versions[0].id.clone();
    std::fs::write(dir.join("My Page.html"), "<p>v2-edited</p>").unwrap();
    let r = h
        .send(
            Request::post(format!(
                "/api/projects/{id}/files/My%20Page.html/versions/{v1}/restore"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    assert_eq!(r.json()["file"]["name"], "My Page.html");
    assert_eq!(
        std::fs::read(dir.join("My Page.html")).unwrap(),
        b"<p>v1</p>"
    );
    assert!(
        !dir.join("My-Page.html").exists(),
        "no sanitized twin created"
    );
    let listed: FileVersionsResponse = h
        .get(&format!("/api/projects/{id}/files/My%20Page.html/versions"))
        .await
        .typed();
    assert_eq!(listed.versions.len(), 2);
    assert!(listed.versions[1].current);
}

/// Review round 1, item 3: each HTML write through the API is a version.
#[tokio::test]
async fn html_writes_create_versions() {
    let h = harness();
    let id = h.create_project("Saves").await;
    for content in ["<p>one</p>", "<p>two</p>", "<p>three</p>", "<p>three</p>"] {
        assert_eq!(h.write(&id, "a.html", content).await.status, StatusCode::OK);
    }
    let listed: FileVersionsResponse = h
        .get(&format!("/api/projects/{id}/files/a.html/versions"))
        .await
        .typed();
    assert_eq!(
        listed.versions.len(),
        3,
        "unchanged rewrite adds no version"
    );
    for (version, content) in
        listed
            .versions
            .iter()
            .zip(["<p>one</p>", "<p>two</p>", "<p>three</p>"])
    {
        let r = h
            .get(&format!(
                "/api/projects/{id}/files/a.html/versions/{}",
                version.id
            ))
            .await;
        assert_eq!(r.json()["content"], content);
    }
    assert!(listed.versions[2].current);
    h.write(&id, "style.css", "a{}").await;
    assert_eq!(
        h.get(&format!("/api/projects/{id}/files/style.css/versions"))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
}

/// Review round 1, item 5: a vanished import folder is never recreated.
#[tokio::test]
async fn missing_import_folder_is_not_recreated() {
    let h = harness();
    let import_dir = h.data.join("external-drive/work");
    std::fs::create_dir_all(&import_dir).unwrap();
    let import_req = |dir: &Path| CreateProjectRequest {
        name: "Imported".to_owned(),
        metadata: Some(ProjectMetadata {
            base_dir: Some(dir.to_string_lossy().into_owned()),
            imported_from: Some("folder".to_owned()),
            ..ProjectMetadata::default()
        }),
        ..CreateProjectRequest::default()
    };
    let (project, _) = h
        .state
        .store
        .create_project(import_req(&import_dir))
        .unwrap();
    let id = project.id;
    std::fs::remove_dir_all(h.data.join("external-drive")).unwrap();

    let r = h.write(&id, "index.html", "<p>x</p>").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["error"]["code"], "CONFLICT");
    let boundary = "B";
    let r = h
        .send(
            Request::post(format!("/api/projects/{id}/upload"))
                .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.png\"\r\n\r\nx\r\n--{boundary}--\r\n"
                )))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let r = h
        .json(
            "PATCH",
            &format!("/api/projects/{id}"),
            &json!({ "name": "Renamed" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "metadata edits still work");
    assert!(
        !h.data.join("external-drive").exists(),
        "folder not recreated"
    );

    let missing = h.data.join("never-existed");
    assert!(h.state.store.create_project(import_req(&missing)).is_err());
    assert!(!missing.exists());
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
    // v2 came from the write itself; this explicit snapshot is v3.
    assert_eq!(r.json()["version"]["version"], 3);

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
    assert_eq!(listed.versions.len(), 4);
    assert!(listed.versions[3].current);

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
