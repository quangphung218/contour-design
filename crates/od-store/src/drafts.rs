//! Images uploaded for work not saved yet (a comment being written, a
//! Manual edit image not applied): the server's own record of them.
//!
//! A draft lives in one Studio page (its session); the images it uploads
//! are written into the project at once. Only this record tells them from
//! the project's own files: until a comment takes them (or a Manual edit
//! writes one into the page) they are left out of what ships (ZIP, Library
//! assets, Duplicate), and they go when the session is released (the
//! Studio left, the page closed, the app quit) or, if nothing released it
//! (a crash), once they are a day old.
//!
//! Every step runs under the database lock, so an upload that lands after
//! its session was released is deleted at once, whatever the order.

use crate::error::{Result, StoreError};
use crate::files::{self, ProjectRoot};
use crate::store::Store;
use rusqlite::{Connection, params};
use std::collections::HashSet;

/// How old an image of a draft no one released may get before a sweep
/// deletes it: long enough for a tab left open over a night.
pub const DRAFT_MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS draft_uploads (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  session TEXT NOT NULL,
  uploaded_at INTEGER NOT NULL,
  PRIMARY KEY (project_id, path)
);
CREATE INDEX IF NOT EXISTS idx_draft_uploads_session ON draft_uploads(session);
CREATE TABLE IF NOT EXISTS draft_sessions_released (
  session TEXT PRIMARY KEY,
  released_at INTEGER NOT NULL
);
";

pub(crate) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

/// Whether `session` is a draft session id a page may send: 1 to 64 ASCII
/// letters, digits or `-`.
#[must_use]
pub fn valid_session(session: &str) -> bool {
    (1..=64).contains(&session.len())
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn check_session(session: &str) -> Result<()> {
    if valid_session(session) {
        Ok(())
    } else {
        Err(StoreError::BadRequest("bad draft session".to_owned()))
    }
}

fn released(conn: &Connection, session: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM draft_sessions_released WHERE session = ?1)",
        [session],
        |r| r.get(0),
    )
}

/// Takes `paths` of `project_id` out of the record: they are a comment's
/// (or a page's) now, and ship with the project.
pub(crate) fn claim(conn: &Connection, project_id: &str, paths: &[String]) -> rusqlite::Result<()> {
    for path in paths {
        conn.execute(
            "DELETE FROM draft_uploads WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
        )?;
    }
    Ok(())
}

/// The record follows a file renamed (or moved with its folder).
pub(crate) fn renamed(
    conn: &Connection,
    project_id: &str,
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    for (from, to) in pairs {
        conn.execute(
            "UPDATE draft_uploads SET path = ?3 WHERE project_id = ?1 AND path = ?2",
            params![project_id, from, to],
        )?;
    }
    Ok(())
}

/// Deletes the file `path` of a draft; one already gone is fine.
fn delete_image(root: &ProjectRoot, path: &str) {
    match files::delete_file(root, path) {
        Ok(_) | Err(StoreError::FileNotFound) => {}
        Err(e) => tracing::warn!(path, error = %e, "cannot delete a draft image"),
    }
}

impl Store {
    /// Notes `path`, just uploaded into `project_id` by the draft session
    /// `session`. Returns false when that session was released already:
    /// the file is deleted at once (an upload that landed after its page
    /// was left).
    ///
    /// # Errors
    /// A bad session id, project or SQLite errors.
    pub fn record_draft_upload(&self, project_id: &str, session: &str, path: &str) -> Result<bool> {
        check_session(session)?;
        let root = self.root_for(project_id)?;
        self.with_conn(|conn| {
            if released(conn, session)? {
                delete_image(&root, path);
                return Ok(false);
            }
            conn.execute(
                "INSERT OR REPLACE INTO draft_uploads (project_id, path, session, uploaded_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![project_id, path, session, crate::now_ms()],
            )?;
            Ok(true)
        })
    }

    /// `paths` of `project_id` are taken by the page (a Manual edit image
    /// written into it): they ship with the project now.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn claim_draft_uploads(&self, project_id: &str, paths: &[String]) -> Result<()> {
        self.with_conn(|conn| Ok(claim(conn, project_id, paths)?))
    }

    /// The session's page is gone: deletes every image it uploaded that
    /// nothing took, and every one that lands later. Returns the paths
    /// deleted.
    ///
    /// # Errors
    /// A bad session id, project or SQLite errors.
    pub fn release_draft_session(&self, project_id: &str, session: &str) -> Result<Vec<String>> {
        check_session(session)?;
        let root = self.root_for(project_id)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO draft_sessions_released (session, released_at) \
                 VALUES (?1, ?2)",
                params![session, crate::now_ms()],
            )?;
            let paths = {
                let mut stmt = conn.prepare(
                    "SELECT path FROM draft_uploads WHERE project_id = ?1 AND session = ?2",
                )?;
                stmt.query_map(params![project_id, session], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for path in &paths {
                delete_image(&root, path);
            }
            conn.execute(
                "DELETE FROM draft_uploads WHERE project_id = ?1 AND session = ?2",
                params![project_id, session],
            )?;
            Ok(paths)
        })
    }

    /// Every page is going (the desktop app quits or restarts): deletes
    /// every draft image not taken, in every project. Returns how many.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn release_all_draft_uploads(&self) -> Result<usize> {
        self.drop_drafts_before(i64::MAX)
    }

    /// Deletes the draft images not taken that are older than `max_age`
    /// milliseconds (a page that crashed or never said it left), and
    /// forgets released sessions as old. Returns how many images went.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn sweep_draft_uploads(&self, max_age: i64) -> Result<usize> {
        self.drop_drafts_before(crate::now_ms().saturating_sub(max_age))
    }

    /// Deletes the draft images not taken uploaded at or before `before`
    /// (ms), and forgets sessions released as long ago.
    fn drop_drafts_before(&self, before: i64) -> Result<usize> {
        let rows: Vec<(String, String)> = self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT project_id, path FROM draft_uploads WHERE uploaded_at <= ?1")?;
            let rows = stmt
                .query_map([before], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        let mut gone = 0;
        for (project, path) in rows {
            // A project whose folder is gone: its rows go all the same.
            let root = self.root_for(&project).ok();
            self.with_conn(|conn| {
                // Taken meanwhile (a comment sent): it stays.
                let n = conn.execute(
                    "DELETE FROM draft_uploads WHERE project_id = ?1 AND path = ?2 \
                     AND uploaded_at <= ?3",
                    params![project, path, before],
                )?;
                if n > 0 {
                    if let Some(root) = &root {
                        delete_image(root, &path);
                    }
                    gone += 1;
                }
                Ok(())
            })?;
        }
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM draft_sessions_released WHERE released_at <= ?1",
                [before],
            )?;
            Ok(())
        })?;
        Ok(gone)
    }

    /// Paths of `project_id` uploaded for a draft and not taken: not the
    /// project's own files yet, so they do not ship.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn unclaimed_draft_uploads(&self, project_id: &str) -> Result<HashSet<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT path FROM draft_uploads WHERE project_id = ?1")?;
            let paths = stmt
                .query_map([project_id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            Ok(paths)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataDir;
    use od_contracts::{CreateCommentRequest, CreateProjectRequest};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn store() -> (tempfile::TempDir, Store, String, String) {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, conv) = store
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        (tmp, store, project.id, conv)
    }

    /// Uploads `name` into `comments/` for `session`; its path.
    fn upload(s: &Store, p: &str, session: &str, name: &str) -> (String, bool) {
        let up = s
            .upload_file(p, Some("comments"), name, PNG)
            .unwrap_or_else(|e| panic!("{e}"));
        let kept = s
            .record_draft_upload(p, session, &up.path)
            .unwrap_or_else(|e| panic!("{e}"));
        (up.path, kept)
    }

    fn exists(s: &Store, p: &str, path: &str) -> bool {
        s.stat_file(p, path).is_ok()
    }

    /// Ages every row and released session of the store by `ms`.
    fn age(s: &Store, ms: i64) {
        s.with_conn(|c| {
            c.execute(
                "UPDATE draft_uploads SET uploaded_at = uploaded_at - ?1",
                [ms],
            )?;
            c.execute(
                "UPDATE draft_sessions_released SET released_at = released_at - ?1",
                [ms],
            )?;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_released_session_takes_its_images_and_no_one_elses() {
        let (_tmp, s, p, _) = store();
        let (mine, _) = upload(&s, &p, "s1", "a.png");
        let (theirs, _) = upload(&s, &p, "s2", "b.png");
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default().len(),
            2,
            "not the project's files yet"
        );
        let gone = s.release_draft_session(&p, "s1").unwrap_or_default();
        assert_eq!(gone, std::slice::from_ref(&mine));
        assert!(!exists(&s, &p, &mine));
        assert!(exists(&s, &p, &theirs), "another tab's draft stays");
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([theirs])
        );
    }

    #[test]
    fn an_upload_landing_after_its_session_was_released_goes_at_once() {
        let (_tmp, s, p, _) = store();
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        let (late, kept) = upload(&s, &p, "s1", "late.png");
        assert!(!kept);
        assert!(!exists(&s, &p, &late), "nothing holds it");
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
    }

    #[test]
    fn a_comment_takes_its_images_and_a_release_leaves_them() {
        let (_tmp, s, p, c) = store();
        let (sent, _) = upload(&s, &p, "s1", "sent.png");
        let (other, _) = upload(&s, &p, "s1", "other.png");
        s.create_comment(
            &p,
            &c,
            CreateCommentRequest {
                file_path: "index.html".into(),
                selector: "h1".into(),
                note: "Bigger".into(),
                attachments: vec![sent.clone()],
                ..CreateCommentRequest::default()
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([other.clone()])
        );
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &sent), "the comment's image stays");
        assert!(!exists(&s, &p, &other));
    }

    #[test]
    fn a_comment_on_an_image_released_meanwhile_is_refused() {
        let (_tmp, s, p, c) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        let res = s.create_comment(
            &p,
            &c,
            CreateCommentRequest {
                file_path: "index.html".into(),
                selector: "h1".into(),
                note: "n".into(),
                attachments: vec![img],
                ..CreateCommentRequest::default()
            },
        );
        assert!(res.is_err(), "no comment pointing to a file gone");
    }

    #[test]
    fn a_sweep_takes_day_old_images_only() {
        let (_tmp, s, p, _) = store();
        let (old, _) = upload(&s, &p, "s1", "old.png");
        age(&s, DRAFT_MAX_AGE_MS + 60 * 60 * 1000);
        let (fresh, _) = upload(&s, &p, "s2", "fresh.png");
        age(&s, 60 * 60 * 1000);
        assert_eq!(s.sweep_draft_uploads(DRAFT_MAX_AGE_MS).unwrap_or(0), 1);
        assert!(!exists(&s, &p, &old), "25 hours old: gone");
        assert!(exists(&s, &p, &fresh), "an hour old: another tab's draft");
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([fresh])
        );
    }

    #[test]
    fn a_sweep_forgets_old_released_sessions() {
        let (_tmp, s, p, _) = store();
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        age(&s, DRAFT_MAX_AGE_MS + 1);
        s.sweep_draft_uploads(DRAFT_MAX_AGE_MS)
            .unwrap_or_else(|e| panic!("{e}"));
        let n: i64 = s
            .with_conn(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM draft_sessions_released", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap_or(-1);
        assert_eq!(n, 0);
    }

    #[test]
    fn release_all_takes_every_draft_image() {
        let (_tmp, s, p, _) = store();
        let (a, _) = upload(&s, &p, "s1", "a.png");
        let (b, _) = upload(&s, &p, "s2", "b.png");
        assert_eq!(s.release_all_draft_uploads().unwrap_or(0), 2);
        assert!(!exists(&s, &p, &a) && !exists(&s, &p, &b));
    }

    #[test]
    fn the_record_follows_a_rename_and_a_delete() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let moved = s
            .rename_file(&p, &img, "comments/b.png")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([moved.path.clone()])
        );
        s.delete_file(&p, &moved.path)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
    }

    #[test]
    fn opening_the_store_sweeps_day_old_draft_images() {
        let (tmp, s, p, _) = store();
        let (old, _) = upload(&s, &p, "s1", "old.png");
        let (fresh, _) = upload(&s, &p, "s2", "fresh.png");
        s.with_conn(|c| {
            c.execute(
                "UPDATE draft_uploads SET uploaded_at = uploaded_at - ?1 WHERE path = ?2",
                params![DRAFT_MAX_AGE_MS + 1, old],
            )?;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
        drop(s);
        // The daemon starts again after a crash.
        let s = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        assert!(!exists(&s, &p, &old));
        assert!(exists(&s, &p, &fresh));
    }

    #[test]
    fn a_session_id_is_checked() {
        assert!(valid_session("a1-B2"));
        assert!(!valid_session(""));
        assert!(!valid_session("../x"));
        assert!(!valid_session(&"a".repeat(65)));
        let (_tmp, s, p, _) = store();
        assert!(s.release_draft_session(&p, "a b").is_err());
    }
}
