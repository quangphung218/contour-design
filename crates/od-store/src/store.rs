use crate::data_dir::DataDir;
use crate::db;
use crate::error::{Result, StoreError};
use crate::files::{self, ProjectRoot};
use crate::paths::{is_html_name, is_safe_id, validate_project_path};
use crate::versions::{self, NewVersion};
use od_contracts::{
    CreateProjectRequest, CreateVersionRequest, DuplicateProjectResponse, FileVersion,
    FileVersionsResponse, Project, ProjectFile, ProjectManifest, PromptSource,
    UpdateProjectRequest, UploadedFile, VersionSource,
};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Relative path of the self-describing manifest inside each project.
pub const PROJECT_MANIFEST_PATH: &str = ".open-design/project.json";

const MAX_NAME_CHARS: usize = 160;

/// Project metadata plus files on disk. Thread-safe; every method blocks, so
/// async callers should use `spawn_blocking`.
#[derive(Debug)]
pub struct Store {
    data: DataDir,
    conn: Mutex<Connection>,
    /// Serializes version-history read-modify-write cycles.
    versions_lock: Mutex<()>,
}

impl Store {
    /// Creates the data layout and opens the database.
    ///
    /// # Errors
    /// Filesystem or SQLite errors.
    pub fn open(data: DataDir) -> Result<Self> {
        data.ensure()?;
        let conn = db::open(&data.db_path())?;
        Ok(Self {
            data,
            conn: Mutex::new(conn),
            versions_lock: Mutex::new(()),
        })
    }

    #[must_use]
    pub fn data_dir(&self) -> &DataDir {
        &self.data
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (each
        // statement is atomic), so recovering the guard is safe.
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn versions_guard(&self) -> MutexGuard<'_, ()> {
        self.versions_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Runs `f` with the raw connection. For tests and future crates.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        f(&self.conn())
    }

    /// # Errors
    /// SQLite errors.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        db::list_projects(&self.conn())
    }

    /// [`Self::list_projects`] with each project's cover file
    /// ([`Project::entry_file`]) filled in.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn list_projects_with_entry(&self) -> Result<Vec<Project>> {
        let mut projects = self.list_projects()?;
        for p in &mut projects {
            let preferred = p.metadata.as_ref().and_then(|m| m.entry_file.clone());
            p.entry_file = self
                .project_root(p)
                .ok()
                .and_then(|root| root.cover_html(preferred.as_deref()));
        }
        Ok(projects)
    }

    /// # Errors
    /// [`StoreError::ProjectNotFound`] or SQLite errors.
    pub fn get_project(&self, id: &str) -> Result<Project> {
        if !is_safe_id(id) {
            return Err(StoreError::ProjectNotFound);
        }
        db::get_project(&self.conn(), id)?.ok_or(StoreError::ProjectNotFound)
    }

    /// Where the project's files live: its import folder, or
    /// `<data>/projects/<id>`.
    ///
    /// # Errors
    /// [`StoreError::InvalidId`] for an unsafe id.
    pub fn project_root(&self, project: &Project) -> Result<ProjectRoot> {
        if let Some(base) = project
            .metadata
            .as_ref()
            .and_then(|m| m.base_dir.as_deref())
            .map(Path::new)
            .filter(|p| p.is_absolute())
        {
            return Ok(ProjectRoot {
                dir: base.to_path_buf(),
                imported: true,
            });
        }
        if !is_safe_id(&project.id) {
            return Err(StoreError::InvalidId);
        }
        Ok(ProjectRoot {
            dir: self.data.projects_dir().join(&project.id),
            imported: false,
        })
    }

    fn write_manifest(&self, project: &Project) -> Result<()> {
        let root = self.project_root(project)?;
        if root.imported && !root.dir.is_dir() {
            // Never recreate a user folder (spec 3.2). SQLite stays the source
            // of truth; the manifest is rewritten on the next update.
            tracing::warn!(project = %project.id, "imported project folder is missing; manifest not written");
            return Ok(());
        }
        let path = root.dir.join(PROJECT_MANIFEST_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let manifest = ProjectManifest::from(project);
        fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
        Ok(())
    }

    /// Creates the row, a first conversation, the folder and its manifest.
    ///
    /// `metadata.baseDir` is accepted here (folder import); the HTTP layer
    /// rejects it from clients.
    ///
    /// # Errors
    /// [`StoreError::BadRequest`] for an empty name or unsafe/duplicate id.
    pub fn create_project(&self, req: CreateProjectRequest) -> Result<(Project, String)> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(StoreError::BadRequest("name required".to_owned()));
        }
        let id = match req.id {
            Some(id) if is_safe_id(&id) => id,
            Some(_) => return Err(StoreError::BadRequest("invalid project id".to_owned())),
            None => new_id(),
        };
        let conversation_id = match req.conversation_id {
            Some(cid) if is_safe_id(&cid) => cid,
            Some(_) => return Err(StoreError::BadRequest("invalid conversation id".to_owned())),
            None => new_id(),
        };
        let now = crate::now_ms();
        let project = Project {
            id,
            name: truncate_chars(name, MAX_NAME_CHARS),
            skill_id: non_empty(req.skill_id),
            design_system_id: non_empty(req.design_system_id),
            pending_prompt: non_empty(req.pending_prompt),
            metadata: req.metadata,
            custom_instructions: non_empty(req.custom_instructions),
            created_at: now,
            updated_at: now,
            entry_file: None,
        };
        let root = self.project_root(&project)?;
        {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            if db::get_project(&tx, &project.id)?.is_some() {
                return Err(StoreError::BadRequest(
                    "project id already exists".to_owned(),
                ));
            }
            db::insert_project(&tx, &project)?;
            db::insert_conversation(&tx, &conversation_id, &project.id, now)?;
            root.ensure_exists()?;
            tx.commit()?;
        }
        self.write_manifest(&project)?;
        Ok((project, conversation_id))
    }

    /// # Errors
    /// [`StoreError::ProjectNotFound`], [`StoreError::BadRequest`] for an
    /// empty name or a `baseDir` change.
    pub fn update_project(&self, id: &str, patch: UpdateProjectRequest) -> Result<Project> {
        let mut project = self.get_project(id)?;
        if let Some(name) = patch.name {
            let name = name.trim();
            if name.is_empty() {
                return Err(StoreError::BadRequest("name required".to_owned()));
            }
            project.name = truncate_chars(name, MAX_NAME_CHARS);
        }
        if let Some(v) = patch.skill_id {
            project.skill_id = non_empty(v);
        }
        if let Some(v) = patch.design_system_id {
            project.design_system_id = non_empty(v);
        }
        if let Some(v) = patch.pending_prompt {
            project.pending_prompt = non_empty(v);
        }
        if let Some(v) = patch.custom_instructions {
            project.custom_instructions = non_empty(v);
        }
        if let Some(metadata) = patch.metadata {
            let old_base = project.metadata.as_ref().and_then(|m| m.base_dir.clone());
            let new_base = metadata.as_ref().and_then(|m| m.base_dir.clone());
            if old_base != new_base {
                return Err(StoreError::BadRequest(
                    "baseDir can only be set by folder import".to_owned(),
                ));
            }
            project.metadata = metadata;
        }
        project.updated_at = crate::now_ms().max(project.updated_at);
        db::update_project(&self.conn(), &project)?;
        self.write_manifest(&project)?;
        Ok(project)
    }

    /// Deletes the row and, for managed projects, the folder. Imported
    /// folders belong to the user and are left on disk.
    ///
    /// # Errors
    /// [`StoreError::ProjectNotFound`] or I/O errors.
    pub fn delete_project(&self, id: &str) -> Result<()> {
        let project = self.get_project(id)?;
        let root = self.project_root(&project)?;
        db::delete_project(&self.conn(), id)?;
        if !root.imported {
            match fs::remove_dir_all(&root.dir) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
                _ => {}
            }
        }
        Ok(())
    }

    /// Copies every visible file into a new managed project.
    ///
    /// # Errors
    /// [`StoreError::ProjectNotFound`] or I/O errors; a half-made copy is
    /// removed.
    pub fn duplicate_project(
        &self,
        id: &str,
        name: Option<&str>,
    ) -> Result<DuplicateProjectResponse> {
        let source = self.get_project(id)?;
        let source_root = self.project_root(&source)?;
        let target_name = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map_or_else(|| format!("{} Copy", source.name.trim()), str::to_owned);
        let mut metadata = source.metadata.clone();
        if let Some(m) = metadata.as_mut() {
            m.base_dir = None;
            m.imported_from = None;
        }
        let req = CreateProjectRequest {
            id: None,
            name: target_name,
            skill_id: source.skill_id.clone(),
            design_system_id: source.design_system_id.clone(),
            pending_prompt: None,
            metadata,
            custom_instructions: source.custom_instructions.clone(),
            conversation_id: None,
        };
        let (project, conversation_id) = self.create_project(req)?;
        let copy = || -> Result<Vec<String>> {
            let target_root = self.project_root(&project)?;
            let mut copied = Vec::new();
            for file in files::list_files(&source_root)? {
                let (_, bytes) = files::read_file(&source_root, &file.name)?;
                files::write_file(
                    &target_root,
                    &file.name,
                    &bytes,
                    true,
                    file.artifact_manifest.as_ref(),
                )?;
                copied.push(file.name);
            }
            Ok(copied)
        };
        match copy() {
            Ok(copied_files) => Ok(DuplicateProjectResponse {
                project,
                conversation_id,
                copied_files,
            }),
            Err(err) => {
                let _ = self.delete_project(&project.id);
                Err(err)
            }
        }
    }

    pub(crate) fn root_for(&self, project_id: &str) -> Result<ProjectRoot> {
        self.project_root(&self.get_project(project_id)?)
    }

    /// # Errors
    /// [`StoreError::ProjectNotFound`] or I/O errors.
    pub fn list_files(&self, project_id: &str) -> Result<Vec<ProjectFile>> {
        files::list_files(&self.root_for(project_id)?)
    }

    /// # Errors
    /// Project, path or I/O errors.
    pub fn read_file(&self, project_id: &str, name: &str) -> Result<(ProjectFile, Vec<u8>)> {
        files::read_file(&self.root_for(project_id)?, name)
    }

    /// Resolves a file for streaming without reading it.
    ///
    /// # Errors
    /// Project, path or I/O errors.
    pub fn stat_file(&self, project_id: &str, name: &str) -> Result<(ProjectFile, PathBuf)> {
        files::stat_file(&self.root_for(project_id)?, name)
    }

    /// # Errors
    /// Project, path, conflict or I/O errors.
    pub fn write_file(
        &self,
        project_id: &str,
        name: &str,
        bytes: &[u8],
        overwrite: bool,
        artifact_manifest: Option<&Value>,
    ) -> Result<ProjectFile> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let file = files::write_file(&root, name, bytes, overwrite, artifact_manifest)?;
        // Like the original app, every HTML save lands in the history.
        if is_html_name(&file.name) {
            if let Ok(text) = std::str::from_utf8(bytes) {
                versions::ensure_current_version(&root.dir, &file.name, text, manual())?;
            }
        }
        Ok(file)
    }

    /// A new file at exactly `name` (see [`files::create_file_exact`]);
    /// HTML gets a baseline version like [`Store::write_file`].
    ///
    /// # Errors
    /// Project, path, conflict or I/O errors.
    pub fn create_file_exact(
        &self,
        project_id: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<ProjectFile> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let file = files::create_file_exact(&root, name, bytes)?;
        if is_html_name(&file.name) {
            if let Ok(text) = std::str::from_utf8(bytes) {
                versions::ensure_current_version(&root.dir, &file.name, text, manual())?;
            }
        }
        Ok(file)
    }

    /// # Errors
    /// Project, path, conflict or I/O errors.
    pub fn rename_file(&self, project_id: &str, from: &str, to: &str) -> Result<ProjectFile> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let (old_name, file) = files::rename_file(&root, from, to)?;
        versions::rename_store(&root.dir, &old_name, &file.name)?;
        Ok(file)
    }

    /// # Errors
    /// Project, path or I/O errors.
    pub fn delete_file(&self, project_id: &str, name: &str) -> Result<()> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let rel = files::delete_file(&root, name)?;
        versions::mark_deleted(&root.dir, &rel)
    }

    /// Overwrites the existing file at exactly `name` (never renamed by the
    /// sanitizer, so `My Deck.html` stays itself). An HTML file gets a
    /// manual version labelled `label`.
    ///
    /// # Errors
    /// Path errors, [`StoreError::FileNotFound`] when there is no such file
    /// (new files go through [`Store::write_file`]), non-UTF-8 HTML.
    pub fn replace_file(
        &self,
        project_id: &str,
        name: &str,
        bytes: &[u8],
        label: &str,
    ) -> Result<ProjectFile> {
        let safe = validate_project_path(name)?;
        if is_html_name(&safe) {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| StoreError::BadRequest("HTML must be UTF-8".to_owned()))?
                .to_owned();
            return self
                .edit_html::<StoreError>(project_id, &safe, label, |_| Ok(text))
                .map(|(file, _)| file);
        }
        let root = self.root_for(project_id)?;
        files::stat_file(&root, &safe)?;
        files::write_file_exact(&root, &safe, bytes)
    }

    /// # Errors
    /// [`StoreError::ProjectNotFound`] or I/O errors.
    pub fn list_folders(&self, project_id: &str) -> Result<Vec<String>> {
        files::list_folders(&self.root_for(project_id)?)
    }

    /// # Errors
    /// Project, path, conflict or I/O errors.
    pub fn create_folder(&self, project_id: &str, path: &str) -> Result<String> {
        files::create_folder(&self.root_for(project_id)?, path)
    }

    /// Deletes a folder and closes the histories of the HTML files in it.
    ///
    /// # Errors
    /// Project, path or I/O errors.
    pub fn delete_folder(&self, project_id: &str, path: &str) -> Result<String> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let (rel, held) = files::delete_folder(&root, path)?;
        for name in held.iter().filter(|n| is_html_name(n)) {
            versions::mark_deleted(&root.dir, name)?;
        }
        Ok(rel)
    }

    /// Moves a folder with everything in it; the histories of its HTML
    /// files follow. Returns the new path and how many listed files moved.
    ///
    /// # Errors
    /// Project, path, conflict or I/O errors.
    pub fn move_folder(&self, project_id: &str, from: &str, to: &str) -> Result<(String, usize)> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let (rel, renamed) = files::move_folder(&root, from, to)?;
        for (old, new) in &renamed {
            versions::rename_store(&root.dir, old, new)?;
        }
        Ok((rel, renamed.len()))
    }

    /// Stores an upload; HTML uploads get a baseline version.
    ///
    /// # Errors
    /// Project, path or I/O errors.
    pub fn upload_file(
        &self,
        project_id: &str,
        dir: Option<&str>,
        original_name: &str,
        bytes: &[u8],
    ) -> Result<UploadedFile> {
        let root = self.root_for(project_id)?;
        let uploaded = files::upload_file(&root, dir, original_name, bytes)?;
        if is_html_name(&uploaded.name) {
            if let Ok(text) = std::str::from_utf8(bytes) {
                let _guard = self.versions_guard();
                versions::ensure_current_version(&root.dir, &uploaded.name, text, manual())?;
            }
        }
        Ok(uploaded)
    }

    /// Lists a file's history, recording the current content as a baseline
    /// when the history is empty.
    ///
    /// # Errors
    /// [`StoreError::NotHtml`], [`StoreError::FileNotFound`] when neither the
    /// file nor a history exists.
    pub fn list_versions(&self, project_id: &str, name: &str) -> Result<FileVersionsResponse> {
        let root = self.root_for(project_id)?;
        let safe = validate_project_path(name)?;
        if !is_html_name(&safe) {
            return Err(StoreError::NotHtml);
        }
        let _guard = self.versions_guard();
        let working = match files::read_file(&root, &safe) {
            Ok(found) => Some(found),
            Err(StoreError::FileNotFound) => None,
            Err(err) => return Err(err),
        };
        let mut list = versions::list_versions(&root.dir, &safe)?;
        if list.is_empty() {
            if let Some((_, bytes)) = &working {
                let text = utf8(bytes)?;
                versions::ensure_current_version(&root.dir, &safe, text, manual())?;
                list = versions::list_versions(&root.dir, &safe)?;
            }
        }
        let file = match working {
            Some((file, _)) => file,
            None => return Err(StoreError::FileNotFound),
        };
        Ok(FileVersionsResponse {
            file,
            versions: list,
        })
    }

    /// Snapshots the file's current bytes.
    ///
    /// # Errors
    /// [`StoreError::NotHtml`], file errors, or non-UTF-8 content.
    pub fn create_version(
        &self,
        project_id: &str,
        name: &str,
        req: CreateVersionRequest,
    ) -> Result<FileVersion> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        let (file, bytes) = files::read_file(&root, name)?;
        if !is_html_name(&file.name) {
            return Err(StoreError::NotHtml);
        }
        let source = req.source.unwrap_or(VersionSource::Manual);
        let prompt_source = match source {
            VersionSource::Restore => Some(PromptSource::Restore),
            VersionSource::Manual => Some(PromptSource::Manual),
            _ if req.prompt.is_some() => Some(PromptSource::Manual),
            _ => None,
        };
        versions::create_version(
            &root.dir,
            &file.name,
            utf8(&bytes)?,
            NewVersion {
                prompt: req.prompt,
                prompt_source,
                source: Some(source),
                label: req.label,
                restore_from_version_id: None,
            },
        )
    }

    /// # Errors
    /// [`StoreError::VersionNotFound`] and path errors.
    pub fn read_version(
        &self,
        project_id: &str,
        name: &str,
        version_id: &str,
    ) -> Result<(FileVersion, String)> {
        let root = self.root_for(project_id)?;
        let safe = validate_project_path(name)?;
        versions::read_version(&root.dir, &safe, version_id)
    }

    /// Writes the snapshot back to the file and records a restore version.
    ///
    /// # Errors
    /// [`StoreError::VersionNotFound`], path or I/O errors.
    pub fn restore_version(
        &self,
        project_id: &str,
        name: &str,
        version_id: &str,
        prompt: Option<String>,
    ) -> Result<(ProjectFile, FileVersion)> {
        let root = self.root_for(project_id)?;
        let safe = validate_project_path(name)?;
        let _guard = self.versions_guard();
        let (restored, content) = versions::read_version(&root.dir, &safe, version_id)?;
        // Exact name: the file may carry characters the sanitizer would
        // rewrite (`My Page.html`), and restore must hit that same file.
        let file = files::write_file_exact(&root, &safe, content.as_bytes())?;
        let manual_prompt = prompt.filter(|p| !p.trim().is_empty());
        let prompt_source = if manual_prompt.is_some() {
            PromptSource::Manual
        } else {
            PromptSource::Restore
        };
        let version = versions::create_version(
            &root.dir,
            &file.name,
            &content,
            NewVersion {
                prompt: manual_prompt.or(restored.prompt),
                prompt_source: Some(prompt_source),
                source: Some(VersionSource::Restore),
                label: None,
                restore_from_version_id: Some(restored.id),
            },
        )?;
        Ok((file, version))
    }
}

pub(crate) fn manual() -> NewVersion {
    NewVersion {
        source: Some(VersionSource::Manual),
        prompt_source: Some(PromptSource::Manual),
        ..NewVersion::default()
    }
}

pub(crate) fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes)
        .map_err(|_| StoreError::BadRequest("HTML file is not valid UTF-8".to_owned()))
}

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
