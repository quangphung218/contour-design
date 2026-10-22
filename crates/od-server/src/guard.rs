//! Request guards (spec 2.2): browser origin check and the optional API
//! token for non-loopback binds.
//!
//! - No `Origin` header (CLI, curl): allowed.
//! - `Origin: null` (sandboxed preview iframes): only `GET .../raw/...`.
//! - Any other origin must be loopback or private LAN on the daemon's port,
//!   or listed in `OD_ALLOWED_ORIGINS`.

use crate::error::ApiErr;
use crate::state::AppState;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use std::net::{IpAddr, SocketAddr};

/// Paths reachable without a token even on a non-loopback bind.
const OPEN_PATHS: [&str; 2] = ["/api/health", "/api/version"];

pub async fn origin_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    if origin.is_empty() {
        return next.run(req).await;
    }
    if origin == "null" {
        return if req.method() == Method::GET && is_raw_path(req.uri().path()) {
            next.run(req).await
        } else {
            ApiErr::forbidden("Origin: null not allowed for this route").into_response()
        };
    }
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let cfg = &state.config;
    let allowed =
        is_allowed_browser_origin(&origin, host, &cfg.ports, &cfg.host, &cfg.allowed_origins)
            || (req.method() == Method::GET && is_portless_loopback_origin(&origin));
    if allowed {
        next.run(req).await
    } else {
        ApiErr::forbidden("Cross-origin requests are not allowed").into_response()
    }
}

/// Requires `OD_API_TOKEN` (Bearer, or Basic with the token as password)
/// from non-loopback peers when a token is configured.
pub async fn token_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(token) = state.config.api_token.as_deref() else {
        return next.run(req).await;
    };
    let peer_is_loopback = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_none_or(|ConnectInfo(addr)| addr.ip().is_loopback());
    if peer_is_loopback || OPEN_PATHS.contains(&req.uri().path()) || has_token(req.headers(), token)
    {
        return next.run(req).await;
    }
    // An agent calling back with its run's OD_TOOL_TOKEN, on its allowlist.
    if bearer(req.headers())
        .is_some_and(|t| state.runs.tokens.authorize(t, req.uri().path()).is_some())
    {
        return next.run(req).await;
    }
    ApiErr::new(
        axum::http::StatusCode::UNAUTHORIZED,
        od_contracts::ErrorCode::Unauthorized,
        "missing or invalid API token",
    )
    .into_response()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

fn has_token(headers: &HeaderMap, token: &str) -> bool {
    let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    if let Some(bearer) = auth.strip_prefix("Bearer ") {
        return constant_time_eq(bearer.trim().as_bytes(), token.as_bytes());
    }
    if let Some(basic) = auth.strip_prefix("Basic ") {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(basic.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok());
        if let Some((_, password)) = decoded.as_deref().and_then(|d| d.split_once(':')) {
            return constant_time_eq(password.as_bytes(), token.as_bytes());
        }
    }
    false
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn is_raw_path(path: &str) -> bool {
    let mut parts = path.trim_start_matches('/').splitn(5, '/');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()),
        (Some("api"), Some("projects"), Some(id), Some("raw"), Some(rest)) if !id.is_empty() && !rest.is_empty()
    )
}

fn is_portless_loopback_origin(origin: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        origin
            .strip_prefix(scheme)
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "[::1]"))
    })
}

#[derive(Debug, PartialEq, Eq)]
struct HostPort {
    hostname: String,
    port: u16,
}

/// Parses `host[:port]` (IPv6 in brackets). `default_port` fills a missing port.
fn parse_host(raw: &str, default_port: u16) -> Option<HostPort> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains(['/', '?', '#', '@', ' ']) {
        return None;
    }
    let (hostname, port) = if let Some(rest) = raw.strip_prefix('[') {
        let (h, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if after.is_empty() => default_port,
            None => return None,
        };
        (format!("[{h}]"), port)
    } else {
        match raw.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), p.parse().ok()?),
            None => (raw.to_owned(), default_port),
        }
    };
    Some(HostPort {
        hostname: hostname.to_lowercase(),
        port,
    })
}

fn parse_origin(origin: &str) -> Option<(&'static str, HostPort)> {
    let (scheme, rest, default_port) = if let Some(rest) = origin.strip_prefix("http://") {
        ("http", rest, 80)
    } else if let Some(rest) = origin.strip_prefix("https://") {
        ("https", rest, 443)
    } else {
        return None;
    };
    Some((scheme, parse_host(rest, default_port)?))
}

fn is_private_ipv4(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_private() || v4.is_link_local(),
        _ => false,
    }
}

fn is_loopback_or_private_lan(host: &str) -> bool {
    matches!(
        host,
        "localhost" | "127.0.0.1" | "::1" | "[::1]" | "0.0.0.0" | "::" | "[::]"
    ) || is_private_ipv4(host)
}

/// Port of the original `isAllowedBrowserOrigin`.
fn is_allowed_browser_origin(
    origin: &str,
    host_header: &str,
    ports: &[u16],
    bind_host: &str,
    extra: &[String],
) -> bool {
    if extra.iter().any(|o| o == origin) {
        return true;
    }
    let Some((_, parsed)) = parse_origin(origin) else {
        return false;
    };
    let Some(request_host) = parse_host(host_header, 80) else {
        return false;
    };
    let explicit = ports.iter().any(|p| {
        ["http", "https"].iter().any(|s| {
            ["127.0.0.1", "localhost", "[::1]", bind_host]
                .iter()
                .any(|h| origin == format!("{s}://{h}:{p}"))
        })
    });
    if explicit {
        return true;
    }
    ports.contains(&parsed.port)
        && parsed.hostname == request_host.hostname
        && is_loopback_or_private_lan(&parsed.hostname)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORTS: [u16; 1] = [7456];

    fn allowed(origin: &str, host: &str) -> bool {
        is_allowed_browser_origin(origin, host, &PORTS, "127.0.0.1", &[])
    }

    #[test]
    fn loopback_and_lan_on_daemon_port() {
        assert!(allowed("http://127.0.0.1:7456", "127.0.0.1:7456"));
        assert!(allowed("http://localhost:7456", "localhost:7456"));
        assert!(allowed("http://[::1]:7456", "[::1]:7456"));
        assert!(allowed("http://192.168.1.5:7456", "192.168.1.5:7456"));
    }

    #[test]
    fn foreign_origins_rejected() {
        assert!(!allowed("https://evil.example", "127.0.0.1:7456"));
        assert!(!allowed("http://127.0.0.1:9999", "127.0.0.1:7456"));
        assert!(!allowed("http://192.168.1.5:7456", "127.0.0.1:7456"));
        assert!(!allowed("file://", "127.0.0.1:7456"));
        assert!(is_allowed_browser_origin(
            "https://evil.example",
            "x",
            &PORTS,
            "127.0.0.1",
            &["https://evil.example".to_owned()]
        ));
    }

    #[test]
    fn raw_path_detection() {
        assert!(is_raw_path("/api/projects/p1/raw/index.html"));
        assert!(!is_raw_path("/api/projects/p1/files/index.html"));
        assert!(!is_raw_path("/api/projects/p1/raw/"));
    }

    #[test]
    fn basic_and_bearer_tokens() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer s3cret".parse().unwrap());
        assert!(has_token(&h, "s3cret"));
        let basic = base64::engine::general_purpose::STANDARD.encode("od:s3cret");
        h.insert(
            header::AUTHORIZATION,
            format!("Basic {basic}").parse().unwrap(),
        );
        assert!(has_token(&h, "s3cret"));
        assert!(!has_token(&h, "other"));
    }
}
