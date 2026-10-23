//! Project memory (spec 1.5): short facts the user wants every later turn
//! of a project to respect. Typed in the memory panel or picked up from a
//! chat message that starts with "remember…" / "ghi nhớ…".
//!
//! The table is created idempotently on every open instead of through a
//! numbered migration, so this track's schema never fights another
//! track's `user_version` bump.

use crate::error::{Result, StoreError};
use crate::store::Store;
use od_contracts::{MemoryEntry, MemorySource};
use rusqlite::{Connection, Row, params};

/// Longest memory entry (characters); longer text is cut.
pub const MAX_MEMORY_CHARS: usize = 500;
/// Most entries a project keeps; the oldest go first.
pub const MAX_MEMORY_ENTRIES: i64 = 50;

const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS project_memories (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  text TEXT NOT NULL,
  source TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_project_memories_project ON project_memories(project_id, created_at);
";

/// Creates the memory table when missing. Safe to run on every open.
///
/// # Errors
/// SQLite errors.
pub(crate) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

/// Lead-ins that turn a chat message into a memory entry, lowercase.
const LEAD_INS: [&str; 7] = [
    "remember that ",
    "remember: ",
    "remember ",
    "ghi nhớ rằng ",
    "ghi nhớ: ",
    "ghi nhớ ",
    "nhớ rằng ",
];

/// The fact a chat message asks to remember, if it starts with a lead-in
/// ("Remember that the brand color is teal" → "the brand color is teal").
#[must_use]
pub fn memory_from_message(message: &str) -> Option<String> {
    let first = message.trim().lines().next()?.trim();
    let lower = first.to_lowercase();
    let rest = LEAD_INS.iter().find_map(|lead| {
        lower
            .starts_with(lead)
            // Lowercasing keeps byte lengths for these lead-ins, so the
            // same offset cuts the original text.
            .then(|| first.get(lead.len()..))
            .flatten()
    })?;
    let fact = rest.trim().trim_end_matches(['.', '!']).trim();
    (!fact.is_empty()).then(|| truncate(fact))
}

fn truncate(text: &str) -> String {
    text.chars().take(MAX_MEMORY_CHARS).collect()
}

fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let source: String = row.get(3)?;
    Ok(MemoryEntry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        text: row.get(2)?,
        source: MemorySource::parse(&source),
        created_at: row.get(4)?,
    })
}

impl Store {
    /// Oldest first, the order the agent reads them.
    ///
    /// # Errors
    /// [`StoreError::ProjectNotFound`] or SQLite errors.
    pub fn list_memory(&self, project_id: &str) -> Result<Vec<MemoryEntry>> {
        self.get_project(project_id)?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, text, source, created_at FROM project_memories \
                 WHERE project_id = ?1 ORDER BY created_at, rowid",
            )?;
            let rows = stmt.query_map([project_id], entry_from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Adds a fact; the same text twice is stored once. Past
    /// [`MAX_MEMORY_ENTRIES`] the oldest entry is dropped.
    ///
    /// # Errors
    /// [`StoreError::BadRequest`] for empty text; project or SQLite errors.
    pub fn add_memory(
        &self,
        project_id: &str,
        text: &str,
        source: MemorySource,
    ) -> Result<MemoryEntry> {
        let text = truncate(text.trim());
        if text.is_empty() {
            return Err(StoreError::BadRequest("memory text required".into()));
        }
        let existing = self
            .list_memory(project_id)?
            .into_iter()
            .find(|m| m.text.eq_ignore_ascii_case(&text));
        if let Some(found) = existing {
            return Ok(found);
        }
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            text,
            source,
            created_at: crate::now_ms(),
        };
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO project_memories (id, project_id, text, source, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    entry.id,
                    entry.project_id,
                    entry.text,
                    entry.source.as_str(),
                    entry.created_at
                ],
            )?;
            conn.execute(
                "DELETE FROM project_memories WHERE project_id = ?1 AND id NOT IN \
                 (SELECT id FROM project_memories WHERE project_id = ?1 \
                  ORDER BY created_at DESC, rowid DESC LIMIT ?2)",
                params![project_id, MAX_MEMORY_ENTRIES],
            )?;
            Ok(())
        })?;
        Ok(entry)
    }

    /// # Errors
    /// [`StoreError::BadRequest`] when the entry is not in this project.
    pub fn delete_memory(&self, project_id: &str, id: &str) -> Result<()> {
        let deleted = self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM project_memories WHERE project_id = ?1 AND id = ?2",
                params![project_id, id],
            )?)
        })?;
        if deleted == 0 {
            return Err(StoreError::BadRequest("memory entry not found".into()));
        }
        Ok(())
    }

    /// Forgets everything about the project. Returns how many went.
    ///
    /// # Errors
    /// Project or SQLite errors.
    pub fn clear_memory(&self, project_id: &str) -> Result<usize> {
        self.get_project(project_id)?;
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM project_memories WHERE project_id = ?1",
                [project_id],
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataDir;
    use od_contracts::CreateProjectRequest;

    #[test]
    fn lead_ins_become_facts() {
        assert_eq!(
            memory_from_message("Remember that the brand color is teal.").as_deref(),
            Some("the brand color is teal")
        );
        assert_eq!(
            memory_from_message("ghi nhớ: luôn dùng font Inter\nrồi làm trang chủ").as_deref(),
            Some("luôn dùng font Inter")
        );
        assert_eq!(memory_from_message("Make it remember me"), None);
        assert_eq!(memory_from_message("remember   "), None);
    }

    #[test]
    fn memory_is_per_project_deduplicated_and_bounded() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let new_project = |name: &str| {
            store
                .create_project(CreateProjectRequest {
                    name: name.into(),
                    ..CreateProjectRequest::default()
                })
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        };
        let (a, b) = (new_project("a"), new_project("b"));
        let first = store
            .add_memory(&a.id, "Brand color is teal", MemorySource::Manual)
            .unwrap_or_else(|e| panic!("{e}"));
        let again = store
            .add_memory(&a.id, "brand color is TEAL", MemorySource::Chat)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(first.id, again.id, "same fact stored once");
        assert!(store.list_memory(&b.id).unwrap_or_default().is_empty());
        assert!(matches!(
            store.add_memory(&a.id, "  ", MemorySource::Manual),
            Err(StoreError::BadRequest(_))
        ));
        assert!(matches!(
            store.delete_memory(&b.id, &first.id),
            Err(StoreError::BadRequest(_))
        ));
        for i in 0..MAX_MEMORY_ENTRIES {
            store
                .add_memory(&a.id, &format!("fact {i}"), MemorySource::Manual)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let kept = store.list_memory(&a.id).unwrap_or_default();
        assert_eq!(kept.len() as i64, MAX_MEMORY_ENTRIES);
        assert_eq!(kept[0].text, "fact 0", "the oldest entry went");
        store
            .delete_memory(&a.id, &kept[0].id)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            store.clear_memory(&a.id).ok().map(|n| n as i64),
            Some(MAX_MEMORY_ENTRIES - 1)
        );
        // Deleting the project takes its memory along.
        store
            .add_memory(&b.id, "x", MemorySource::Manual)
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .delete_project(&b.id)
            .unwrap_or_else(|e| panic!("{e}"));
        let left: i64 = store
            .with_conn(|c| {
                Ok(c.query_row("SELECT COUNT(*) FROM project_memories", [], |r| r.get(0))?)
            })
            .unwrap_or(-1);
        assert_eq!(left, 0);
    }
}
