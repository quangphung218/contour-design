//! External MCP servers for agents (spec 1.5 "MCP client"): the user
//! declares stdio or HTTP servers, the daemon tests them, runs the OAuth
//! flow when a server asks for it, and hands them to the agent at spawn.
//!
//! Secrets (env and header values, OAuth tokens) live in 0600 files under
//! the data dir, are masked in API responses and never logged.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod client;
pub mod config;
pub mod oauth;
pub mod tokens;
pub mod wiring;

pub use config::{AuthMode, McpConfig, McpServer, Transport};
pub use tokens::{OAuthStatus, TokenStore};

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("{0}")]
    Invalid(String),
    #[error("the server requires sign-in (OAuth)")]
    NeedsAuth { resource_metadata: Option<String> },
    #[error("MCP: {0}")]
    Protocol(String),
    #[error("HTTP: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// `<data>/mcp-config.json`.
#[must_use]
pub fn config_path(data_root: &std::path::Path) -> std::path::PathBuf {
    data_root.join("mcp-config.json")
}

/// `<data>/mcp-tokens.json`.
#[must_use]
pub fn tokens_path(data_root: &std::path::Path) -> std::path::PathBuf {
    data_root.join("mcp-tokens.json")
}
