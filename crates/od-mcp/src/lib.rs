//! `od mcp`: an MCP stdio server whose every tool proxies to the daemon's
//! HTTP API (spec 2.5), `od mcp live-artifacts` (the second server), and
//! `od mcp install` for claude, codex and cursor.

pub mod brief;
pub mod client;
pub mod install;
pub mod live;
pub mod locate;
pub mod server;
pub mod tools;

pub use client::{DEFAULT_DAEMON_URL, Daemon};
pub use live::{LiveMcp, serve_live_stdio};
pub use server::{OdMcp, serve_stdio};
