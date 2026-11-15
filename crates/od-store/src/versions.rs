//! Per-file version history for HTML files.
//!
//! Layout: `<project>/.file-versions/<first 24 hex of sha256(fileName)>/`
//! holding `manifest.json` (schemaVersion 2) and `NNNN-<id>.html` snapshots.
//! Compatible with the original app's on-disk format.

use crate::error::{Result, StoreError};
use crate::paths::{is_html_name, kind_for, mime_for};
use od_contracts::{FileVersion, PromptSource, VersionSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const VERSION_ROOT: &str = ".file-versions";
const MANIFEST: &str = "manifest.json";
const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: String,
    file_name: String,
    version: u32,
    label: String,
    created_at: i64,
    source: VersionSource,
    prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_source: Option<PromptSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restore_from_version_id: Option<String>,
    size: u64,
    mime: String,
    kind: od_contracts::FileKind,
    content_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_version_id: Option<String>,
}

impl Entry {
    fn public(&self, current: Option<&str>) -> FileVersion {
        FileVersion {
            id: self.id.clone(),
            file_name: self.file_name.clone(),
            version: self.version,
            label: self.label.clone(),
            created_at: self.created_at,
            source: self.source,
            prompt: self.prompt.clone(),
            prompt_source: self.prompt_source,
            restore_from_version_id: self.restore_from_version_id.clone(),
            size: self.size,
            mime: self.mime.clone(),
            kind: self.kind,
            current: current == Some(self.id.as_str()),
            content_digest: self.content_digest.clone(),
            parent_version_id: self.parent_version_id.clone(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    current_version_id: Option<String>,
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deleted_at: Option<i64>,
    /// Closed only because its page took a name that is not a page's
    /// ([`rename_store`]): a rename back to a page's name opens it again.
    /// A history closed by a delete stays closed through such a round trip.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    renamed_away: bool,
    /// The digest of the page as it took that name: only the same file
    /// renamed back opens the history again, not one put at that name
    /// outside the app (or changed there) meanwhile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    away_digest: Option<String>,
}

/// Options for [`create_version`].
#[derive(Debug, Clone, Default)]
pub struct NewVersion {
    pub prompt: Option<String>,
    pub prompt_source: Option<PromptSource>,
    pub source: Option<VersionSource>,
    pub label: Option<String>,
    pub restore_from_version_id: Option<String>,
}

fn is_version_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The state of a file's history, as [`resolve`] finds it. Every public
/// function of this module goes through it and matches every state, so
/// none reads the closing marks of a manifest on its own.
///
/// What each kind of operation does in each state:
///
/// | Operation | `Linked` | `Absent` | `Open` | `Closed` |
/// |---|---|---|---|---|
/// | Record a version ([`create_version`], [`ensure_current_version`]) | refused (the caller checks first, [`refuse_linked_history`]) | starts v1 | appends | wiped, starts v1 |
/// | Close ([`mark_deleted`], [`note_write`]) | skipped: nothing is read through the link | nothing | closes | stays closed |
/// | Read ([`list_versions`], [`read_version`]) | refused | empty | its entries | empty, as `Absent`: not the file's now |
/// | Move ([`rename_store`]) | refused | closes what the target held | moves | moves, still closed |
///
/// A `Store` operation resolves (and refuses) first, then changes the
/// disk, then updates the history; an error in that last step is logged,
/// never reported as the operation failing.
enum History {
    /// The history folder, or the folder of all of them, is a link the
    /// agent made: what is written there would land wherever it points,
    /// maybe outside the project.
    Linked,
    /// No history: no manifest, or no entries.
    Absent,
    Open(Manifest),
    /// The file it was recorded for is gone (deleted, or replaced by
    /// another). `away` is the digest of the page as it took a name that
    /// is not a page's ([`rename_store`]): only the same bytes renamed back
    /// to a page's name open it again. `None` is closed for good.
    Closed {
        manifest: Manifest,
        away: Option<String>,
    },
}

/// The history folder of `file_name` and the state of the history in it.
///
/// # Errors
/// I/O and JSON errors reading its manifest.
fn resolve(project_dir: &Path, file_name: &str) -> Result<(PathBuf, History)> {
    let (dir, linked) = locate(project_dir, file_name);
    if linked {
        return Ok((dir, History::Linked));
    }
    let manifest = read_manifest(&dir)?;
    let history = if manifest.entries.is_empty() {
        History::Absent
    } else if manifest.deleted_at.is_none() {
        History::Open(manifest)
    } else {
        let away = if manifest.renamed_away {
            manifest.away_digest.clone()
        } else {
            None
        };
        History::Closed { manifest, away }
    };
    Ok((dir, history))
}

/// The history folder of `file_name`, and whether it (or the folder of
/// all of them) is a link: the part of [`resolve`] that reads no manifest.
fn locate(project_dir: &Path, file_name: &str) -> (PathBuf, bool) {
    let digest = hex::encode(Sha256::digest(file_name.as_bytes()));
    let root = project_dir.join(VERSION_ROOT);
    let dir = root.join(&digest[..24]);
    let linked = [&root, &dir]
        .iter()
        .any(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_symlink()));
    (dir, linked)
}

fn linked() -> StoreError {
    StoreError::BadRequest(format!(
        "{VERSION_ROOT} holds a link; Contour does not write version history through it"
    ))
}

/// Writes an open or closed history back; the only place that sets the
/// closing marks [`resolve`] reads.
fn save(dir: &Path, file_name: &str, history: History) -> Result<()> {
    let mut manifest = match history {
        History::Linked | History::Absent => return Ok(()),
        History::Open(mut manifest) => {
            manifest.deleted_at = None;
            manifest.renamed_away = false;
            manifest.away_digest = None;
            manifest
        }
        History::Closed { mut manifest, away } => {
            manifest.deleted_at = manifest.deleted_at.or_else(|| Some(crate::now_ms()));
            manifest.renamed_away = away.is_some();
            manifest.away_digest = away;
            manifest
        }
    };
    write_manifest(dir, file_name, &mut manifest)
}

/// Refuses, as recording a version of `file_name` would, a history folder
/// that is a link. Checked before a write that records one (or moves a
/// history) changes the disk: the file is not written when its history
/// will be refused.
///
/// # Errors
/// [`StoreError::BadRequest`] when the history folder holds a link.
pub fn refuse_linked_history(project_dir: &Path, file_name: &str) -> Result<()> {
    if locate(project_dir, file_name).1 {
        return Err(linked());
    }
    Ok(())
}

/// Lowercase hex sha256 of the snapshot bytes.
#[must_use]
pub fn content_digest(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    match fs::read_to_string(dir.join(MANIFEST)) {
        Ok(raw) => {
            let mut manifest: Manifest = serde_json::from_str(&raw)?;
            manifest.entries.retain(|e| is_version_id(&e.id));
            if manifest.schema_version < SCHEMA_VERSION {
                manifest.current_version_id = manifest.entries.last().map(|e| e.id.clone());
            } else if let Some(id) = &manifest.current_version_id
                && !manifest.entries.iter().any(|e| &e.id == id)
            {
                manifest.current_version_id = None;
            }
            Ok(manifest)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(err) => Err(err.into()),
    }
}

fn write_manifest(dir: &Path, file_name: &str, manifest: &mut Manifest) -> Result<()> {
    fs::create_dir_all(dir)?;
    manifest.schema_version = SCHEMA_VERSION;
    file_name.clone_into(&mut manifest.file_name);
    // Through a temp file and a rename: a link at the manifest is
    // replaced, never written through.
    crate::files::replace_atomic(&dir.join(MANIFEST), &serde_json::to_vec_pretty(manifest)?)
}

fn require_html(file_name: &str) -> Result<()> {
    if is_html_name(file_name) {
        Ok(())
    } else {
        Err(StoreError::NotHtml)
    }
}

/// The open history of a page, for reading: a closed one is not the file
/// at that name now ([`History`]).
fn open_for_read(project_dir: &Path, file_name: &str) -> Result<(PathBuf, Option<Manifest>)> {
    require_html(file_name)?;
    let (dir, history) = resolve(project_dir, file_name)?;
    match history {
        History::Linked => Err(linked()),
        History::Open(manifest) => Ok((dir, Some(manifest))),
        History::Absent | History::Closed { .. } => Ok((dir, None)),
    }
}

/// # Errors
/// [`StoreError::NotHtml`], [`StoreError::BadRequest`] for a linked
/// history folder, or I/O and JSON errors.
pub fn list_versions(project_dir: &Path, file_name: &str) -> Result<Vec<FileVersion>> {
    let Some(manifest) = open_for_read(project_dir, file_name)?.1 else {
        return Ok(Vec::new());
    };
    let current = manifest.current_version_id.as_deref();
    Ok(manifest.entries.iter().map(|e| e.public(current)).collect())
}

/// Returns the version and its snapshot text.
///
/// # Errors
/// [`StoreError::VersionNotFound`] for an unknown id, a missing snapshot,
/// or a closed history.
pub fn read_version(
    project_dir: &Path,
    file_name: &str,
    version_id: &str,
) -> Result<(FileVersion, String)> {
    require_html(file_name)?;
    if !is_version_id(version_id) {
        return Err(StoreError::BadRequest("version id required".to_owned()));
    }
    let (dir, manifest) = open_for_read(project_dir, file_name)?;
    let manifest = manifest.ok_or(StoreError::VersionNotFound)?;
    let entry = manifest
        .entries
        .iter()
        .find(|e| e.id == version_id)
        .ok_or(StoreError::VersionNotFound)?;
    let content = fs::read_to_string(dir.join(safe_content_path(entry))).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            StoreError::VersionNotFound
        } else {
            StoreError::from_read_io(err)
        }
    })?;
    Ok((
        entry.public(manifest.current_version_id.as_deref()),
        content,
    ))
}

/// Snapshot file name, ignoring anything that could leave the store folder.
fn safe_content_path(entry: &Entry) -> String {
    let p = &entry.content_path;
    let ok = p.ends_with(".html")
        && !p.contains("..")
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        p.clone()
    } else {
        format!("{}.html", entry.id)
    }
}

/// Appends a snapshot of `content` and makes it current.
///
/// A closed history (its file deleted or replaced) is wiped first: it is
/// not this file's, a restore of one of its entries included.
///
/// # Errors
/// [`StoreError::NotHtml`], [`StoreError::BadRequest`] for a linked
/// history folder, or I/O and JSON errors.
pub fn create_version(
    project_dir: &Path,
    file_name: &str,
    content: &str,
    opts: NewVersion,
) -> Result<FileVersion> {
    require_html(file_name)?;
    let (dir, history) = resolve(project_dir, file_name)?;
    let mut manifest = match history {
        History::Linked => return Err(linked()),
        History::Absent => Manifest::default(),
        History::Open(manifest) => manifest,
        History::Closed { .. } => {
            let _ = fs::remove_dir_all(&dir);
            Manifest::default()
        }
    };
    fs::create_dir_all(&dir)?;

    let restored_from = opts
        .restore_from_version_id
        .as_deref()
        .and_then(|id| manifest.entries.iter().find(|e| e.id == id))
        .map(|e| (e.id.clone(), e.version, e.content_digest.is_some()));
    let version = manifest
        .entries
        .iter()
        .map(|e| e.version)
        .max()
        .unwrap_or(0)
        + 1;
    let id = uuid::Uuid::new_v4().to_string();
    let content_path = format!("{version:04}-{id}.html");
    let source = opts
        .source
        .unwrap_or(match (&opts.restore_from_version_id, opts.prompt_source) {
            (Some(_), _) | (_, Some(PromptSource::Restore)) => VersionSource::Restore,
            (_, Some(PromptSource::Manual)) => VersionSource::Manual,
            _ => VersionSource::Ai,
        });
    let label = opts
        .label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map_or_else(
            || match &restored_from {
                Some((_, from, _)) => format!("Version {version} · restored from v{from}"),
                None => format!("Version {version}"),
            },
            str::to_owned,
        );
    let entry = Entry {
        id: id.clone(),
        file_name: file_name.to_owned(),
        version,
        label,
        created_at: crate::now_ms(),
        source,
        prompt: opts
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned),
        prompt_source: opts.prompt_source,
        restore_from_version_id: opts.restore_from_version_id.filter(|id| is_version_id(id)),
        size: content.len() as u64,
        mime: mime_for(file_name).to_owned(),
        kind: kind_for(file_name),
        content_path: content_path.clone(),
        content_digest: Some(content_digest(content)),
        parent_version_id: restored_from
            .filter(|(_, _, has_digest)| source == VersionSource::Restore && *has_digest)
            .map(|(id, _, _)| id),
    };
    fs::write(dir.join(&content_path), content.as_bytes())?;
    let public = entry.public(Some(&id));
    manifest.entries.push(entry);
    manifest.current_version_id = Some(id);
    write_manifest(&dir, file_name, &mut manifest)?;
    Ok(public)
}

/// Creates a baseline version when the history is empty (or closed).
/// Returns the current version when it holds `content` already.
///
/// # Errors
/// Same as [`create_version`].
pub fn ensure_current_version(
    project_dir: &Path,
    file_name: &str,
    content: &str,
    opts: NewVersion,
) -> Result<FileVersion> {
    require_html(file_name)?;
    let (dir, history) = resolve(project_dir, file_name)?;
    match history {
        History::Linked => return Err(linked()),
        History::Open(manifest) => {
            if let Some(current) = manifest
                .current_version_id
                .as_deref()
                .and_then(|id| manifest.entries.iter().find(|e| e.id == id))
                && fs::read_to_string(dir.join(safe_content_path(current)))
                    .ok()
                    .as_deref()
                    == Some(content)
            {
                return Ok(current.public(Some(&current.id)));
            }
        }
        History::Absent | History::Closed { .. } => {}
    }
    create_version(project_dir, file_name, content, opts)
}

/// Closes a file's history; the next save starts a fresh one. A file that
/// is not a page holds a history only while it is the page it was renamed
/// from ([`rename_store`]); that history goes for good. A linked history
/// folder is left alone: nothing is read through it.
///
/// # Errors
/// I/O and JSON errors.
pub fn mark_deleted(project_dir: &Path, file_name: &str) -> Result<()> {
    let (dir, history) = resolve(project_dir, file_name)?;
    match history {
        History::Linked | History::Absent => Ok(()),
        History::Open(_) | History::Closed { .. } if !is_html_name(file_name) => {
            match fs::remove_dir_all(&dir) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
                _ => Ok(()),
            }
        }
        History::Open(manifest) => save(
            &dir,
            file_name,
            History::Closed {
                manifest,
                away: None,
            },
        ),
        History::Closed { .. } => Ok(()),
    }
}

/// Moves a history along with a renamed file; returns whether one moved
/// (what [`rename_store`]`(to, from)` undoes). A history left at the
/// target, by a file gone outside the app, is replaced, or closed when the
/// file brings none: it is not the renamed file's. A page renamed to a
/// name that is not a page's takes its history along closed, and gets it
/// back once renamed to a page's name again (`page.html` → `page.md` →
/// `page.html` keeps it); one already closed (its page deleted) stays
/// closed.
///
/// # Errors
/// [`StoreError::BadRequest`] when either history folder is a link
/// (nothing moved); I/O and JSON errors, the history is then where it was.
pub fn rename_store(project_dir: &Path, from: &str, to: &str) -> Result<bool> {
    if from == to {
        return Ok(false);
    }
    refuse_linked_history(project_dir, to)?;
    let (old_dir, history) = resolve(project_dir, from)?;
    if matches!(history, History::Linked) {
        return Err(linked());
    }
    let new_dir = locate(project_dir, to).0;
    if matches!(history, History::Absent) {
        mark_deleted(project_dir, to)?;
        return Ok(false);
    }
    if new_dir.exists() {
        fs::remove_dir_all(&new_dir)?;
    }
    fs::rename(&old_dir, &new_dir)?;
    let relabel = || {
        let relabelled = |mut manifest: Manifest| {
            for entry in &mut manifest.entries {
                to.clone_into(&mut entry.file_name);
            }
            manifest
        };
        let moved = match (history, is_html_name(from), is_html_name(to)) {
            (History::Linked | History::Absent, _, _) => return Ok(()),
            // Back at a page's name: open again only as the same bytes.
            (History::Closed { manifest, away }, false, true) => {
                if away.is_some() && away == renamed_digest(project_dir, from, to) {
                    History::Open(relabelled(manifest))
                } else {
                    History::Closed {
                        manifest: relabelled(manifest),
                        away: None,
                    }
                }
            }
            (History::Open(manifest), true, false) => History::Closed {
                manifest: relabelled(manifest),
                away: renamed_digest(project_dir, from, to),
            },
            (History::Open(manifest), _, _) => History::Open(relabelled(manifest)),
            (History::Closed { manifest, away }, _, _) => History::Closed {
                manifest: relabelled(manifest),
                away,
            },
        };
        save(&new_dir, to, moved)
    };
    if let Err(err) = relabel() {
        let _ = fs::rename(&new_dir, &old_dir);
        return Err(err);
    }
    Ok(true)
}

/// Whether `file_name` (not a page's name) holds the history of a page
/// renamed to it, which a write in the app follows ([`note_write`]).
#[must_use]
pub fn renamed_away(project_dir: &Path, file_name: &str) -> bool {
    !is_html_name(file_name)
        && resolve(project_dir, file_name).is_ok_and(|(_, history)| match history {
            History::Closed { away, .. } => away.is_some(),
            History::Linked | History::Absent | History::Open(_) => false,
        })
}

/// A file that is not a page, written by the app (`before` what it held
/// just before). If it is a page renamed away ([`rename_store`]) and still
/// that page (`before` matches the digest it left with), the digest
/// follows the write, so a rename back keeps the history. A file replaced
/// outside the app meanwhile is not the page: its history closes for good.
///
/// # Errors
/// I/O and JSON errors.
pub fn note_write(
    project_dir: &Path,
    file_name: &str,
    before: Option<&[u8]>,
    after: &[u8],
) -> Result<()> {
    if is_html_name(file_name) {
        return Ok(());
    }
    let (dir, history) = resolve(project_dir, file_name)?;
    match history {
        History::Closed {
            manifest,
            away: Some(left),
        } => {
            let same = before.is_some_and(|b| hex::encode(Sha256::digest(b)) == left);
            // Not the page: no digest, so no rename back opens the history.
            let away = same.then(|| hex::encode(Sha256::digest(after)));
            save(&dir, file_name, History::Closed { manifest, away })
        }
        History::Linked
        | History::Absent
        | History::Open(_)
        | History::Closed { away: None, .. } => Ok(()),
    }
}

/// The digest of the file being renamed: at `to` once renamed, at `from`
/// while its history moves first (the undo of a rename that failed).
fn renamed_digest(project_dir: &Path, from: &str, to: &str) -> Option<String> {
    [to, from]
        .iter()
        .find_map(|name| fs::read(project_dir.join(name)).ok())
        .map(|bytes| hex::encode(Sha256::digest(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_dir(dir: &Path, name: &str) -> PathBuf {
        resolve(dir, name).unwrap().0
    }

    #[test]
    fn create_list_read_restore_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let v1 = create_version(dir, "index.html", "<p>one</p>", NewVersion::default()).unwrap();
        let v2 = create_version(dir, "index.html", "<p>two</p>", NewVersion::default()).unwrap();
        assert_eq!((v1.version, v2.version), (1, 2));
        let list = list_versions(dir, "index.html").unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[1].current && !list[0].current);
        let (_, content) = read_version(dir, "index.html", &v1.id).unwrap();
        assert_eq!(content, "<p>one</p>");
        let restored = create_version(
            dir,
            "index.html",
            &content,
            NewVersion {
                restore_from_version_id: Some(v1.id.clone()),
                ..NewVersion::default()
            },
        )
        .unwrap();
        assert_eq!(restored.source, VersionSource::Restore);
        assert_eq!(restored.parent_version_id.as_deref(), Some(v1.id.as_str()));
        assert_eq!(restored.label, "Version 3 · restored from v1");
    }

    #[test]
    fn non_html_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            list_versions(tmp.path(), "a.css"),
            Err(StoreError::NotHtml)
        ));
    }

    #[test]
    fn deleted_history_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "a.html", "x", NewVersion::default()).unwrap();
        mark_deleted(dir, "a.html").unwrap();
        let v = create_version(dir, "a.html", "y", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1);
        assert_eq!(list_versions(dir, "a.html").unwrap().len(), 1);
    }

    #[test]
    fn a_page_renamed_to_another_kind_closes_its_history_and_takes_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "a.html", "x", NewVersion::default()).unwrap();
        rename_store(dir, "a.html", "a.txt").unwrap();
        // A new a.html starts its own history.
        let v = ensure_current_version(dir, "a.html", "y", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1, "not the old page's version 2");
        // A history left at b.html (its page gone outside the app) is not
        // the history of a file renamed to b.html.
        create_version(dir, "b.html", "old", NewVersion::default()).unwrap();
        rename_store(dir, "notes.txt", "b.html").unwrap();
        let v = ensure_current_version(dir, "b.html", "new", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1);
        assert_eq!(list_versions(dir, "b.html").unwrap().len(), 1);
    }

    #[test]
    fn a_page_renamed_away_and_back_keeps_its_history() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        create_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        // The file moves along with its history (as `Store::rename_file`).
        fs::create_dir(dir.join("notes")).unwrap();
        fs::write(dir.join("page.md"), "two").unwrap();
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        fs::rename(dir.join("page.md"), dir.join("notes/page.md")).unwrap();
        assert!(rename_store(dir, "page.md", "notes/page.md").unwrap());
        fs::rename(dir.join("notes/page.md"), dir.join("page.html")).unwrap();
        assert!(rename_store(dir, "notes/page.md", "page.html").unwrap());
        let v = ensure_current_version(dir, "page.html", "three", NewVersion::default()).unwrap();
        assert_eq!(v.version, 3, "the round trip lost the history");
        let list = list_versions(dir, "page.html").unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|v| v.file_name == "page.html"));
    }

    #[test]
    fn a_file_changed_while_away_does_not_take_the_pages_history_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        create_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        fs::write(dir.join("page.md"), "two").unwrap();
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        // The agent's shell puts another page.md there.
        fs::remove_file(dir.join("page.md")).unwrap();
        fs::write(dir.join("page.html"), "# agent notes").unwrap();
        assert!(rename_store(dir, "page.md", "page.html").unwrap());
        let v = ensure_current_version(dir, "page.html", "# agent notes", NewVersion::default())
            .unwrap();
        assert_eq!(v.version, 1, "the old page's history came back");
        assert_eq!(list_versions(dir, "page.html").unwrap().len(), 1);
    }

    /// A write only follows a page renamed away: other files get no
    /// history from it.
    #[test]
    fn a_write_to_a_file_with_no_history_away_records_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        note_write(dir, "style.css", None, b"a{}").unwrap();
        note_write(dir, "page.html", None, b"<p>").unwrap();
        assert!(!dir.join(VERSION_ROOT).exists());
        // A deleted page's history taken along stays closed.
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        mark_deleted(dir, "page.html").unwrap();
        fs::write(dir.join("page.md"), "one").unwrap();
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        note_write(dir, "page.md", Some(b"one"), b"one").unwrap();
        assert!(rename_store(dir, "page.md", "page.html").unwrap());
        let v = ensure_current_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1);
    }

    /// A write follows the page only when what it replaced was the page:
    /// a file put there outside the app (or one never read) closes the
    /// history, even if the app then writes the page's own bytes back.
    #[test]
    fn a_write_over_another_file_closes_the_history_away() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let len = |name: &str| list_versions(dir, name).unwrap().len();
        // Replaced outside the app, then the page's bytes written back.
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        create_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        fs::write(dir.join("page.md"), "two").unwrap();
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        note_write(dir, "page.md", Some(b"# agent"), b"two").unwrap();
        fs::rename(dir.join("page.md"), dir.join("page.html")).unwrap();
        assert!(rename_store(dir, "page.md", "page.html").unwrap());
        ensure_current_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        assert_eq!(len("page.html"), 1, "the history came back");
        // Unread as it left (no digest), then written.
        create_version(dir, "b.html", "one", NewVersion::default()).unwrap();
        create_version(dir, "b.html", "two", NewVersion::default()).unwrap();
        assert!(rename_store(dir, "b.html", "b.md").unwrap());
        note_write(dir, "b.md", None, b"x").unwrap();
        fs::write(dir.join("b.html"), "x").unwrap();
        assert!(rename_store(dir, "b.md", "b.html").unwrap());
        ensure_current_version(dir, "b.html", "x", NewVersion::default()).unwrap();
        assert_eq!(len("b.html"), 1, "a page never read took its history back");
    }

    /// A manifest the agent made a link is replaced, never written
    /// through.
    #[cfg(unix)]
    #[test]
    fn a_manifest_that_is_a_link_is_replaced_not_written_through() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        fs::create_dir(&dir).unwrap();
        create_version(&dir, "a.html", "x", NewVersion::default()).unwrap();
        let outside = tmp.path().join("outside.txt");
        fs::write(&outside, "mine").unwrap();
        let manifest = history_dir(&dir, "a.html").join(MANIFEST);
        fs::remove_file(&manifest).unwrap();
        std::os::unix::fs::symlink(&outside, &manifest).unwrap();
        let _ = create_version(&dir, "a.html", "y", NewVersion::default());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "mine");
    }

    /// No file could be read as the page left (nor as it came back): not
    /// known to be the page, so its history stays closed.
    #[test]
    fn a_page_unread_as_it_left_does_not_take_its_history_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        assert!(rename_store(dir, "page.md", "page.html").unwrap());
        let v = ensure_current_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1);
    }

    /// The undo of a rename back (its history moves first, the file is
    /// still at the name it leaves) finds the file there.
    #[test]
    fn undoing_a_rename_back_keeps_the_history_for_the_same_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        create_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        fs::write(dir.join("page.html"), "two").unwrap();
        // Forward and undone before the file moved.
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        assert!(rename_store(dir, "page.md", "page.html").unwrap());
        assert!(matches!(
            resolve(dir, "page.html").unwrap().1,
            History::Open(_)
        ));
        let v = ensure_current_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        assert_eq!(v.version, 2, "the same version, not a new history");
        assert_eq!(list_versions(dir, "page.html").unwrap().len(), 2);
    }

    #[test]
    fn a_deleted_pages_history_stays_closed_through_a_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        create_version(dir, "page.html", "two", NewVersion::default()).unwrap();
        mark_deleted(dir, "page.html").unwrap();
        // Another page.html (the agent's), renamed away and back.
        assert!(rename_store(dir, "page.html", "page.md").unwrap());
        assert!(rename_store(dir, "page.md", "page.html").unwrap());
        let v = ensure_current_version(dir, "page.html", "new", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1, "the deleted page's history came back");
        assert_eq!(list_versions(dir, "page.html").unwrap().len(), 1);
    }

    #[test]
    fn a_history_taken_along_by_a_file_that_is_not_a_page_goes_with_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "page.html", "one", NewVersion::default()).unwrap();
        rename_store(dir, "page.html", "page.md").unwrap();
        mark_deleted(dir, "page.md").unwrap();
        // Another page.md, renamed to a page, starts its own history.
        assert!(!rename_store(dir, "page.md", "page.html").unwrap());
        let v = ensure_current_version(dir, "page.html", "new", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1);
    }

    #[test]
    fn a_page_with_no_history_renamed_onto_one_left_behind_does_not_take_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // b.html's history, its page gone outside the app; a.html has none.
        create_version(dir, "b.html", "old", NewVersion::default()).unwrap();
        assert!(!rename_store(dir, "a.html", "b.html").unwrap());
        let v = ensure_current_version(dir, "b.html", "new", NewVersion::default()).unwrap();
        assert_eq!(v.version, 1, "not b's old history's version 2");
    }

    #[cfg(unix)]
    #[test]
    fn a_history_that_cannot_be_relabelled_stays_where_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "a.html", "x", NewVersion::default()).unwrap();
        // Its folder read-only: the manifest (written through a temp file
        // and a rename) cannot be replaced; the folder itself still moves.
        let old = history_dir(dir, "a.html");
        fs::set_permissions(&old, fs::Permissions::from_mode(0o555)).unwrap();
        let res = rename_store(dir, "a.html", "b.html");
        fs::set_permissions(
            history_dir(dir, "a.html"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(res.is_err());
        assert_eq!(list_versions(dir, "a.html").unwrap().len(), 1);
        assert!(list_versions(dir, "b.html").unwrap().is_empty());
    }

    /// Closing a history whose folder is a link: nothing is read through
    /// it, so there is nothing to close, and nothing is written there.
    #[cfg(unix)]
    #[test]
    fn closing_a_linked_history_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join(VERSION_ROOT)).unwrap();
        for name in ["a.html", "a.txt"] {
            mark_deleted(&dir, name).unwrap();
        }
        assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
    }

    #[test]
    fn rename_moves_history() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        create_version(dir, "a.html", "x", NewVersion::default()).unwrap();
        rename_store(dir, "a.html", "b.html").unwrap();
        assert!(list_versions(dir, "a.html").unwrap().is_empty());
        let moved = list_versions(dir, "b.html").unwrap();
        assert_eq!(moved[0].file_name, "b.html");
    }
}
