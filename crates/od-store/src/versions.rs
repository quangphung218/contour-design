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

fn store_dir(project_dir: &Path, file_name: &str) -> PathBuf {
    let digest = hex::encode(Sha256::digest(file_name.as_bytes()));
    project_dir.join(VERSION_ROOT).join(&digest[..24])
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
            } else if let Some(id) = &manifest.current_version_id {
                if !manifest.entries.iter().any(|e| &e.id == id) {
                    manifest.current_version_id = None;
                }
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
    fs::write(dir.join(MANIFEST), serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

fn require_html(file_name: &str) -> Result<()> {
    if is_html_name(file_name) {
        Ok(())
    } else {
        Err(StoreError::NotHtml)
    }
}

/// # Errors
/// [`StoreError::NotHtml`] or I/O and JSON errors.
pub fn list_versions(project_dir: &Path, file_name: &str) -> Result<Vec<FileVersion>> {
    require_html(file_name)?;
    let manifest = read_manifest(&store_dir(project_dir, file_name))?;
    let current = manifest.current_version_id.as_deref();
    Ok(manifest.entries.iter().map(|e| e.public(current)).collect())
}

/// Returns the version and its snapshot text.
///
/// # Errors
/// [`StoreError::VersionNotFound`] for an unknown id or missing snapshot.
pub fn read_version(
    project_dir: &Path,
    file_name: &str,
    version_id: &str,
) -> Result<(FileVersion, String)> {
    require_html(file_name)?;
    if !is_version_id(version_id) {
        return Err(StoreError::BadRequest("version id required".to_owned()));
    }
    let dir = store_dir(project_dir, file_name);
    let manifest = read_manifest(&dir)?;
    let entry = manifest
        .entries
        .iter()
        .find(|e| e.id == version_id)
        .ok_or(StoreError::VersionNotFound)?;
    let content = fs::read_to_string(dir.join(safe_content_path(entry))).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            StoreError::VersionNotFound
        } else {
            err.into()
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
/// A history marked deleted (the file was removed) is wiped first, unless
/// this is a restore of one of its own entries.
///
/// # Errors
/// [`StoreError::NotHtml`] or I/O and JSON errors.
pub fn create_version(
    project_dir: &Path,
    file_name: &str,
    content: &str,
    opts: NewVersion,
) -> Result<FileVersion> {
    require_html(file_name)?;
    let dir = store_dir(project_dir, file_name);
    let mut manifest = read_manifest(&dir)?;
    let restoring_own = opts
        .restore_from_version_id
        .as_deref()
        .is_some_and(|id| manifest.entries.iter().any(|e| e.id == id));
    if manifest.deleted_at.is_some() && !restoring_own {
        let _ = fs::remove_dir_all(&dir);
        manifest = Manifest::default();
    }
    manifest.deleted_at = None;
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

/// Creates a baseline version when the history is empty (or deleted).
/// Returns the current version otherwise.
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
    let dir = store_dir(project_dir, file_name);
    let manifest = read_manifest(&dir)?;
    if manifest.deleted_at.is_none() {
        if let Some(current) = manifest
            .current_version_id
            .as_deref()
            .and_then(|id| manifest.entries.iter().find(|e| e.id == id))
        {
            if fs::read_to_string(dir.join(safe_content_path(current)))
                .ok()
                .as_deref()
                == Some(content)
            {
                return Ok(current.public(Some(&current.id)));
            }
        }
    }
    create_version(project_dir, file_name, content, opts)
}

/// Marks a file's history deleted; the next save starts a fresh history.
///
/// # Errors
/// I/O and JSON errors.
pub fn mark_deleted(project_dir: &Path, file_name: &str) -> Result<()> {
    if !is_html_name(file_name) {
        return Ok(());
    }
    let dir = store_dir(project_dir, file_name);
    let mut manifest = read_manifest(&dir)?;
    if manifest.entries.is_empty() {
        return Ok(());
    }
    manifest.deleted_at = Some(crate::now_ms());
    write_manifest(&dir, file_name, &mut manifest)
}

/// Moves a history along with a renamed HTML file. An existing history at
/// the target is replaced.
///
/// # Errors
/// I/O and JSON errors.
pub fn rename_store(project_dir: &Path, from: &str, to: &str) -> Result<()> {
    if from == to || !is_html_name(from) || !is_html_name(to) {
        return Ok(());
    }
    let old_dir = store_dir(project_dir, from);
    if !old_dir.is_dir() {
        return Ok(());
    }
    let new_dir = store_dir(project_dir, to);
    if new_dir.exists() {
        fs::remove_dir_all(&new_dir)?;
    }
    fs::rename(&old_dir, &new_dir)?;
    let mut manifest = read_manifest(&new_dir)?;
    for entry in &mut manifest.entries {
        to.clone_into(&mut entry.file_name);
    }
    write_manifest(&new_dir, to, &mut manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

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
