//! W3 platform routes (spec 1.5): terminal tabs, automations (routines) and
//! the external MCP servers agents get at spawn. Mounted with one line in
//! [`crate::router`].
//!
//! Every route here can start programs on this machine, so on top of the
//! daemon-wide origin and token guards each request must come from a
//! loopback peer, name a loopback `Host`, and carry no `Origin` or a
//! loopback one (no LAN origins, no API-token access from elsewhere).

pub mod terminals;

use crate::error::ApiErr;
use crate::state::AppState;
use axum::extract::{ConnectInfo, Request};
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
    fn from_env() -> Self {
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
pub fn mount(api: Router<AppState>, _state: &AppState) -> Router<AppState> {
    let platform = Platform::from_env();
    api.merge(
        terminals::routes()
            .layer(middleware::from_fn(loopback_only))
            .layer(Extension(platform)),
    )
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

fn loopback_origin(origin: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        origin
            .strip_prefix(scheme)
            .is_some_and(|rest| !rest.contains(['/', '@']) && is_loopback_name(hostname(rest)))
    })
}

/// See the module docs. A request without peer info (not served through
/// `into_make_service_with_connect_info`) is refused too.
pub async fn loopback_only(req: Request, next: Next) -> Response {
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
        .is_none_or(|o| o.to_str().is_ok_and(loopback_origin));
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
        assert!(loopback_origin("http://127.0.0.1:7456"));
        assert!(loopback_origin("http://[::1]:7456"));
        assert!(!loopback_origin("http://192.168.1.5:7456"));
        assert!(!loopback_origin("null"));
        assert!(!loopback_origin("http://127.0.0.1@evil.example"));
        assert!(!loopback_origin("file://"));
    }
}
