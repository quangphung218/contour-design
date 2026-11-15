//! File operations inside one project folder. All names go through
//! [`resolve_safe_real`]; listing skips hidden entries, generated trees,
//! symlinks and `.artifact.json` sidecars. Reads follow a symlink; nothing
//! is written, deleted or moved through one (see `through_link`), so the
//! file or folder it points to is never changed under another name.

use crate::error::{Result, StoreError};
use crate::paths::{
    assert_visible_for_import, is_ignored_dir_name, kind_for, mime_for, resolve_safe_real,
    sanitize_name, sanitize_path, through_link, to_project_path, validate_project_path,
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
/// Path errors, [`StoreError::FileNotFound`] when it is missing or not a
/// regular file, [`StoreError::NoReadPermission`] when the OS refuses.
pub fn stat_file(root: &ProjectRoot, name: &str) -> Result<(ProjectFile, PathBuf)> {
    let full = root.resolve(name)?;
    let meta = fs::metadata(&full).map_err(StoreError::from_read_io)?;
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
    let bytes = fs::read(&full).map_err(StoreError::from_read_io)?;
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

/// Replaces `target` with `bytes` through a temp file and a rename: a
/// reader never sees a partial file, and a link at `target` is replaced
/// (the rename acts on the name), never written through.
///
/// # Errors
/// I/O errors.
pub(crate) fn replace_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic(target, bytes, Place::Overwrite)
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

/// Creates `name` at exactly that path (validated, not sanitized), never
/// overwriting: an import keeps the names its pages link to.
///
/// # Errors
/// Path or I/O errors, [`StoreError::FileExists`] when the path is taken.
pub fn create_file_exact(root: &ProjectRoot, name: &str, bytes: &[u8]) -> Result<ProjectFile> {
    assert_visible_for_import(name, root.imported)?;
    let exact = validate_project_path(name)?;
    write_named(root, exact, bytes, false, None)
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
    let manifest = artifact_manifest.filter(|m| m.is_object());
    let sidecar_rel = format!("{rel}{ARTIFACT_SUFFIX}");
    refuse_link(root, &rel)?;
    if manifest.is_some() {
        refuse_link(root, &sidecar_rel)?;
    }
    let place = if overwrite {
        Place::Overwrite
    } else {
        Place::CreateNew
    };
    write_atomic(&target, bytes, place)?;
    if let Some(manifest) = manifest {
        let sidecar = root.resolve(&sidecar_rel)?;
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
    // A link, or a file in a linked folder, is not listed: deleting it
    // would delete the file it points to.
    if through_link(&root.dir, &validate_project_path(name)?) {
        return Err(StoreError::FileNotFound);
    }
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
/// Returns the old name, the file, and whether a sidecar moved (what
/// [`put_back`] must move back).
///
/// # Errors
/// Path errors, [`StoreError::FileNotFound`] for a missing source or one
/// through a link, [`StoreError::FileExists`] when the target exists,
/// [`StoreError::BadRequest`] for a target through a link.
pub fn rename_file(
    root: &ProjectRoot,
    from: &str,
    to: &str,
) -> Result<(String, ProjectFile, bool)> {
    assert_visible_for_import(to, root.imported)?;
    let old_name = validate_project_path(from)?;
    let new_name = sanitize_path(to)?;
    let source = root.resolve(&old_name)?;
    let source_meta = fs::metadata(&source).map_err(StoreError::from_file_io)?;
    if !source_meta.is_file() {
        return Err(StoreError::FileNotFound);
    }
    let mut target = root.resolve(&new_name)?;
    let mut new_name = new_name;
    if source == target {
        if old_name == new_name {
            return Ok((
                old_name,
                describe(&root.real_dir(), new_name, &source_meta),
                false,
            ));
        }
        // One file under both names: a file system that folds case
        // (APFS: `a.html` → `A.html`), or a link. The file takes the new
        // name on disk too, not only in what is recorded by path.
        if fs::symlink_metadata(root.dir.join(&old_name))
            .map_err(StoreError::from_file_io)?
            .is_symlink()
        {
            return Err(StoreError::FileExists);
        }
        target = root.real_dir().join(&new_name);
    } else if fs::symlink_metadata(&target).is_ok() {
        return Err(StoreError::FileExists);
    }
    // A link, or a file in a linked folder, is not listed: renaming it
    // would rename the file it points to, under another name than the
    // one what is recorded by path follows.
    if through_link(&root.dir, &old_name) {
        return Err(StoreError::FileNotFound);
    }
    refuse_link(root, &new_name)?;
    // Not resolved: a sidecar that is a link moves as the link, never the
    // file it points to.
    let old_sidecar = root.real_dir().join(format!("{old_name}{ARTIFACT_SUFFIX}"));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&source, &target)?;
    // The name as stored (a folder's case is the folder's own).
    if let Ok(real) = fs::canonicalize(&target) {
        new_name = to_project_path(&root.real_dir(), &real);
        target = real;
    }
    let mut sidecar_moved = false;
    let mut follow = || -> std::io::Result<fs::Metadata> {
        if fs::symlink_metadata(&old_sidecar).is_ok_and(|m| !m.is_dir()) {
            let new_sidecar = root.real_dir().join(format!("{new_name}{ARTIFACT_SUFFIX}"));
            fs::rename(&old_sidecar, new_sidecar)?;
            sidecar_moved = true;
        }
        fs::metadata(&target)
    };
    match follow() {
        Ok(meta) => Ok((
            old_name,
            describe(&root.real_dir(), new_name, &meta),
            sidecar_moved,
        )),
        Err(err) => {
            // All or nothing: the file back where its sidecar and what is
            // recorded by path still are.
            put_back(root, &new_name, &old_name, sidecar_moved);
            Err(err.into())
        }
    }
}

/// [`StoreError::BadRequest`] when `rel` goes through a link: writing
/// there would change the file or folder it points to.
fn refuse_link(root: &ProjectRoot, rel: &str) -> Result<()> {
    if through_link(&root.dir, rel) {
        return Err(StoreError::BadRequest(
            "this path goes through a link; Contour does not change what a link points to"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Undoes [`rename_file`] or [`move_folder`] (`now` back to `was`) when what
/// follows the rename failed. Best effort: the error being reported is the
/// one that made the undo needed. The sidecar moves back only when the
/// rename moved it (`sidecar_moved`): one found at `now` otherwise is another
/// file's, left by a file gone outside the app.
pub fn put_back(root: &ProjectRoot, now: &str, was: &str, sidecar_moved: bool) {
    let real = root.real_dir();
    let (now_path, was_path) = (real.join(now), real.join(was));
    if fs::rename(&now_path, &was_path).is_err() || !sidecar_moved {
        return;
    }
    let sidecar = |p: &str| real.join(format!("{p}{ARTIFACT_SUFFIX}"));
    if was_path.is_file() && fs::symlink_metadata(sidecar(now)).is_ok_and(|m| !m.is_dir()) {
        let _ = fs::rename(sidecar(now), sidecar(was));
    }
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
        let full = root.resolve(&rel_dir)?;
        refuse_link(root, &rel_dir)?;
        fs::create_dir_all(full)?;
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
    refuse_link(root, &rel)?;
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
    // A link (or a folder in a linked one) is not a listed folder: the
    // folder it points to, and all in it, would go.
    if !meta.is_dir() || through_link(&root.dir, &rel) {
        return Err(StoreError::FileNotFound);
    }
    let mut held = Vec::new();
    collect(&full, &rel, &mut held)?;
    fs::remove_dir_all(&full)?;
    Ok((rel, held.into_iter().map(|f| f.name).collect()))
}

/// Moves folder `from` to exactly `to` with one rename, so everything in
/// it moves: hidden files, ignored trees, empty folders, symlinks (as
/// links). Returns the new project path and the `(old, new)` names of the
/// listed files it held, so their histories can follow.
///
/// # Errors
/// Path errors, [`StoreError::FileNotFound`] when `from` is not a folder
/// (a symlink, or a folder in a linked one, is not one),
/// [`StoreError::ReservedPath`] for hidden folders,
/// [`StoreError::FileExists`] when `to` exists, [`StoreError::BadRequest`]
/// for a move into itself or into a linked folder.
pub fn move_folder(
    root: &ProjectRoot,
    from: &str,
    to: &str,
) -> Result<(String, Vec<(String, String)>)> {
    let from = validate_project_path(from)?;
    let to = validate_project_path(to)?;
    if [&from, &to]
        .iter()
        .any(|p| p.split('/').any(|s| s.starts_with('.')))
    {
        return Err(StoreError::ReservedPath);
    }
    let source = root.resolve(&from)?;
    let meta = fs::symlink_metadata(root.dir.join(&from)).map_err(StoreError::from_file_io)?;
    // A folder in a linked folder is not listed: the folder that would
    // move is not the one named.
    if !meta.is_dir() || through_link(&root.dir, &from) {
        return Err(StoreError::FileNotFound);
    }
    let target = root.resolve(&to)?;
    refuse_link(root, &to)?;
    if target.starts_with(&source) {
        return Err(StoreError::BadRequest(
            "a folder cannot move into itself".to_owned(),
        ));
    }
    if fs::symlink_metadata(&target).is_ok() {
        return Err(StoreError::FileExists);
    }
    let mut held = Vec::new();
    collect(&source, &from, &mut held)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&source, &target)?;
    let renamed = held
        .into_iter()
        .map(|f| {
            let new = format!("{to}{}", &f.name[from.len()..]);
            (f.name, new)
        })
        .collect();
    Ok((to, renamed))
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

    /// A link renamed to the name of the file it points at is not that
    /// file renamed (what is recorded by path must not move to it).
    #[cfg(unix)]
    #[test]
    fn a_link_renamed_onto_its_own_target_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.html"), b"<p>").unwrap();
        std::os::unix::fs::symlink("a.html", tmp.path().join("b.html")).unwrap();
        let root = ProjectRoot {
            dir: tmp.path().to_path_buf(),
            imported: false,
        };
        assert!(matches!(
            rename_file(&root, "b.html", "a.html"),
            Err(StoreError::FileExists)
        ));
        assert!(
            fs::symlink_metadata(tmp.path().join("b.html"))
                .unwrap()
                .is_symlink()
        );
    }

    /// A link (as the agent's shell makes) is not a listed file: renaming
    /// it renames neither it nor the file it points to.
    #[cfg(unix)]
    #[test]
    fn a_link_or_a_file_in_a_linked_folder_is_not_renamed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("a.html"), b"<p>").unwrap();
        std::os::unix::fs::symlink("a.html", dir.join("l.html")).unwrap();
        fs::create_dir(dir.join("real")).unwrap();
        fs::write(dir.join("real/b.html"), b"<p>").unwrap();
        std::os::unix::fs::symlink("real", dir.join("ld")).unwrap();
        let root = ProjectRoot {
            dir: dir.to_path_buf(),
            imported: false,
        };
        assert!(matches!(
            rename_file(&root, "l.html", "m.html"),
            Err(StoreError::FileNotFound)
        ));
        assert!(matches!(
            rename_file(&root, "ld/b.html", "c.html"),
            Err(StoreError::FileNotFound)
        ));
        assert!(dir.join("a.html").is_file() && dir.join("real/b.html").is_file());
        assert!(!dir.join("m.html").exists() && !dir.join("c.html").exists());
        // The file itself still renames.
        rename_file(&root, "a.html", "m.html").unwrap();
        assert!(dir.join("m.html").is_file());
    }

    /// A project with `a.html`, `real/b.html`, the link `l.html` → `a.html`
    /// and the link `ld` → `real` (as the agent's shell makes them).
    #[cfg(unix)]
    fn linked(tmp: &tempfile::TempDir) -> ProjectRoot {
        let dir = tmp.path();
        fs::write(dir.join("a.html"), b"a").unwrap();
        fs::create_dir_all(dir.join("real/sub")).unwrap();
        fs::write(dir.join("real/b.html"), b"b").unwrap();
        fs::write(dir.join("real/sub/c.html"), b"c").unwrap();
        std::os::unix::fs::symlink("a.html", dir.join("l.html")).unwrap();
        std::os::unix::fs::symlink("real", dir.join("ld")).unwrap();
        managed(tmp)
    }

    /// What `linked` made is all still there, the links as links.
    #[cfg(unix)]
    fn untouched(tmp: &tempfile::TempDir) {
        let dir = tmp.path();
        assert_eq!(fs::read(dir.join("a.html")).unwrap(), b"a");
        assert_eq!(fs::read(dir.join("real/b.html")).unwrap(), b"b");
        assert_eq!(fs::read(dir.join("real/sub/c.html")).unwrap(), b"c");
        for link in ["l.html", "ld"] {
            assert!(fs::symlink_metadata(dir.join(link)).unwrap().is_symlink());
        }
        let mut real: Vec<_> = fs::read_dir(dir.join("real"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        real.sort();
        assert_eq!(
            real,
            ["b.html", "sub"],
            "nothing added to the linked folder"
        );
    }

    /// Deleting a link, or a file or folder in a linked folder, never
    /// deletes what it points to.
    #[cfg(unix)]
    #[test]
    fn deleting_through_a_link_spares_what_it_points_to() {
        let tmp = tempfile::tempdir().unwrap();
        let root = linked(&tmp);
        for name in ["l.html", "ld/b.html"] {
            assert!(
                matches!(delete_file(&root, name), Err(StoreError::FileNotFound)),
                "{name}"
            );
        }
        for path in ["ld", "ld/sub"] {
            assert!(
                matches!(delete_folder(&root, path), Err(StoreError::FileNotFound)),
                "{path}"
            );
        }
        untouched(&tmp);
    }

    /// A folder in a linked folder, or a move into one, is refused: the
    /// folder that would move (or land) is not the one named.
    #[cfg(unix)]
    #[test]
    fn moving_a_folder_through_a_link_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = linked(&tmp);
        fs::create_dir(tmp.path().join("pages")).unwrap();
        assert!(matches!(
            move_folder(&root, "ld/sub", "moved"),
            Err(StoreError::FileNotFound)
        ));
        assert!(matches!(
            move_folder(&root, "pages", "ld/pages"),
            Err(StoreError::BadRequest(_))
        ));
        assert!(!tmp.path().join("moved").exists());
        assert!(tmp.path().join("pages").is_dir());
        untouched(&tmp);
    }

    /// Writing at a link, or in a linked folder, is refused rather than
    /// changing the file or folder it points to.
    #[cfg(unix)]
    #[test]
    fn writing_through_a_link_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = linked(&tmp);
        let refused = |res: Result<ProjectFile>, what: &str| {
            assert!(
                matches!(res, Err(StoreError::BadRequest(_))),
                "{what}: {res:?}"
            );
        };
        refused(write_file(&root, "l.html", b"x", true, None), "l.html");
        refused(
            write_file(&root, "ld/b.html", b"x", true, None),
            "ld/b.html",
        );
        refused(
            write_file(&root, "ld/new.html", b"x", false, None),
            "ld/new",
        );
        refused(write_file_exact(&root, "l.html", b"x"), "exact");
        refused(create_file_exact(&root, "ld/new.html", b"x"), "create");
        // The sidecar a link holds is not written through either.
        std::os::unix::fs::symlink("a.html", tmp.path().join("m.html.artifact.json")).unwrap();
        let manifest = serde_json::json!({"kind": "page"});
        refused(
            write_file(&root, "m.html", b"m", true, Some(&manifest)),
            "sidecar",
        );
        assert!(matches!(
            upload_file(&root, Some("ld"), "new.png", b"x"),
            Err(StoreError::BadRequest(_))
        ));
        for path in ["ld", "ld/new"] {
            assert!(
                matches!(create_folder(&root, path), Err(StoreError::BadRequest(_))),
                "{path}"
            );
        }
        assert!(matches!(
            rename_file(&root, "a.html", "ld/a.html"),
            Err(StoreError::BadRequest(_))
        ));
        untouched(&tmp);
    }

    /// A sidecar that is a link moves as the link: the file it points to
    /// stays where it is.
    #[cfg(unix)]
    #[test]
    fn a_sidecar_that_is_a_link_moves_as_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("a.html"), b"<p>").unwrap();
        fs::write(dir.join("notes.json"), b"{}").unwrap();
        std::os::unix::fs::symlink("notes.json", dir.join(format!("a.html{ARTIFACT_SUFFIX}")))
            .unwrap();
        rename_file(&managed(&tmp), "a.html", "b.html").unwrap();
        assert_eq!(fs::read(dir.join("notes.json")).unwrap(), b"{}");
        assert!(
            fs::symlink_metadata(dir.join(format!("b.html{ARTIFACT_SUFFIX}")))
                .unwrap()
                .is_symlink()
        );
    }

    /// Undoing a rename whose sidecar did not move leaves the sidecar at
    /// the new name where it is: it is another file's (one gone outside
    /// the app), not the renamed file's.
    #[test]
    fn putting_back_a_rename_leaves_a_sidecar_it_did_not_move() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("b.html"), b"<p>").unwrap();
        fs::write(dir.join(format!("b.html{ARTIFACT_SUFFIX}")), b"stale").unwrap();
        put_back(&managed(&tmp), "b.html", "a.html", false);
        assert!(dir.join("a.html").is_file());
        assert!(!dir.join(format!("a.html{ARTIFACT_SUFFIX}")).exists());
        assert_eq!(
            fs::read(dir.join(format!("b.html{ARTIFACT_SUFFIX}"))).unwrap(),
            b"stale"
        );
    }

    /// Windows refuses to replace a read-only file (std renames without
    /// `FILE_RENAME_FLAG_IGNORE_READONLY_ATTRIBUTE`): the sidecar cannot
    /// follow, and the stale one at the new name must not replace the
    /// renamed file's own.
    #[cfg(windows)]
    #[test]
    fn a_sidecar_that_cannot_replace_a_read_only_one_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("a.html"), b"<p>").unwrap();
        fs::write(dir.join(format!("a.html{ARTIFACT_SUFFIX}")), b"own").unwrap();
        let stale = dir.join(format!("b.html{ARTIFACT_SUFFIX}"));
        fs::write(&stale, b"stale").unwrap();
        let mut perms = fs::metadata(&stale).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&stale, perms.clone()).unwrap();
        let res = rename_file(&managed(&tmp), "a.html", "b.html");
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(&stale, perms);
        assert!(res.is_err(), "the read-only sidecar was replaced");
        assert_eq!(
            fs::read(dir.join(format!("a.html{ARTIFACT_SUFFIX}"))).unwrap(),
            b"own"
        );
        assert!(dir.join("a.html").is_file());
        assert_eq!(fs::read(&stale).unwrap(), b"stale");
    }

    /// The sidecar cannot take the new name (a folder holds it): the file
    /// goes back to its old name, with its sidecar.
    #[test]
    fn a_rename_whose_sidecar_cannot_follow_puts_the_file_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("a.html"), b"<p>").unwrap();
        fs::write(dir.join(format!("a.html{ARTIFACT_SUFFIX}")), b"{}").unwrap();
        fs::create_dir(dir.join(format!("b.html{ARTIFACT_SUFFIX}"))).unwrap();
        fs::write(dir.join(format!("b.html{ARTIFACT_SUFFIX}/x")), b"").unwrap();
        let root = ProjectRoot {
            dir: dir.to_path_buf(),
            imported: false,
        };
        assert!(rename_file(&root, "a.html", "b.html").is_err());
        assert!(dir.join("a.html").is_file(), "back at its old name");
        assert!(!dir.join("b.html").exists());
        assert!(dir.join(format!("a.html{ARTIFACT_SUFFIX}")).is_file());
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

    /// QA-085: a file the user cannot read is a read refusal, not a write
    /// one. Root reads anything, so the check needs a plain user.
    #[cfg(unix)]
    #[test]
    fn unreadable_file_is_a_read_refusal() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let page = tmp.path().join("index.html");
        fs::write(&page, b"<p>").unwrap();
        fs::set_permissions(&page, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&page).is_ok() {
            return; // running as root
        }
        let err = read_file(&managed(&tmp), "index.html").unwrap_err();
        fs::set_permissions(&page, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(err, StoreError::NoReadPermission), "{err:?}");
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
