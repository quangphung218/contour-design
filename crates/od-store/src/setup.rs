//! First-run setup marks for Settings ▸ About ▸ Your usage: when a new
//! data folder first reached each [`SetupStep`] (opened, agent found,
//! agent tested, first project, first preview). Kept on this machine and
//! never sent anywhere.
//!
//! Only a data folder this schema creates records setup: one made by an
//! earlier version already went through it unseen, so it is stamped "not
//! recorded" instead of a guess. Each step is written once and never
//! overwritten; marks are not tied to a project, so deleting the first
//! project keeps them.
//!
//! Created idempotently on every open, like `usage_marks`.

use crate::error::Result;
use crate::store::Store;
use od_contracts::{SetupMark, SetupStep};
use rusqlite::{Connection, params};

const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS setup_marks (
  step TEXT PRIMARY KEY,
  at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS setup_since (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  recorded INTEGER NOT NULL,
  at INTEGER NOT NULL
);
";

/// Creates the tables. On the first open of this schema, stamps whether
/// setup is recorded (`fresh`: the database was just created) and, if so,
/// records [`SetupStep::Opened`].
pub(crate) fn ensure_schema(conn: &Connection, fresh: bool) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    let now = crate::now_ms();
    conn.execute(
        "INSERT OR IGNORE INTO setup_since (id, recorded, at) VALUES (1, ?1, ?2)",
        params![fresh, now],
    )?;
    if fresh {
        insert(conn, SetupStep::Opened, now)?;
    }
    Ok(())
}

/// Writes `step` once, and only in a data folder that records setup.
pub(crate) fn insert(conn: &Connection, step: SetupStep, at: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO setup_marks (step, at) \
         SELECT ?1, ?2 FROM setup_since WHERE id = 1 AND recorded = 1",
        params![step.as_str(), at],
    )? > 0)
}

impl Store {
    /// Records `step` unless it was reached before. Returns whether a mark
    /// was written (false too when this data folder does not record setup).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn mark_setup(&self, step: SetupStep) -> Result<bool> {
        self.with_conn(|conn| Ok(insert(conn, step, crate::now_ms())?))
    }

    /// The steps reached, in step order; `None` when this data folder is
    /// older than setup recording.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn setup_marks(&self) -> Result<Option<Vec<SetupMark>>> {
        self.with_conn(|conn| {
            let recorded: bool =
                conn.query_row("SELECT recorded FROM setup_since WHERE id = 1", [], |r| {
                    r.get(0)
                })?;
            if !recorded {
                return Ok(None);
            }
            let mut stmt = conn.prepare("SELECT step, at FROM setup_marks")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut marks = Vec::new();
            for row in rows {
                let (step, at) = row?;
                // A step a later version added is skipped, not an error.
                if let Some(step) = SetupStep::parse(&step) {
                    marks.push(SetupMark { step, at });
                }
            }
            marks.sort_by_key(|m| m.step);
            Ok(Some(marks))
        })
    }
}
