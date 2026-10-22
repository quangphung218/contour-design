//! `OD_TOOL_TOKEN`: one bearer token per run, so the agent can call back
//! into the daemon for a short list of tool endpoints (spec 2.2).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Shortest lifetime of a tool token (spec 2.2: at least 15 minutes).
pub const MIN_TOOL_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

/// What a tool token is allowed to touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolGrant {
    pub run_id: String,
    pub project_id: Option<String>,
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub struct ToolTokens {
    grants: Mutex<HashMap<String, ToolGrant>>,
}

impl ToolTokens {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, ToolGrant>> {
        self.grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Mints a token for `run_id`, valid for at least
    /// [`MIN_TOOL_TOKEN_TTL`].
    pub fn issue(&self, run_id: &str, project_id: Option<&str>, ttl: Duration) -> String {
        let token = format!(
            "odt_{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let grant = ToolGrant {
            run_id: run_id.to_owned(),
            project_id: project_id.map(str::to_owned),
            expires_at: Instant::now() + ttl.max(MIN_TOOL_TOKEN_TTL),
        };
        let mut grants = self.lock();
        grants.retain(|_, g| g.expires_at > Instant::now());
        grants.insert(token.clone(), grant);
        token
    }

    /// The grant when `token` is live and `path` is on its allowlist: the
    /// run's own project and the `/api/tools/` family.
    #[must_use]
    pub fn authorize(&self, token: &str, path: &str) -> Option<ToolGrant> {
        let grants = self.lock();
        let grant = grants.get(token)?;
        if grant.expires_at <= Instant::now() {
            return None;
        }
        let own_project = grant
            .project_id
            .as_deref()
            .is_some_and(|p| path.starts_with(&format!("/api/projects/{p}/")));
        (own_project || path.starts_with("/api/tools/")).then(|| grant.clone())
    }

    /// Drops every token of a finished run.
    pub fn revoke_run(&self, run_id: &str) {
        self.lock().retain(|_, g| g.run_id != run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_scoped_to_its_run_and_project() {
        let tokens = ToolTokens::default();
        let t = tokens.issue("r1", Some("p1"), Duration::ZERO);
        assert!(tokens.authorize(&t, "/api/projects/p1/files").is_some());
        assert!(
            tokens
                .authorize(&t, "/api/tools/design-systems/read")
                .is_some()
        );
        assert!(tokens.authorize(&t, "/api/projects/p2/files").is_none());
        assert!(tokens.authorize(&t, "/api/projects").is_none());
        assert!(tokens.authorize("nope", "/api/tools/x").is_none());
        tokens.revoke_run("r1");
        assert!(tokens.authorize(&t, "/api/tools/x").is_none());
    }
}
