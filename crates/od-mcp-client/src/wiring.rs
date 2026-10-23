//! Spawn-time wiring: the enabled servers in each agent's own format.
//!
//! - Claude Code: a `{"mcpServers": …}` file passed with `--mcp-config`,
//!   written 0600 under `<data>/mcp-runs/` and removed when the run ends
//!   (a project-folder `.mcp.json` could clobber the user's own file).
//! - ACP agents: the `mcpServers` of `session/new` (stdio only, as ACP
//!   models it).
//! - OpenCode: `OPENCODE_CONFIG_CONTENT`, merged over the user's config.
//!
//! OAuth servers get `Authorization: Bearer …` unless the user pinned a
//! non-empty Authorization header themselves.

use crate::config::{AuthMode, McpServer, Transport};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Folder of per-run Claude config files.
#[must_use]
pub fn run_config_dir(data_root: &Path) -> PathBuf {
    data_root.join("mcp-runs")
}

fn headers_with_token(s: &McpServer, token: Option<&String>) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = s
        .headers
        .iter()
        .filter(|(k, v)| !(k.eq_ignore_ascii_case("authorization") && v.trim().is_empty()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let pinned = out.keys().any(|k| k.eq_ignore_ascii_case("authorization"));
    if let (Some(t), false, AuthMode::Oauth) = (token, pinned, s.auth) {
        out.insert("Authorization".into(), format!("Bearer {t}"));
    }
    out
}

/// The servers in every supported agent format.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentMcp {
    /// Claude Code `--mcp-config` document.
    pub claude: Option<Value>,
    /// ACP `session/new` `mcpServers`.
    pub acp: Vec<Value>,
    /// OpenCode `OPENCODE_CONFIG_CONTENT`.
    pub opencode: Option<Value>,
}

/// Builds the wiring for `servers` (disabled ones are left out).
/// `tokens` maps server id → OAuth access token.
#[must_use]
pub fn build(servers: &[McpServer], tokens: &HashMap<String, String>) -> AgentMcp {
    let enabled: Vec<&McpServer> = servers.iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        return AgentMcp::default();
    }
    let mut claude = Map::new();
    let mut opencode = Map::new();
    let mut acp = Vec::new();
    for s in enabled {
        let token = tokens.get(&s.id);
        match s.transport {
            Transport::Stdio => {
                let command = s.command.clone().unwrap_or_default();
                claude.insert(
                    s.id.clone(),
                    json!({"command": command, "args": s.args, "env": s.env}),
                );
                let mut argv = vec![command.clone()];
                argv.extend(s.args.iter().cloned());
                opencode.insert(
                    s.id.clone(),
                    json!({"type": "local", "command": argv, "environment": s.env, "enabled": true}),
                );
                acp.push(json!({
                    "name": s.id,
                    "command": command,
                    "args": s.args,
                    "env": s.env.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
                }));
            }
            Transport::Http => {
                let headers = headers_with_token(s, token);
                let url = s.url.clone().unwrap_or_default();
                claude.insert(
                    s.id.clone(),
                    json!({"type": "http", "url": url, "headers": headers}),
                );
                opencode.insert(
                    s.id.clone(),
                    json!({"type": "remote", "url": url, "headers": headers, "enabled": true}),
                );
            }
        }
    }
    AgentMcp {
        claude: Some(json!({ "mcpServers": claude })),
        acp,
        opencode: Some(json!({ "mcp": opencode })),
    }
}

/// Writes a run's Claude config (0600) and returns its path.
///
/// # Errors
/// I/O errors.
pub fn write_run_config(data_root: &Path, run_id: &str, doc: &Value) -> std::io::Result<PathBuf> {
    let safe: String = run_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let path = run_config_dir(data_root).join(format!("{safe}.json"));
    crate::config::write_private(&path, doc.to_string().as_bytes())?;
    Ok(path)
}

/// Removes leftovers of runs a crashed daemon never cleaned up.
pub fn wipe_run_configs(data_root: &Path) {
    let dir = run_config_dir(data_root);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpConfig;

    fn cfg() -> McpConfig {
        serde_json::from_value(json!({"servers": [
            {"id": "fs", "transport": "stdio", "command": "npx", "args": ["-y", "srv"], "env": {"ROOT": "/tmp"}},
            {"id": "remote", "transport": "http", "url": "https://mcp.example/mcp", "auth": "oauth",
             "headers": {"Authorization": " ", "X-Team": "a"}},
            {"id": "pinned", "transport": "http", "url": "https://p.example/mcp", "auth": "oauth",
             "headers": {"authorization": "Bearer user-pat"}},
            {"id": "off", "transport": "stdio", "command": "x", "enabled": false}
        ]}))
        .unwrap()
    }

    #[test]
    fn every_format_and_bearer_merge() {
        let tokens = HashMap::from([
            ("remote".to_owned(), "tok-1".to_owned()),
            ("pinned".to_owned(), "tok-2".to_owned()),
        ]);
        let w = build(&cfg().servers, &tokens);
        let c = &w.claude.unwrap()["mcpServers"];
        assert_eq!(
            c["fs"],
            json!({"command": "npx", "args": ["-y", "srv"], "env": {"ROOT": "/tmp"}})
        );
        assert_eq!(
            c["remote"]["headers"],
            json!({"Authorization": "Bearer tok-1", "X-Team": "a"})
        );
        assert_eq!(
            c["pinned"]["headers"],
            json!({"authorization": "Bearer user-pat"}),
            "user header wins"
        );
        assert!(c.get("off").is_none());
        assert_eq!(w.acp.len(), 1, "ACP gets stdio servers only");
        assert_eq!(w.acp[0]["env"], json!([{"name": "ROOT", "value": "/tmp"}]));
        let o = &w.opencode.unwrap()["mcp"];
        assert_eq!(o["fs"]["command"], json!(["npx", "-y", "srv"]));
        assert_eq!(o["remote"]["type"], "remote");
        assert_eq!(build(&[], &tokens), AgentMcp::default());
    }

    #[test]
    fn run_configs_are_private_and_wiped() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_run_config(dir.path(), "../../etc/x", &json!({"a": 1})).unwrap();
        assert_eq!(p.parent().unwrap(), run_config_dir(dir.path()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        wipe_run_configs(dir.path());
        assert!(!p.exists());
    }
}
