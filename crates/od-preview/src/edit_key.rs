//! Edit keys: proof that a text edit or an inspected element comes from the
//! bridge, not from the artifact's own scripts, which share its window.
//!
//! The host names each preview load with a fresh id (`raw?bridge=1&key=<id>`).
//! The first response for that id carries a new secret key inside the
//! bridge's `<script>` tag; the bridge takes it before any author script
//! runs and removes it from the DOM. The host claims the key once, by a
//! POST the guard refuses to sandboxed frames, and from then on accepts
//! only bridge messages that carry it. A later load with the same id gets
//! no key, and a claimed id stays spent until its TTL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// How long an issued key waits to be claimed.
pub const EDIT_KEY_TTL: Duration = Duration::from_secs(10 * 60);
/// Most ids remembered at once; the oldest go first.
const MAX_ENTRIES: usize = 256;

/// `[A-Za-z0-9_-]{16,64}`: what the host sends as the load's id.
#[must_use]
pub fn is_edit_key_id(id: &str) -> bool {
    (16..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Debug)]
struct Entry {
    project_id: String,
    /// Taken by the claim; the spent id stays until its TTL so a reload
    /// with it gets no new key.
    key: Option<String>,
    issued_at: i64,
}

/// In-memory id → key registry. Keys die with the daemon.
#[derive(Debug)]
pub struct EditKeys {
    ttl_ms: i64,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Default for EditKeys {
    fn default() -> Self {
        Self::with_ttl(EDIT_KEY_TTL)
    }
}

impl EditKeys {
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

    /// A new key for the first load of `id`; `None` for a bad id or one
    /// already issued (a page fetching its own URL again gets nothing).
    pub fn issue(&self, project_id: &str, id: &str) -> Option<String> {
        if !is_edit_key_id(id) {
            return None;
        }
        let now = od_store::now_ms();
        let mut entries = self.lock();
        entries.retain(|_, e| now.saturating_sub(e.issued_at) < self.ttl_ms);
        if entries.contains_key(id) {
            return None;
        }
        if entries.len() >= MAX_ENTRIES {
            let oldest = entries
                .iter()
                .min_by_key(|(_, e)| e.issued_at)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }
        let key = uuid::Uuid::new_v4().simple().to_string();
        entries.insert(
            id.to_owned(),
            Entry {
                project_id: project_id.to_owned(),
                key: Some(key.clone()),
                issued_at: now,
            },
        );
        Some(key)
    }

    /// The key issued for `id` in `project_id`, once.
    pub fn claim(&self, project_id: &str, id: &str) -> Option<String> {
        let now = od_store::now_ms();
        let mut entries = self.lock();
        let entry = entries.get_mut(id)?;
        if entry.project_id != project_id || now.saturating_sub(entry.issued_at) >= self.ttl_ms {
            return None;
        }
        entry.key.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "load-0123456789abcdef";

    #[test]
    fn issued_once_and_claimed_once() {
        let keys = EditKeys::default();
        let key = keys.issue("p1", ID).unwrap();
        assert_eq!(key.len(), 32);
        // The page loading its own URL again gets no key.
        assert_eq!(keys.issue("p1", ID), None);
        assert_eq!(keys.claim("p2", ID), None, "another project");
        assert_eq!(keys.claim("p1", ID).as_deref(), Some(key.as_str()));
        assert_eq!(keys.claim("p1", ID), None);
        // A claimed id stays spent: a reload with it gets no new key.
        assert_eq!(keys.issue("p1", ID), None);
        assert_ne!(keys.issue("p1", "load-fedcba9876543210"), Some(key));
    }

    #[test]
    fn bad_ids_and_stale_keys() {
        let keys = EditKeys::default();
        for id in ["short", "has space 0123456789", &"x".repeat(65)] {
            assert_eq!(keys.issue("p", id), None, "{id}");
        }
        let stale = EditKeys::with_ttl(Duration::ZERO);
        assert!(stale.issue("p", ID).is_some());
        assert_eq!(stale.claim("p", ID), None);
    }

    #[test]
    fn bounded() {
        let keys = EditKeys::default();
        for i in 0..MAX_ENTRIES + 10 {
            assert!(keys.issue("p", &format!("load-{i:016}")).is_some());
        }
        assert_eq!(keys.lock().len(), MAX_ENTRIES);
    }
}
