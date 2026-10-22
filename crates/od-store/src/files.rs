//! File operations inside one project folder. All names go through
//! [`resolve_safe_real`]; listing skips hidden entries, generated trees,
//! symlinks and `.artifact.json` sidecars.

use crate::error::{Result, StoreError};
use crate::paths::{
    assert_visible_for_import, is_ignored_dir_name, kind_for, mime_for, resolve_safe_real,
    sanitize_name, sanitize_path, to_project_path, validate_project_path,
};
use od_contracts::{ProjectFile, UploadedFile};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const ARTIFACT_SUFFIX: &str = ".artifact.json";

/// A project folder plus whether it is a user folder (folder import), which
/// tightens the hidden-segment rule.
#[derive(Debug, Clone)]
pub struct ProjectRoot {
    pub dir: PathBuf,
    pub imported: bool,
}

impl ProjectRoot {
    fn resolve(&self, name: &str) -> Result<PathBuf> {
        assert_visible_for_import(name, self.imported)?;
        resolve_safe_real(&self.dir, name)
    }

    fn real_dir(&self) -> PathBuf {
        fs::canonicalize(&self.dir).unwrap_or_else(|_| self.dir.clone())
    }
}

pub(crate) fn mtime_ms(meta: &fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

fn read_artifact_manifest(root: &Path, rel: &str) -> Option<Value> {
    let raw = fs::read_to_string(root.join(format!("{rel}{ARTIFACT_SUFFIX}"))).ok()?;
    serde_json::from_str::<Value>(&raw)
        .ok()
        .filter(Value::is_object)
}

fn describe(root: &Path, rel: String, meta: &fs::Metadata) -> ProjectFile {
    let manifest = read_artifact_manifest(root, &rel);
    let artifact_kind = manifest
        .as_ref()
        .and_then(|m| m.get("kind"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    ProjectFile {
        kind: kind_for(&rel),
        mime: mime_for(&rel).to_owned(),
        name: rel.clone(),
        path: rel,
        entry_type: "file".to_owned(),
        size: meta.len(),
        mtime: mtime_ms(meta),
        artifact_kind,
        artifact_manifest: manifest,
    }
}

/// Recursive listing, newest first.
///
/// # Errors
/// I/O errors other than a missing project folder.
pub fn list_files(root: &ProjectRoot) -> Result<Vec<ProjectFile>> {
    let mut out = Vec::new();
    collect(&root.dir, "", &mut out)?;
    out.sort_by(|a, b| {
        b.mtime
            .total_cmp(&a.mtime)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

fn collect(dir: &Path, rel_dir: &str, out: &mut Vec<ProjectFile>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let rel = if rel_dir.is_empty() {
            name.clone()
        } else {
            format!("{rel_dir}/{name}")
        };
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !is_ignored_dir_name(&name) {
                collect(&entry.path(), &rel, out)?;
            }
            continue;
        }
        if !file_type.is_file() || name.ends_with(ARTIFACT_SUFFIX) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.into()),
        };
        let project_root = project_root_of(dir, rel_dir);
        out.push(describe(&project_root, rel, &meta));
    }
    Ok(())
}

/// Walks `rel_dir`'s segment count back up from `dir` to the project root.
fn project_root_of(dir: &Path, rel_dir: &str) -> PathBuf {
    let depth = rel_dir.split('/').filter(|s| !s.is_empty()).count();
    let mut root = dir.to_path_buf();
    for _ in 0..depth {
        root.pop();
    }
    root
}

/// Resolves an existing file and describes it without reading its bytes.
///
/// # Errors
/// Path errors, or [`StoreError::FileNotFound`] when it is missing or not a
/// regular file.
pub fn stat_file(root: &ProjectRoot, name: &str) -> Result<(ProjectFile, PathBuf)> {
    let full = root.resolve(name)?;
    let meta = fs::metadata(&full).map_err(StoreError::from_file_io)?;
    if !meta.is_file() {
        return Err(StoreError::FileNotFound);
    }
    let real_root = root.real_dir();
    let rel = to_project_path(&real_root, &full);
    Ok((describe(&real_root, rel, &meta), full))
}

/// # Errors
/// See [`stat_file`].
pub fn read_file(root: &ProjectRoot, name: &str) -> Result<(ProjectFile, Vec<u8>)> {
    let (file, full) = stat_file(root, name)?;
    let bytes = fs::read(&full).map_err(StoreError::from_file_io)?;
    Ok((file, bytes))
}

/// Writes atomically (temp file + rename in the same folder).
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target.parent().ok_or(StoreError::InvalidPath)?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".od-tmp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_data()?;
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

/// Writes `bytes` to a sanitized `name`. With `overwrite == false` an
/// existing file is an error. An object `artifact_manifest` is stored as the
/// `<name>.artifact.json` sidecar.
///
/// # Errors
/// Path errors, [`StoreError::FileExists`], or I/O errors.
pub fn write_file(
    root: &ProjectRoot,
    name: &str,
    bytes: &[u8],
    overwrite: bool,
    artifact_manifest: Option<&Value>,
) -> Result<ProjectFile> {
    assert_visible_for_import(name, root.imported)?;
    let safe_name = sanitize_path(name)?;
    fs::create_dir_all(&root.dir)?;
    let target = root.resolve(&safe_name)?;
    if !overwrite && fs::symlink_metadata(&target).is_ok() {
        return Err(StoreError::FileExists);
    }
    if target.is_dir() {
        return Err(StoreError::BadRequest("path is a directory".to_owned()));
    }
    write_atomic(&target, bytes)?;
    if let Some(manifest) = artifact_manifest.filter(|m| m.is_object()) {
        let sidecar = root.resolve(&format!("{safe_name}{ARTIFACT_SUFFIX}"))?;
        write_atomic(&sidecar, &serde_json::to_vec_pretty(manifest)?)?;
    }
    let meta = fs::metadata(&target)?;
    Ok(describe(&root.real_dir(), safe_name, &meta))
}

/// Deletes a file and its artifact sidecar.
///
/// # Errors
/// Path errors or [`StoreError::FileNotFound`].
pub fn delete_file(root: &ProjectRoot, name: &str) -> Result<String> {
    let full = root.resolve(name)?;
    let meta = fs::symlink_metadata(&full).map_err(StoreError::from_file_io)?;
    if meta.is_dir() {
        return Err(StoreError::BadRequest("path is a directory".to_owned()));
    }
    fs::remove_file(&full).map_err(StoreError::from_file_io)?;
    let rel = to_project_path(&root.real_dir(), &full);
    let _ = fs::remove_file(full.with_file_name(format!(
        "{}{ARTIFACT_SUFFIX}",
        full.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    )));
    Ok(rel)
}

/// Renames `from` to a sanitized `to`, moving the artifact sidecar along.
///
/// # Errors
/// Path errors, [`StoreError::FileNotFound`] for a missing source,
/// [`StoreError::FileExists`] when the target exists.
pub fn rename_file(root: &ProjectRoot, from: &str, to: &str) -> Result<(String, ProjectFile)> {
    assert_visible_for_import(to, root.imported)?;
    let old_name = validate_project_path(from)?;
    let new_name = sanitize_path(to)?;
    let source = root.resolve(&old_name)?;
    let source_meta = fs::metadata(&source).map_err(StoreError::from_file_io)?;
    if !source_meta.is_file() {
        return Err(StoreError::FileNotFound);
    }
    let target = root.resolve(&new_name)?;
    if source == target {
        return Ok((old_name, describe(&root.real_dir(), new_name, &source_meta)));
    }
    if fs::symlink_metadata(&target).is_ok() {
        return Err(StoreError::FileExists);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&source, &target)?;
    let old_sidecar = root.resolve(&format!("{old_name}{ARTIFACT_SUFFIX}"))?;
    if old_sidecar.is_file() {
        let new_sidecar = root.resolve(&format!("{new_name}{ARTIFACT_SUFFIX}"))?;
        fs::rename(old_sidecar, new_sidecar)?;
    }
    let meta = fs::metadata(&target)?;
    Ok((old_name, describe(&root.real_dir(), new_name, &meta)))
}

/// Picks `name`, `name-1`, `name-2`… so an upload never clobbers a file.
fn unique_name(dir: &Path, safe: &str) -> String {
    let path = Path::new(safe);
    let stem = path
        .file_stem()
        .map_or_else(|| "file".to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (0..10_000)
        .map(|i| {
            if i == 0 {
                safe.to_owned()
            } else {
                format!("{stem}-{i}{ext}")
            }
        })
        .find(|candidate| fs::symlink_metadata(dir.join(candidate)).is_err())
        .unwrap_or_else(|| format!("{stem}-{}{ext}", crate::now_ms()))
}

/// Stores an uploaded file under optional subfolder `dir`, never overwriting.
///
/// # Errors
/// Path or I/O errors.
pub fn upload_file(
    root: &ProjectRoot,
    dir: Option<&str>,
    original_name: &str,
    bytes: &[u8],
) -> Result<UploadedFile> {
    let rel_dir = match dir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => {
            assert_visible_for_import(d, root.imported)?;
            sanitize_path(d)?
        }
        None => String::new(),
    };
    fs::create_dir_all(&root.dir)?;
    let base = if rel_dir.is_empty() {
        root.real_dir()
    } else {
        let resolved = root.resolve(&rel_dir)?;
        fs::create_dir_all(&resolved)?;
        resolved
    };
    let file_name = unique_name(&base, &sanitize_name(original_name));
    let rel = if rel_dir.is_empty() {
        file_name
    } else {
        format!("{rel_dir}/{file_name}")
    };
    let target = root.resolve(&rel)?;
    write_atomic(&target, bytes)?;
    let meta = fs::metadata(&target)?;
    Ok(UploadedFile {
        name: rel.clone(),
        path: rel,
        size: meta.len(),
        mtime: mtime_ms(&meta),
        original_name: original_name.to_owned(),
    })
}
