//! `POST/GET /api/active`: which project and file the user has open right
//! now. The UI posts on every route change; MCP tools fall back to it when
//! `project` is omitted. RAM only, expires 5 minutes after the last post
//! (spec 2.5).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Extension;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// Active context lifetime.
pub const ACTIVE_TTL_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Active {
    project_id: String,
    file_name: Option<String>,
    ts: i64,
}

/// The single active context, with an injectable clock for tests.
pub struct ActiveContext {
    current: Mutex<Option<Active>>,
    now: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl Default for ActiveContext {
    fn default() -> Self {
        Self::with_clock(od_store::now_ms)
    }
}

impl ActiveContext {
    pub fn with_clock(now: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        Self {
            current: Mutex::new(None),
            now: Box::new(now),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set(&self, project_id: String, file_name: Option<String>) -> Active {
        let active = Active {
            project_id,
            file_name,
            ts: (self.now)(),
        };
        *self.lock() = Some(active.clone());
        active
    }

    fn clear(&self) {
        *self.lock() = None;
    }

    /// The current context and its age, dropping it once expired.
    fn get(&self) -> Option<(Active, i64)> {
        let now = (self.now)();
        let mut guard = self.lock();
        let age = guard.as_ref().map(|a| now - a.ts)?;
        if age > ACTIVE_TTL_MS {
            *guard = None;
            return None;
        }
        guard.clone().map(|a| (a, age))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostActive {
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
}

pub async fn post(
    Extension(ctx): Extension<Arc<ActiveContext>>,
    ApiJson(body): ApiJson<PostActive>,
) -> ApiResult<ApiJson<Value>> {
    if body.active == Some(false) {
        ctx.clear();
        return Ok(ApiJson(json!({ "active": false })));
    }
    let project_id = body
        .project_id
        .filter(|p| !p.is_empty())
        .ok_or_else(|| ApiErr::bad_request("projectId is required"))?;
    let file_name = body.file_name.filter(|f| !f.is_empty());
    let a = ctx.set(project_id, file_name);
    Ok(ApiJson(json!({
        "active": true,
        "projectId": a.project_id,
        "fileName": a.file_name,
        "ts": a.ts,
    })))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(ctx): Extension<Arc<ActiveContext>>,
) -> ApiResult<ApiJson<Value>> {
    let Some((a, age)) = ctx.get() else {
        return Ok(ApiJson(json!({ "active": false })));
    };
    let id = a.project_id.clone();
    // A deleted project still reports its id; the name is then null.
    let name = state
        .blocking(move |s| Ok(s.get_project(&id).ok().map(|p| p.name)))
        .await?;
    Ok(ApiJson(json!({
        "active": true,
        "projectId": a.project_id,
        "projectName": name,
        "fileName": a.file_name,
        "ts": a.ts,
        "ageMs": age,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    #[test]
    fn expires_after_ttl() {
        let clock = Arc::new(AtomicI64::new(1_000));
        let c = Arc::clone(&clock);
        let ctx = ActiveContext::with_clock(move || c.load(Ordering::SeqCst));
        ctx.set("p1".into(), Some("index.html".into()));
        clock.store(1_000 + ACTIVE_TTL_MS, Ordering::SeqCst);
        assert_eq!(
            ctx.get().map(|(a, age)| (a.project_id, age)),
            Some(("p1".into(), ACTIVE_TTL_MS))
        );
        clock.store(1_001 + ACTIVE_TTL_MS, Ordering::SeqCst);
        assert_eq!(ctx.get(), None);
        // Expiry clears it: going back in time does not revive it.
        clock.store(1_000, Ordering::SeqCst);
        assert_eq!(ctx.get(), None);
    }

    #[test]
    fn clear_forgets() {
        let ctx = ActiveContext::default();
        ctx.set("p".into(), None);
        ctx.clear();
        assert_eq!(ctx.get(), None);
    }
}
