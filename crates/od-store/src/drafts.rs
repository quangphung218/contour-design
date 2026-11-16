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
//! Every change to the record runs under the database lock, so an upload
//! that lands after its session was released is deleted at once, whatever
//! the order. Images are read (to tell them, [`content_ident`]) before it
//! is taken, and checked again under it.

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
  ident TEXT,
  PRIMARY KEY (project_id, path)
);
CREATE INDEX IF NOT EXISTS idx_draft_uploads_session ON draft_uploads(session);
CREATE TABLE IF NOT EXISTS draft_sessions_released (
  session TEXT PRIMARY KEY,
  released_at INTEGER NOT NULL
);
";

pub(crate) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    // A record made before files were told apart by `ident` gains it.
    let has_ident: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('draft_uploads') WHERE name = 'ident')",
        [],
        |r| r.get(0),
    )?;
    if !has_ident {
        conn.execute_batch("ALTER TABLE draft_uploads ADD COLUMN ident TEXT")?;
    }
    Ok(())
}

/// What tells the file the server wrote (a draft's image, an upload of a
/// batch) from another file put at the same path later: its content,
/// `sha256:<len>:<hex>` of the bytes the server wrote, taken from those
/// bytes (the disk is not read back). No metadata does: a size is shared
/// by many images, an mtime by every file written within the clock's step
/// (2 s on FAT, a few ms on ext4) or set back by `unzip`, `rsync -t`,
/// `cp -p`, and an inode is kept by a write in place, reused once freed,
/// and none at all on Windows.
///
/// What stays: a file of the very same bytes put at the path by someone
/// else (the agent, Finder) is taken for the server's, and deleted as it
/// would be. Nothing of it is lost: it is that same content.
pub(crate) fn content_ident(bytes: &[u8]) -> String {
    format!(
        "sha256:{}:{}",
        bytes.len(),
        hex::encode(<sha2::Sha256 as sha2::Digest>::digest(bytes))
    )
}

/// What the file at a path is, told by [`content_ident`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holds {
    /// The file `ident` was taken from.
    Same,
    /// Another file.
    Other,
    /// No file (gone), or the path cannot be resolved.
    Nothing,
    /// A file the disk will not let be read: whose it is cannot be told.
    Unknown,
}

/// What `path` holds, against `ident`: a [`content_ident`], or for a row
/// made before identities were of content, a [`file_ident`]. A file of
/// another size is another without being read; one of the same size is
/// read and hashed.
pub(crate) fn holds(root: &ProjectRoot, path: &str, ident: &str) -> Holds {
    let Some(content) = ident.strip_prefix("sha256:") else {
        return match file_ident(root, path) {
            Some(now) if now == ident => Holds::Same,
            Some(_) => Holds::Other,
            None => Holds::Nothing,
        };
    };
    let Some((len, _)) = content.split_once(':') else {
        return Holds::Other;
    };
    let Ok((_, full)) = files::stat_file(root, path) else {
        return Holds::Nothing;
    };
    match std::fs::symlink_metadata(&full) {
        Ok(meta) if meta.len().to_string() != len => return Holds::Other,
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Holds::Nothing,
        Err(_) => return Holds::Unknown,
    }
    match std::fs::read(&full) {
        Ok(bytes) if content_ident(&bytes) == ident => Holds::Same,
        Ok(_) => Holds::Other,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Holds::Nothing,
        Err(_) => Holds::Unknown,
    }
}

/// The identity rows made before [`content_ident`] kept (at most a day
/// old): size, mtime to the nanosecond and, where there is one, inode.
/// Read only to tell those rows' files. `None` when the file cannot be
/// read.
pub(crate) fn file_ident(root: &ProjectRoot, path: &str) -> Option<String> {
    let (_, full) = files::stat_file(root, path).ok()?;
    let meta = std::fs::symlink_metadata(full).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}:{mtime}:{}", meta.len(), inode(&meta)))
}

#[cfg(unix)]
fn inode(meta: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(meta)
}

#[cfg(not(unix))]
fn inode(_: &std::fs::Metadata) -> u64 {
    0
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
    // A rename onto itself moves nothing: its row stays.
    for (from, to) in pairs.iter().filter(|(from, to)| from != to) {
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
    // `length` of SQLite itself: `substr` counts characters, Rust's `len`
    // bytes (`Ảnh/` is 4 of one, 6 of the other).
    conn.execute(
        "DELETE FROM draft_uploads WHERE project_id = ?1 AND substr(path, 1, length(?2)) = ?2",
        params![project_id, prefix],
    )?;
    Ok(())
}

/// How much later than its record a draft image's file may have been
/// written and still be that image (clocks of a write and its record).
const MTIME_SLACK_MS: f64 = 2000.0;

/// What a delete of a draft's image does with the file at its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// The draft's (or none): it goes.
    Delete,
    /// Another file (put there by the agent, Finder, an unzip, a restore):
    /// the user's, it stays.
    Other,
    /// Whose it is cannot be told (the disk will not let it be read): it
    /// stays, and so does its row, for a later release or sweep.
    Unknown,
}

/// The fate of `path`, a draft's image: the file of `ident` ([`holds`]),
/// or for a record made before it was kept (`None`), one not written after
/// `uploaded_at` (ms).
fn fate(root: &ProjectRoot, path: &str, ident: Option<&str>, uploaded_at: i64) -> Fate {
    match ident {
        Some(ident) => match holds(root, path, ident) {
            Holds::Same | Holds::Nothing => Fate::Delete,
            Holds::Other => Fate::Other,
            Holds::Unknown => Fate::Unknown,
        },
        None => {
            if files::stat_file(root, path)
                .is_ok_and(|(file, _)| file.mtime > uploaded_at as f64 + MTIME_SLACK_MS)
            {
                Fate::Other
            } else {
                Fate::Delete
            }
        }
    }
}

/// Acts on `fate` for `path`. One already gone is fine. False when the
/// draft's file may still be there (the disk refused the delete, or the
/// read that tells it): its row must stay, or the file would ship as the
/// project's.
fn settle(root: &ProjectRoot, path: &str, fate: Fate) -> bool {
    match fate {
        Fate::Other => {
            tracing::info!(path, "a draft image's path holds another file: kept");
            true
        }
        Fate::Unknown => {
            tracing::warn!(path, "a draft image cannot be read: kept for later");
            false
        }
        Fate::Delete => match files::delete_file(root, path) {
            Ok(_) | Err(StoreError::FileNotFound) => true,
            Err(e) => {
                tracing::warn!(path, error = %e, "cannot delete a draft image");
                false
            }
        },
    }
}

/// A draft image's [`fate`] told before the database lock is taken:
/// reading and hashing images (5 of 20 MB take some 70 ms) would hold
/// every other call of the store. `stamp` is the file's metadata
/// ([`file_ident`]) before it was read: a write while it is read changes
/// it.
struct Foretold {
    fate: Fate,
    stamp: Option<String>,
}

fn foretell(root: &ProjectRoot, path: &str, ident: Option<&str>, uploaded_at: i64) -> Foretold {
    let stamp = file_ident(root, path);
    let fate = fate(root, path, ident, uploaded_at);
    Foretold { fate, stamp }
}

/// Deletes the file `path` of a draft if it is still the draft's
/// ([`settle`]), under the lock, with the fate told before it: kept
/// while the file's metadata is as it was then, told again otherwise.
/// What stays: a file written over in place between the two, its size,
/// mtime and inode kept, takes the fate of the one before (a window of
/// a few ms, for a writer outside the app).
fn delete_foretold(
    root: &ProjectRoot,
    path: &str,
    ident: Option<&str>,
    uploaded_at: i64,
    told: Foretold,
) -> bool {
    let fate = match told.stamp {
        Some(stamp) if file_ident(root, path).is_some_and(|now| now == stamp) => told.fate,
        _ => fate(root, path, ident, uploaded_at),
    };
    settle(root, path, fate)
}

/// A row taken out of the record by a write that may still fail
/// ([`Store::take_draft_uploads`]).
#[derive(Debug, Clone)]
pub struct TakenDraft(DraftRow);

/// A row of `draft_uploads`.
#[derive(Debug, Clone)]
struct DraftRow {
    path: String,
    session: String,
    uploaded_at: i64,
    ident: Option<String>,
}

impl Store {
    /// Notes `path`, just uploaded into `project_id` by the draft session
    /// `session`, its content `ident` ([`content_ident`] of the bytes
    /// written). Returns false when that session was released already:
    /// the file is deleted at once (an upload that landed after its page
    /// was left), if it is still that content.
    ///
    /// # Errors
    /// A bad session id, project or SQLite errors.
    pub(crate) fn record_draft_upload(
        &self,
        project_id: &str,
        session: &str,
        path: &str,
        ident: &str,
    ) -> Result<bool> {
        check_session(session)?;
        let root = self.root_for(project_id)?;
        let at = crate::now_ms();
        let late = self.with_conn(|conn| {
            // Noted even for a session released: one the disk will not
            // delete below stays a draft's, for the sweep.
            conn.execute(
                "INSERT OR REPLACE INTO draft_uploads \
                 (project_id, path, session, uploaded_at, ident) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![project_id, path, session, at, ident],
            )?;
            Ok(released(conn, session)?)
        })?;
        if late {
            let row = DraftRow {
                path: path.to_owned(),
                session: session.to_owned(),
                uploaded_at: at,
                ident: Some(ident.to_owned()),
            };
            self.drop_rows(&root, project_id, vec![row], || {})?;
        }
        Ok(!late)
    }

    /// The session's page is gone: deletes every image it uploaded that
    /// nothing took, and every one that lands later. One the disk would not
    /// delete keeps its row, for the sweep. Returns the paths released.
    ///
    /// # Errors
    /// A bad session id, project or SQLite errors.
    pub fn release_draft_session(&self, project_id: &str, session: &str) -> Result<Vec<String>> {
        self.release_draft_session_each(project_id, session, || {})
    }

    /// [`Self::release_draft_session`], calling `told` once the images'
    /// fates are told and before the lock is taken again to act on them,
    /// so a test can act there as another request would.
    pub(crate) fn release_draft_session_each(
        &self,
        project_id: &str,
        session: &str,
        told: impl FnOnce(),
    ) -> Result<Vec<String>> {
        check_session(session)?;
        let root = self.root_for(project_id)?;
        let rows: Vec<DraftRow> = self.with_conn(|conn| {
            // From here on an upload of the session goes at once.
            conn.execute(
                "INSERT OR IGNORE INTO draft_sessions_released (session, released_at) \
                 VALUES (?1, ?2)",
                params![session, crate::now_ms()],
            )?;
            let mut stmt = conn.prepare(
                "SELECT path, uploaded_at, ident FROM draft_uploads \
                 WHERE project_id = ?1 AND session = ?2",
            )?;
            let rows = stmt
                .query_map(params![project_id, session], |r| {
                    Ok(DraftRow {
                        path: r.get(0)?,
                        session: session.to_owned(),
                        uploaded_at: r.get(1)?,
                        ident: r.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        self.drop_rows(&root, project_id, rows, told)
    }

    /// Deletes the images of `rows`, drafts of released sessions, and
    /// forgets them. Their fates are told before the lock is taken
    /// ([`foretell`]), then `told` is called, so a test can act there as
    /// another request would. A row taken meanwhile (a comment sent, a
    /// page written), renamed or deleted is no longer the draft's: it
    /// stays. One the disk would not delete keeps its row, for the sweep.
    /// Returns the paths deleted.
    fn drop_rows(
        &self,
        root: &ProjectRoot,
        project_id: &str,
        rows: Vec<DraftRow>,
        told: impl FnOnce(),
    ) -> Result<Vec<String>> {
        let rows: Vec<_> = rows
            .into_iter()
            .map(|row| {
                let fate = foretell(root, &row.path, row.ident.as_deref(), row.uploaded_at);
                (row, fate)
            })
            .collect();
        told();
        self.with_conn(|conn| {
            let mut paths = Vec::new();
            for (row, fate) in rows {
                let still: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM draft_uploads WHERE project_id = ?1 \
                     AND path = ?2 AND session = ?3 AND uploaded_at = ?4 AND ident IS ?5)",
                    params![
                        project_id,
                        row.path,
                        row.session,
                        row.uploaded_at,
                        row.ident
                    ],
                    |r| r.get(0),
                )?;
                if still
                    && delete_foretold(root, &row.path, row.ident.as_deref(), row.uploaded_at, fate)
                {
                    forget(conn, project_id, &row.path)?;
                    paths.push(row.path);
                }
            }
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
    /// (ms), and forgets sessions released as long ago. One the disk would
    /// not delete keeps its row, for the next sweep.
    fn drop_drafts_before(&self, before: i64) -> Result<usize> {
        let rows: Vec<(String, String, i64, Option<String>)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, path, uploaded_at, ident FROM draft_uploads \
                 WHERE uploaded_at <= ?1",
            )?;
            let rows = stmt
                .query_map([before], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        let mut gone = 0;
        for (project, path, at, ident) in rows {
            // A project whose folder is gone: its rows go all the same.
            let root = self.root_for(&project).ok();
            let fate = root
                .as_ref()
                .map(|root| foretell(root, &path, ident.as_deref(), at));
            self.with_conn(|conn| {
                // Taken meanwhile (a comment sent): it stays.
                let still: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM draft_uploads WHERE project_id = ?1 \
                     AND path = ?2 AND uploaded_at <= ?3)",
                    params![project, path, before],
                    |r| r.get(0),
                )?;
                let deleted = still
                    && match (root.as_ref(), fate) {
                        (Some(root), Some(fate)) => {
                            delete_foretold(root, &path, ident.as_deref(), at, fate)
                        }
                        _ => true,
                    };
                if deleted {
                    forget(conn, &project, &path)?;
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
    /// nothing; one taken already (a write tried again) is fine. Give them
    /// back with [`Self::give_back_draft_uploads`] only if the write failed
    /// before the page was written: once it is, they are the page's even
    /// if a later step fails.
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
                        "SELECT session, uploaded_at, ident FROM draft_uploads \
                         WHERE project_id = ?1 AND path = ?2",
                        params![project_id, path],
                        |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, Option<String>>(2)?,
                            ))
                        },
                    )
                    .optional()?;
                if let Some((session, uploaded_at, ident)) = row {
                    taken.push(TakenDraft(DraftRow {
                        path: path.clone(),
                        session,
                        uploaded_at,
                        ident,
                    }));
                }
            }
            claim(conn, project_id, paths)?;
            Ok(taken)
        })
    }

    /// A write that took `taken` failed before the page was written: they
    /// are drafts' images again. One whose session was released meanwhile
    /// goes at once, as an upload landing after it does.
    pub fn give_back_draft_uploads(&self, project_id: &str, taken: Vec<TakenDraft>) {
        self.give_back_draft_uploads_each(project_id, taken, || {});
    }

    /// [`Self::give_back_draft_uploads`], calling `told` once the fates
    /// of the images of sessions released are told ([`Self::drop_rows`]).
    pub(crate) fn give_back_draft_uploads_each(
        &self,
        project_id: &str,
        taken: Vec<TakenDraft>,
        told: impl FnOnce(),
    ) {
        let res = self.with_conn(|conn| {
            let mut late = Vec::new();
            for TakenDraft(t) in taken {
                conn.execute(
                    "INSERT OR IGNORE INTO draft_uploads \
                     (project_id, path, session, uploaded_at, ident) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![project_id, t.path, t.session, t.uploaded_at, t.ident],
                )?;
                if released(conn, &t.session)? {
                    late.push(t);
                }
            }
            Ok(late)
        });
        // A project whose folder is gone: its rows stay for the sweep.
        let res = res.and_then(|late| match self.root_for(project_id) {
            Ok(root) if !late.is_empty() => self.drop_rows(&root, project_id, late, told),
            _ => Ok(Vec::new()),
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
            .record_draft_upload(p, session, &up.path, &content_ident(PNG))
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

    /// A folder name with diacritics: more bytes than characters.
    #[test]
    fn a_folder_with_diacritics_deleted_takes_its_rows() {
        let (_tmp, s, p, _) = store();
        let img = s
            .upload_file(&p, Some("Ảnh"), "shot.png", PNG)
            .unwrap_or_else(|e| panic!("{e}"))
            .path;
        assert!(
            s.record_draft_upload(&p, "s1", &img, &content_ident(PNG))
                .unwrap_or_else(|e| panic!("{e}"))
        );
        s.delete_folder(&p, "Ảnh").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty(),
            "its row stayed with the folder gone"
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
    fn a_draft_image_renamed_onto_itself_stays_a_drafts() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "img.png");
        s.rename_file(&p, &img, &img)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([img]),
            "not taken for the project's, so it ships nowhere"
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
    fn a_file_put_where_a_draft_image_was_stays_whatever_its_mtime() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "shot.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        // The draft's image goes outside the app; the user moves an old
        // image of theirs there (Finder, `mv`, unzip keep its mtime).
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&full, b"the user's old picture").unwrap_or_else(|e| panic!("{e}"));
        age_file(&s, &p, &img, 60 * 60 * 1000);
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &img), "the user's file was deleted");
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
        // Same for the quit of the app and the sweep.
        let (other, _) = upload(&s, &p, "s2", "other.png");
        let full = s.stat_file(&p, &other).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&full, b"another of theirs").unwrap_or_else(|e| panic!("{e}"));
        age_file(&s, &p, &other, 60 * 60 * 1000);
        s.release_all_draft_uploads()
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &other));
    }

    #[test]
    fn a_draft_image_found_gone_on_delete_leaves_no_row() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "gone.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            s.delete_file(&p, &img),
            Err(StoreError::FileNotFound)
        ));
        assert!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty(),
            "a file put there later would be taken for the draft's"
        );
    }

    #[test]
    fn a_record_made_before_ident_gains_the_column() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let conn =
            Connection::open(tmp.path().join("old.sqlite")).unwrap_or_else(|e| panic!("{e}"));
        conn.execute_batch(
            "CREATE TABLE draft_uploads (project_id TEXT NOT NULL, path TEXT NOT NULL, \
             session TEXT NOT NULL, uploaded_at INTEGER NOT NULL, PRIMARY KEY (project_id, path));",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        ensure_schema(&conn).unwrap_or_else(|e| panic!("{e}"));
        ensure_schema(&conn).unwrap_or_else(|e| panic!("{e}"));
        conn.execute(
            "INSERT INTO draft_uploads VALUES ('p', 'a.png', 's', 0, 'x')",
            [],
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_record_with_no_ident_takes_only_a_file_not_newer_than_it() {
        let (_tmp, s, p, _) = store();
        // `file_ident` could not read them when they were recorded.
        let (img, _) = upload(&s, &p, "s1", "draft.png");
        let (newer, _) = upload(&s, &p, "s1", "newer.png");
        s.with_conn(|c| {
            c.execute("UPDATE draft_uploads SET ident = NULL", [])?;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
        // A file at `newer`'s path written a minute after its record: the
        // agent's, the user's.
        s.with_conn(|c| {
            c.execute(
                "UPDATE draft_uploads SET uploaded_at = uploaded_at - 60000 WHERE path = ?1",
                [&newer],
            )?;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
        let full = s.stat_file(&p, &newer).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::write(&full, b"written later").unwrap_or_else(|e| panic!("{e}"));
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!exists(&s, &p, &img), "the draft's image went");
        assert!(exists(&s, &p, &newer), "a file newer than the record stays");
    }

    #[test]
    fn a_write_failing_after_its_session_was_released_deletes_its_images() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "hero.png");
        let taken = s
            .take_draft_uploads(&p, std::slice::from_ref(&img))
            .unwrap_or_else(|e| panic!("{e}"));
        // The page went while the write ran, then the write failed.
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &img), "taken: the release leaves it");
        s.give_back_draft_uploads(&p, taken);
        assert!(!exists(&s, &p, &img), "no page will take it now");
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
    }

    /// Its session released while the write ran, the write failed, and
    /// the disk refuses the delete: the image is a draft's again, out of
    /// the ZIP, for a later release or sweep.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_whose_image_cannot_be_deleted_gives_it_back() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "hero.png");
        let taken = s
            .take_draft_uploads(&p, std::slice::from_ref(&img))
            .unwrap_or_else(|e| panic!("{e}"));
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        let dir = full.parent().unwrap_or_else(|| panic!("no folder"));
        let lock = |mode| {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
                .unwrap_or_else(|e| panic!("{e}"));
        };
        lock(0o555);
        s.give_back_draft_uploads(&p, taken);
        lock(0o755);
        assert!(exists(&s, &p, &img), "the disk kept it");
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([img.clone()]),
            "a draft's again, not the project's"
        );
        assert_eq!(s.release_all_draft_uploads().unwrap_or(0), 1);
        assert!(!exists(&s, &p, &img));
    }

    #[cfg(unix)]
    #[test]
    fn a_draft_image_that_cannot_be_deleted_stays_a_drafts() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "stuck.png");
        let (other, _) = upload(&s, &p, "s2", "other.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        let dir = full.parent().unwrap_or_else(|| panic!("no folder"));
        let lock = |mode| {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
                .unwrap_or_else(|e| panic!("{e}"));
        };
        // The disk refuses the deletes: the files stay, and so do their
        // rows, out of the ZIP and for the next release or sweep.
        lock(0o555);
        let released = s.release_draft_session(&p, "s1");
        let all = s.release_all_draft_uploads();
        lock(0o755);
        released.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(all.unwrap_or_else(|e| panic!("{e}")), 0, "none went");
        assert!(exists(&s, &p, &img) && exists(&s, &p, &other));
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([img.clone(), other.clone()])
        );
        assert_eq!(s.release_all_draft_uploads().unwrap_or(0), 2);
        assert!(!exists(&s, &p, &img) && !exists(&s, &p, &other));
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
    }

    #[test]
    fn a_file_replaced_where_a_draft_image_is_is_the_users() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        s.replace_file(&p, &img, b"the user's picture", "Edit")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty(),
            "left out of the ZIP"
        );
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(exists(&s, &p, &img));
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

    /// Several uploads for a draft, all or nothing (QA-105): the second
    /// that cannot be recorded and both go, rows and files, so Try again
    /// lands them at the same names.
    #[test]
    fn uploads_one_of_which_cannot_be_recorded_keep_none() {
        let (_tmp, s, p, _) = store();
        let refuse = "CREATE TRIGGER no_b BEFORE INSERT ON draft_uploads \
                      WHEN NEW.path LIKE '%b.png' BEGIN SELECT RAISE(ABORT, 'busy'); END;";
        s.with_conn(|c| Ok(c.execute_batch(refuse)?))
            .unwrap_or_else(|e| panic!("{e}"));
        let both = [("a.png", PNG), ("b.png", PNG)];
        assert!(
            s.upload_files(&p, Some("comments"), &both, Some("s1"))
                .is_err()
        );
        for path in ["comments/a.png", "comments/b.png"] {
            assert!(!exists(&s, &p, path), "{path} kept");
        }
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
        s.with_conn(|c| Ok(c.execute_batch("DROP TRIGGER no_b;")?))
            .unwrap_or_else(|e| panic!("{e}"));
        let paths: Vec<String> = s
            .upload_files(&p, Some("comments"), &both, Some("s1"))
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["comments/a.png", "comments/b.png"]);
        // A session released already: none kept, none an error.
        s.release_draft_session(&p, "s2")
            .unwrap_or_else(|e| panic!("{e}"));
        let late = s
            .upload_files(&p, Some("comments"), &[("c.png", PNG)], Some("s2"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(late.is_empty());
        assert!(!exists(&s, &p, "comments/c.png"));
    }

    /// Writes `bytes` over the file at `full` in place (same inode), then
    /// gives it back its old mtime: `unzip -o`, `rsync -t`, `cp -p` onto it.
    /// Every identity of metadata (size, mtime, inode) stays the same.
    fn overwrite_keeping_mtime(full: &std::path::Path, bytes: &[u8]) {
        let was = std::fs::metadata(full)
            .and_then(|m| m.modified())
            .unwrap_or_else(|e| panic!("{e}"));
        let mut f = std::fs::File::options()
            .write(true)
            .truncate(true)
            .open(full)
            .unwrap_or_else(|e| panic!("{e}"));
        std::io::Write::write_all(&mut f, bytes).unwrap_or_else(|e| panic!("{e}"));
        f.set_modified(was).unwrap_or_else(|e| panic!("{e}"));
    }

    /// Deletes the file at `full` and puts `bytes` there as a new file with
    /// the old one's mtime: a clock too coarse to tell them apart (FAT 2 s,
    /// a raw kernel clock), on a disk that reuses the inode or has none.
    fn recreate_keeping_mtime(full: &std::path::Path, bytes: &[u8]) {
        let was = std::fs::metadata(full)
            .and_then(|m| m.modified())
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_file(full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(full, bytes).unwrap_or_else(|e| panic!("{e}"));
        std::fs::File::options()
            .write(true)
            .open(full)
            .and_then(|f| f.set_modified(was))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// Same size as [`PNG`], other bytes: the user's own picture.
    const THEIRS: &[u8] = b"\x89PNG\r\n\x1a\x00";

    fn read(s: &Store, p: &str, path: &str) -> Vec<u8> {
        s.read_file(p, path)
            .map(|(_, bytes)| bytes)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// A draft image written over in place by the user with a picture of
    /// the same size, its mtime set back (an unzip, `rsync -t`): no
    /// metadata tells it from the draft's. A release, the quit of the app
    /// and the sweep leave it.
    #[test]
    fn a_draft_image_overwritten_in_place_with_its_size_and_mtime_is_the_users() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        overwrite_keeping_mtime(&full, THEIRS);
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            exists(&s, &p, &img),
            "the user's file, same size and mtime, deleted"
        );
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
        let (other, _) = upload(&s, &p, "s2", "b.png");
        let full = s.stat_file(&p, &other).unwrap_or_else(|e| panic!("{e}")).1;
        overwrite_keeping_mtime(&full, THEIRS);
        s.release_all_draft_uploads()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&s, &p, &other), THEIRS, "deleted on quit");
    }

    /// The draft's image itself, untouched or put back byte for byte (a
    /// restore, a copy of it), is still the draft's: it goes.
    #[test]
    fn a_draft_image_put_back_byte_for_byte_still_goes() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&full, PNG).unwrap_or_else(|e| panic!("{e}"));
        age_file(&s, &p, &img, 60 * 60 * 1000);
        assert_eq!(
            s.release_draft_session(&p, "s1").unwrap_or_default(),
            std::slice::from_ref(&img)
        );
        assert!(!exists(&s, &p, &img));
    }

    /// An upload landing after its page was left goes at once, but only
    /// while its path still holds what was written: a file the agent put
    /// there in between (same size) stays.
    #[test]
    fn a_late_upload_whose_path_was_taken_meanwhile_leaves_the_other_file() {
        let (_tmp, s, p, _) = store();
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        let up = s
            .upload_file(&p, Some("comments"), "late.png", PNG)
            .unwrap_or_else(|e| panic!("{e}"));
        let full = s
            .stat_file(&p, &up.path)
            .unwrap_or_else(|e| panic!("{e}"))
            .1;
        recreate_keeping_mtime(&full, THEIRS);
        let kept = s
            .record_draft_upload(&p, "s1", &up.path, &content_ident(PNG))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!kept);
        assert_eq!(read(&s, &p, &up.path), THEIRS, "the agent's file deleted");
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
    }

    /// A release tells its images before it takes the lock to delete
    /// them. One a comment takes in between stays the comment's.
    #[test]
    fn an_image_taken_while_its_release_reads_it_stays() {
        let (_tmp, s, p, c) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let gone = s
            .release_draft_session_each(&p, "s1", || {
                s.create_comment(
                    &p,
                    &c,
                    CreateCommentRequest {
                        file_path: "index.html".into(),
                        selector: "h1".into(),
                        note: "n".into(),
                        attachments: vec![img.clone()],
                        ..CreateCommentRequest::default()
                    },
                )
                .unwrap_or_else(|e| panic!("{e}"));
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(gone.is_empty(), "{gone:?}");
        assert!(exists(&s, &p, &img), "the comment's image deleted");
    }

    /// A file put at a draft image's path once the release told it (the
    /// draft's then) is told again under the lock: the user's, it stays.
    #[test]
    fn a_file_put_while_its_release_reads_it_is_told_again() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        s.release_draft_session_each(&p, "s1", || {
            std::fs::write(&full, b"the user's, written just now")
                .unwrap_or_else(|e| panic!("{e}"));
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&s, &p, &img), b"the user's, written just now");
    }

    /// A write failing after its session was released tells its images
    /// before it takes the lock to delete them. One another write takes
    /// in between is that write's: it stays.
    #[test]
    fn an_image_taken_while_a_failed_write_gives_it_back_stays() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "hero.png");
        let taken = s
            .take_draft_uploads(&p, std::slice::from_ref(&img))
            .unwrap_or_else(|e| panic!("{e}"));
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        s.give_back_draft_uploads_each(&p, taken, || {
            s.take_draft_uploads(&p, std::slice::from_ref(&img))
                .unwrap_or_else(|e| panic!("{e}"));
        });
        assert!(exists(&s, &p, &img), "the other write's image deleted");
        assert!(s.unclaimed_draft_uploads(&p).unwrap_or_default().is_empty());
    }

    /// An upload landing after its page was left, which the disk will not
    /// delete, stays a draft's: out of the ZIP, for the sweep.
    #[cfg(unix)]
    #[test]
    fn a_late_upload_that_cannot_be_deleted_stays_a_drafts() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, s, p, _) = store();
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        let up = s
            .upload_file(&p, Some("comments"), "late.png", PNG)
            .unwrap_or_else(|e| panic!("{e}"));
        let full = s
            .stat_file(&p, &up.path)
            .unwrap_or_else(|e| panic!("{e}"))
            .1;
        let dir = full.parent().unwrap_or_else(|| panic!("no folder"));
        let lock = |mode| {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
                .unwrap_or_else(|e| panic!("{e}"));
        };
        lock(0o555);
        let kept = s.record_draft_upload(&p, "s1", &up.path, &content_ident(PNG));
        lock(0o755);
        assert!(!kept.unwrap_or_else(|e| panic!("{e}")));
        assert!(exists(&s, &p, &up.path), "the disk kept it");
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([up.path.clone()]),
            "shipped as the project's"
        );
        assert_eq!(s.release_all_draft_uploads().unwrap_or(0), 1);
        assert!(!exists(&s, &p, &up.path));
    }

    /// A row made before identities were of content (`size:mtime:inode`,
    /// kept at most a day) is still read as it was written.
    #[test]
    fn a_row_with_an_identity_of_metadata_is_read_as_before() {
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let (moved, _) = upload(&s, &p, "s1", "b.png");
        let root = s.root_for(&p).unwrap_or_else(|e| panic!("{e}"));
        for path in [&img, &moved] {
            let old = file_ident(&root, path).unwrap_or_else(|| panic!("no ident"));
            s.with_conn(|c| {
                c.execute(
                    "UPDATE draft_uploads SET ident = ?1 WHERE path = ?2",
                    params![old, path],
                )?;
                Ok(())
            })
            .unwrap_or_else(|e| panic!("{e}"));
        }
        let full = s.stat_file(&p, &moved).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&full, b"the user's, another size").unwrap_or_else(|e| panic!("{e}"));
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!exists(&s, &p, &img), "the draft's image went");
        assert!(exists(&s, &p, &moved), "another file at the path stays");
    }

    /// The disk cannot tell the file read: it stays, and so does its row,
    /// for a later release or sweep.
    #[cfg(unix)]
    #[test]
    fn a_draft_image_that_cannot_be_read_stays_a_drafts() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, s, p, _) = store();
        let (img, _) = upload(&s, &p, "s1", "a.png");
        let full = s.stat_file(&p, &img).unwrap_or_else(|e| panic!("{e}")).1;
        let mode = |m| {
            std::fs::set_permissions(&full, std::fs::Permissions::from_mode(m))
                .unwrap_or_else(|e| panic!("{e}"));
        };
        mode(0o000);
        let released = s.release_draft_session(&p, "s1");
        mode(0o644);
        assert!(released.unwrap_or_else(|e| panic!("{e}")).is_empty());
        assert!(exists(&s, &p, &img));
        assert_eq!(
            s.unclaimed_draft_uploads(&p).unwrap_or_default(),
            HashSet::from([img.clone()])
        );
        assert_eq!(s.release_all_draft_uploads().unwrap_or(0), 1);
    }

    /// An upload of a batch for a released session is deleted at once, its
    /// path free again. The batch keeps the store's write lock until it is
    /// done, undo included: another upload of the app waits for it, then
    /// lands where it likes, its file left alone.
    #[cfg(unix)]
    #[test]
    fn a_batch_undone_keeps_the_file_another_upload_put_where_one_of_its_was() {
        let (_tmp, s, p, _) = store();
        s.release_draft_session(&p, "s1")
            .unwrap_or_else(|e| panic!("{e}"));
        // The page fails: its history would be written through a link.
        let dir = s.root_for(&p).unwrap_or_else(|e| panic!("{e}")).dir;
        let elsewhere = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(elsewhere.path(), dir.join(crate::versions::VERSION_ROOT))
            .unwrap_or_else(|e| panic!("{e}"));
        let theirs: &[u8] = b"\x89PNG\r\n\x1a\nanother page's";
        let both = [("a.png", PNG), ("b.html", b"<p>b</p>".as_slice())];
        let other = std::thread::scope(|sc| {
            let mut other = None;
            let failed = s.upload_files_each(&p, Some("comments"), &both, Some("s1"), |path| {
                if path == "comments/a.png" {
                    assert!(!exists(&s, &p, path), "a late draft image is deleted");
                    assert!(s.versions_busy(), "the batch let go of the lock");
                    other = Some(sc.spawn(|| s.upload_file(&p, Some("comments"), "a.png", theirs)));
                }
            });
            assert!(failed.is_err());
            other
                .unwrap_or_else(|| panic!("no other upload"))
                .join()
                .unwrap_or_else(|_| panic!("the other upload panicked"))
                .unwrap_or_else(|e| panic!("{e}"))
        });
        assert_eq!(
            read(&s, &p, &other.path),
            theirs,
            "another upload's file deleted"
        );
        assert!(!exists(&s, &p, "comments/b.html"));
    }

    /// The page is left while its batch is stored: the release deletes an
    /// image recorded already, and another upload of the app waits for the
    /// batch (the lock) before a later image fails. The undo deletes only
    /// what is still the batch's; the other upload's file stays.
    #[cfg(unix)]
    #[test]
    fn a_batch_undone_after_its_page_was_left_mid_way_keeps_the_other_upload() {
        let (_tmp, s, p, _) = store();
        let dir = s.root_for(&p).unwrap_or_else(|e| panic!("{e}")).dir;
        let elsewhere = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(elsewhere.path(), dir.join(crate::versions::VERSION_ROOT))
            .unwrap_or_else(|e| panic!("{e}"));
        let batch = [
            ("keep.png", PNG),
            ("a.png", PNG),
            ("b.html", b"<p>b</p>".as_slice()),
        ];
        let other = std::thread::scope(|sc| {
            let mut other = None;
            let failed = s.upload_files_each(&p, Some("comments"), &batch, Some("s1"), |path| {
                if path == "comments/a.png" {
                    assert!(exists(&s, &p, path), "recorded, the session alive");
                    let gone = s
                        .release_draft_session(&p, "s1")
                        .unwrap_or_else(|e| panic!("{e}"));
                    assert_eq!(gone.len(), 2, "{gone:?}");
                    assert!(s.versions_busy(), "the batch let go of the lock");
                    other = Some(sc.spawn(|| s.upload_file(&p, Some("comments"), "a.png", THEIRS)));
                }
            });
            assert!(failed.is_err());
            other
                .unwrap_or_else(|| panic!("no other upload"))
                .join()
                .unwrap_or_else(|_| panic!("the other upload panicked"))
                .unwrap_or_else(|e| panic!("{e}"))
        });
        assert_eq!(
            read(&s, &p, &other.path),
            THEIRS,
            "another upload's file deleted by the undo"
        );
        for path in ["comments/keep.png", "comments/b.html"] {
            assert!(!exists(&s, &p, path), "{path} kept");
        }
    }

    /// The undo still deletes what the batch stored when nothing touched
    /// it. A file the agent wrote over in place (its shell, an unzip) is
    /// no longer the batch's, even of the same size and mtime; nor one
    /// put there anew with that mtime (a coarse clock: FAT, Windows, where
    /// there is no inode).
    #[test]
    fn a_batch_undone_deletes_its_own_files() {
        let (_tmp, s, p, _) = store();
        let refuse = "CREATE TRIGGER no_d BEFORE INSERT ON draft_uploads \
                      WHEN NEW.path LIKE '%d.png' BEGIN SELECT RAISE(ABORT, 'busy'); END;";
        s.with_conn(|c| Ok(c.execute_batch(refuse)?))
            .unwrap_or_else(|e| panic!("{e}"));
        let batch = [
            ("a.png", PNG),
            ("b.png", PNG),
            ("c.png", PNG),
            ("d.png", PNG),
        ];
        let failed = s.upload_files_each(&p, Some("comments"), &batch, Some("s1"), |path| {
            let full = s.stat_file(&p, path).unwrap_or_else(|e| panic!("{e}")).1;
            match path {
                "comments/b.png" => overwrite_keeping_mtime(&full, THEIRS),
                "comments/c.png" => recreate_keeping_mtime(&full, THEIRS),
                _ => {}
            }
        });
        assert!(failed.is_err());
        assert!(!exists(&s, &p, "comments/a.png"));
        assert!(!exists(&s, &p, "comments/d.png"));
        assert_eq!(read(&s, &p, "comments/b.png"), THEIRS, "written over, kept");
        assert_eq!(read(&s, &p, "comments/c.png"), THEIRS, "put anew, kept");
    }

    /// The batch undo on a real FAT32 volume (mtime in 2 s steps): the
    /// agent puts a file of the same size where one of the batch was,
    /// within the same 2 s. On macOS:
    /// `hdiutil create -size 40m -fs "MS-DOS FAT32" -volname ODFAT -o fat.dmg`,
    /// `hdiutil attach -nobrowse -mountpoint <dir> fat.dmg`, then
    /// `PROBE_DIR=<dir> cargo test -p od-store fat32 -- --ignored`.
    #[test]
    #[ignore = "needs a FAT32 volume at PROBE_DIR"]
    fn a_batch_undone_on_fat32_keeps_a_file_of_the_same_size() {
        let probe = std::env::var("PROBE_DIR").unwrap_or_else(|_| panic!("PROBE_DIR unset"));
        let tmp = tempfile::tempdir_in(probe).unwrap_or_else(|e| panic!("{e}"));
        let s = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, _) = s
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let p = project.id;
        let refuse = "CREATE TRIGGER no_b BEFORE INSERT ON draft_uploads \
                      WHEN NEW.path LIKE '%b.png' BEGIN SELECT RAISE(ABORT, 'busy'); END;";
        s.with_conn(|c| Ok(c.execute_batch(refuse)?))
            .unwrap_or_else(|e| panic!("{e}"));
        let batch = [("a.png", PNG), ("b.png", PNG)];
        let mut same_mtime = false;
        let failed = s.upload_files_each(&p, Some("comments"), &batch, Some("s1"), |path| {
            if path == "comments/a.png" {
                let full = s.stat_file(&p, path).unwrap_or_else(|e| panic!("{e}")).1;
                let mtime =
                    |f: &std::path::Path| std::fs::metadata(f).and_then(|m| m.modified()).ok();
                let was = mtime(&full);
                std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
                std::fs::write(&full, THEIRS).unwrap_or_else(|e| panic!("{e}"));
                same_mtime = was == mtime(&full);
            }
        });
        assert!(failed.is_err());
        eprintln!("same mtime on the volume: {same_mtime}");
        assert_eq!(
            read(&s, &p, "comments/a.png"),
            THEIRS,
            "the agent's file deleted"
        );
    }

    /// A project whose filling fails is not left half made.
    #[test]
    fn a_project_that_cannot_be_filled_is_removed() {
        let (_tmp, s, _, _) = store();
        let before = s.list_projects().unwrap_or_else(|e| panic!("{e}")).len();
        let mut dir = None;
        let made = s.create_project_with(
            CreateProjectRequest {
                name: "half".into(),
                ..CreateProjectRequest::default()
            },
            |project, _| {
                s.write_file(&project.id, "index.html", b"<p>a</p>", true, None)?;
                dir = Some(s.stat_file(&project.id, "index.html")?.1);
                Err::<(), _>(StoreError::BadRequest("the second file".into()))
            },
        );
        assert!(made.is_err());
        assert_eq!(
            s.list_projects().unwrap_or_else(|e| panic!("{e}")).len(),
            before
        );
        let page = dir.unwrap_or_else(|| panic!("filled"));
        assert!(!page.exists(), "{page:?} left on disk");
    }
}
