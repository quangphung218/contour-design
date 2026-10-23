//! OAuth 2.1 for HTTP MCP servers (MCP authorization spec): protected
//! resource metadata (RFC 9728) → authorization server metadata (RFC
//! 8414) → dynamic client registration (RFC 7591) → authorization code
//! with PKCE S256, redirected to the daemon's own loopback callback →
//! token exchange and refresh, bound to the server URL (RFC 8707).
//!
//! Every endpoint learned from metadata must be `https`, or `http` on a
//! loopback host. The HTTP client follows no redirects, so a code or a
//! token is only ever sent to the endpoint the metadata named.

use crate::McpError;
use crate::client::read_limited;
use crate::config::{McpServer, check_url};
use crate::tokens::{StoredToken, TokenStore};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use url::Url;

/// A started flow is valid this long.
pub const FLOW_TTL: Duration = Duration::from_secs(10 * 60);
/// Pending flows kept at once (oldest dropped first).
const MAX_FLOWS: usize = 32;
const MAX_METADATA: usize = 256 * 1024;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 32 random bytes from the OS (two v4 UUIDs: 244 random bits).
fn random_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    b64(&bytes)
}

/// PKCE S256 challenge of `verifier`.
#[must_use]
pub fn challenge(verifier: &str) -> String {
    b64(&Sha256::digest(verifier.as_bytes()))
}

fn is_loopback_host(u: &Url) -> bool {
    match u.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// An endpoint the daemon will send secrets to or open in the browser.
///
/// # Errors
/// [`McpError::Invalid`] unless https, or http on loopback.
pub fn check_endpoint(raw: &str) -> Result<Url, McpError> {
    let u = check_url(raw)?;
    if u.scheme() == "https" || is_loopback_host(&u) {
        Ok(u)
    } else {
        Err(McpError::Invalid(format!("{raw:?} must use https")))
    }
}

/// The HTTP client OAuth and MCP calls use: no redirects, bounded time.
///
/// # Errors
/// When the TLS backend cannot start.
pub fn http_client() -> Result<reqwest::Client, McpError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("open-design-rs/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| McpError::Http(e.to_string()))
}

async fn get_json(http: &reqwest::Client, url: &Url) -> Result<Option<Value>, McpError> {
    let res = http
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| McpError::Http(format!("{url}: {e}")))?;
    if !res.status().is_success() {
        return Ok(None);
    }
    let body = read_limited(res, MAX_METADATA).await?;
    Ok(serde_json::from_slice(&body).ok())
}

/// `/.well-known/<name>` URLs for `base`: path-inserted first, then root.
fn well_known(base: &Url, name: &str) -> Vec<Url> {
    let path = base.path().trim_end_matches('/');
    let mut out = Vec::new();
    let mut root = base.clone();
    root.set_query(None);
    root.set_fragment(None);
    if !path.is_empty() {
        let mut u = root.clone();
        u.set_path(&format!("/.well-known/{name}{path}"));
        out.push(u);
    }
    root.set_path(&format!("/.well-known/{name}"));
    out.push(root);
    out
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthServerMetadata {
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Option<Vec<String>>,
}

/// What discovery found for one MCP server.
#[derive(Debug, Clone)]
pub struct Discovery {
    pub resource: String,
    pub issuer: String,
    pub metadata: AuthServerMetadata,
    pub scopes: Vec<String>,
}

/// Finds the authorization server of `server_url`. `hint` is the
/// `resource_metadata` URL from a 401 challenge.
///
/// # Errors
/// Missing or unsafe metadata, no S256 support.
pub async fn discover(
    http: &reqwest::Client,
    server_url: &str,
    hint: Option<&str>,
) -> Result<Discovery, McpError> {
    let resource = check_url(server_url)?;
    let mut prm_urls = Vec::new();
    if let Some(h) = hint {
        prm_urls.push(check_endpoint(h)?);
    }
    prm_urls.extend(well_known(&resource, "oauth-protected-resource"));
    let mut prm = None;
    for u in &prm_urls {
        if let Some(v) = get_json(http, u).await? {
            prm = Some(v);
            break;
        }
    }
    // Servers predating RFC 9728 act as their own authorization server.
    let issuer = prm
        .as_ref()
        .and_then(|p| {
            p.get("authorization_servers")?
                .as_array()?
                .first()?
                .as_str()
        })
        .map_or_else(|| resource.origin().ascii_serialization(), str::to_owned);
    let scopes: Vec<String> = prm
        .as_ref()
        .and_then(|p| p.get("scopes_supported")?.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    let issuer_url = check_endpoint(&issuer)?;
    let mut metadata = None;
    for name in ["oauth-authorization-server", "openid-configuration"] {
        for u in well_known(&issuer_url, name) {
            if let Some(v) = get_json(http, &u).await? {
                if let Ok(m) = serde_json::from_value::<AuthServerMetadata>(v) {
                    metadata = Some(m);
                    break;
                }
            }
        }
        if metadata.is_some() {
            break;
        }
    }
    let metadata = metadata.ok_or_else(|| {
        McpError::Invalid(format!(
            "no OAuth authorization server metadata at {issuer}"
        ))
    })?;
    check_endpoint(&metadata.authorization_endpoint)?;
    check_endpoint(&metadata.token_endpoint)?;
    if let Some(r) = &metadata.registration_endpoint {
        check_endpoint(r)?;
    }
    if metadata
        .code_challenge_methods_supported
        .as_ref()
        .is_some_and(|m| !m.iter().any(|x| x == "S256"))
    {
        return Err(McpError::Invalid(
            "the authorization server does not support PKCE S256".into(),
        ));
    }
    Ok(Discovery {
        resource: resource.to_string(),
        issuer,
        metadata,
        scopes,
    })
}

/// Registers this app as a public client (no secret).
async fn register(
    http: &reqwest::Client,
    endpoint: &str,
    redirect_uri: &str,
) -> Result<String, McpError> {
    let body = serde_json::json!({
        "client_name": "Nét (local)",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let res = http
        .post(check_endpoint(endpoint)?)
        .json(&body)
        .send()
        .await
        .map_err(|e| McpError::Http(e.to_string()))?;
    let status = res.status();
    let body = read_limited(res, MAX_METADATA).await?;
    if !status.is_success() {
        return Err(McpError::Http(format!(
            "client registration answered {status}"
        )));
    }
    serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("client_id")?.as_str().map(str::to_owned))
        .ok_or_else(|| McpError::Protocol("registration returned no client_id".into()))
}

/// A started authorization, waiting for the browser to come back.
#[derive(Clone)]
pub struct PendingFlow {
    pub server_id: String,
    verifier: String,
    pub client_id: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
    pub resource: String,
    started: std::time::Instant,
}

impl std::fmt::Debug for PendingFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingFlow")
            .field("server_id", &self.server_id)
            .field("verifier", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// Flows by `state`. Each state is accepted once.
#[derive(Debug, Default)]
pub struct Flows {
    pending: Mutex<HashMap<String, PendingFlow>>,
}

impl Flows {
    fn insert(&self, state: String, flow: PendingFlow) {
        let mut p = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        p.retain(|_, f| f.started.elapsed() < FLOW_TTL);
        while p.len() >= MAX_FLOWS {
            let oldest = p
                .iter()
                .min_by_key(|(_, f)| f.started)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => p.remove(&k),
                None => break,
            };
        }
        p.insert(state, flow);
    }

    /// Takes the flow for `state` (one use), if it is still fresh.
    #[must_use]
    pub fn take(&self, state: &str) -> Option<PendingFlow> {
        let flow = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(state)?;
        (flow.started.elapsed() < FLOW_TTL).then_some(flow)
    }
}

/// Starts an authorization for `server`; returns the URL to open.
///
/// # Errors
/// Discovery or registration failures.
pub async fn begin(
    http: &reqwest::Client,
    server: &McpServer,
    tokens: &TokenStore,
    flows: &Flows,
    redirect_uri: &str,
    hint: Option<&str>,
) -> Result<String, McpError> {
    let server_url = server
        .url
        .as_deref()
        .ok_or_else(|| McpError::Invalid("OAuth needs an http server".into()))?;
    let d = discover(http, server_url, hint).await?;
    let client_id = match (&server.oauth_client_id, tokens.client_id(&d.issuer)) {
        (Some(id), _) => id.clone(),
        (None, Some(id)) => id,
        (None, None) => {
            let endpoint = d.metadata.registration_endpoint.as_deref().ok_or_else(|| {
                McpError::Invalid(
                    "the server needs a pre-registered client id (oauthClientId)".into(),
                )
            })?;
            let id = register(http, endpoint, redirect_uri).await?;
            tokens.set_client_id(&d.issuer, &id)?;
            id
        }
    };
    let verifier = random_token();
    let state = random_token();
    let mut url = check_endpoint(&d.metadata.authorization_endpoint)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("code_challenge", &challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", &d.resource);
        if !d.scopes.is_empty() {
            q.append_pair("scope", &d.scopes.join(" "));
        }
    }
    flows.insert(
        state,
        PendingFlow {
            server_id: server.id.clone(),
            verifier,
            client_id,
            token_endpoint: d.metadata.token_endpoint,
            redirect_uri: redirect_uri.to_owned(),
            resource: d.resource,
            started: std::time::Instant::now(),
        },
    );
    Ok(url.to_string())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    token_type: Option<String>,
}

async fn token_request(
    http: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
) -> Result<TokenResponse, McpError> {
    let res = http
        .post(check_endpoint(endpoint)?)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| McpError::Http(e.to_string()))?;
    let status = res.status();
    let body = read_limited(res, MAX_METADATA).await?;
    if !status.is_success() {
        // The body may echo the code; report only the OAuth error name.
        let code = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|v| v.get("error")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| status.to_string());
        return Err(McpError::Http(format!("token endpoint refused: {code}")));
    }
    let t: TokenResponse = serde_json::from_slice(&body)
        .map_err(|_| McpError::Protocol("token endpoint returned no access_token".into()))?;
    if t.token_type
        .as_deref()
        .is_some_and(|ty| !ty.eq_ignore_ascii_case("bearer"))
    {
        return Err(McpError::Protocol("unsupported token type".into()));
    }
    Ok(t)
}

fn expires(now_ms: i64, t: &TokenResponse) -> Option<i64> {
    t.expires_in
        .map(|s| now_ms.saturating_add(s.saturating_mul(1000)))
}

/// Exchanges the code from the callback and stores the token.
///
/// # Errors
/// Token endpoint errors.
pub async fn finish(
    http: &reqwest::Client,
    flow: &PendingFlow,
    code: &str,
    tokens: &TokenStore,
    now_ms: i64,
) -> Result<(), McpError> {
    let t = token_request(
        http,
        &flow.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &flow.redirect_uri),
            ("client_id", &flow.client_id),
            ("code_verifier", &flow.verifier),
            ("resource", &flow.resource),
        ],
    )
    .await?;
    tokens.put(
        &flow.server_id,
        StoredToken {
            expires_at: expires(now_ms, &t),
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            token_endpoint: flow.token_endpoint.clone(),
            client_id: flow.client_id.clone(),
            resource: flow.resource.clone(),
        },
    )
}

/// The server's access token, refreshed first when it is about to expire.
/// `None` when there is none or the refresh failed (the agent then meets
/// the server's 401 and the user reconnects).
pub async fn access_token(
    http: &reqwest::Client,
    tokens: &TokenStore,
    server_id: &str,
    now_ms: i64,
) -> Option<String> {
    let stored = tokens.get(server_id)?;
    if !stored.is_stale(now_ms) {
        return Some(stored.access_token);
    }
    let refresh = stored.refresh_token.as_deref()?;
    match token_request(
        http,
        &stored.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", &stored.client_id),
            ("resource", &stored.resource),
        ],
    )
    .await
    {
        Ok(t) => {
            let fresh = StoredToken {
                expires_at: expires(now_ms, &t),
                access_token: t.access_token,
                refresh_token: t.refresh_token.or(stored.refresh_token.clone()),
                ..stored
            };
            let access = fresh.access_token.clone();
            if let Err(e) = tokens.put(server_id, fresh) {
                tracing::warn!(server = server_id, error = %e, "storing a refreshed MCP token");
            }
            Some(access)
        }
        Err(e) => {
            tracing::warn!(server = server_id, error = %e, "refreshing an MCP token failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_appendix_b() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let v = random_token();
        assert_eq!(v.len(), 43);
        assert_ne!(v, random_token());
    }

    #[test]
    fn endpoints_must_be_https_or_loopback() {
        assert!(check_endpoint("https://auth.example/token").is_ok());
        assert!(check_endpoint("http://127.0.0.1:9/token").is_ok());
        assert!(check_endpoint("http://localhost:9/token").is_ok());
        for bad in [
            "http://auth.example/token",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://u:p@a.example/",
        ] {
            assert!(check_endpoint(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn well_known_paths() {
        let u = Url::parse("https://a.example/mcp/v1?x=1").unwrap();
        let got: Vec<String> = well_known(&u, "oauth-protected-resource")
            .iter()
            .map(Url::to_string)
            .collect();
        assert_eq!(
            got,
            [
                "https://a.example/.well-known/oauth-protected-resource/mcp/v1",
                "https://a.example/.well-known/oauth-protected-resource"
            ]
        );
    }

    #[test]
    fn a_state_is_accepted_once() {
        let flows = Flows::default();
        let flow = PendingFlow {
            server_id: "s".into(),
            verifier: "v".into(),
            client_id: "c".into(),
            token_endpoint: "https://a/t".into(),
            redirect_uri: "http://127.0.0.1/cb".into(),
            resource: "https://a/mcp".into(),
            started: std::time::Instant::now(),
        };
        flows.insert("st".into(), flow);
        assert!(flows.take("st").is_some());
        assert!(flows.take("st").is_none());
        assert!(flows.take("other").is_none());
        assert!(!format!("{:?}", flows).contains("\"v\""));
    }

    #[test]
    fn an_expired_flow_is_refused() {
        let flows = Flows::default();
        let old = std::time::Instant::now()
            .checked_sub(FLOW_TTL + Duration::from_secs(1))
            .unwrap();
        flows.pending.lock().unwrap().insert(
            "st".into(),
            PendingFlow {
                server_id: "s".into(),
                verifier: "v".into(),
                client_id: "c".into(),
                token_endpoint: "https://a/t".into(),
                redirect_uri: "http://127.0.0.1/cb".into(),
                resource: "https://a/mcp".into(),
                started: old,
            },
        );
        assert!(flows.take("st").is_none());
    }
}
