//! Project management queries (Projects page, spec 1.1): search over
//! projects, conversations and messages, and "recently viewed".
//!
//! The view table is created idempotently on every open instead of through
//! a numbered migration, like project memory, so this track's schema never
//! fights another track's `user_version` bump.

use crate::error::{Result, StoreError};
use crate::paths::is_safe_id;
use crate::store::Store;
use rusqlite::{Connection, params};
use serde::Serialize;

const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS project_views (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  viewed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS global_memories (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
";

/// Kinds of global memory, as the original's memory types.
pub const MEMORY_KINDS: [&str; 6] = [
    "user",
    "feedback",
    "project",
    "reference",
    "profile",
    "rule",
];
/// Longest global memory entry (characters).
pub const MAX_GLOBAL_MEMORY_CHARS: usize = 1000;
/// Most global entries kept.
pub const MAX_GLOBAL_MEMORIES: usize = 100;

/// A fact every project's prompt carries (Settings ▸ Memory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalMemory {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub created_at: i64,
}

/// Most hits a search returns.
pub const MAX_SEARCH_HITS: usize = 50;
/// Characters of context kept on each side of a message match.
const SNIPPET_CONTEXT: usize = 60;

/// Creates the view table when missing. Safe to run on every open.
///
/// # Errors
/// SQLite errors.
pub(crate) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

/// What a search hit points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HitKind {
    Project,
    Conversation,
    Message,
}

/// One search result, best first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: HitKind,
    pub project_id: String,
    pub project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Conversation title, when the hit is in a conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The matching text around the query, for message hits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub updated_at: i64,
}

/// `%` and `_` match literally.
fn like_pattern(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + 2);
    out.push('%');
    for c in query.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// `text` cut to the first case-insensitive match of `query` with some
/// context, on character boundaries, newlines folded to spaces.
#[must_use]
pub fn snippet(text: &str, query: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let chars: Vec<char> = flat.chars().collect();
    let lower: Vec<char> = chars.iter().flat_map(|c| c.to_lowercase()).collect();
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    // Lowercasing can change lengths (rare); fall back to the start then.
    let at = if lower.len() == chars.len() && !needle.is_empty() {
        lower
            .windows(needle.len())
            .position(|w| w == needle.as_slice())
            .unwrap_or(0)
    } else {
        0
    };
    let start = at.saturating_sub(SNIPPET_CONTEXT);
    let end = (at + needle.len() + SNIPPET_CONTEXT).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out.trim().to_owned()
}

/// A project's latest run as the message history records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRunStatus {
    pub project_id: String,
    /// `running`, `succeeded`, `failed` or `canceled`.
    pub status: String,
    pub run_id: Option<String>,
    pub at: i64,
    /// The newest message of the project is an assistant turn asking a
    /// `<question-form>` that nobody answered yet.
    pub awaiting_input: bool,
    /// The run succeeded but left work: an unfinished todo in its last
    /// plan, or it stopped at the token limit.
    pub incomplete: bool,
}

/// Whether a run's events end with work left (the original's
/// `endedWithUnfinishedWork`).
#[must_use]
pub fn unfinished(events: &[serde_json::Value]) -> bool {
    let stopped_short = events.iter().any(|e| {
        e["type"] == "usage" && matches!(e["stopReason"].as_str(), Some("max_tokens" | "length"))
    });
    let last_plan = events.iter().rev().find(|e| {
        e["type"] == "tool_use" && matches!(e["name"].as_str(), Some("TodoWrite" | "update_plan"))
    });
    let open_todo = last_plan.is_some_and(|e| {
        let items = e["input"]["todos"]
            .as_array()
            .or_else(|| e["input"]["plan"].as_array());
        items.is_some_and(|list| {
            list.iter()
                .any(|t| t["status"].as_str().is_some_and(|s| s != "completed"))
        })
    });
    stopped_short || open_todo
}

fn asks_question(content: &str) -> bool {
    content.contains("<question-form") || content.contains("<ask-question")
}

impl Store {
    /// Projects whose name matches, then conversations by title, then
    /// messages by content; newest first within each kind, at most
    /// [`MAX_SEARCH_HITS`]. Case-insensitive for ASCII (SQLite `LIKE`).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = like_pattern(query);
        let limit = i64::try_from(MAX_SEARCH_HITS).unwrap_or(i64::MAX);
        self.with_conn(|conn| {
            let mut hits = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT id, name, updated_at FROM projects WHERE name LIKE ?1 ESCAPE '\\' \
                 ORDER BY updated_at DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], |r| {
                Ok(SearchHit {
                    kind: HitKind::Project,
                    project_id: r.get(0)?,
                    project_name: r.get(1)?,
                    conversation_id: None,
                    title: None,
                    snippet: None,
                    updated_at: r.get(2)?,
                })
            })?;
            hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);

            let mut stmt = conn.prepare(
                "SELECT c.id, c.title, c.updated_at, p.id, p.name FROM conversations c \
                 JOIN projects p ON p.id = c.project_id \
                 WHERE c.title LIKE ?1 ESCAPE '\\' ORDER BY c.updated_at DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], |r| {
                Ok(SearchHit {
                    kind: HitKind::Conversation,
                    conversation_id: Some(r.get(0)?),
                    title: r.get(1)?,
                    updated_at: r.get(2)?,
                    project_id: r.get(3)?,
                    project_name: r.get(4)?,
                    snippet: None,
                })
            })?;
            hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);

            // One hit per conversation: its newest matching message.
            let mut stmt = conn.prepare(
                "SELECT m.content, MAX(m.created_at), c.id, c.title, p.id, p.name \
                 FROM messages m JOIN conversations c ON c.id = m.conversation_id \
                 JOIN projects p ON p.id = c.project_id \
                 WHERE m.content LIKE ?1 ESCAPE '\\' \
                 GROUP BY c.id ORDER BY MAX(m.created_at) DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], |r| {
                let content: String = r.get(0)?;
                Ok(SearchHit {
                    kind: HitKind::Message,
                    snippet: Some(snippet(&content, query)),
                    updated_at: r.get(1)?,
                    conversation_id: Some(r.get(2)?),
                    title: r.get(3)?,
                    project_id: r.get(4)?,
                    project_name: r.get(5)?,
                })
            })?;
            hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            hits.truncate(MAX_SEARCH_HITS);
            Ok(hits)
        })
    }

    /// Every project's latest run (by end, start, then creation time) from
    /// the persisted messages; projects without a run are absent.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn stored_run_statuses(&self) -> Result<Vec<StoredRunStatus>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.project_id, m.run_status, m.run_id, \
                 COALESCE(m.ended_at, m.started_at, m.created_at) AS t, m.id \
                 FROM messages m JOIN conversations c ON c.id = m.conversation_id \
                 WHERE m.run_status IS NOT NULL ORDER BY t, m.id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    StoredRunStatus {
                        project_id: r.get(0)?,
                        status: r.get(1)?,
                        run_id: r.get(2)?,
                        at: r.get(3)?,
                        awaiting_input: false,
                        incomplete: false,
                    },
                    r.get::<_, String>(4)?,
                ))
            })?;
            // Ascending order: the last row per project wins.
            let mut latest: std::collections::BTreeMap<String, (StoredRunStatus, String)> =
                std::collections::BTreeMap::new();
            for row in rows {
                let row = row?;
                latest.insert(row.0.project_id.clone(), row);
            }
            // Events of each project's latest succeeded run: inline and batched.
            let mut events_of = conn.prepare(
                "SELECT events_json FROM messages WHERE id = ?1 \
                 UNION ALL SELECT events_json FROM message_event_batches WHERE message_id = ?1",
            )?;
            for (status, message_id) in latest.values_mut() {
                if status.status != "succeeded" {
                    continue;
                }
                let mut events: Vec<serde_json::Value> = Vec::new();
                let chunks = events_of
                    .query_map([message_id.as_str()], |r| r.get::<_, Option<String>>(0))?;
                for chunk in chunks {
                    if let Some(raw) = chunk? {
                        if let Ok(serde_json::Value::Array(list)) = serde_json::from_str(&raw) {
                            events.extend(list);
                        }
                    }
                }
                status.incomplete = unfinished(&events);
            }
            // Only each project's newest message is read, not every content.
            let mut stmt = conn.prepare(
                "SELECT project_id, role, content FROM ( \
                   SELECT c.project_id, m.role, m.content, ROW_NUMBER() OVER ( \
                     PARTITION BY c.project_id ORDER BY m.created_at DESC, m.position DESC \
                   ) AS rn FROM messages m JOIN conversations c ON c.id = m.conversation_id \
                 ) WHERE rn = 1",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)? == "assistant" && asks_question(&r.get::<_, String>(2)?),
                ))
            })?;
            let newest =
                rows.collect::<rusqlite::Result<std::collections::BTreeMap<String, bool>>>()?;
            Ok(latest
                .into_values()
                .map(|(mut s, _)| {
                    s.awaiting_input =
                        s.status == "succeeded" && newest.get(&s.project_id) == Some(&true);
                    s
                })
                .collect())
        })
    }

    /// Global memory, oldest first.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn list_global_memory(&self) -> Result<Vec<GlobalMemory>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, kind, text, created_at FROM global_memories ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(GlobalMemory {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Adds a global memory entry.
    ///
    /// # Errors
    /// [`StoreError::BadRequest`] for an empty text, an unknown kind or a
    /// full list; SQLite errors.
    pub fn add_global_memory(&self, kind: &str, text: &str) -> Result<GlobalMemory> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_GLOBAL_MEMORY_CHARS {
            return Err(StoreError::BadRequest(format!(
                "text must be 1–{MAX_GLOBAL_MEMORY_CHARS} characters"
            )));
        }
        if !MEMORY_KINDS.contains(&kind) {
            return Err(StoreError::BadRequest(format!(
                "kind must be one of {}",
                MEMORY_KINDS.join(", ")
            )));
        }
        let entry = GlobalMemory {
            id: crate::store::new_id(),
            kind: kind.to_owned(),
            text: text.to_owned(),
            created_at: crate::now_ms(),
        };
        self.with_conn(|conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM global_memories", [], |r| r.get(0))?;
            if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_GLOBAL_MEMORIES {
                return Err(StoreError::BadRequest(format!(
                    "at most {MAX_GLOBAL_MEMORIES} memories; delete one first"
                )));
            }
            conn.execute(
                "INSERT INTO global_memories (id, kind, text, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![entry.id, entry.kind, entry.text, entry.created_at],
            )?;
            Ok(())
        })?;
        Ok(entry)
    }

    /// Returns whether the entry existed.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn delete_global_memory(&self, id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM global_memories WHERE id = ?1", [id])? > 0)
        })
    }

    /// Records that the project was opened now.
    ///
    /// # Errors
    /// [`StoreError::ProjectNotFound`] or SQLite errors.
    pub fn mark_viewed(&self, project_id: &str) -> Result<i64> {
        if !is_safe_id(project_id) {
            return Err(StoreError::ProjectNotFound);
        }
        let now = crate::now_ms();
        self.with_conn(|conn| {
            let changed = conn.execute(
                "INSERT INTO project_views (project_id, viewed_at) \
                 SELECT id, ?2 FROM projects WHERE id = ?1 \
                 ON CONFLICT(project_id) DO UPDATE SET viewed_at = excluded.viewed_at",
                params![project_id, now],
            )?;
            if changed == 0 {
                return Err(StoreError::ProjectNotFound);
            }
            Ok(now)
        })
    }

    /// `(project id, viewed at)`, most recent first.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn recently_viewed(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, viewed_at FROM project_views \
                 ORDER BY viewed_at DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfinished_work_is_open_todos_or_a_token_stop() {
        use serde_json::json;
        let plan = |statuses: &[&str]| {
            json!({"type": "tool_use", "name": "TodoWrite", "input": {"todos":
                statuses.iter().map(|s| json!({"content": "x", "status": s})).collect::<Vec<_>>()}})
        };
        assert!(!unfinished(&[]));
        assert!(unfinished(&[plan(&["completed", "in_progress"])]));
        assert!(!unfinished(&[
            plan(&["pending"]),
            plan(&["completed", "completed"])
        ]));
        assert!(unfinished(&[
            json!({"type": "usage", "stopReason": "max_tokens"})
        ]));
        assert!(!unfinished(&[
            json!({"type": "usage", "stopReason": "end_turn"})
        ]));
    }

    #[test]
    fn like_escapes_wildcards() {
        assert_eq!(like_pattern("50%_x"), "%50\\%\\_x%");
    }

    #[test]
    fn snippet_centres_on_the_match() {
        let long = format!("{} needle {}", "a".repeat(100), "b".repeat(100));
        let s = snippet(&long, "NEEDLE");
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.contains("needle"));
        assert!(s.chars().count() <= 2 * SNIPPET_CONTEXT + 8);
        assert_eq!(snippet("short\ntext", "text"), "short text");
    }
}
