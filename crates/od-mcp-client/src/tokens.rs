//! `<data>/mcp-tokens.json` (0600): OAuth tokens per server and the client
//! ids dynamic registration handed out per authorization server.
//!
//! Tokens never leave the daemon except to the agent it spawns: the API
//! reports only whether a server is connected, and `Debug` output (and so
//! any log line) shows them redacted.

use crate::McpError;
use crate::config::{tighten, write_private};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// A token is refreshed this long before it expires.
pub const REFRESH_MARGIN_MS: i64 = 60_000;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Epoch milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub token_endpoint: String,
    pub client_id: String,
    /// The MCP server URL the token is bound to (RFC 8707).
    pub resource: String,
}

impl std::fmt::Debug for StoredToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredToken")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .field("token_endpoint", &self.token_endpoint)
            .field("client_id", &self.client_id)
            .finish()
    }
}

impl StoredToken {
    /// Whether it expires within the refresh margin of `now_ms`.
    #[must_use]
    pub fn is_stale(&self, now_ms: i64) -> bool {
        self.expires_at
            .is_some_and(|t| t <= now_ms.saturating_add(REFRESH_MARGIN_MS))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenFile {
    #[serde(default)]
    servers: BTreeMap<String, StoredToken>,
    /// Authorization server issuer → registered client id.
    #[serde(default)]
    clients: BTreeMap<String, String>,
}

/// What the API may say about a server's OAuth state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatus {
    pub connected: bool,
    pub expires_at: Option<i64>,
}

/// The token file, read and rewritten whole under one lock.
#[derive(Debug)]
pub struct TokenStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl TokenStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> TokenFile {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return TokenFile::default();
        };
        tighten(&self.path);
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ignoring an unreadable MCP token file");
            TokenFile::default()
        })
    }

    fn update<T>(&self, f: impl FnOnce(&mut TokenFile) -> T) -> Result<T, McpError> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut file = self.read();
        let out = f(&mut file);
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|e| McpError::Invalid(e.to_string()))?;
        write_private(&self.path, &bytes)?;
        Ok(out)
    }

    #[must_use]
    pub fn get(&self, server_id: &str) -> Option<StoredToken> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.read().servers.remove(server_id)
    }

    #[must_use]
    pub fn status(&self, server_id: &str) -> OAuthStatus {
        self.get(server_id)
            .map_or_else(OAuthStatus::default, |t| OAuthStatus {
                connected: true,
                expires_at: t.expires_at,
            })
    }

    /// # Errors
    /// I/O errors.
    pub fn put(&self, server_id: &str, token: StoredToken) -> Result<(), McpError> {
        self.update(|f| {
            f.servers.insert(server_id.to_owned(), token);
        })
    }

    /// Forgets a server's token. Returns whether there was one.
    ///
    /// # Errors
    /// I/O errors.
    pub fn remove(&self, server_id: &str) -> Result<bool, McpError> {
        self.update(|f| f.servers.remove(server_id).is_some())
    }

    /// Drops tokens of servers no longer configured.
    ///
    /// # Errors
    /// I/O errors.
    pub fn retain(&self, keep: impl Fn(&str) -> bool) -> Result<(), McpError> {
        self.update(|f| f.servers.retain(|k, _| keep(k)))
    }

    #[must_use]
    pub fn client_id(&self, issuer: &str) -> Option<String> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.read().clients.remove(issuer)
    }

    /// # Errors
    /// I/O errors.
    pub fn set_client_id(&self, issuer: &str, client_id: &str) -> Result<(), McpError> {
        self.update(|f| {
            f.clients.insert(issuer.to_owned(), client_id.to_owned());
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> StoredToken {
        StoredToken {
            access_token: "at-secret".into(),
            refresh_token: Some("rt-secret".into()),
            expires_at: Some(10_000),
            token_endpoint: "https://a/token".into(),
            client_id: "cid".into(),
            resource: "https://a/mcp".into(),
        }
    }

    #[test]
    fn debug_output_never_shows_tokens() {
        let shown = format!("{:?}", token());
        assert!(
            !shown.contains("at-secret") && !shown.contains("rt-secret"),
            "{shown}"
        );
    }

    #[test]
    fn store_roundtrip_is_private_and_stale_check() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path().join("t.json"));
        assert!(!store.status("s").connected);
        store.put("s", token()).unwrap();
        assert_eq!(
            store.status("s"),
            OAuthStatus {
                connected: true,
                expires_at: Some(10_000)
            }
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(token().is_stale(10_000 - REFRESH_MARGIN_MS));
        assert!(!token().is_stale(10_000 - REFRESH_MARGIN_MS - 1));
        store.retain(|id| id != "s").unwrap();
        assert!(store.get("s").is_none());
    }
}
