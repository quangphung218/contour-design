//! Short-lived preview scopes: the random path segment in
//! `/api/projects/:id/preview/<scope>/…` that grants read access to one
//! project's files under the strict preview CSP.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// How long a scope stays valid after it is minted.
pub const SCOPE_TTL: Duration = Duration::from_secs(60 * 60);

/// `[A-Za-z0-9_-]{8,128}`.
#[must_use]
pub fn is_scope_token(token: &str) -> bool {
    (8..=128).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A minted scope and its expiry (Unix ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewScope {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug)]
struct Entry {
    project_id: String,
    expires_at: i64,
    /// Shared by bridged `raw` documents; never handed out by `mint`.
    reusable: bool,
}

/// In-memory scope registry. Scopes die with the daemon.
#[derive(Debug)]
pub struct PreviewScopes {
    ttl_ms: i64,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Default for PreviewScopes {
    fn default() -> Self {
        Self::with_ttl(SCOPE_TTL)
    }
}

impl PreviewScopes {
    #[must_use]
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl_ms: i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX),
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        // A panic while holding the map cannot leave it inconsistent.
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn create(
        &self,
        entries: &mut HashMap<String, Entry>,
        project_id: &str,
        reusable: bool,
    ) -> PreviewScope {
        let now = od_store::now_ms();
        entries.retain(|_, e| e.expires_at > now);
        let token = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = now.saturating_add(self.ttl_ms);
        entries.insert(
            token.clone(),
            Entry {
                project_id: project_id.to_owned(),
                expires_at,
                reusable,
            },
        );
        PreviewScope { token, expires_at }
    }

    /// A fresh scope for one `preview-url` caller.
    pub fn mint(&self, project_id: &str) -> PreviewScope {
        let mut entries = self.lock();
        self.create(&mut entries, project_id, false)
    }

    /// The project's shared live scope, created on first use. Bridged `raw`
    /// documents embed it in `<base href>`, so reusing one keeps their bytes
    /// stable across reads (a new scope per read would reload the iframe).
    pub fn acquire(&self, project_id: &str) -> PreviewScope {
        let now = od_store::now_ms();
        let mut entries = self.lock();
        let live = entries
            .iter()
            .find(|(_, e)| e.reusable && e.project_id == project_id && e.expires_at > now)
            .map(|(token, e)| PreviewScope {
                token: token.clone(),
                expires_at: e.expires_at,
            });
        live.unwrap_or_else(|| self.create(&mut entries, project_id, true))
    }

    /// Whether `token` is a live scope of `project_id`.
    pub fn resolve(&self, project_id: &str, token: &str) -> bool {
        let now = od_store::now_ms();
        self.lock()
            .get(token)
            .is_some_and(|e| e.project_id == project_id && e.expires_at > now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_scope_is_bound_to_its_project() {
        let scopes = PreviewScopes::default();
        let s = scopes.mint("p1");
        assert!(is_scope_token(&s.token));
        assert!(scopes.resolve("p1", &s.token));
        assert!(!scopes.resolve("p2", &s.token));
        assert!(!scopes.resolve("p1", "nope-nope-nope"));
        assert_ne!(scopes.mint("p1").token, s.token);
    }

    #[test]
    fn expired_scope_is_rejected() {
        let scopes = PreviewScopes::with_ttl(Duration::ZERO);
        let s = scopes.mint("p1");
        assert!(!scopes.resolve("p1", &s.token));
        assert_ne!(scopes.acquire("p1").token, scopes.acquire("p1").token);
    }

    #[test]
    fn acquire_reuses_but_mint_does_not_adopt() {
        let scopes = PreviewScopes::default();
        let minted = scopes.mint("p1");
        let shared = scopes.acquire("p1");
        assert_ne!(minted.token, shared.token);
        assert_eq!(scopes.acquire("p1"), shared);
        assert_ne!(scopes.acquire("p2").token, shared.token);
    }

    #[test]
    fn token_shape() {
        assert!(is_scope_token("abcd1234"));
        assert!(is_scope_token("A_b-0123456789"));
        assert!(!is_scope_token("short"));
        assert!(!is_scope_token("has/slash1"));
        assert!(!is_scope_token(&"x".repeat(129)));
    }
}
