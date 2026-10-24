//! W3 platform routes (spec 1.5): terminal tabs, automations (routines) and
//! the external MCP servers agents get at spawn. Mounted with one line in
//! [`crate::router`].
//!
//! Every route here can start programs on this machine, so on top of the
//! daemon-wide origin and token guards each request must come from a
//! loopback peer, name a loopback `Host`, and carry no `Origin` or a
//! loopback one on one of the daemon's ports (no LAN origins, no other
//! local site, no API-token access from elsewhere).

pub mod automations;
pub mod evolution;
pub mod mcp;
pub mod terminals;

use crate::error::ApiErr;
use crate::state::AppState;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// Shared state of the platform routes.
#[derive(Clone)]
pub struct Platform {
    pub terminals: Arc<od_term::Terminals>,
}

impl Platform {
    /// `OD_TERMINAL_SHELL` overrides the user's shell.
    #[must_use]
    pub fn from_env() -> Self {
        let shell = std::env::var_os("OD_TERMINAL_SHELL")
            .filter(|s| !s.is_empty())
            .map(Into::into);
        Self {
            terminals: Arc::new(od_term::Terminals::new(od_term::TermConfig {
                shell,
                ..od_term::TermConfig::default()
            })),
        }
    }
}

/// Adds the platform routes to `api`.
pub fn mount(api: Router<AppState>, state: &AppState) -> Router<AppState> {
    let mut routes = terminals::routes().layer(Extension(state.platform.clone()));
    if let Some(scheduler) = automations::scheduler(state) {
        routes = routes.merge(evolution::routes(state).layer(Extension(Arc::clone(&scheduler))));
        routes = routes.merge(automations::routes().layer(Extension(scheduler)));
    }
    if let Some(mcp) = mcp::Mcp::new(state) {
        routes = routes.merge(mcp::routes().layer(Extension(mcp)));
    }
    let ports: Arc<[u16]> = state.config.ports.clone().into();
    api.merge(routes.layer(middleware::from_fn_with_state(ports, loopback_only)))
}

fn is_loopback_name(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `host[:port]` → host, for `Host` headers and origins.
fn hostname(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default();
    }
    authority.rsplit_once(':').map_or(authority, |(h, _)| h)
}

/// A loopback origin naming one of `ports` explicitly: a portless one
/// (`http://localhost`) is another local site, not the app.
fn loopback_origin(origin: &str, ports: &[u16]) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        origin.strip_prefix(scheme).is_some_and(|rest| {
            !rest.contains(['/', '@'])
                && is_loopback_name(hostname(rest))
                && port(rest).is_some_and(|p| ports.contains(&p))
        })
    })
}

/// The explicit port of `host:port` (IPv6 in brackets).
fn port(authority: &str) -> Option<u16> {
    let after_host = match authority.strip_prefix('[') {
        Some(rest) => rest.split_once(']')?.1,
        None => authority,
    };
    after_host.rsplit_once(':')?.1.parse().ok()
}

/// See the module docs. A request without peer info (not served through
/// `into_make_service_with_connect_info`) is refused too.
pub async fn loopback_only(State(ports): State<Arc<[u16]>>, req: Request, next: Next) -> Response {
    let peer_ok = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|ConnectInfo(addr)| addr.ip().is_loopback());
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| is_loopback_name(hostname(h)));
    let origin_ok = req
        .headers()
        .get(header::ORIGIN)
        .is_none_or(|o| o.to_str().is_ok_and(|o| loopback_origin(o, &ports)));
    if peer_ok && host_ok && origin_ok {
        next.run(req).await
    } else {
        ApiErr::forbidden("this route only answers loopback clients").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_names_and_origins() {
        for h in ["127.0.0.1:7456", "localhost", "[::1]:80", "LOCALHOST:1"] {
            assert!(is_loopback_name(hostname(h)), "{h}");
        }
        for h in [
            "192.168.1.5:7456",
            "evil.example",
            "127.0.0.1.evil.example",
            "",
            "0.0.0.0:1",
        ] {
            assert!(!is_loopback_name(hostname(h)), "{h}");
        }
        let ports = [7456, 5173];
        for ok in [
            "http://127.0.0.1:7456",
            "http://[::1]:7456",
            "http://localhost:5173",
        ] {
            assert!(loopback_origin(ok, &ports), "{ok}");
        }
        for bad in [
            "http://192.168.1.5:7456",
            "null",
            "http://127.0.0.1@evil.example",
            "file://",
            "http://localhost",
            "http://[::1]",
            "http://127.0.0.1:8080",
        ] {
            assert!(!loopback_origin(bad, &ports), "{bad}");
        }
    }
}
