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

    /// The HTML file to show for the project: `preferred` (its declared
    /// entry) if it exists, else `index.html`, else the first top-level
    /// HTML file by name. Looks at one directory only, so it stays cheap
    /// for a whole project list.
    #[must_use]
    pub fn cover_html(&self, preferred: Option<&str>) -> Option<String> {
        let is_html = |n: &str| {
            let n = n.to_ascii_lowercase();
            n.ends_with(".html") || n.ends_with(".htm")
        };
        let exists = |n: &str| self.resolve(n).is_ok_and(|p| p.is_file());
        if let Some(p) = preferred.filter(|p| is_html(p) && exists(p)) {
            return Some(p.to_owned());
        }
        if exists("index.html") {
            return Some("index.html".to_owned());
        }
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.') && is_html(n))
            .collect();
        names.sort();
        names.into_iter().next()
    }

    fn real_dir(&self) -> PathBuf {
        fs::canonicalize(&self.dir).unwrap_or_else(|_| self.dir.clone())
    }

    /// Makes sure the root exists before a write. A managed folder is
    /// created; an imported (user) folder must already exist, so a write
    /// never recreates e.g. an unplugged drive's path on the system disk.
    ///
    /// # Errors
    /// [`StoreError::ProjectFolderMissing`] for a missing imported folder.
    pub fn ensure_exists(&self) -> Result<()> {
        if self.imported {
            if self.dir.is_dir() {
                Ok(())
            } else {
                Err(StoreError::ProjectFolderMissing)
            }
        } else {
            Ok(fs::create_dir_all(&self.dir)?)
        }
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

/// How [`write_atomic`] puts the finished temp file in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Replace whatever is at the target.
    Overwrite,
    /// Fail with [`StoreError::FileExists`] if the target exists, atomically
    /// (no check-then-write race between concurrent writers).
    CreateNew,
}

/// Writes to a temp file in the target folder, then moves it into place:
/// `rename` to overwrite, `hard_link` (fails if the target exists) to create.
fn write_atomic(target: &Path, bytes: &[u8], place: Place) -> Result<()> {
    let parent = target.parent().ok_or(StoreError::InvalidPath)?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".od-tmp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_data()?;
        match place {
            Place::Overwrite => fs::rename(&tmp, target),
            Place::CreateNew => place_new(&tmp, target, bytes, |from, to| fs::hard_link(from, to)),
        }
    })();
    if place == Place::CreateNew || result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    match result {
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Err(StoreError::FileExists),
        other => Ok(other?),
    }
}

/// Claims `target` without overwriting: hard-links the finished temp file
/// (readers never see a partial file). File systems without hard links
/// (exFAT, FAT, many SMB shares) fall back to `create_new` and writing in
/// place: still never overwrites, but a reader could see a partial file.
fn place_new(
    tmp: &Path,
    target: &Path,
    bytes: &[u8],
    link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match link(tmp, target) {
        Err(err) if err.kind() != std::io::ErrorKind::AlreadyExists => {
            tracing::debug!(error = %err, "hard link unavailable; using create_new");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            file.write_all(bytes)?;
            file.sync_data()
        }
        other => other,
    }
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
    write_named(root, safe_name, bytes, overwrite, artifact_manifest)
}

/// Overwrites the file at exactly `name` (validated, not sanitized). For
/// writing back to a file that already exists under a name the sanitizer
/// would change, e.g. restoring `My Page.html`.
///
/// # Errors
/// Path or I/O errors.
pub fn write_file_exact(root: &ProjectRoot, name: &str, bytes: &[u8]) -> Result<ProjectFile> {
    let exact = validate_project_path(name)?;
    write_named(root, exact, bytes, true, None)
}

fn write_named(
    root: &ProjectRoot,
    rel: String,
    bytes: &[u8],
    overwrite: bool,
    artifact_manifest: Option<&Value>,
) -> Result<ProjectFile> {
    root.ensure_exists()?;
    let target = root.resolve(&rel)?;
    if target.is_dir() {
        return Err(StoreError::BadRequest("path is a directory".to_owned()));
    }
    let place = if overwrite {
        Place::Overwrite
    } else {
        Place::CreateNew
    };
    write_atomic(&target, bytes, place)?;
    if let Some(manifest) = artifact_manifest.filter(|m| m.is_object()) {
        let sidecar = root.resolve(&format!("{rel}{ARTIFACT_SUFFIX}"))?;
        write_atomic(
            &sidecar,
            &serde_json::to_vec_pretty(manifest)?,
            Place::Overwrite,
        )?;
    }
    let meta = fs::metadata(&target)?;
    Ok(describe(&root.real_dir(), rel, &meta))
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

/// Candidate names `name`, `name-1`, `name-2`…
fn candidate_names(safe: &str) -> impl Iterator<Item = String> + '_ {
    let path = Path::new(safe);
    let stem = path
        .file_stem()
        .map_or_else(|| "file".to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (0..10_000).map(move |i| {
        if i == 0 {
            safe.to_owned()
        } else {
            format!("{stem}-{i}{ext}")
        }
    })
}

/// Stores an uploaded file under optional subfolder `dir`, never overwriting.
/// Each candidate name is claimed atomically, so concurrent uploads with the
/// same name all land under distinct names.
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
    root.ensure_exists()?;
    if !rel_dir.is_empty() {
        fs::create_dir_all(root.resolve(&rel_dir)?)?;
    }
    let safe = sanitize_name(original_name);
    for file_name in candidate_names(&safe) {
        let rel = if rel_dir.is_empty() {
            file_name
        } else {
            format!("{rel_dir}/{file_name}")
        };
        let target = root.resolve(&rel)?;
        match write_atomic(&target, bytes, Place::CreateNew) {
            Ok(()) => {
                let meta = fs::metadata(&target)?;
                return Ok(UploadedFile {
                    name: rel.clone(),
                    path: rel,
                    size: meta.len(),
                    mtime: mtime_ms(&meta),
                    original_name: original_name.to_owned(),
                });
            }
            Err(StoreError::FileExists) => {}
            Err(err) => return Err(err),
        }
    }
    Err(StoreError::FileExists)
}

/// Every folder in the project, sorted, skipping the same hidden and
/// generated trees as [`list_files`]. Empty folders are listed too, so a
/// folder made in the files panel shows before anything is put in it.
///
/// # Errors
/// I/O errors other than a missing project folder.
pub fn list_folders(root: &ProjectRoot) -> Result<Vec<String>> {
    fn walk(dir: &Path, rel_dir: &str, out: &mut Vec<String>) -> Result<()> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || is_ignored_dir_name(&name) || !entry.file_type()?.is_dir() {
                continue;
            }
            let rel = if rel_dir.is_empty() {
                name
            } else {
                format!("{rel_dir}/{name}")
            };
            walk(&entry.path(), &rel, out)?;
            out.push(rel);
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&root.dir, "", &mut out)?;
    out.sort();
    Ok(out)
}

/// Creates folder `path` (sanitized like file names) and its parents.
///
/// # Errors
/// Path errors, [`StoreError::FileExists`] when a file holds the name.
pub fn create_folder(root: &ProjectRoot, path: &str) -> Result<String> {
    assert_visible_for_import(path, root.imported)?;
    let rel = sanitize_path(path)?;
    root.ensure_exists()?;
    let full = root.resolve(&rel)?;
    if full.exists() && !full.is_dir() {
        return Err(StoreError::FileExists);
    }
    fs::create_dir_all(&full)?;
    Ok(rel)
}

/// Deletes folder `path` with everything in it. Returns its project path
/// and the files it held, so their histories can be closed.
///
/// # Errors
/// Path errors, [`StoreError::FileNotFound`] when it is not a folder,
/// [`StoreError::ReservedPath`] for hidden folders (app state lives there).
pub fn delete_folder(root: &ProjectRoot, path: &str) -> Result<(String, Vec<String>)> {
    let rel = validate_project_path(path)?;
    if rel.split('/').any(|s| s.starts_with('.')) {
        return Err(StoreError::ReservedPath);
    }
    let full = root.resolve(&rel)?;
    let meta = fs::symlink_metadata(&full).map_err(StoreError::from_file_io)?;
    if !meta.is_dir() {
        return Err(StoreError::FileNotFound);
    }
    let mut held = Vec::new();
    collect(&full, &rel, &mut held)?;
    fs::remove_dir_all(&full)?;
    Ok((rel, held.into_iter().map(|f| f.name).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsupported(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    #[test]
    fn cover_html_prefers_the_entry_then_index_then_first_html() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ProjectRoot {
            dir: tmp.path().to_path_buf(),
            imported: false,
        };
        assert_eq!(root.cover_html(None), None);
        fs::write(tmp.path().join("notes.txt"), b"x").unwrap();
        fs::write(tmp.path().join(".hidden.html"), b"x").unwrap();
        fs::write(tmp.path().join("b.html"), b"x").unwrap();
        fs::write(tmp.path().join("a.htm"), b"x").unwrap();
        assert_eq!(root.cover_html(None).as_deref(), Some("a.htm"));
        fs::write(tmp.path().join("index.html"), b"x").unwrap();
        assert_eq!(root.cover_html(None).as_deref(), Some("index.html"));
        assert_eq!(root.cover_html(Some("b.html")).as_deref(), Some("b.html"));
        // A declared entry that is missing, not HTML or outside the folder
        // falls back.
        assert_eq!(
            root.cover_html(Some("gone.html")).as_deref(),
            Some("index.html")
        );
        assert_eq!(
            root.cover_html(Some("notes.txt")).as_deref(),
            Some("index.html")
        );
        assert_eq!(
            root.cover_html(Some("../x.html")).as_deref(),
            Some("index.html")
        );
    }

    #[test]
    fn create_new_falls_back_when_hard_links_are_unsupported() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join(".od-tmp-x");
        fs::write(&staged, b"data").unwrap();
        let target = tmp.path().join("a.png");
        place_new(&staged, &target, b"data", unsupported).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"data");
        // Still never overwrites.
        let err = place_new(&staged, &target, b"other", unsupported).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"data");
    }

    #[test]
    fn create_new_keeps_already_exists_from_link() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a.png");
        let err = place_new(&target, &target, b"x", |_, _| {
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        })
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(!target.exists(), "no fallback write when the name is taken");
    }

    #[test]
    fn listing_hides_leftover_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".od-tmp-abc"), b"partial").unwrap();
        fs::write(tmp.path().join("index.html"), b"<p>").unwrap();
        let root = ProjectRoot {
            dir: tmp.path().to_path_buf(),
            imported: false,
        };
        let names: Vec<_> = list_files(&root)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["index.html"]);
    }

    fn managed(tmp: &tempfile::TempDir) -> ProjectRoot {
        ProjectRoot {
            dir: tmp.path().to_path_buf(),
            imported: false,
        }
    }

    #[test]
    fn folders_are_listed_even_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let root = managed(&tmp);
        assert_eq!(
            create_folder(&root, "assets/My Icons").unwrap(),
            "assets/My-Icons"
        );
        create_folder(&root, "notes").unwrap();
        fs::create_dir_all(tmp.path().join(".od/versions")).unwrap();
        fs::create_dir_all(tmp.path().join("node_modules/x")).unwrap();
        assert_eq!(
            list_folders(&root).unwrap(),
            ["assets", "assets/My-Icons", "notes"]
        );
        fs::write(tmp.path().join("page.html"), b"<p>").unwrap();
        assert!(matches!(
            create_folder(&root, "page.html"),
            Err(StoreError::FileExists)
        ));
        assert!(create_folder(&root, "../out").is_err());
    }

    #[test]
    fn deleting_a_folder_reports_its_files_and_spares_app_state() {
        let tmp = tempfile::tempdir().unwrap();
        let root = managed(&tmp);
        fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        fs::write(tmp.path().join("a/one.html"), b"1").unwrap();
        fs::write(tmp.path().join("a/b/two.css"), b"2").unwrap();
        let (rel, mut held) = delete_folder(&root, "a").unwrap();
        held.sort();
        assert_eq!(rel, "a");
        assert_eq!(held, ["a/b/two.css", "a/one.html"]);
        assert!(!tmp.path().join("a").exists());

        fs::create_dir_all(tmp.path().join(".od")).unwrap();
        assert!(matches!(
            delete_folder(&root, ".od"),
            Err(StoreError::ReservedPath)
        ));
        assert!(tmp.path().join(".od").exists());
        fs::write(tmp.path().join("f.txt"), b"x").unwrap();
        assert!(matches!(
            delete_folder(&root, "f.txt"),
            Err(StoreError::FileNotFound)
        ));
        assert!(delete_folder(&root, "").is_err());
    }
}
