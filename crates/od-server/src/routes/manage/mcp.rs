//! Integrations ▸ "Connect Contour to your coding agent" (spec 2.5):
//!
//! - `GET /api/mcp/install-info` → how to start `od mcp` (this daemon's
//!   own binary and URL) and a ready-to-paste snippet per client.
//! - `POST /api/mcp/install/:agent` `{uninstall?}` registers or removes the
//!   server in claude, codex or cursor, like `od mcp install`.
//! - `GET /api/mcp/catalog` → template servers to add with one click.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use base64::Engine as _;
use od_mcp::install::{self, Agent, InstallOptions, LaunchSpec, SERVER_NAME};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSnippet {
    pub id: &'static str,
    pub name: &'static str,
    /// `command` (a shell line), `json` or `toml`.
    pub format: &'static str,
    pub snippet: String,
    /// Where the snippet goes, when it is a config file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<&'static str>,
    /// The daemon can install it itself (`POST /api/mcp/install/:id`).
    pub installable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deeplink: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    pub command: String,
    pub args: Vec<String>,
    pub daemon_url: Option<String>,
    pub clients: Vec<ClientSnippet>,
}

fn launch(state: &AppState) -> LaunchSpec {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .map_or_else(|| "od".to_owned(), |p| p.to_string_lossy().into_owned());
    let mut args = vec!["mcp".to_owned()];
    if let Some(url) = state.config.daemon_url.as_deref() {
        if url.trim_end_matches('/') != od_mcp::DEFAULT_DAEMON_URL {
            args.extend(["--daemon-url".to_owned(), url.to_owned()]);
        }
    }
    LaunchSpec { command: exe, args }
}

fn toml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// One snippet per client, in the original's order.
#[must_use]
pub fn snippets(spec: &LaunchSpec) -> Vec<ClientSnippet> {
    let server = json!({"command": spec.command, "args": spec.args});
    let shared = json!({"mcpServers": {SERVER_NAME: server}});
    let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap_or_default();
    let quoted = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let claude = format!(
        "claude mcp add-json --scope user {SERVER_NAME} {}",
        quoted(&server.to_string())
    );
    let codex = format!(
        "[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [{}]",
        toml_str(&spec.command),
        spec.args
            .iter()
            .map(|a| toml_str(a))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let cursor_link = format!(
        "cursor://anysphere.cursor-deeplink/mcp/install?name={SERVER_NAME}&config={}",
        base64::engine::general_purpose::STANDARD.encode(server.to_string())
    );
    let vscode = json!({"servers": {SERVER_NAME: {"type": "stdio", "command": spec.command, "args": spec.args}}});
    let zed = json!({"context_servers": {SERVER_NAME: {"source": "custom", "command": spec.command, "args": spec.args}}});
    let snippet = |id, name, format, snippet, config_path, installable| ClientSnippet {
        id,
        name,
        format,
        snippet,
        config_path,
        installable,
        deeplink: None,
    };
    vec![
        snippet("claude", "Claude Code", "command", claude, None, true),
        snippet(
            "codex",
            "Codex",
            "toml",
            codex,
            Some("~/.codex/config.toml"),
            true,
        ),
        ClientSnippet {
            deeplink: Some(cursor_link),
            ..snippet(
                "cursor",
                "Cursor",
                "json",
                pretty(&shared),
                Some("~/.cursor/mcp.json"),
                true,
            )
        },
        snippet(
            "kiro",
            "Kiro CLI",
            "json",
            pretty(&shared),
            Some("~/.kiro/settings/mcp.json"),
            false,
        ),
        snippet("vscode", "VS Code", "json", pretty(&vscode), None, false),
        snippet(
            "antigravity",
            "Antigravity",
            "json",
            pretty(&shared),
            None,
            false,
        ),
        snippet("zed", "Zed", "json", pretty(&zed), None, false),
        snippet(
            "windsurf",
            "Windsurf",
            "json",
            pretty(&shared),
            Some("~/.codeium/windsurf/mcp_config.json"),
            false,
        ),
    ]
}

pub async fn install_info(State(state): State<AppState>) -> ApiJson<InstallInfo> {
    let spec = launch(&state);
    ApiJson(InstallInfo {
        clients: snippets(&spec),
        command: spec.command,
        args: spec.args,
        daemon_url: state.config.daemon_url.clone(),
    })
}

#[derive(Debug, Default, Deserialize)]
pub struct InstallRequest {
    #[serde(default)]
    pub uninstall: bool,
}

#[derive(Debug, Serialize)]
pub struct InstallResponse {
    pub ok: bool,
    pub message: String,
}

pub async fn install_route(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<ApiJson<InstallResponse>> {
    let req: InstallRequest = if body.iter().all(u8::is_ascii_whitespace) {
        InstallRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiErr::bad_request(e.to_string()))?
    };
    let agent = Agent::parse(&agent).map_err(ApiErr::bad_request)?;
    let spec = launch(&state);
    let home = install::home_dir().ok_or_else(|| ApiErr::internal("HOME is not set"))?;
    let path = state.runs.engine.config().path.clone();
    let res = tokio::task::spawn_blocking(move || {
        let plan = install::plan(agent, &spec, &home, SERVER_NAME);
        let runner = |bin: &str, argv: &[String]| -> std::io::Result<i32> {
            let found = std::env::split_paths(&path)
                .map(|d| d.join(bin))
                .find(|p| p.is_file())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not on PATH"))?;
            let status = std::process::Command::new(found)
                .args(argv)
                .env("PATH", &path)
                .stdin(std::process::Stdio::null())
                .status()?;
            Ok(status.code().unwrap_or(1))
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = install::execute(
            agent,
            &plan,
            InstallOptions {
                print: false,
                uninstall: req.uninstall,
                json: true,
            },
            &runner,
            &mut out,
            &mut err,
        );
        (code, out)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    let (code, out) = res;
    let code = code.map_err(|e| ApiErr::internal(e.to_string()))?;
    let report: Value = serde_json::from_slice(&out).unwrap_or(Value::Null);
    let message = report.get("message").and_then(Value::as_str).map_or_else(
        || String::from_utf8_lossy(&out).trim().to_owned(),
        str::to_owned,
    );
    Ok(ApiJson(InstallResponse {
        ok: code == 0,
        message,
    }))
}

/// A server from the catalog, in `PUT /api/mcp/servers` shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: &'static str,
    pub group: &'static str,
    pub server: Value,
}

fn npx(pkg: &str) -> Value {
    let mut args = vec!["-y".to_owned()];
    args.extend(pkg.split_whitespace().map(str::to_owned));
    json!({"transport": "stdio", "command": "npx", "args": args})
}

fn uvx(args: &str) -> Value {
    json!({"transport": "stdio", "command": "uvx", "args": args.split_whitespace().collect::<Vec<_>>()})
}

fn http(url: &str) -> Value {
    json!({"transport": "http", "url": url})
}

/// The original's `MCP_TEMPLATES`, grouped as there.
#[must_use]
pub fn catalog() -> Vec<CatalogEntry> {
    let e = |id, group, server| CatalogEntry { id, group, server };
    vec![
        e(
            "higgsfield-openclaw",
            "image-generation",
            http("https://mcp.higgsfield.ai/mcp"),
        ),
        e(
            "pollinations",
            "image-generation",
            npx("@pollinations_ai/mcp"),
        ),
        e("allyson", "image-generation", npx("allyson-mcp")),
        e(
            "bedrock-image",
            "image-generation",
            uvx("bedrock-image-mcp-server@latest"),
        ),
        e(
            "prompt-to-asset",
            "image-generation",
            npx("prompt-to-asset"),
        ),
        e(
            "nanobanana",
            "image-generation",
            http("https://nanobanana.mcp.acedata.cloud/mcp"),
        ),
        e(
            "seedream",
            "image-generation",
            http("https://seedream.mcp.acedata.cloud/mcp"),
        ),
        e(
            "fal-ai",
            "image-generation",
            uvx("--from fal-mcp-server fal-mcp"),
        ),
        e("imagician", "image-editing", npx("@flowy11/imagician")),
        e(
            "imagesorcery",
            "image-editing",
            npx("@sunriseapps/imagesorcery-mcp"),
        ),
        e("photopea", "image-editing", npx("photopea-mcp-server")),
        e("topaz-labs", "image-editing", npx("@topazlabs/mcp")),
        e(
            "transloadit",
            "image-editing",
            npx("@transloadit/mcp-server stdio"),
        ),
        e(
            "screenshot-website-fast",
            "web-capture",
            npx("@just-every/mcp-screenshot-website-fast"),
        ),
        e("screenshotone", "web-capture", npx("@screenshotone/mcp")),
        e("pagecast", "web-capture", npx("@mcpware/pagecast")),
        e(
            "figma-context",
            "design-systems",
            npx("figma-developer-mcp --stdio"),
        ),
        e(
            "design-token-bridge",
            "design-systems",
            npx("design-token-bridge-mcp"),
        ),
        e(
            "design-system-extractor",
            "design-systems",
            npx("mcp-design-system-extractor@latest"),
        ),
        e(
            "figma-use",
            "design-systems",
            http("http://localhost:38451/mcp"),
        ),
        e(
            "aesthetics-wiki",
            "design-systems",
            uvx("aesthetics-wiki-mcp"),
        ),
        e(
            "21st-dev-magic",
            "ui-components",
            npx("@21st-dev/magic@latest API_KEY=__YOUR_API_KEY__"),
        ),
        e(
            "shadcn-ui",
            "ui-components",
            npx("@jpisnice/shadcn-ui-mcp-server"),
        ),
        e("flyonui", "ui-components", npx("flyonui-mcp")),
        e("antv-chart", "data-viz", npx("@antv/mcp-server-chart")),
        e("mermaid", "data-viz", npx("@peng-shawn/mermaid-mcp-server")),
        e("mcp-dashboards", "data-viz", npx("mcp-dashboards --stdio")),
        e(
            "excalidraw-architect",
            "data-viz",
            uvx("excalidraw-architect-mcp"),
        ),
        e(
            "edgeone-pages",
            "publishing",
            npx("edgeone-pages-mcp@latest"),
        ),
        e("pagedrop", "publishing", npx("pagedrop-mcp")),
        e("pdfspark", "publishing", npx("pdfspark-api")),
        e("ogforge", "publishing", npx("ogforge-api")),
        e("qrmint", "publishing", npx("qr-mcp")),
        e("slideshot", "publishing", npx("slideshot-mcp")),
        e(
            "deckrun",
            "publishing",
            http("https://deckrun-mcp-free.agenticdecks.com/mcp/"),
        ),
        e(
            "filesystem",
            "utilities",
            npx("@modelcontextprotocol/server-filesystem <allowed-dir>"),
        ),
        e(
            "github",
            "utilities",
            npx("@modelcontextprotocol/server-github"),
        ),
        e(
            "fetch",
            "utilities",
            npx("@modelcontextprotocol/server-fetch"),
        ),
        e("a11y", "utilities", npx("a11y-mcp-server")),
    ]
}

#[derive(Debug, Serialize)]
pub struct CatalogResponse {
    pub templates: Vec<CatalogEntry>,
}

pub async fn catalog_route() -> ApiJson<CatalogResponse> {
    ApiJson(CatalogResponse {
        templates: catalog(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_carry_the_real_launch_command() {
        let spec = LaunchSpec {
            command: "/opt/od's/od".to_owned(),
            args: vec![
                "mcp".to_owned(),
                "--daemon-url".to_owned(),
                "http://127.0.0.1:9".to_owned(),
            ],
        };
        let all = snippets(&spec);
        assert_eq!(all.len(), 8);
        let claude = &all[0].snippet;
        assert!(claude.starts_with("claude mcp add-json --scope user open-design '"));
        assert!(
            claude.contains(r"od'\''s"),
            "single quotes are escaped: {claude}"
        );
        assert!(all[1].snippet.contains("args = [\"mcp\", \"--daemon-url\""));
        let cursor: Value = serde_json::from_str(&all[2].snippet).unwrap_or(Value::Null);
        assert_eq!(
            cursor["mcpServers"]["open-design"]["command"],
            "/opt/od's/od"
        );
        assert!(
            all[2]
                .deeplink
                .as_deref()
                .is_some_and(|d| d.starts_with("cursor://"))
        );
    }

    #[test]
    fn catalog_ids_are_unique_and_valid_servers() {
        let all = catalog();
        let mut ids: Vec<_> = all.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        assert!(all.iter().all(|e| e.server["transport"].is_string()));
    }
}
