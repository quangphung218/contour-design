//! The stdio MCP server (spec 2.5). Stateless apart from brief drafts: every
//! tool and resource is an HTTP call to the daemon. Exits after 30 minutes
//! without a request.

use crate::brief::BriefStore;
use crate::client::Daemon;
use crate::tools::{self, TOOL_DEFS, ToolOutput};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Idle time after which `od mcp` exits.
pub const IDLE_EXIT: Duration = Duration::from_secs(30 * 60);

pub const FOCUS_URI: &str = "od://focus/active";

const INSTRUCTIONS: &str = "Contour (local). Read and write design projects, and commission Contour's own agent with start_run. \
Most tools take an optional project; when omitted they use the project the user has open in Contour (active for about 5 minutes after the last interaction). \
For a new artifact: collect_brief, confirm_brief, create_project, start_run, then poll get_run.";

/// Seconds since start of the last request, for the idle exit.
#[derive(Debug)]
struct Activity {
    start: Instant,
    last: AtomicU64,
}

impl Activity {
    fn touch(&self) {
        self.last
            .store(self.start.elapsed().as_secs(), Ordering::Relaxed);
    }

    fn idle(&self) -> Duration {
        let last = self.last.load(Ordering::Relaxed);
        self.start
            .elapsed()
            .saturating_sub(Duration::from_secs(last))
    }
}

#[derive(Clone)]
pub struct OdMcp {
    daemon: Daemon,
    briefs: Arc<BriefStore>,
    activity: Arc<Activity>,
}

impl OdMcp {
    #[must_use]
    pub fn new(daemon: Daemon) -> Self {
        Self {
            daemon,
            briefs: Arc::new(BriefStore::default()),
            activity: Arc::new(Activity {
                start: Instant::now(),
                last: AtomicU64::new(0),
            }),
        }
    }
}

fn from_json<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, McpError> {
    serde_json::from_value(v).map_err(|e| McpError::internal_error(e.to_string(), None))
}

fn to_result(out: ToolOutput) -> Result<CallToolResult, McpError> {
    let content: Vec<Value> = out
        .text
        .into_iter()
        .map(|t| json!({ "type": "text", "text": t }))
        .collect();
    let mut v = json!({ "content": content, "isError": out.is_error });
    if let Some(s) = out.structured {
        v["structuredContent"] = s;
    }
    from_json(v)
}

impl OdMcp {
    async fn resources(&self) -> Vec<Value> {
        let mut out = vec![json!({
            "uri": FOCUS_URI,
            "name": "Active Contour project",
            "description": "Project and file the user has open right now (JSON).",
            "mimeType": "application/json",
        })];
        // Daemon down: the focus resource is still listed.
        if let Ok(v) = self.daemon.get(&["api", "skills"]).await {
            for s in v
                .get("skills")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = s.get("id").and_then(Value::as_str) else {
                    continue;
                };
                out.push(json!({
                    "uri": format!("od://skills/{id}/SKILL.md"),
                    "name": s.get("name").and_then(Value::as_str).unwrap_or(id),
                    "description": s.get("description"),
                    "mimeType": "text/markdown",
                }));
            }
        }
        if let Ok(v) = self.daemon.get(&["api", "design-systems"]).await {
            for d in v
                .get("designSystems")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = d.get("id").and_then(Value::as_str) else {
                    continue;
                };
                out.push(json!({
                    "uri": format!("od://design-systems/{id}/DESIGN.md"),
                    "name": d.get("title").and_then(Value::as_str).unwrap_or(id),
                    "description": d.get("summary"),
                    "mimeType": "text/markdown",
                }));
            }
        }
        out
    }

    async fn read(&self, uri: &str) -> Result<(String, String), McpError> {
        let not_found = || McpError::resource_not_found(format!("unknown resource {uri}"), None);
        let daemon_err = |e: crate::client::DaemonError| match e.status() {
            Some(404) => McpError::resource_not_found(e.to_string(), None),
            _ => McpError::internal_error(e.to_string(), None),
        };
        if uri == FOCUS_URI {
            let v = self
                .daemon
                .get(&["api", "active"])
                .await
                .map_err(daemon_err)?;
            return Ok((
                "application/json".to_owned(),
                serde_json::to_string_pretty(&v).unwrap_or_default(),
            ));
        }
        let rest = uri.strip_prefix("od://").ok_or_else(not_found)?;
        let (kind, tail) = rest.split_once('/').ok_or_else(not_found)?;
        let (id, file) = tail.split_once('/').ok_or_else(not_found)?;
        let collection = match (kind, file) {
            ("skills", "SKILL.md") => "skills",
            ("design-systems", "DESIGN.md") => "design-systems",
            _ => return Err(not_found()),
        };
        let v = self
            .daemon
            .get(&["api", collection, id])
            .await
            .map_err(daemon_err)?;
        let body = v
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::internal_error("daemon response has no body", None))?;
        Ok(("text/markdown".to_owned(), body.to_owned()))
    }
}

impl ServerHandler for OdMcp {
    fn get_info(&self) -> ServerConfig {
        let caps: ServerCapabilities =
            serde_json::from_value(json!({ "tools": {}, "resources": {} })).unwrap_or_default();
        ServerConfig::new(caps)
            .with_server_info(Implementation::new(
                "open-design",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.activity.touch();
        from_json(json!({ "tools": *TOOL_DEFS }))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.activity.touch();
        let args = request.arguments.clone().unwrap_or_default();
        let out = tools::call(&self.daemon, &self.briefs, &request.name, args).await;
        self.activity.touch();
        Ok(to_result(out)?.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        self.activity.touch();
        from_json(json!({ "resources": self.resources().await }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        self.activity.touch();
        let (mime, text) = self.read(&request.uri).await?;
        let result: ReadResourceResult = from_json(json!({
            "contents": [{ "uri": request.uri, "mimeType": mime, "text": text }],
        }))?;
        Ok(result.into())
    }
}

/// Serves MCP on stdin/stdout until the client disconnects or the server
/// has been idle for [`IDLE_EXIT`].
///
/// # Errors
/// Transport errors during the handshake.
pub async fn serve_stdio(daemon_url: &str) -> anyhow::Result<()> {
    let server = OdMcp::new(Daemon::new(daemon_url)?);
    let activity = Arc::clone(&server.activity);
    let running = server.serve(rmcp::transport::stdio()).await?;
    let idle = async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if activity.idle() >= IDLE_EXIT {
                tracing::info!("od mcp idle for 30 minutes; exiting");
                return;
            }
        }
    };
    tokio::select! {
        res = running.waiting() => { res?; }
        () = idle => {}
    }
    Ok(())
}
