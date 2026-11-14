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
use rusqlite::{Connection, OptionalExtension, params};
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

/// The record follows a file renamed (or moved with its folder). A row
/// left at the new path (its file gone outside the app) is no longer true.
pub(crate) fn renamed(
    conn: &Connection,
    project_id: &str,
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    for (from, to) in pairs {
        forget(conn, project_id, to)?;
        conn.execute(
            "UPDATE draft_uploads SET path = ?3 WHERE project_id = ?1 AND path = ?2",
            params![project_id, from, to],
        )?;
    }
    Ok(())
}

/// A file is written at `path` that is not a draft's (the user's upload,
/// a write, a rename onto it): a row left there by a draft whose file went
/// outside the app no longer says anything about it.
pub(crate) fn forget(conn: &Connection, project_id: &str, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM draft_uploads WHERE project_id = ?1 AND path = ?2",
        params![project_id, path],
    )?;
    Ok(())
}

/// Every row under folder `dir` goes with the folder deleted.
pub(crate) fn forget_folder(
    conn: &Connection,
    project_id: &str,
    dir: &str,
) -> rusqlite::Result<()> {
    let prefix = format!("{}/", dir.trim_end_matches('/'));
    conn.execute(
        "DELETE FROM draft_uploads WHERE project_id = ?1 AND substr(path, 1, ?3) = ?2",
        params![
            project_id,
            prefix,
            i64::try_from(prefix.len()).unwrap_or(i64::MAX)
        ],
    )?;
    Ok(())
}

/// How much later than its record a draft image's file may have been
/// written and still be that image (clocks of a write and its record).
const MTIME_SLACK_MS: f64 = 2000.0;

/// Deletes the file `path` of a draft recorded at `uploaded_at` (ms); one
/// already gone is fine. A file written there after the record is another
/// file (put back by the agent, Finder or a restore): it stays.
fn delete_image(root: &ProjectRoot, path: &str, uploaded_at: i64) {
    if let Ok((file, _)) = files::stat_file(root, path)
        && file.mtime > uploaded_at as f64 + MTIME_SLACK_MS
    {
        tracing::info!(path, "a draft image's path holds a newer file: kept");
        return;
    }
    match files::delete_file(root, path) {
        Ok(_) | Err(StoreError::FileNotFound) => {}
        Err(e) => tracing::warn!(path, error = %e, "cannot delete a draft image"),
    }
}

/// A row taken out of the record by a write that may still fail
/// ([`Store::take_draft_uploads`]).
#[derive(Debug, Clone)]
pub struct TakenDraft {
    path: String,
    session: String,
    uploaded_at: i64,
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
                delete_image(&root, path, i64::MAX);
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
            let rows: Vec<(String, i64)> = {
                let mut stmt = conn.prepare(
                    "SELECT path, uploaded_at FROM draft_uploads \
                     WHERE project_id = ?1 AND session = ?2",
                )?;
                stmt.query_map(params![project_id, session], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (path, at) in &rows {
                delete_image(&root, path, *at);
            }
            let paths: Vec<String> = rows.into_iter().map(|(p, _)| p).collect();
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
        let rows: Vec<(String, String, i64)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, path, uploaded_at FROM draft_uploads WHERE uploaded_at <= ?1",
            )?;
            let rows = stmt
                .query_map([before], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        let mut gone = 0;
        for (project, path, at) in rows {
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
                        delete_image(root, &path, at);
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

    /// Takes `paths` of `project_id` out of the record for a write that
    /// puts them in a page, under the lock a release takes: once taken no
    /// release deletes them. One that is not there any more (a release
    /// came first) refuses the write rather than let the page point to
    /// nothing. Give them back with [`Self::give_back_draft_uploads`] if
    /// the write fails.
    ///
    /// # Errors
    /// [`StoreError::FileNotFound`] for an image gone, SQLite errors.
    pub fn take_draft_uploads(
        &self,
        project_id: &str,
        paths: &[String],
    ) -> Result<Vec<TakenDraft>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let root = self.root_for(project_id)?;
        self.with_conn(|conn| {
            for path in paths {
                files::stat_file(&root, path)?;
            }
            let mut taken = Vec::new();
            for path in paths {
                let row = conn
                    .query_row(
                        "SELECT session, uploaded_at FROM draft_uploads \
                         WHERE project_id = ?1 AND path = ?2",
                        params![project_id, path],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                    )
                    .optional()?;
                if let Some((session, uploaded_at)) = row {
                    taken.push(TakenDraft {
                        path: path.clone(),
                        session,
                        uploaded_at,
                    });
                }
            }
            claim(conn, project_id, paths)?;
            Ok(taken)
        })
    }

    /// A write that took `taken` failed: they are drafts' images again.
    pub fn give_back_draft_uploads(&self, project_id: &str, taken: Vec<TakenDraft>) {
        let res = self.with_conn(|conn| {
            for t in &taken {
                conn.execute(
                    "INSERT OR IGNORE INTO draft_uploads (project_id, path, session, uploaded_at) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![project_id, t.path, t.session, t.uploaded_at],
                )?;
            }
            Ok(())
        });
        if let Err(e) = res {
            tracing::warn!(error = %e, "cannot give draft images back");
        }
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

    /// Sets `path`'s mtime `ms` in the past, as a file uploaded then.
    fn age_file(s: &Store, p: &str, path: &str, ms: i64) {
        let full = s.stat_file(p, path).unwrap_or_else(|e| panic!("{e}")).1;
        let at = std::time::SystemTime::now()
            - std::time::Duration::from_millis(u64::try_from(ms).unwrap_or(0));
        std::fs::File::options()
            .write(true)
            .open(full)
            .and_then(|f| f.set_modified(at))
            .unwrap_or_else(|e| panic!("{e}"));
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
        age_file(&s, &p, &old, DRAFT_MAX_AGE_MS + 2 * 60 * 60 * 1000);
        let (fresh, _) = upload(&s, &p, "s2", "fresh.png");
        age(&s, 60 * 60 * 1000);
        age_file(&s, &p, &fresh, 60 * 60 * 1000);
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
        age_file(&s, &p, &old, DRAFT_MAX_AGE_MS + 1000);
        drop(s);
        // The daemon starts again after a crash.
        let s = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        assert!(!exists(&s, &p, &old));
        assert!(exists(&s, &p, &fresh));
    }

    #[test]
    fn a_folder_deleted_takes_its_rows_and_a_file_put_there_later_stays() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "shot.png");
        s.delete_folder(&p, "comments")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty(),
            "its rows went with the folder"
        );
        // The user's own image, same name, same folder.
        let mine = s
            .upload_file(&p, Some("comments"), "shot.png", PNG)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(mine.path, img, "the same path");
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        s.release_all_draft_uploads()
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            exists(&s, &p, &img),
            "the user's file was taken for the draft's"
        );
    }

    #[test]
    fn a_file_written_where_a_draft_image_was_is_not_the_drafts() {
        let (tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        // Gone outside the app, then written again: by the app ...
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        s.write_file(&p, &img, PNG, true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
        // ... by an upload of the user's, or a new file at exactly it.
        for (session, name, how) in [("s3", "c.png", 0), ("s4", "d.png", 1)] {
            let (path, _) = upload(&s, &p, session, name);
            let full = s.stat_file(&p, &path).unwrap_or_else(|e| panic!("{e}")).1;
            std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
            if how == 0 {
                s.upload_file(&p, Some("comments"), name, PNG)
                    .unwrap_or_else(|e| panic!("{e}"));
            } else {
                s.create_file_exact(&p, &path, PNG)
                    .unwrap_or_else(|e| panic!("{e}"));
            }
            assert!(
                !s.unclaimed_draft_uploads(&p)
                    .unwrap_or_default()
                    .contains(&path),
                "{path} is the user's now"
            );
        }
        // ... or by the agent, Finder: a file newer than the record stays.
        let (other, _) = upload(&s, &p, "s2", "b.png");
        age(&s, 60_000);
        let full = s.stat_file(&p, &other).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&full, b"user's").unwrap_or_else(|e| panic!("{e}"));
        s.release_draft_session(&p, "s2")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &other), "a newer file at the path stays");
        assert!(exists(&s, &p, &img));
        drop(tmp);
    }

    #[test]
    fn a_rename_onto_a_path_a_draft_left_works() {
        let (_tmp, s, p, _) = store();
        let (left, _) = upload(&s, &p, "s1", "left.png");
        let full = s.stat_file(&p, &left).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        let (img, _) = upload(&s, &p, "s2", "img.png");
        let moved = s
            .rename_file(&p, &img, &left)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([moved.path])
        );
    }

    #[test]
    fn a_write_takes_its_images_before_a_release_and_gives_them_back_if_it_fails() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "hero.png");
        let taken = s
            .take_draft_uploads(&p, std::slice::from_ref(&img))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(taken.len(), 1);
        // The write failed: a draft's again, and a release takes it.
        s.give_back_draft_uploads(&p, taken);
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([img.clone()])
        );
        s.take_draft_uploads(&p, std::slice::from_ref(&img))
            .unwrap_or_else(|e| panic!("{e}"));
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &img), "taken: the release leaves it");
        // Released before the write: the write is refused.
        let (late, _) = upload(&s, &p, "s2", "late.png");
        s.release_draft_session(&p, "s2")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(s.take_draft_uploads(&p, &[late]).is_err());
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
