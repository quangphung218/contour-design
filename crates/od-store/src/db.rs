//! SQLite schema and queries. Migrations are numbered and tracked with
//! `PRAGMA user_version` (spec 3.3).

use crate::error::{Result, StoreError};
use od_contracts::{Project, ProjectMetadata};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;

/// Ordered migrations; index `i` upgrades `user_version` from `i` to `i + 1`.
/// Never edit a shipped entry: append a new one.
const MIGRATIONS: &[&str] = &[r"
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  skill_id TEXT,
  design_system_id TEXT,
  pending_prompt TEXT,
  metadata_json TEXT,
  custom_instructions TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title TEXT,
  session_mode TEXT NOT NULL DEFAULT 'design',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_conversations_project ON conversations(project_id, updated_at DESC);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  agent_id TEXT,
  run_id TEXT,
  run_status TEXT,
  last_run_event_id INTEGER,
  position INTEGER NOT NULL,
  events_json TEXT,
  attachments_json TEXT,
  comment_attachments_json TEXT,
  produced_files_json TEXT,
  feedback_json TEXT,
  started_at INTEGER,
  ended_at INTEGER,
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_messages_conversation ON messages(conversation_id, position);

CREATE TABLE agent_sessions (
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  stable_prompt_hash TEXT,
  model TEXT,
  cwd TEXT,
  last_message_id TEXT,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (conversation_id, agent_id)
);

CREATE TABLE templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  source_project_id TEXT,
  files_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE preview_comments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  file_path TEXT NOT NULL,
  element_id TEXT NOT NULL,
  selector TEXT NOT NULL,
  label TEXT NOT NULL,
  text TEXT NOT NULL,
  position_json TEXT NOT NULL,
  html_hint TEXT NOT NULL,
  slide_index INTEGER,
  slide_key INTEGER NOT NULL DEFAULT -1,
  sort_key INTEGER,
  note TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_preview_comments_project ON preview_comments(project_id, file_path);
"];

/// Tables that must exist after migration. Used by tests.
pub const CORE_TABLES: [&str; 6] = [
    "projects",
    "conversations",
    "messages",
    "agent_sessions",
    "templates",
    "preview_comments",
];

/// Opens (creating if needed) the database in WAL mode and migrates it.
///
/// # Errors
/// SQLite errors, or [`StoreError::BadRequest`] when the file was written by
/// a newer schema than this build knows.
pub fn open(path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Current schema version this build migrates to.
#[must_use]
pub fn schema_version() -> usize {
    MIGRATIONS.len()
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let current = usize::try_from(current).unwrap_or(0);
    if current > MIGRATIONS.len() {
        return Err(StoreError::BadRequest(format!(
            "database schema v{current} is newer than this build (v{})",
            MIGRATIONS.len()
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", index + 1))?;
        tx.commit()?;
        tracing::info!(version = index + 1, "applied database migration");
    }
    Ok(())
}

const PROJECT_COLUMNS: &str = "id, name, skill_id, design_system_id, pending_prompt, \
     metadata_json, custom_instructions, created_at, updated_at";

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let metadata_json: Option<String> = row.get(5)?;
    let metadata = metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<ProjectMetadata>(raw).ok());
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        skill_id: row.get(2)?,
        design_system_id: row.get(3)?,
        pending_prompt: row.get(4)?,
        metadata,
        custom_instructions: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn metadata_json(project: &Project) -> Result<Option<String>> {
    Ok(project
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?)
}

/// Newest first.
pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROJECT_COLUMNS} FROM projects ORDER BY updated_at DESC, id"
    ))?;
    let rows = stmt.query_map([], project_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Option<Project>> {
    Ok(conn
        .query_row(
            &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"),
            [id],
            project_from_row,
        )
        .optional()?)
}

pub fn insert_project(conn: &Connection, project: &Project) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO projects ({PROJECT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ),
        params![
            project.id,
            project.name,
            project.skill_id,
            project.design_system_id,
            project.pending_prompt,
            metadata_json(project)?,
            project.custom_instructions,
            project.created_at,
            project.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_project(conn: &Connection, project: &Project) -> Result<()> {
    conn.execute(
        "UPDATE projects SET name = ?2, skill_id = ?3, design_system_id = ?4, pending_prompt = ?5, \
         metadata_json = ?6, custom_instructions = ?7, updated_at = ?8 WHERE id = ?1",
        params![
            project.id,
            project.name,
            project.skill_id,
            project.design_system_id,
            project.pending_prompt,
            metadata_json(project)?,
            project.custom_instructions,
            project.updated_at,
        ],
    )?;
    Ok(())
}

/// Returns whether a row was deleted. Conversations, messages and comments
/// go with it through `ON DELETE CASCADE`.
pub fn delete_project(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM projects WHERE id = ?1", [id])? > 0)
}

pub fn insert_conversation(conn: &Connection, id: &str, project_id: &str, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO conversations (id, project_id, title, session_mode, created_at, updated_at) \
         VALUES (?1, ?2, NULL, 'design', ?3, ?3)",
        params![id, project_id, now],
    )?;
    Ok(())
}

pub fn count_conversations(conn: &Connection, project_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM conversations WHERE project_id = ?1",
        [project_id],
        |r| r.get(0),
    )?)
}
