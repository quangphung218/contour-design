//! A small MCP client: `initialize`, then `tools/list`, over stdio or
//! streamable HTTP. The daemon uses it to test a server from Settings and
//! to learn that a server wants OAuth (HTTP 401).

use crate::McpError;
use crate::config::{McpServer, Transport};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, WWW_AUTHENTICATE,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Whole exchange (spawn or connect, initialize, list).
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// Largest JSON-RPC message or HTTP body read.
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;
const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn initialize(id: u64) -> Value {
    request(
        id,
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "contour", "version": env!("CARGO_PKG_VERSION")},
        }),
    )
}

fn initialized() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

/// The `result` of a response, or its JSON-RPC error.
fn result_of(msg: Value) -> Result<Value, McpError> {
    if let Some(err) = msg.get("error") {
        let text = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_owned();
        return Err(McpError::Protocol(text));
    }
    msg.get("result")
        .cloned()
        .ok_or_else(|| McpError::Protocol("response without result".into()))
}

fn tools_of(result: &Value) -> Vec<ToolInfo> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| {
                    Some(ToolInfo {
                        name: t.get("name")?.as_str()?.to_owned(),
                        description: t
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Connects, initializes and lists the server's tools. `bearer` is the
/// OAuth access token for HTTP servers.
///
/// # Errors
/// [`McpError::NeedsAuth`] on HTTP 401, protocol, I/O and timeout errors.
pub async fn list_tools(
    server: &McpServer,
    http: &reqwest::Client,
    bearer: Option<&str>,
) -> Result<Vec<ToolInfo>, McpError> {
    let work = async {
        match server.transport {
            Transport::Stdio => stdio_tools(server).await,
            Transport::Http => http_tools(server, http, bearer).await,
        }
    };
    tokio::time::timeout(TIMEOUT, work)
        .await
        .map_err(|_| McpError::Protocol("timed out talking to the server".into()))?
}

/// Kills the server's whole tree when the exchange ends, however it ends:
/// a timeout drops it mid-await, and `kill_on_drop` alone reaches only the
/// direct child, not what it started (`npx` and its node). On POSIX that
/// is the server's process group plus the groups of its descendants (one
/// that left with `setsid` is in neither); on Windows its Job Object.
struct KillTree(Option<u32>);

impl KillTree {
    /// Kills the tree now. Call it before reaping the server: once reaped,
    /// its descendants belong to init and the walk no longer finds them.
    fn kill(&mut self) {
        let Some(pid) = self.0.take() else {
            return;
        };
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::{Pid, getpgrp};
            let own = getpgrp().as_raw();
            let mut groups = od_job::descendant_groups(pid);
            groups.extend(i32::try_from(pid));
            for g in groups.into_iter().filter(|&g| g > 1 && g != own) {
                let _ = killpg(Pid::from_raw(g), Signal::SIGKILL);
            }
        }
        #[cfg(windows)]
        od_job::kill(pid);
    }
}

impl Drop for KillTree {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn stdio_tools(server: &McpServer) -> Result<Vec<ToolInfo>, McpError> {
    let program = server.command.as_deref().unwrap_or_default();
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(&server.args)
        .env_remove("OD_API_TOKEN")
        .env_remove("OD_TOOL_TOKEN")
        .envs(&server.env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| McpError::Protocol(format!("could not start {program}: {e}")))?;
    #[cfg(windows)]
    if let (Some(pid), Some(handle)) = (child.id(), child.raw_handle()) {
        // Children it starts afterwards join the job.
        let _ = od_job::track(pid, handle);
    }
    let mut tree = KillTree(child.id());
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| McpError::Protocol("no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| McpError::Protocol("no stdout".into()))?;
    let mut lines = BufReader::new(stdout);
    let send = |msg: Value| {
        let mut line = msg.to_string();
        line.push('\n');
        line
    };
    stdin.write_all(send(initialize(1)).as_bytes()).await?;
    stdin.flush().await?;
    result_of(read_reply(&mut lines, 1).await?)?;
    stdin.write_all(send(initialized()).as_bytes()).await?;
    stdin
        .write_all(send(request(2, "tools/list", json!({}))).as_bytes())
        .await?;
    stdin.flush().await?;
    let tools = tools_of(&result_of(read_reply(&mut lines, 2).await?)?);
    drop(stdin);
    tree.kill();
    let _ = child.start_kill();
    let _ = child.wait().await;
    Ok(tools)
}

/// Reads newline-delimited messages until the response to `id`; server
/// requests, notifications and junk lines are skipped.
async fn read_reply<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    id: u64,
) -> Result<Value, McpError> {
    loop {
        let mut buf = Vec::new();
        let n = (&mut *reader)
            .take(MAX_MESSAGE as u64 + 1)
            .read_until(b'\n', &mut buf)
            .await?;
        if n == 0 {
            return Err(McpError::Protocol("the server closed its output".into()));
        }
        if buf.len() > MAX_MESSAGE {
            return Err(McpError::Protocol("message too large".into()));
        }
        if let Ok(msg) = serde_json::from_slice::<Value>(&buf) {
            if msg.get("id").and_then(Value::as_u64) == Some(id) && msg.get("method").is_none() {
                return Ok(msg);
            }
        }
    }
}

/// `resource_metadata="…"` from a `WWW-Authenticate: Bearer` challenge.
#[must_use]
pub fn resource_metadata_hint(headers: &HeaderMap) -> Option<String> {
    headers.get_all(WWW_AUTHENTICATE).iter().find_map(|v| {
        let v = v.to_str().ok()?;
        let rest = &v[v.find("resource_metadata=")? + "resource_metadata=".len()..];
        let value = rest.strip_prefix('"').map_or_else(
            || rest.split([',', ' ']).next().unwrap_or_default(),
            |r| r.split('"').next().unwrap_or_default(),
        );
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// Reads a response body up to `max` bytes.
///
/// # Errors
/// Too large, or the transfer failed.
pub async fn read_limited(mut res: reqwest::Response, max: usize) -> Result<Vec<u8>, McpError> {
    let mut out = Vec::new();
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| McpError::Http(e.to_string()))?
    {
        if out.len() + chunk.len() > max {
            return Err(McpError::Http("response too large".into()));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Finds the response to `id` in an SSE body.
fn from_sse(body: &str, id: u64) -> Option<Value> {
    let mut data = String::new();
    for line in body.lines().chain(std::iter::once("")) {
        if let Some(d) = line.strip_prefix("data:") {
            data.push_str(d.trim_start());
            data.push('\n');
        } else if line.is_empty() && !data.is_empty() {
            if let Ok(msg) = serde_json::from_str::<Value>(&data) {
                if msg.get("id").and_then(Value::as_u64) == Some(id) {
                    return Some(msg);
                }
            }
            data.clear();
        }
    }
    None
}

fn base_headers(server: &McpServer, bearer: Option<&str>) -> Result<HeaderMap, McpError> {
    let mut h = HeaderMap::new();
    h.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let mut user_auth = false;
    for (k, v) in &server.headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| McpError::Invalid(format!("header {k:?}")))?;
        if name == AUTHORIZATION {
            if v.trim().is_empty() {
                continue;
            }
            user_auth = true;
        }
        let mut value =
            HeaderValue::from_str(v).map_err(|_| McpError::Invalid(format!("header {k:?}")))?;
        value.set_sensitive(true);
        h.insert(name, value);
    }
    if let (Some(token), false) = (bearer, user_auth) {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| McpError::Invalid("token".into()))?;
        value.set_sensitive(true);
        h.insert(AUTHORIZATION, value);
    }
    Ok(h)
}

async fn http_tools(
    server: &McpServer,
    http: &reqwest::Client,
    bearer: Option<&str>,
) -> Result<Vec<ToolInfo>, McpError> {
    let url = crate::config::check_url(server.url.as_deref().unwrap_or_default())?;
    let mut headers = base_headers(server, bearer)?;
    let post = |headers: HeaderMap, body: Value| {
        http.post(url.clone()).headers(headers).json(&body).send()
    };

    let res = post(headers.clone(), initialize(1))
        .await
        .map_err(|e| McpError::Http(format!("cannot reach {}: {e}", url.as_str())))?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(McpError::NeedsAuth {
            resource_metadata: resource_metadata_hint(res.headers()),
        });
    }
    if let Some(session) = res.headers().get(SESSION_HEADER).cloned() {
        headers.insert(SESSION_HEADER, session);
    }
    result_of(http_reply(res, 1).await?)?;
    headers.insert(
        "mcp-protocol-version",
        HeaderValue::from_static(PROTOCOL_VERSION),
    );
    let _ = post(headers.clone(), initialized()).await;
    let res = post(headers, request(2, "tools/list", json!({})))
        .await
        .map_err(|e| McpError::Http(e.to_string()))?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(McpError::NeedsAuth {
            resource_metadata: resource_metadata_hint(res.headers()),
        });
    }
    Ok(tools_of(&result_of(http_reply(res, 2).await?)?))
}

async fn http_reply(res: reqwest::Response, id: u64) -> Result<Value, McpError> {
    let status = res.status();
    if !status.is_success() {
        return Err(McpError::Http(format!("server answered {status}")));
    }
    let sse = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let body = read_limited(res, MAX_MESSAGE).await?;
    if sse {
        from_sse(&String::from_utf8_lossy(&body), id)
            .ok_or_else(|| McpError::Protocol("no response in the event stream".into()))
    } else {
        serde_json::from_slice(&body).map_err(|e| McpError::Protocol(format!("bad JSON: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS `ps` is setuid root: inside `sandbox-exec` (how the maintainer's pre-merge check runs
    /// `make check`) it cannot be executed, so the process-tree cleanup these
    /// tests check has nothing to list. They skip there and run everywhere else.
    #[cfg(unix)]
    fn ps_unavailable() -> bool {
        std::process::Command::new("ps")
            .args(["-p", "1"])
            .output()
            .is_err()
    }

    #[test]
    fn www_authenticate_hint() {
        let mut h = HeaderMap::new();
        h.insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Bearer realm="x", resource_metadata="https://a.example/.well-known/oauth-protected-resource""#),
        );
        assert_eq!(
            resource_metadata_hint(&h).as_deref(),
            Some("https://a.example/.well-known/oauth-protected-resource")
        );
        assert_eq!(resource_metadata_hint(&HeaderMap::new()), None);
    }

    #[test]
    fn sse_bodies() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/x\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\n\
                    data: \"result\":{\"tools\":[{\"name\":\"t\"}]}}\n\n";
        let msg = from_sse(body, 2).unwrap();
        assert_eq!(tools_of(&result_of(msg).unwrap())[0].name, "t");
        assert!(from_sse(body, 3).is_none());
    }

    /// Pids whose command line is exactly `sleep <secs>`.
    #[cfg(unix)]
    fn sleepers(secs: &str) -> String {
        let out = std::process::Command::new("pgrep")
            .args(["-f", &format!("^sleep {secs}$")])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A grandchild that left the server's group and session (`setsid`,
    /// as daemonizing helpers do) ends with the server too.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_abandoned_stdio_server_leaves_no_session_leavers() {
        if ps_unavailable() {
            eprintln!("skipping: ps cannot run in this sandbox");
            return;
        }
        let secs = (900_000 + std::process::id() % 100_000).to_string();
        let server: McpServer = serde_json::from_value(json!({
            "id": "detached",
            "transport": "stdio",
            "command": "/bin/sh",
            "args": ["-c", format!("perl -MPOSIX -e 'setsid(); exec q(sleep), {secs}' & wait")],
        }))
        .unwrap();
        let http = reqwest::Client::new();
        let res = tokio::time::timeout(
            Duration::from_millis(1000),
            list_tools(&server, &http, None),
        )
        .await;
        assert!(res.is_err(), "the server never answers");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut left = sleepers(&secs);
        while !left.is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            left = sleepers(&secs);
        }
        if !left.is_empty() {
            let _ = std::process::Command::new("pkill")
                .args(["-9", "-f", &format!("^sleep {secs}$")])
                .status();
        }
        assert!(
            left.is_empty(),
            "the detached grandchild outlived the test: {left}"
        );
    }

    /// A server that answers ends with its detached grandchild too: the tree
    /// is walked before the server is reaped, while the grandchild is still
    /// its descendant.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_finished_stdio_server_leaves_no_session_leavers() {
        if ps_unavailable() {
            eprintln!("skipping: ps cannot run in this sandbox");
            return;
        }
        let secs = (700_000 + std::process::id() % 100_000).to_string();
        let script = format!(
            r#"perl -MPOSIX -e 'setsid(); exec q(sleep), {secs}' & sleep 0.5
read l; echo '{{"jsonrpc":"2.0","id":1,"result":{{}}}}'
read l; read l; echo '{{"jsonrpc":"2.0","id":2,"result":{{"tools":[{{"name":"t"}}]}}}}'
wait"#
        );
        let server: McpServer = serde_json::from_value(json!({
            "id": "answers",
            "transport": "stdio",
            "command": "/bin/sh",
            "args": ["-c", script],
        }))
        .unwrap();
        let http = reqwest::Client::new();
        let tools = list_tools(&server, &http, None).await.unwrap();
        assert_eq!(tools[0].name, "t");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut left = sleepers(&secs);
        while !left.is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            left = sleepers(&secs);
        }
        if !left.is_empty() {
            let _ = std::process::Command::new("pkill")
                .args(["-9", "-f", &format!("^sleep {secs}$")])
                .status();
        }
        assert!(
            left.is_empty(),
            "the detached grandchild outlived the test: {left}"
        );
    }

    /// A test given up on (timeout, or the request dropped) also ends what
    /// the server started: `npx` and its node, here `sh` and its `sleep`.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_abandoned_stdio_server_leaves_no_children() {
        let secs = (800_000 + std::process::id() % 100_000).to_string();
        let server: McpServer = serde_json::from_value(json!({
            "id": "slow",
            "transport": "stdio",
            "command": "/bin/sh",
            "args": ["-c", format!("sleep {secs}; true")],
        }))
        .unwrap();
        let http = reqwest::Client::new();
        let res =
            tokio::time::timeout(Duration::from_millis(500), list_tools(&server, &http, None))
                .await;
        assert!(res.is_err(), "the server never answers");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut left = sleepers(&secs);
        while !left.is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            left = sleepers(&secs);
        }
        if !left.is_empty() {
            let _ = std::process::Command::new("pkill")
                .args(["-9", "-f", &format!("^sleep {secs}$")])
                .status();
        }
        assert!(
            left.is_empty(),
            "the server's child outlived the test: {left}"
        );
    }
}
