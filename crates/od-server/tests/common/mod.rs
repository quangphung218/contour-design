//! Shared harness for run tests: a real daemon on a loopback port, mock
//! agent CLIs (shell scripts) first on `PATH`, and a tiny SSE client.

#![allow(dead_code, clippy::unwrap_used)] // each test binary uses a subset

use od_content::{ContentRoots, ContentStore};
use od_contracts::{CreateProjectRequest, CreateProjectResponse, CreateRunResponse};
use od_runtime::EngineConfig;
use od_server::{AppState, RunContext, ServerConfig, router};
use od_store::{DataDir, Store};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct Daemon {
    pub tmp: tempfile::TempDir,
    pub base: String,
    pub data: PathBuf,
    pub mocks: PathBuf,
    pub state: AppState,
    pub http: reqwest::Client,
}

pub struct Options {
    pub content_root: Option<PathBuf>,
    pub idle_timeout: Option<Duration>,
    pub kill_grace: Duration,
    pub extra_env: Vec<(String, String)>,
    /// Shell asked for `PATH` on `POST /api/agents/rescan`, as in the app.
    pub login_shell: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            content_root: None,
            idle_timeout: None,
            kill_grace: od_runtime::process::KILL_GRACE,
            extra_env: Vec::new(),
            login_shell: None,
        }
    }
}

pub async fn daemon() -> Daemon {
    daemon_with(Options::default()).await
}

pub async fn daemon_with(opts: Options) -> Daemon {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let mocks = tmp.path().join("mocks");
    std::fs::create_dir_all(&mocks).unwrap();
    let store = Store::open(DataDir::new(&data)).unwrap();
    let content_root = opts
        .content_root
        .unwrap_or_else(|| tmp.path().join("no-content"));
    let content = ContentStore::load(ContentRoots::bundled(&content_root).with_user_root(&data));
    let runs = od_server::open_runs(&store, &od_server::DaemonLock::acquire(&data).unwrap())
        .await
        .unwrap();
    let mut engine = EngineConfig::from_env();
    // Mocks and the system tools only: agent CLIs installed on this machine
    // (Homebrew, npm) are never found, not even for a `--version` probe.
    engine.path =
        std::env::join_paths([mocks.as_path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    engine.idle_timeout = opts.idle_timeout;
    engine.login_shell = opts.login_shell.map(PathBuf::into_os_string);
    engine.kill_grace = opts.kill_grace;
    engine.retry_base = Duration::from_millis(50);
    // A mock's first exec can take seconds on a loaded Mac (it scans new
    // binaries); tests assert the flags the help probe finds.
    engine.probe_timeout = Duration::from_secs(60);
    engine.base_env.extend(
        opts.extra_env
            .into_iter()
            .map(|(k, v)| (k.into(), v.into())),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState {
        store: Arc::new(store),
        content: Arc::new(content),
        config: Arc::new(ServerConfig {
            host: "127.0.0.1".to_owned(),
            ports: vec![addr.port()],
            api_token: None,
            allowed_origins: Vec::new(),
            resource_root: content_root,
            daemon_url: Some(format!("http://{addr}")),
            mcp_daemon_url: true,
        }),
        runs: Arc::new(RunContext::new(Arc::clone(&runs), engine)),
        preview: od_server::Preview::default(),
        platform: od_server::Platform::from_env(),
    };
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Daemon {
        tmp,
        base: format!("http://{addr}"),
        data,
        mocks,
        state,
        http: reqwest::Client::builder().no_proxy().build().unwrap(),
    }
}

/// Writes an executable mock CLI named `name` into the mocks folder.
pub fn write_mock(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// Shell that pulls the `--session-id`/`--resume` value into `$sid`.
pub const PARSE_SESSION: &str = r#"
sid=""; prev=""
for a in "$@"; do
  case "$prev" in --session-id|--resume) sid="$a";; esac
  prev="$a"
done
"#;

/// Probe answers so detection and the help probe stay quick.
pub const PROBES: &str = r#"
case "$1" in --version) echo "mock 1.0"; exit 0;; esac
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then echo "--include-partial-messages"; exit 0; fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then echo "usage"; exit 0; fi
"#;

#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub id: u64,
    pub event: String,
    pub data: Value,
}

impl Daemon {
    pub async fn post(&self, path: &str, body: &Value) -> reqwest::Response {
        self.http
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await
            .unwrap()
    }

    pub async fn get_json(&self, path: &str) -> Value {
        self.http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn project(&self) -> (String, String) {
        let res: CreateProjectResponse = self
            .post(
                "/api/projects",
                &serde_json::to_value(CreateProjectRequest {
                    name: "Run test".into(),
                    ..CreateProjectRequest::default()
                })
                .unwrap(),
            )
            .await
            .json()
            .await
            .unwrap();
        (res.project.id, res.conversation_id)
    }

    pub async fn start(&self, body: Value) -> CreateRunResponse {
        let res = self.post("/api/runs", &body).await;
        let status = res.status();
        let text = res.text().await.unwrap();
        assert_eq!(status.as_u16(), 202, "{text}");
        serde_json::from_str(&text).unwrap()
    }

    /// Opens the run's SSE stream after `after`.
    pub async fn stream(&self, run_id: &str, after: Option<u64>) -> SseReader {
        let mut req = self
            .http
            .get(format!("{}/api/runs/{run_id}/events", self.base));
        if let Some(after) = after {
            req = req.header("Last-Event-ID", after.to_string());
        }
        let res = req.send().await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        SseReader {
            res,
            buf: String::new(),
        }
    }

    /// Every event of the run after `after`, until the stream closes.
    pub async fn events(&self, run_id: &str, after: Option<u64>) -> Vec<SseEvent> {
        let mut reader = self.stream(run_id, after).await;
        let mut out = Vec::new();
        let collect = async {
            while let Some(ev) = reader.next().await {
                out.push(ev);
            }
        };
        tokio::time::timeout(Duration::from_secs(30), collect)
            .await
            .expect("run did not end in time");
        out
    }

    /// Waits until the assistant message is no longer running.
    pub async fn settled_messages(&self, project: &str, conversation: &str) -> Vec<Value> {
        for _ in 0..200 {
            let body = self
                .get_json(&format!(
                    "/api/projects/{project}/conversations/{conversation}/messages"
                ))
                .await;
            let messages = body["messages"].as_array().cloned().unwrap_or_default();
            if messages.iter().all(|m| m["runStatus"] != json!("running")) {
                return messages;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("messages never settled");
    }
}

pub struct SseReader {
    res: reqwest::Response,
    buf: String,
}

impl SseReader {
    /// Reads any SSE response (not only a run's).
    pub fn new(res: reqwest::Response) -> Self {
        Self {
            res,
            buf: String::new(),
        }
    }

    pub async fn next(&mut self) -> Option<SseEvent> {
        loop {
            if let Some(end) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..end + 2).collect();
                let mut ev = SseEvent {
                    id: 0,
                    event: "message".into(),
                    data: Value::Null,
                };
                let mut data = String::new();
                let mut has_data = false;
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("id:") {
                        ev.id = v.trim().parse().unwrap_or(0);
                    } else if let Some(v) = line.strip_prefix("event:") {
                        v.trim().clone_into(&mut ev.event);
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.strip_prefix(' ').unwrap_or(v));
                        has_data = true;
                    }
                }
                if !has_data {
                    continue; // keepalive comment
                }
                ev.data = serde_json::from_str(&data).unwrap();
                return Some(ev);
            }
            match self.res.chunk().await.ok()? {
                Some(bytes) => self.buf.push_str(&String::from_utf8_lossy(&bytes)),
                None => return None,
            }
        }
    }
}

pub fn agent_payloads(events: &[SseEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e.event == "agent")
        .map(|e| e.data.clone())
        .collect()
}

pub fn end_of(events: &[SseEvent]) -> &Value {
    &events
        .last()
        .filter(|e| e.event == "end")
        .expect("stream ends with `end`")
        .data
}

pub fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../od-runtime/tests/fixtures")
        .join(rel)
}
