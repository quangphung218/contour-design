//! `<data>/mcp-config.json`: the external MCP servers the user declared,
//! in the shape Claude Code's `.mcp.json` and Cursor use. The file can hold
//! API keys (env, headers), so it is written 0600 and the API shows secret
//! values masked.

use crate::McpError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// Shown instead of env and header values; sending it back keeps the
/// stored value.
pub const MASK: &str = "••••••";
pub const MAX_SERVERS: usize = 32;
const MAX_ARGS: usize = 64;
const MAX_VALUE: usize = 8 * 1024;
const MAX_ENTRIES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// A local process speaking JSON-RPC over stdin/stdout.
    Stdio,
    /// Streamable HTTP (one endpoint, JSON or SSE responses).
    Http,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    None,
    /// OAuth 2.1 with PKCE; the daemon holds the tokens.
    Oauth,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpServer {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub transport: Transport,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub auth: AuthMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Pre-registered OAuth client id, for servers without dynamic
    /// client registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServer>,
}

fn invalid(msg: impl Into<String>) -> McpError {
    McpError::Invalid(msg.into())
}

/// Ids become keys in agent configs (`mcpServers.<id>`) and file names.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_".contains(c))
}

fn is_env_name(k: &str) -> bool {
    let mut chars = k.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && k.len() <= 128
}

fn is_header_name(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 128
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn plain(v: &str, what: &str) -> Result<(), McpError> {
    if v.len() > MAX_VALUE || v.chars().any(|c| c == '\0' || c == '\r' || c == '\n') {
        return Err(invalid(format!(
            "{what}: too long or has control characters"
        )));
    }
    Ok(())
}

/// An `http(s)` URL without credentials in it.
///
/// # Errors
/// [`McpError::Invalid`].
pub fn check_url(raw: &str) -> Result<url::Url, McpError> {
    let u = url::Url::parse(raw).map_err(|_| invalid(format!("invalid url {raw:?}")))?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return Err(invalid("url must be http(s)://host/…"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(invalid("put credentials in headers, not in the url"));
    }
    Ok(u)
}

impl McpServer {
    fn validate(&self) -> Result<(), McpError> {
        if !is_valid_id(&self.id) {
            return Err(invalid(format!(
                "server id {:?}: use 1–64 of a-z 0-9 - _",
                self.id
            )));
        }
        if let Some(label) = &self.label {
            plain(label, "label")?;
        }
        match self.transport {
            Transport::Stdio => {
                let cmd = self.command.as_deref().unwrap_or("").trim();
                if cmd.is_empty() {
                    return Err(invalid(format!("{}: stdio needs a command", self.id)));
                }
                plain(cmd, "command")?;
                if self.args.len() > MAX_ARGS {
                    return Err(invalid("too many args"));
                }
                for a in &self.args {
                    plain(a, "arg")?;
                }
                if self.url.is_some() || !self.headers.is_empty() || self.auth == AuthMode::Oauth {
                    return Err(invalid(format!(
                        "{}: url, headers and oauth are for http",
                        self.id
                    )));
                }
            }
            Transport::Http => {
                check_url(self.url.as_deref().unwrap_or(""))?;
                if self.command.is_some() || !self.args.is_empty() || !self.env.is_empty() {
                    return Err(invalid(format!(
                        "{}: command, args and env are for stdio",
                        self.id
                    )));
                }
            }
        }
        if self.env.len() > MAX_ENTRIES || self.headers.len() > MAX_ENTRIES {
            return Err(invalid("too many env or header entries"));
        }
        for (k, v) in &self.env {
            if !is_env_name(k) {
                return Err(invalid(format!("env name {k:?}")));
            }
            plain(v, "env value")?;
        }
        for (k, v) in &self.headers {
            if !is_header_name(k) {
                return Err(invalid(format!("header name {k:?}")));
            }
            plain(v, "header value")?;
        }
        if let Some(c) = &self.oauth_client_id {
            plain(c, "oauthClientId")?;
        }
        Ok(())
    }

    /// The same server with env and header values replaced by [`MASK`].
    #[must_use]
    pub fn masked(&self) -> Self {
        let mask =
            |m: &BTreeMap<String, String>| m.keys().map(|k| (k.clone(), MASK.to_owned())).collect();
        Self {
            env: mask(&self.env),
            headers: mask(&self.headers),
            ..self.clone()
        }
    }
}

impl McpConfig {
    /// Checks every server and that ids are unique.
    ///
    /// # Errors
    /// [`McpError::Invalid`] naming the first problem.
    pub fn validate(&self) -> Result<(), McpError> {
        if self.servers.len() > MAX_SERVERS {
            return Err(invalid(format!("at most {MAX_SERVERS} servers")));
        }
        let mut seen = std::collections::HashSet::new();
        for s in &self.servers {
            s.validate()?;
            if !seen.insert(s.id.as_str()) {
                return Err(invalid(format!("duplicate server id {:?}", s.id)));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn find(&self, id: &str) -> Option<&McpServer> {
        self.servers.iter().find(|s| s.id == id)
    }

    /// Restores masked values from `old` (same server id and key). Headers
    /// only while the URL stays, env only while the command stays: they
    /// were given for that host or program.
    ///
    /// # Errors
    /// [`McpError::Invalid`] when a masked value has nothing to restore.
    pub fn unmask(&mut self, old: &Self) -> Result<(), McpError> {
        for s in &mut self.servers {
            let prev = old.find(&s.id);
            let same_url = prev.filter(|p| p.url == s.url);
            let same_command = prev.filter(|p| p.command == s.command);
            let restore = |m: &mut BTreeMap<String, String>,
                           prev: Option<&BTreeMap<String, String>>|
             -> Result<(), McpError> {
                for (k, v) in m.iter_mut() {
                    if v == MASK {
                        *v = prev.and_then(|p| p.get(k)).cloned().ok_or_else(|| {
                            invalid(format!("no stored value for {k:?}: enter it again"))
                        })?;
                    }
                }
                Ok(())
            };
            restore(&mut s.env, same_command.map(|p| &p.env))?;
            restore(&mut s.headers, same_url.map(|p| &p.headers))?;
        }
        Ok(())
    }
}

/// Reads the config; a missing file is an empty config.
///
/// # Errors
/// Unreadable or malformed file.
pub fn load(path: &Path) -> Result<McpConfig, McpError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            tighten(path);
            let cfg: McpConfig = serde_json::from_slice(&bytes)
                .map_err(|e| McpError::Invalid(format!("{}: {e}", path.display())))?;
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(McpConfig::default()),
        Err(e) => Err(McpError::Io(e)),
    }
}

/// Validates and writes the config (0600).
///
/// # Errors
/// Validation or I/O errors.
pub fn save(path: &Path, cfg: &McpConfig) -> Result<(), McpError> {
    cfg.validate()?;
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| McpError::Invalid(e.to_string()))?;
    write_private(path, &bytes)?;
    Ok(())
}

/// Brings a secrets file someone created with wider permissions to 0600.
pub(crate) fn tighten(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.permissions().mode() & 0o077 != 0 {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Writes `bytes` to `path` atomically, readable by the owner only. On
/// Windows the file inherits the per-user data folder's ACL.
///
/// # Errors
/// I/O errors.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: serde_json::Value) -> McpConfig {
        serde_json::from_value(json).expect("config")
    }

    #[test]
    fn masked_values_come_back_only_for_the_same_host_or_program() {
        let old = cfg(serde_json::json!({"servers": [
            {"id": "h", "transport": "http", "url": "https://a.example/mcp", "headers": {"K": "ka"}},
            {"id": "s", "transport": "stdio", "command": "srv-a", "env": {"T": "ta"}}
        ]}));
        let mut same = old.masked_all();
        same.unmask(&old).expect("same host and program");
        assert_eq!(same, old);

        let mut moved = old.masked_all();
        moved.servers[0].url = Some("https://b.example/mcp".into());
        assert!(moved.unmask(&old).is_err(), "header for another host");
        let mut swapped = old.masked_all();
        swapped.servers[1].command = Some("srv-b".into());
        assert!(swapped.unmask(&old).is_err(), "env for another program");
    }

    impl McpConfig {
        fn masked_all(&self) -> Self {
            Self {
                servers: self.servers.iter().map(McpServer::masked).collect(),
            }
        }
    }
}
