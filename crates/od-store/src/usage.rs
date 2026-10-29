//! Local usage marks for Settings ▸ About ▸ Your usage: when each project
//! was first previewed and whether it was ever exported. The database had
//! neither before 0.2.0-beta.7, so recording starts when this schema first
//! appears (`usage_since`) and only projects created after that count.
//!
//! Created idempotently on every open, like `project_views`.

use crate::error::Result;
use crate::paths::is_safe_id;
use crate::store::Store;
use rusqlite::{Connection, params};

const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS usage_marks (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (project_id, kind)
);
CREATE TABLE IF NOT EXISTS usage_since (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  at INTEGER NOT NULL
);
";

/// What a mark records; only the first occurrence is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageMark {
    /// The project's HTML first shown in the preview.
    FirstPreview,
    /// The project first exported, in any format.
    Export,
}

impl UsageMark {
    fn as_str(self) -> &'static str {
        match self {
            Self::FirstPreview => "first_preview",
            Self::Export => "export",
        }
    }
}

/// One project created since recording started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectUsage {
    pub created_at: i64,
    pub first_preview_at: Option<i64>,
    pub exported: bool,
    /// User messages over all its conversations.
    pub turns: u32,
}

/// Creates the tables and stamps when recording started. Safe to run on
/// every open: the stamp is written once.
pub(crate) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    conn.execute(
        "INSERT OR IGNORE INTO usage_since (id, at) VALUES (1, ?1)",
        [crate::now_ms()],
    )?;
    Ok(())
}

impl Store {
    /// Records `mark` for the project unless it already has one. Returns
    /// whether a mark was written (false for an unknown project too).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn mark_usage(&self, project_id: &str, mark: UsageMark) -> Result<bool> {
        if !is_safe_id(project_id) {
            return Ok(false);
        }
        self.with_conn(|conn| {
            Ok(conn.execute(
                "INSERT OR IGNORE INTO usage_marks (project_id, kind, at) \
                 SELECT id, ?2, ?3 FROM projects WHERE id = ?1",
                params![project_id, mark.as_str(), crate::now_ms()],
            )? > 0)
        })
    }

    /// When recording started, and every project created since.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn project_usage(&self) -> Result<(i64, Vec<ProjectUsage>)> {
        self.with_conn(|conn| {
            let since: i64 = conn.query_row("SELECT at FROM usage_since WHERE id = 1", [], |r| {
                r.get(0)
            })?;
            let mut stmt = conn.prepare(
                "SELECT p.created_at, \
                   (SELECT at FROM usage_marks WHERE project_id = p.id AND kind = 'first_preview'), \
                   EXISTS (SELECT 1 FROM usage_marks WHERE project_id = p.id AND kind = 'export'), \
                   (SELECT COUNT(*) FROM messages m JOIN conversations c ON c.id = m.conversation_id \
                     WHERE c.project_id = p.id AND m.role = 'user') \
                 FROM projects p WHERE p.created_at >= ?1",
            )?;
            let rows = stmt.query_map([since], |r| {
                Ok(ProjectUsage {
                    created_at: r.get(0)?,
                    first_preview_at: r.get(1)?,
                    exported: r.get(2)?,
                    turns: r.get(3)?,
                })
            })?;
            Ok((since, rows.collect::<rusqlite::Result<Vec<_>>>()?))
        })
    }
}
