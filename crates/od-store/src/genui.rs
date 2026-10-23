//! GenUI surfaces (track C4-depth): the questions a plugin run puts to the
//! user (confirm, authorize, choose, fill a form) and the answers, port of
//! the original `genui/store.ts` and `genui/registry.ts`.
//!
//! Kept in their own `<data>/genui.sqlite` (the original's
//! `genui_surfaces` table). A surface's answer is remembered at its
//! `persist` tier: `run` (never reused), `conversation` or `project`
//! (the next run reuses it without asking, "F8" in the original), until
//! the user revokes it, it expires, or the surface's schema changes.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Debug, thiserror::Error)]
pub enum GenuiError {
    #[error("{0}")]
    BadRequest(String),
    /// Nothing pending for that run and surface (never asked, answered,
    /// or the run ended).
    #[error("no pending surface for runId/surfaceId")]
    NotPending,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = GenuiError> = std::result::Result<T, E>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS genui_surfaces (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    conversation_id TEXT,
    run_id TEXT,
    plugin_id TEXT NOT NULL,
    surface_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    persist TEXT NOT NULL,
    schema_digest TEXT,
    spec_json TEXT NOT NULL,
    value_json TEXT,
    status TEXT NOT NULL,
    responded_by TEXT,
    requested_at INTEGER NOT NULL,
    responded_at INTEGER,
    expires_at INTEGER
);
CREATE INDEX IF NOT EXISTS genui_by_run ON genui_surfaces(run_id, surface_id);
CREATE INDEX IF NOT EXISTS genui_by_project ON genui_surfaces(project_id, surface_id);
";

const COLUMNS: &str = "id, project_id, conversation_id, run_id, plugin_id, surface_id, kind, \
     persist, schema_digest, spec_json, value_json, status, responded_by, requested_at, \
     responded_at, expires_at";

/// One asked surface and, once answered, its answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceRow {
    pub id: String,
    pub project_id: String,
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
    /// The plugin that declared it (the original's snapshot id).
    pub plugin_snapshot_id: String,
    pub surface_id: String,
    /// `confirmation | oauth-prompt | choice | form`.
    pub kind: String,
    /// `run | conversation | project`.
    pub persist: String,
    pub schema_digest: Option<String>,
    /// The surface as declared (prompt, schema, oauth…), plus `context`.
    pub spec: Value,
    pub value: Value,
    /// `pending | resolved | timeout | invalidated`.
    pub status: String,
    /// `user | agent | auto | cache`.
    pub responded_by: Option<String>,
    pub requested_at: i64,
    pub responded_at: Option<i64>,
    pub expires_at: Option<i64>,
}

fn row(r: &Row<'_>) -> rusqlite::Result<SurfaceRow> {
    let json = |s: Option<String>| {
        s.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null)
    };
    Ok(SurfaceRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        conversation_id: r.get(2)?,
        run_id: r.get(3)?,
        plugin_snapshot_id: r.get(4)?,
        surface_id: r.get(5)?,
        kind: r.get(6)?,
        persist: r.get(7)?,
        schema_digest: r.get(8)?,
        spec: json(r.get(9)?),
        value: json(r.get(10)?),
        status: r.get(11)?,
        responded_by: r.get(12)?,
        requested_at: r.get(13)?,
        responded_at: r.get(14)?,
        expires_at: r.get(15)?,
    })
}

/// Stable hex digest of a JSON schema: same keys in any order, same
/// digest (the original's `schemaDigest`).
#[must_use]
pub fn schema_digest(schema: &Value) -> String {
    fn canonical(v: &Value) -> Value {
        match v {
            Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
            Value::Object(o) => {
                let mut keys: Vec<&String> = o.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|k| (k.clone(), canonical(&o[k])))
                        .collect(),
                )
            }
            other => other.clone(),
        }
    }
    hex::encode(Sha256::digest(canonical(schema).to_string().as_bytes()))
}

/// What a run asks: `spec` is the declared surface (`id`, `kind`,
/// `persist`, `prompt`, `schema`…).
#[derive(Debug, Clone)]
pub struct SurfaceRequest<'a> {
    pub project_id: &'a str,
    pub conversation_id: Option<&'a str>,
    pub run_id: &'a str,
    pub plugin_id: &'a str,
    pub spec: &'a Value,
}

/// An answer to a pending surface.
#[derive(Debug, Clone, Copy)]
pub struct Answer<'a> {
    pub run_id: &'a str,
    pub surface_id: &'a str,
    pub value: &'a Value,
    /// `user | agent | auto`.
    pub responded_by: &'a str,
}

/// The surfaces database.
#[derive(Debug)]
pub struct GenuiStore {
    conn: Mutex<Connection>,
}

fn field<'a>(spec: &'a Value, key: &str) -> &'a str {
    spec.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// `persist` of a declared surface; unknown tiers count as `run`.
fn tier(spec: &Value) -> &str {
    match field(spec, "persist") {
        t @ ("conversation" | "project") => t,
        _ => "run",
    }
}

impl GenuiStore {
    /// Opens (creating) the database at `path`.
    ///
    /// # Errors
    /// SQLite and I/O errors.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn by_id(conn: &Connection, id: &str) -> Result<SurfaceRow> {
        Ok(conn.query_row(
            &format!("SELECT {COLUMNS} FROM genui_surfaces WHERE id = ?1"),
            [id],
            row,
        )?)
    }

    /// The remembered answer for `req` at its tier, if still valid. An
    /// expired answer or one given to another schema is invalidated.
    fn cached(
        conn: &Connection,
        req: &SurfaceRequest<'_>,
        digest: Option<&str>,
        now: i64,
    ) -> Result<Option<SurfaceRow>> {
        let surface_id = field(req.spec, "id");
        let found = match tier(req.spec) {
            "project" => conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM genui_surfaces WHERE project_id = ?1 AND surface_id = ?2 \
                         AND status = 'resolved' ORDER BY responded_at DESC LIMIT 1"
                    ),
                    params![req.project_id, surface_id],
                    row,
                )
                .optional()?,
            "conversation" => match req.conversation_id {
                Some(c) => conn
                    .query_row(
                        &format!(
                            "SELECT {COLUMNS} FROM genui_surfaces WHERE conversation_id = ?1 AND surface_id = ?2 \
                             AND status = 'resolved' ORDER BY responded_at DESC LIMIT 1"
                        ),
                        params![c, surface_id],
                        row,
                    )
                    .optional()?,
                None => None,
            },
            _ => None,
        };
        let Some(found) = found else { return Ok(None) };
        let expired = found.expires_at.is_some_and(|e| e <= now);
        let other_schema =
            matches!((digest, found.schema_digest.as_deref()), (Some(a), Some(b)) if a != b);
        if expired || other_schema {
            conn.execute(
                "UPDATE genui_surfaces SET status = 'invalidated' WHERE id = ?1",
                [&found.id],
            )?;
            return Ok(None);
        }
        Ok(Some(found))
    }

    /// Asks `req`, or reuses a remembered answer. Returns the row and
    /// whether it was reused (then nothing needs asking).
    ///
    /// # Errors
    /// [`GenuiError::BadRequest`] for a surface without an id; SQLite.
    pub fn request_or_reuse(
        &self,
        req: &SurfaceRequest<'_>,
        now: i64,
    ) -> Result<(SurfaceRow, bool)> {
        let surface_id = field(req.spec, "id");
        if surface_id.is_empty() {
            return Err(GenuiError::BadRequest(
                "a GenUI surface needs an id".to_owned(),
            ));
        }
        let digest = req
            .spec
            .get("schema")
            .filter(|s| !s.is_null())
            .map(schema_digest);
        let conn = self.conn();
        if let Some(hit) = Self::cached(&conn, req, digest.as_deref(), now)? {
            return Ok((hit, true));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let kind = match field(req.spec, "kind") {
            k @ ("oauth-prompt" | "choice" | "form") => k,
            _ => "confirmation",
        };
        let expires_at = req
            .spec
            .get("timeout")
            .and_then(Value::as_i64)
            .map(|t| now + t);
        conn.execute(
            "INSERT INTO genui_surfaces (id, project_id, conversation_id, run_id, plugin_id, surface_id, \
             kind, persist, schema_digest, spec_json, value_json, status, responded_by, requested_at, \
             responded_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, 'pending', \
             NULL, ?11, NULL, ?12)",
            params![
                id,
                req.project_id,
                req.conversation_id,
                req.run_id,
                req.plugin_id,
                surface_id,
                kind,
                tier(req.spec),
                digest,
                req.spec.to_string(),
                now,
                expires_at,
            ],
        )?;
        Ok((Self::by_id(&conn, &id)?, false))
    }

    /// Answers the run's pending `surface_id`, once: a second answer (or
    /// one to a surface never asked) finds nothing pending.
    ///
    /// # Errors
    /// [`GenuiError::NotPending`]; SQLite errors.
    pub fn respond(&self, a: Answer<'_>, now: i64) -> Result<SurfaceRow> {
        // The connection lock spans the lookup and the write: two answers
        // to one surface cannot both find it pending.
        let conn = self.conn();
        let pending: Option<String> = conn
            .query_row(
                "SELECT id FROM genui_surfaces WHERE run_id = ?1 AND surface_id = ?2 \
                 AND status = 'pending' ORDER BY requested_at DESC LIMIT 1",
                params![a.run_id, a.surface_id],
                |r| r.get(0),
            )
            .optional()?;
        let id = pending.ok_or(GenuiError::NotPending)?;
        conn.execute(
            "UPDATE genui_surfaces SET value_json = ?2, status = 'resolved', responded_by = ?3, \
             responded_at = ?4 WHERE id = ?1",
            params![id, a.value.to_string(), a.responded_by, now],
        )?;
        Self::by_id(&conn, &id)
    }

    /// Forgets every remembered answer to `surface_id` in the project;
    /// the next run asks again. Returns how many rows changed.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn revoke(&self, project_id: &str, surface_id: &str) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE genui_surfaces SET status = 'invalidated', responded_by = COALESCE(responded_by, 'user') \
             WHERE project_id = ?1 AND surface_id = ?2 AND status != 'invalidated'",
            params![project_id, surface_id],
        )?)
    }

    /// Answers a surface ahead of any run (`od ui prefill`).
    ///
    /// # Errors
    /// [`GenuiError::BadRequest`] without an id; SQLite.
    pub fn prefill(
        &self,
        project_id: &str,
        plugin_id: &str,
        spec: &Value,
        value: &Value,
        now: i64,
    ) -> Result<SurfaceRow> {
        let surface_id = field(spec, "id");
        if surface_id.is_empty() || plugin_id.is_empty() {
            return Err(GenuiError::BadRequest(
                "snapshotId and surfaceId are required".to_owned(),
            ));
        }
        let persist = match field(spec, "persist") {
            t @ ("run" | "conversation") => t,
            _ => "project",
        };
        let kind = match field(spec, "kind") {
            k @ ("oauth-prompt" | "choice" | "form") => k,
            _ => "confirmation",
        };
        let digest = spec
            .get("schema")
            .filter(|s| !s.is_null())
            .map(schema_digest);
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO genui_surfaces (id, project_id, conversation_id, run_id, plugin_id, surface_id, \
             kind, persist, schema_digest, spec_json, value_json, status, responded_by, requested_at, \
             responded_at, expires_at) VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'resolved', \
             'auto', ?10, ?10, ?11)",
            params![
                id,
                project_id,
                plugin_id,
                surface_id,
                kind,
                persist,
                digest,
                spec.to_string(),
                value.to_string(),
                now,
                spec.get("expiresAt").and_then(Value::as_i64),
            ],
        )?;
        Self::by_id(&conn, &id)
    }

    /// A run's surfaces, oldest first.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn for_run(&self, run_id: &str) -> Result<Vec<SurfaceRow>> {
        self.query("run_id = ?1 ORDER BY requested_at ASC, rowid ASC", run_id)
    }

    /// A project's surfaces, newest first.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn for_project(&self, project_id: &str) -> Result<Vec<SurfaceRow>> {
        self.query(
            "project_id = ?1 ORDER BY requested_at DESC, rowid DESC",
            project_id,
        )
    }

    fn query(&self, filter: &str, arg: &str) -> Result<Vec<SurfaceRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM genui_surfaces WHERE {filter}"
        ))?;
        let rows = stmt.query_map([arg], row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, GenuiStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = GenuiStore::open(&dir.path().join("genui.sqlite")).unwrap();
        (dir, s)
    }

    fn ask<'a>(spec: &'a Value, run: &'a str, conv: &'a str) -> SurfaceRequest<'a> {
        SurfaceRequest {
            project_id: "p1",
            conversation_id: Some(conv),
            run_id: run,
            plugin_id: "plug",
            spec,
        }
    }

    fn answer<'a>(run: &'a str, id: &'a str, value: &'a Value) -> Answer<'a> {
        Answer {
            run_id: run,
            surface_id: id,
            value,
            responded_by: "user",
        }
    }

    #[test]
    fn digests_ignore_key_order() {
        assert_eq!(
            schema_digest(&json!({"a": 1, "b": {"c": 2, "d": 3}})),
            schema_digest(&json!({"b": {"d": 3, "c": 2}, "a": 1}))
        );
        assert_ne!(
            schema_digest(&json!({"a": 1})),
            schema_digest(&json!({"a": 2}))
        );
    }

    #[test]
    fn an_answer_is_given_once() {
        let (_d, s) = store();
        let spec = json!({"id": "go", "kind": "confirmation", "persist": "run"});
        let (row, reused) = s.request_or_reuse(&ask(&spec, "r1", "c1"), 10).unwrap();
        assert!(!reused);
        assert_eq!(
            (row.status.as_str(), row.kind.as_str()),
            ("pending", "confirmation")
        );
        let v = json!(true);
        let row = s.respond(answer("r1", "go", &v), 11).unwrap();
        assert_eq!(
            (row.status.as_str(), row.responded_by.as_deref()),
            ("resolved", Some("user"))
        );
        assert_eq!(row.value, json!(true));
        assert!(matches!(
            s.respond(answer("r1", "go", &v), 12),
            Err(GenuiError::NotPending)
        ));
        assert!(matches!(
            s.respond(answer("r2", "go", &v), 12),
            Err(GenuiError::NotPending)
        ));
        // A run-tier answer is never reused.
        let (_, reused) = s.request_or_reuse(&ask(&spec, "r2", "c1"), 13).unwrap();
        assert!(!reused);
    }

    #[test]
    fn remembered_answers_follow_their_tier_until_revoked() {
        let (_d, s) = store();
        let project = json!({"id": "auth", "kind": "oauth-prompt", "persist": "project"});
        let conv = json!({"id": "style", "kind": "choice", "persist": "conversation",
                          "schema": {"type": "object", "properties": {"v": {"enum": ["a", "b"]}}}});
        s.request_or_reuse(&ask(&project, "r1", "c1"), 1).unwrap();
        s.request_or_reuse(&ask(&conv, "r1", "c1"), 1).unwrap();
        s.respond(answer("r1", "auth", &json!({"authorized": true})), 2)
            .unwrap();
        s.respond(answer("r1", "style", &json!({"v": "a"})), 2)
            .unwrap();

        // Another conversation: the project answer is reused, the
        // conversation one is asked again.
        let (row, reused) = s.request_or_reuse(&ask(&project, "r2", "c2"), 3).unwrap();
        assert!(reused && row.value["authorized"] == true);
        assert!(!s.request_or_reuse(&ask(&conv, "r2", "c2"), 3).unwrap().1);
        assert!(s.request_or_reuse(&ask(&conv, "r3", "c1"), 3).unwrap().1);

        // A changed schema asks again.
        let mut conv2 = conv.clone();
        conv2["schema"]["properties"]["v"]["enum"] = json!(["a", "b", "c"]);
        assert!(!s.request_or_reuse(&ask(&conv2, "r4", "c1"), 4).unwrap().1);

        // Revoke: asked again next time.
        assert!(s.revoke("p1", "auth").unwrap() >= 1);
        assert!(!s.request_or_reuse(&ask(&project, "r5", "c3"), 5).unwrap().1);
        assert_eq!(s.for_run("r1").unwrap().len(), 2);
        assert_eq!(
            s.for_project("p1").unwrap().len(),
            5,
            "reuse inserts nothing"
        );
    }

    #[test]
    fn expired_answers_and_prefills() {
        let (_d, s) = store();
        let spec = json!({"id": "t", "kind": "confirmation", "persist": "project", "timeout": 100});
        s.request_or_reuse(&ask(&spec, "r1", "c1"), 0).unwrap();
        s.respond(answer("r1", "t", &json!(true)), 1).unwrap();
        assert!(
            s.request_or_reuse(&ask(&spec, "r2", "c1"), 50).unwrap().1,
            "still fresh"
        );
        assert!(
            !s.request_or_reuse(&ask(&spec, "r3", "c1"), 500).unwrap().1,
            "expired"
        );

        let row = s
            .prefill(
                "p1",
                "plug",
                &json!({"id": "pre", "kind": "form"}),
                &json!({"x": 1}),
                9,
            )
            .unwrap();
        assert_eq!(
            (row.status.as_str(), row.persist.as_str()),
            ("resolved", "project")
        );
        let pre = json!({"id": "pre", "kind": "form", "persist": "project"});
        assert!(s.request_or_reuse(&ask(&pre, "r9", "c9"), 10).unwrap().1);
        assert!(
            s.request_or_reuse(&ask(&json!({"kind": "form"}), "r9", "c9"), 10)
                .is_err()
        );
    }
}
