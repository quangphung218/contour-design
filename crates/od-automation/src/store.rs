//! Routines and their run history in `<data>/automations.sqlite`.
//!
//! A database of its own, created with idempotent `CREATE … IF NOT EXISTS`
//! statements: it never touches `app.sqlite` or its `user_version`, so it
//! merges with other schema work without renumbering.

use crate::cron::Schedule;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS routines (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  project_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  prompt TEXT NOT NULL,
  schedule TEXT NOT NULL,
  enabled INTEGER NOT NULL,
  next_run_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS routine_runs (
  id TEXT PRIMARY KEY,
  routine_id TEXT NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
  trigger TEXT NOT NULL,
  status TEXT NOT NULL,
  run_id TEXT,
  conversation_id TEXT,
  error TEXT,
  started_at INTEGER NOT NULL,
  ended_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_routine_runs_routine ON routine_runs(routine_id, started_at DESC);
";

pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_PROMPT_CHARS: usize = 20_000;
/// History rows kept per routine.
pub const HISTORY_KEEP: i64 = 200;

#[derive(Debug, thiserror::Error)]
pub enum AutomationError {
    #[error("{0}")]
    Invalid(String),
    #[error("routine not found")]
    NotFound,
    #[error("the routine is already running")]
    Busy,
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, AutomationError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Schedule,
    Manual,
}

/// One routine run in the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    Succeeded,
    Failed,
    Canceled,
    /// Due while the previous run was still going (no overlap).
    Skipped,
}

impl RunState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Skipped => "skipped",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "canceled" => Self::Canceled,
            "skipped" => Self::Skipped,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRun {
    pub id: String,
    pub routine_id: String,
    pub trigger: Trigger,
    pub status: RunState,
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    pub error: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub name: String,
    pub project_id: String,
    pub agent_id: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_run: Option<RoutineRun>,
}

/// `POST /api/routines`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRoutine {
    pub name: String,
    pub project_id: String,
    pub agent_id: String,
    pub prompt: String,
    pub schedule: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// `PATCH /api/routines/:id`: only the fields present change.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutinePatch {
    pub name: Option<String>,
    pub agent_id: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<String>,
    pub enabled: Option<bool>,
}

fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(AutomationError::Invalid(format!(
            "name must be 1–{MAX_NAME_CHARS} characters without control characters"
        )));
    }
    Ok(name.to_owned())
}

fn check_prompt(prompt: &str) -> Result<String> {
    if prompt.trim().is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(AutomationError::Invalid(format!(
            "prompt must be 1–{MAX_PROMPT_CHARS} characters"
        )));
    }
    Ok(prompt.to_owned())
}

/// Ids we store and later pass on: agent ids, project ids.
pub fn check_id(kind: &str, id: &str) -> Result<String> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    if ok {
        Ok(id.to_owned())
    } else {
        Err(AutomationError::Invalid(format!("invalid {kind} id")))
    }
}

pub fn parse_schedule(raw: &str) -> Result<Schedule> {
    raw.parse::<Schedule>()
        .map_err(|e| AutomationError::Invalid(e.to_string()))
}

const ROUTINE_COLUMNS: &str = "id, name, project_id, agent_id, prompt, schedule, enabled, next_run_at, created_at, updated_at";
const RUN_COLUMNS: &str =
    "id, routine_id, trigger, status, run_id, conversation_id, error, started_at, ended_at";

fn routine_from_row(row: &Row<'_>) -> rusqlite::Result<Routine> {
    Ok(Routine {
        id: row.get(0)?,
        name: row.get(1)?,
        project_id: row.get(2)?,
        agent_id: row.get(3)?,
        prompt: row.get(4)?,
        schedule: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        next_run_at: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_run: None,
    })
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<RoutineRun> {
    let trigger: String = row.get(2)?;
    let status: String = row.get(3)?;
    Ok(RoutineRun {
        id: row.get(0)?,
        routine_id: row.get(1)?,
        trigger: if trigger == "manual" {
            Trigger::Manual
        } else {
            Trigger::Schedule
        },
        status: RunState::parse(&status),
        run_id: row.get(4)?,
        conversation_id: row.get(5)?,
        error: row.get(6)?,
        started_at: row.get(7)?,
        ended_at: row.get(8)?,
    })
}

/// The routines database. Every method blocks.
#[derive(Debug)]
pub struct RoutineStore {
    conn: Mutex<Connection>,
}

impl RoutineStore {
    /// Opens (creating) the database. History rows a crashed daemon left
    /// `running` become `failed`.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| AutomationError::Invalid(format!("data dir: {e}")))?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "UPDATE routine_runs SET status = 'failed', error = 'interrupted: the daemon stopped', \
             ended_at = ?1 WHERE status = 'running'",
            [now_ms()],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Validates and inserts; `next_run_at` comes from `next` (the caller's
    /// clock) when enabled.
    ///
    /// # Errors
    /// [`AutomationError::Invalid`] for bad fields, SQLite errors.
    pub fn create(
        &self,
        new: &NewRoutine,
        next: impl Fn(&Schedule) -> Option<i64>,
    ) -> Result<Routine> {
        let name = check_name(&new.name)?;
        let prompt = check_prompt(&new.prompt)?;
        let project_id = check_id("project", &new.project_id)?;
        let agent_id = check_id("agent", &new.agent_id)?;
        let schedule = parse_schedule(&new.schedule)?;
        let now = now_ms();
        let next_run_at = if new.enabled { next(&schedule) } else { None };
        let id = uuid::Uuid::new_v4().to_string();
        self.conn().execute(
            &format!("INSERT INTO routines ({ROUTINE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)"),
            params![id, name, project_id, agent_id, prompt, schedule.to_string(), new.enabled, next_run_at, now],
        )?;
        self.get(&id)
    }

    /// # Errors
    /// [`AutomationError::NotFound`], SQLite errors.
    pub fn get(&self, id: &str) -> Result<Routine> {
        let conn = self.conn();
        let mut routine = conn
            .query_row(
                &format!("SELECT {ROUTINE_COLUMNS} FROM routines WHERE id = ?1"),
                [id],
                routine_from_row,
            )
            .optional()?
            .ok_or(AutomationError::NotFound)?;
        routine.last_run = last_run(&conn, id)?;
        Ok(routine)
    }

    /// Every routine, newest first.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn list(&self) -> Result<Vec<Routine>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ROUTINE_COLUMNS} FROM routines ORDER BY created_at DESC, id"
        ))?;
        let mut routines = stmt
            .query_map([], routine_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for r in &mut routines {
            r.last_run = last_run(&conn, &r.id)?;
        }
        Ok(routines)
    }

    /// Enabled routines due at `now_ms`.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn due(&self, now_ms: i64) -> Result<Vec<Routine>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ROUTINE_COLUMNS} FROM routines \
             WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?1 ORDER BY next_run_at, id"
        ))?;
        Ok(stmt
            .query_map([now_ms], routine_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Applies `patch`; the next run is recomputed with `next` whenever the
    /// schedule or the switch changes.
    ///
    /// # Errors
    /// [`AutomationError::NotFound`], [`AutomationError::Invalid`], SQLite.
    pub fn update(
        &self,
        id: &str,
        patch: &RoutinePatch,
        next: impl Fn(&Schedule) -> Option<i64>,
    ) -> Result<Routine> {
        let mut r = self.get(id)?;
        if let Some(name) = &patch.name {
            r.name = check_name(name)?;
        }
        if let Some(prompt) = &patch.prompt {
            r.prompt = check_prompt(prompt)?;
        }
        if let Some(agent) = &patch.agent_id {
            r.agent_id = check_id("agent", agent)?;
        }
        let schedule = parse_schedule(patch.schedule.as_deref().unwrap_or(&r.schedule))?;
        r.schedule = schedule.to_string();
        let reschedule = patch.schedule.is_some() || patch.enabled.is_some();
        if let Some(enabled) = patch.enabled {
            r.enabled = enabled;
        }
        if reschedule {
            r.next_run_at = if r.enabled { next(&schedule) } else { None };
        }
        self.conn().execute(
            "UPDATE routines SET name = ?2, agent_id = ?3, prompt = ?4, schedule = ?5, enabled = ?6, \
             next_run_at = ?7, updated_at = ?8 WHERE id = ?1",
            params![id, r.name, r.agent_id, r.prompt, r.schedule, r.enabled, r.next_run_at, now_ms()],
        )?;
        self.get(id)
    }

    /// Sets when the routine fires next (`None` for never).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn set_next_run(&self, id: &str, next_run_at: Option<i64>) -> Result<()> {
        self.conn().execute(
            "UPDATE routines SET next_run_at = ?2 WHERE id = ?1",
            params![id, next_run_at],
        )?;
        Ok(())
    }

    /// Returns whether a row was deleted (its history goes with it).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn delete(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn()
            .execute("DELETE FROM routines WHERE id = ?1", [id])?
            > 0)
    }

    /// Records the start of a run (or a skip, which is already final).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn begin_run(
        &self,
        routine_id: &str,
        trigger: Trigger,
        status: RunState,
    ) -> Result<RoutineRun> {
        let now = now_ms();
        let run = RoutineRun {
            id: uuid::Uuid::new_v4().to_string(),
            routine_id: routine_id.to_owned(),
            trigger,
            status,
            run_id: None,
            conversation_id: None,
            error: None,
            started_at: now,
            ended_at: (status != RunState::Running).then_some(now),
        };
        let conn = self.conn();
        conn.execute(
            &format!("INSERT INTO routine_runs ({RUN_COLUMNS}) VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5, ?6)"),
            params![
                run.id,
                routine_id,
                if trigger == Trigger::Manual { "manual" } else { "schedule" },
                status.as_str(),
                run.started_at,
                run.ended_at
            ],
        )?;
        conn.execute(
            "DELETE FROM routine_runs WHERE routine_id = ?1 AND id NOT IN \
             (SELECT id FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC, rowid DESC LIMIT ?2)",
            params![routine_id, HISTORY_KEEP],
        )?;
        Ok(run)
    }

    /// Links the agent run once it exists.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn attach_run(&self, id: &str, run_id: &str, conversation_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE routine_runs SET run_id = ?2, conversation_id = ?3 WHERE id = ?1",
            params![id, run_id, conversation_id],
        )?;
        Ok(())
    }

    /// Records how a run ended.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn end_run(&self, id: &str, status: RunState, error: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE routine_runs SET status = ?2, error = ?3, ended_at = ?4 WHERE id = ?1",
            params![id, status.as_str(), error, now_ms()],
        )?;
        Ok(())
    }

    /// History, newest first.
    ///
    /// # Errors
    /// [`AutomationError::NotFound`], SQLite errors.
    pub fn runs(&self, routine_id: &str, limit: u32) -> Result<Vec<RoutineRun>> {
        self.get(routine_id)?;
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM routine_runs WHERE routine_id = ?1 \
             ORDER BY started_at DESC, rowid DESC LIMIT ?2"
        ))?;
        Ok(stmt
            .query_map(params![routine_id, limit], run_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn last_run(conn: &Connection, routine_id: &str) -> Result<Option<RoutineRun>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {RUN_COLUMNS} FROM routine_runs WHERE routine_id = ?1 \
                 ORDER BY started_at DESC, rowid DESC LIMIT 1"
            ),
            [routine_id],
            run_from_row,
        )
        .optional()?)
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
