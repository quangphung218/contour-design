//! `od mcp live-artifacts` (spec 2.5): a second stdio MCP server with the
//! live artifact tools. Each tool is one call to `/api/tools/live-artifacts/*`
//! with the run's `OD_TOOL_TOKEN` (the daemon derives the project from it);
//! without a token, the project comes from `--project` / `OD_PROJECT_ID` or
//! the tool's `projectId` argument. Connector tools of the original are not
//! offered (no third-party connectors in this build).

use crate::client::{Daemon, DaemonError};
use crate::tools::ToolOutput;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

/// Environment variables read by [`serve_live_stdio`] callers.
pub const TOOL_TOKEN_ENV: &str = "OD_TOOL_TOKEN";
pub const PROJECT_ENV: &str = "OD_PROJECT_ID";

const INSTRUCTIONS: &str = "OpenDesign live artifacts (local). Register an HTML page rendered from template.html ({{data.x}} bindings, data-od-repeat=\"item in data.items\") plus data.json, and refresh it from a project JSON file. Templates must not contain scripts, iframes, event handlers or javascript: URLs.";

const INPUT_DESC: &str = "LiveArtifactCreateInput / LiveArtifactUpdateInput JSON: title, slug, pinned, status, preview {type, entry}, document {dataJson, dataSchemaJson, sourceJson {type: local_file|daemon_tool, toolName: project_files.read_json|project_files.search, input, outputMapping {dataPaths [{from, to}]}, refreshPermission: none|manual_refresh_granted_for_read_only}}.";

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

pub static LIVE_TOOL_DEFS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    let project = json!({ "type": "string", "description": "Project id. Optional when the server runs with an OD_TOOL_TOKEN or --project." });
    let artifact = json!({ "type": "string", "description": "Live artifact id (la-…)." });
    let template = json!({ "type": "string", "description": "template.html (html_template_v1). No scripts, iframes, on* handlers or javascript: URLs." });
    let provenance = json!({ "type": "object", "description": "provenance.json: generatedAt, generatedBy, notes, sources[]." });
    let input =
        json!({ "type": "object", "description": INPUT_DESC, "additionalProperties": true });
    vec![
        tool(
            "live_artifacts_create",
            "Create a project-scoped live artifact (template + data, rendered to index.html).",
            json!({ "projectId": project, "input": input, "templateHtml": template, "provenanceJson": provenance }),
            &["input"],
        ),
        tool(
            "live_artifacts_list",
            "List the project's live artifacts (compact, without documents).",
            json!({ "projectId": project }),
            &[],
        ),
        tool(
            "live_artifacts_update",
            "Update a live artifact's fields, document, template or provenance.",
            json!({ "projectId": project, "artifactId": artifact, "input": input, "templateHtml": template, "provenanceJson": provenance }),
            &["artifactId"],
        ),
        tool(
            "live_artifacts_refresh",
            "Refresh a live artifact from its source: updates data.json and index.html and appends to refreshes.jsonl.",
            json!({ "projectId": project, "artifactId": artifact }),
            &["artifactId"],
        ),
    ]
});

#[derive(Clone)]
pub struct LiveMcp {
    daemon: Daemon,
    project: Option<String>,
}

impl LiveMcp {
    #[must_use]
    pub fn new(daemon: Daemon, project: Option<String>) -> Self {
        Self { daemon, project }
    }

    /// Body for a tool endpoint: the caller's arguments that the endpoint
    /// knows, plus the default project when none was given.
    fn body(&self, args: &Map<String, Value>, keys: &[&str]) -> Value {
        let mut body = Map::new();
        for k in keys {
            if let Some(v) = args.get(*k).filter(|v| !v.is_null()) {
                body.insert((*k).to_owned(), v.clone());
            }
        }
        if !body.contains_key("projectId") {
            if let Some(p) = &self.project {
                body.insert("projectId".to_owned(), json!(p));
            }
        }
        Value::Object(body)
    }

    /// Runs one tool; daemon errors become `isError` results.
    pub async fn call(&self, name: &str, args: Map<String, Value>) -> ToolOutput {
        let result: Result<Value, DaemonError> = match name {
            "live_artifacts_create" => {
                let body = self.body(
                    &args,
                    &["projectId", "input", "templateHtml", "provenanceJson"],
                );
                self.daemon
                    .post(&["api", "tools", "live-artifacts", "create"], &body)
                    .await
            }
            "live_artifacts_list" => {
                let body = self.body(&args, &["projectId"]);
                let query: Vec<(&str, &str)> = body
                    .get("projectId")
                    .and_then(Value::as_str)
                    .map(|p| vec![("projectId", p)])
                    .unwrap_or_default();
                self.daemon
                    .get_query(&["api", "tools", "live-artifacts", "list"], &query)
                    .await
            }
            "live_artifacts_update" => {
                let body = self.body(
                    &args,
                    &[
                        "projectId",
                        "artifactId",
                        "input",
                        "templateHtml",
                        "provenanceJson",
                    ],
                );
                self.daemon
                    .post(&["api", "tools", "live-artifacts", "update"], &body)
                    .await
            }
            "live_artifacts_refresh" => {
                let body = self.body(&args, &["projectId", "artifactId"]);
                self.daemon
                    .post(&["api", "tools", "live-artifacts", "refresh"], &body)
                    .await
            }
            other => return ToolOutput::error(format!("unknown tool {other}")),
        };
        match result {
            Ok(v) => {
                let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                ToolOutput {
                    text: vec![text],
                    structured: v.is_object().then_some(v),
                    is_error: false,
                }
            }
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

fn from_json<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, McpError> {
    serde_json::from_value(v).map_err(|e| McpError::internal_error(e.to_string(), None))
}

impl ServerHandler for LiveMcp {
    fn get_info(&self) -> ServerConfig {
        let caps: ServerCapabilities =
            serde_json::from_value(json!({ "tools": {} })).unwrap_or_default();
        ServerConfig::new(caps)
            .with_server_info(Implementation::new(
                "open-design-live-artifacts",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        from_json(json!({ "tools": *LIVE_TOOL_DEFS }))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.clone().unwrap_or_default();
        let out = self.call(&request.name, args).await;
        let content: Vec<Value> = out
            .text
            .into_iter()
            .map(|t| json!({ "type": "text", "text": t }))
            .collect();
        let mut v = json!({ "content": content, "isError": out.is_error });
        if let Some(s) = out.structured {
            v["structuredContent"] = s;
        }
        let result: CallToolResult = from_json(v)?;
        Ok(result.into())
    }
}

/// Serves the live artifact tools on stdin/stdout until the client
/// disconnects.
///
/// # Errors
/// An invalid daemon URL or token; transport errors during the handshake.
pub async fn serve_live_stdio(
    daemon_url: &str,
    tool_token: Option<&str>,
    project: Option<String>,
) -> anyhow::Result<()> {
    let server = LiveMcp::new(Daemon::with_bearer(daemon_url, tool_token)?, project);
    let running = server.serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}
