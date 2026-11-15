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
        let store = Self {
            data,
            conn: Mutex::new(conn),
            versions_lock: Mutex::new(()),
        };
        // Images of drafts a page never released (it crashed) go after a
        // day; the projects opened meanwhile already leave them out.
        if let Err(e) = store.sweep_draft_uploads(crate::drafts::DRAFT_MAX_AGE_MS) {
            tracing::warn!(error = %e, "cannot sweep old draft images");
        }
        Ok(store)
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
        // The agent can make the folder a link (to one outside the
        // project): nothing is written there. SQLite stays the source of
        // truth.
        if crate::paths::through_link(
            &root.dir,
            PROJECT_MANIFEST_PATH
                .rsplit_once('/')
                .map_or(PROJECT_MANIFEST_PATH, |(dir, _)| dir),
        ) {
            tracing::warn!(project = %project.id, "the project's manifest folder is a link; manifest not written");
            return Ok(());
        }
        let manifest = ProjectManifest::from(project);
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        // Through a temp file and a rename: a link at the manifest is
        // replaced, never written through.
        files::replace_atomic(&path, &bytes).map_err(|e| e.in_folder(&root.dir))
    }

    /// Creates the row, a first conversation, the folder and its manifest.
    ///
    /// `metadata.baseDir` is accepted here (folder import); the HTTP layer
    /// rejects it from clients.
    ///
    /// # Errors
    /// [`StoreError::BadRequest`] for an empty name or unsafe/duplicate id.
    pub fn create_project(&self, req: CreateProjectRequest) -> Result<(Project, String)> {
        self.create_project_by(req, true)
    }

    /// [`Self::create_project`] for an automation's run: neither a setup
    /// step nor counted in Your usage ([`crate::usage::UsageMark::Automated`]).
    ///
    /// # Errors
    /// As [`Self::create_project`].
    pub fn create_automation_project(
        &self,
        req: CreateProjectRequest,
    ) -> Result<(Project, String)> {
        let (project, cid) = self.create_project_by(req, false)?;
        self.mark_usage(&project.id, crate::usage::UsageMark::Automated)?;
        Ok((project, cid))
    }

    /// `by_person`: someone made it, so it can be their first project
    /// (setup step [`od_contracts::SetupStep::Project`]).
    fn create_project_by(
        &self,
        req: CreateProjectRequest,
        by_person: bool,
    ) -> Result<(Project, String)> {
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
            // The media library is made on its own; it is nobody's first project.
            let media = project.metadata.as_ref().and_then(|m| m.intent.as_deref())
                == Some("media-library");
            if by_person && !media {
                crate::setup::insert(&tx, od_contracts::SetupStep::Project, now)?;
            }
            root.ensure_exists()?;
            // The manifest goes first: a folder that cannot be written to
            // (read-only, full disk) rolls the row back instead of leaving
            // a half project (QA-028).
            if let Err(err) = self.write_manifest(&project) {
                if !root.imported {
                    // Only the folder made just above; never a user folder.
                    let _ = fs::remove_dir_all(&root.dir);
                }
                return Err(err);
            }
            tx.commit()?;
        }
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
        // `updated_at` orders Recent projects by the user's last send
        // (`mark_project_sent`): a rename or a metadata change keeps it,
        // and the reply carries the stored value, not the one read above.
        project.updated_at =
            db::update_project(&self.conn(), &project)?.ok_or(StoreError::ProjectNotFound)?;
        self.write_manifest(&project)?;
        Ok(project)
    }

    /// Records that the user sent a turn in project `id` at `at` (epoch
    /// ms), which moves it up Recent projects. Never moves it back: a
    /// queued message sent late keeps the time it was queued.
    ///
    /// # Errors
    /// [`StoreError::ProjectNotFound`] or SQLite errors.
    pub fn mark_project_sent(&self, id: &str, at: i64) -> Result<()> {
        if !db::mark_project_sent(&self.conn(), id, at)? {
            return Err(StoreError::ProjectNotFound);
        }
        Ok(())
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
        // Images of comments not sent yet are no part of the project.
        let drafts = self.unclaimed_draft_uploads(id)?;
        let copy = || -> Result<Vec<String>> {
            let target_root = self.project_root(&project)?;
            let mut copied = Vec::new();
            for file in files::list_files(&source_root)?
                .into_iter()
                .filter(|f| !drafts.contains(&f.name))
            {
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
        match copy().and_then(|copied| {
            self.mark_usage(&project.id, crate::usage::UsageMark::Copied)?;
            Ok(copied)
        }) {
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
        let safe = crate::paths::sanitize_path(name).ok();
        let existed = overwrite
            && safe
                .as_deref()
                .is_some_and(|safe| files::stat_file(&root, safe).is_ok());
        if let Some(safe) = &safe
            && records_version(safe, bytes)
        {
            versions::refuse_linked_history(&root.dir, safe)?;
        }
        let before = match &safe {
            Some(safe) if existed => self.away_before(&root, safe),
            _ => None,
        };
        let file = files::write_file(&root, name, bytes, overwrite, artifact_manifest)?;
        self.forget_draft_row(project_id, &file.name);
        if !existed {
            self.forget_comments_at(&root, project_id, &file.name);
        }
        note_write(&root, &file.name, before.as_deref(), bytes);
        // Like the original app, every HTML save lands in the history.
        snapshot_written(&root, &file.name, bytes);
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
        if let Ok(exact) = validate_project_path(name)
            && records_version(&exact, bytes)
        {
            versions::refuse_linked_history(&root.dir, &exact)?;
        }
        let file = files::create_file_exact(&root, name, bytes)?;
        self.forget_draft_row(project_id, &file.name);
        self.forget_comments_at(&root, project_id, &file.name);
        snapshot_written(&root, &file.name, bytes);
        Ok(file)
    }

    /// # Errors
    /// Project, path, conflict or I/O errors.
    pub fn rename_file(&self, project_id: &str, from: &str, to: &str) -> Result<ProjectFile> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        // A history that cannot follow is refused before the file moves;
        // undoing the move below stays for the errors only moving finds.
        // A file gone is not found, whatever its history (as a delete).
        for name in [validate_project_path(from), crate::paths::sanitize_path(to)]
            .into_iter()
            .flatten()
        {
            versions::refuse_linked_history(&root.dir, &name).map_err(
                |err| match files::stat_file(&root, from) {
                    Err(StoreError::FileNotFound) => StoreError::FileNotFound,
                    _ => err,
                },
            )?;
        }
        let (old_name, file, sidecar_moved) = files::rename_file(&root, from, to)?;
        if let Err(err) =
            self.follow_renames(&root, project_id, &[(old_name.clone(), file.name.clone())])
        {
            // All or nothing: the file back at its old name, where what
            // is recorded by path still is.
            files::put_back(&root, &file.name, &old_name, sidecar_moved);
            return Err(err);
        }
        Ok(file)
    }

    /// # Errors
    /// Project, path or I/O errors.
    pub fn delete_file(&self, project_id: &str, name: &str) -> Result<()> {
        let root = self.root_for(project_id)?;
        let _guard = self.versions_guard();
        // A draft's image deleted (its x, Cancel), or found gone already
        // (the agent, Finder): no longer one to release, and a file put at
        // its path later is not taken for it. The comments on the file go
        // too (QA-100), and comments sent with it as an image lose it. It
        // is no longer the project's entry page, and its history closes:
        // neither goes to the next file at its name.
        let forget = |rel: &str| {
            let forgot = self.with_conn(|conn| {
                crate::drafts::forget(conn, project_id, rel)?;
                crate::comments::forget(conn, project_id, rel)?;
                forget_entry(conn, project_id, rel, false)
            });
            log_unforgotten(rel, forgot);
            close_history(&root, rel);
        };
        match files::delete_file(&root, name) {
            Ok(rel) => {
                forget(&rel);
                Ok(())
            }
            Err(StoreError::FileNotFound) => {
                forget(&validate_project_path(name)?);
                Err(StoreError::FileNotFound)
            }
            Err(e) => Err(e),
        }
    }

    /// What `name` holds before a write, read only when it carries the
    /// history of a page renamed to it ([`versions::note_write`] needs it).
    fn away_before(&self, root: &ProjectRoot, name: &str) -> Option<Vec<u8>> {
        if !versions::renamed_away(&root.dir, name) {
            return None;
        }
        files::read_file(root, name).ok().map(|(_, bytes)| bytes)
    }

    /// A file written at `path` that is not a draft's: a row a draft left
    /// there (its file gone outside the app) is no longer true. Called
    /// after the write, so an error is logged ([`log_unforgotten`]).
    fn forget_draft_row(&self, project_id: &str, path: &str) {
        let forgot = self.with_conn(|conn| Ok(crate::drafts::forget(conn, project_id, path)?));
        log_unforgotten(path, forgot);
    }

    /// A new file at `path` (none was there): comments left there by a
    /// file gone outside the app are not about it (QA-100).
    /// Nor does a history a page renamed to that name (not a page's) took
    /// along ([`versions::rename_store`]). Called after the write, so an
    /// error is logged ([`log_unforgotten`]).
    /// The caller holds [`Self::versions_guard`].
    fn forget_comments_at(&self, root: &ProjectRoot, project_id: &str, path: &str) {
        let forgot = self.with_conn(|conn| Ok(crate::comments::forget(conn, project_id, path)?));
        log_unforgotten(path, forgot);
        if !is_html_name(path) {
            close_history(root, path);
        }
    }

    /// What is recorded by path follows files renamed or moved: the
    /// comments sent on them (their pins and images, QA-099), the images
    /// of drafts, the project's entry file, and the histories. All or
    /// nothing: on an error none moved (the caller puts the files back).
    fn follow_renames(
        &self,
        root: &ProjectRoot,
        project_id: &str,
        pairs: &[(String, String)],
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            crate::comments::renamed(&tx, project_id, pairs)?;
            crate::drafts::renamed(&tx, project_id, pairs)?;
            for (from, to) in pairs {
                tx.execute(
                    "UPDATE projects SET metadata_json = json_set(metadata_json, '$.entryFile', ?3) \
                     WHERE id = ?1 AND json_valid(metadata_json) \
                     AND json_extract(metadata_json, '$.entryFile') = ?2",
                    rusqlite::params![project_id, from, to],
                )?;
            }
            let mut moved: Vec<&(String, String)> = Vec::new();
            let undo = |moved: &[&(String, String)]| {
                for (from, to) in moved.iter().rev() {
                    let _ = versions::rename_store(&root.dir, to, from);
                }
            };
            for pair in pairs {
                match versions::rename_store(&root.dir, &pair.0, &pair.1) {
                    Ok(true) => moved.push(pair),
                    Ok(false) => {}
                    Err(err) => {
                        undo(&moved);
                        return Err(err);
                    }
                }
            }
            if let Err(err) = tx.commit() {
                undo(&moved);
                return Err(err.into());
            }
            Ok(())
        })
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
        let _guard = self.versions_guard();
        files::stat_file(&root, &safe)?;
        let before = self.away_before(&root, &safe);
        let file = files::write_file_exact(&root, &safe, bytes)?;
        self.forget_draft_row(project_id, &file.name);
        note_write(&root, &file.name, before.as_deref(), bytes);
        Ok(file)
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
        // The drafts' images in it went too: their rows go with them, or a
        // file put at one of those paths later would be taken for one. So
        // do the comments on its files, and the images in it from comments.
        let forgot = self.with_conn(|conn| {
            crate::drafts::forget_folder(conn, project_id, &rel)?;
            crate::comments::forget_folder(conn, project_id, &rel)?;
            forget_entry(conn, project_id, &rel, true)
        });
        log_unforgotten(&rel, forgot);
        for name in &held {
            close_history(&root, name);
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
        // The folder of all histories a link: refused before anything
        // moves. A page's own history folder a link is found as its
        // history follows, and the folder moves back. A folder gone is not
        // found, whatever the histories (as a delete).
        let from_rel = validate_project_path(from)?;
        versions::refuse_linked_history(&root.dir, &from_rel).map_err(|err| {
            if fs::symlink_metadata(root.dir.join(&from_rel)).is_ok_and(|m| m.is_dir()) {
                err
            } else {
                StoreError::FileNotFound
            }
        })?;
        let (rel, renamed) = files::move_folder(&root, from, to)?;
        if let Err(err) = self.follow_renames(&root, project_id, &renamed) {
            files::put_back(&root, &rel, &from_rel, false);
            return Err(err);
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
        let _guard = self.versions_guard();
        let uploaded = files::upload_file(&root, dir, original_name, bytes)?;
        // Its name is known only once written: a page whose history would be
        // refused (a linked history folder) goes again, as a write's would
        // not have been written.
        if records_version(&uploaded.name, bytes)
            && let Err(err) = versions::refuse_linked_history(&root.dir, &uploaded.name)
        {
            let _ = files::delete_file(&root, &uploaded.path);
            return Err(err);
        }
        // Not a draft's until the caller records it so.
        self.forget_draft_row(project_id, &uploaded.path);
        self.forget_comments_at(&root, project_id, &uploaded.path);
        snapshot_written(&root, &uploaded.name, bytes);
        Ok(uploaded)
    }

    /// Stores several uploads under `dir`, all or nothing: one that fails
    /// (or, for the draft session `draft`, cannot be recorded as its) and
    /// those already stored are deleted again before the error is
    /// returned. An upload never overwrites, so a kept half would make
    /// Try again add `-1` copies (QA-105). Returns the files kept: with a
    /// `draft` released already, those deleted at once are left out
    /// ([`Self::record_draft_upload`]).
    ///
    /// # Errors
    /// As [`Self::upload_file`] and [`Self::record_draft_upload`].
    pub fn upload_files<N: AsRef<str>, B: AsRef<[u8]>>(
        &self,
        project_id: &str,
        dir: Option<&str>,
        uploads: &[(N, B)],
        draft: Option<&str>,
    ) -> Result<Vec<UploadedFile>> {
        let mut written: Vec<String> = Vec::with_capacity(uploads.len());
        let mut kept = Vec::with_capacity(uploads.len());
        let stored = uploads.iter().try_for_each(|(name, bytes)| {
            let f = self.upload_file(project_id, dir, name.as_ref(), bytes.as_ref())?;
            written.push(f.path.clone());
            match draft {
                Some(session) if !self.record_draft_upload(project_id, session, &f.path)? => {}
                _ => kept.push(f),
            }
            Ok(())
        });
        match stored {
            Ok(()) => Ok(kept),
            Err(err) => {
                for path in &written {
                    match self.delete_file(project_id, path) {
                        Ok(()) | Err(StoreError::FileNotFound) => {}
                        Err(undo) => {
                            tracing::warn!(file = %path, error = %undo, "an upload that failed cannot be undone");
                        }
                    }
                }
                Err(err)
            }
        }
    }

    /// Creates a project ([`Self::create_project`]) and runs `fill` on it
    /// (its id, its first conversation's), all or nothing: when `fill`
    /// fails the project is deleted again, so Try again does not leave a
    /// second, half-made one.
    ///
    /// # Errors
    /// As [`Self::create_project`], or what `fill` returns.
    pub fn create_project_with<T>(
        &self,
        req: CreateProjectRequest,
        fill: impl FnOnce(&Project, &str) -> Result<T>,
    ) -> Result<(Project, String, T)> {
        let (project, conversation_id) = self.create_project(req)?;
        match fill(&project, &conversation_id) {
            Ok(filled) => Ok((project, conversation_id, filled)),
            Err(err) => {
                if let Err(cleanup) = self.delete_project(&project.id) {
                    tracing::warn!(project = %project.id, error = %cleanup, "a project half made cannot be removed");
                }
                Err(err)
            }
        }
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
        if list.is_empty()
            && let Some((_, bytes)) = &working
        {
            let text = utf8(bytes)?;
            versions::ensure_current_version(&root.dir, &safe, text, manual())?;
            list = versions::list_versions(&root.dir, &safe)?;
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
    /// [`StoreError::VersionNotFound`], path or I/O errors;
    /// [`StoreError::BadRequest`] when the file is not UTF-8 (it could not
    /// be kept in the history). On an error the file is as it was.
    pub fn restore_version(
        &self,
        project_id: &str,
        name: &str,
        version_id: &str,
        prompt: Option<String>,
    ) -> Result<(ProjectFile, FileVersion)> {
        self.restore_version_if(project_id, name, version_id, prompt, None)
    }

    /// [`Self::restore_version`], only while the file still reads as the
    /// version `expect` (when given): an undo never overwrites a change
    /// made since (an agent's write, another tab's edit).
    ///
    /// # Errors
    /// [`StoreError::Conflict`] when the file moved on; those of
    /// [`Self::restore_version`].
    pub fn restore_version_if(
        &self,
        project_id: &str,
        name: &str,
        version_id: &str,
        prompt: Option<String>,
        expect: Option<&str>,
    ) -> Result<(ProjectFile, FileVersion)> {
        let root = self.root_for(project_id)?;
        let safe = validate_project_path(name)?;
        let _guard = self.versions_guard();
        let (restored, content) = versions::read_version(&root.dir, &safe, version_id)?;
        if let Some(expect) = expect {
            let (_, expected) = versions::read_version(&root.dir, &safe, expect)?;
            let (_, now) = files::read_file(&root, &safe)?;
            if now != expected.as_bytes() {
                return Err(StoreError::Conflict(
                    "the file changed since this edit; reload to see it".into(),
                ));
            }
        }
        // What the file holds now is kept in the history before it is
        // overwritten (as an edit does): a change made outside the app
        // since the last version is not lost to the restore. A page that is
        // not UTF-8 cannot be kept (a version holds text), so it is not
        // overwritten either.
        let before = match files::read_file(&root, &safe) {
            Ok((_, now)) => {
                versions::ensure_current_version(&root.dir, &safe, utf8(&now)?, manual())?;
                Some(now)
            }
            Err(StoreError::FileNotFound) => None,
            Err(err) => return Err(err),
        };
        // Exact name: the file may carry characters the sanitizer would
        // rewrite (`My Page.html`), and restore must hit that same file.
        let file = files::write_file_exact(&root, &safe, content.as_bytes())?;
        let manual_prompt = prompt.filter(|p| !p.trim().is_empty());
        let prompt_source = if manual_prompt.is_some() {
            PromptSource::Manual
        } else {
            PromptSource::Restore
        };
        let version = record_or_put_back(&root, &file.name, before.as_deref(), || {
            versions::create_version(
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
            )
        })?;
        Ok((file, version))
    }
}

/// Records the version an operation returns after it wrote `name` (a
/// restore, an edit). The caller needs that version, so an error here is
/// not only logged ([`snapshot_written`]): the file goes back to `before`
/// (kept in the history already; `None`: there was no file) and the
/// error is reported, true, since the disk is as it was.
pub(crate) fn record_or_put_back<T>(
    root: &ProjectRoot,
    name: &str,
    before: Option<&[u8]>,
    record: impl FnOnce() -> Result<T>,
) -> Result<T> {
    record().inspect_err(|err| {
        let put_back = match before {
            Some(bytes) => files::write_file_exact(root, name, bytes).map(drop),
            None => files::delete_file(root, name).map(drop),
        };
        if let Err(back) = put_back {
            tracing::warn!(file = name, error = %err, put_back = %back, "a page written cannot be recorded in its history, nor put back");
        }
    })
}

/// Whether writing `bytes` at `name` records a version: every HTML save
/// that is text.
fn records_version(name: &str, bytes: &[u8]) -> bool {
    is_html_name(name) && std::str::from_utf8(bytes).is_ok()
}

/// [`versions::note_write`] after a write that already changed the file:
/// a history that cannot follow is logged, not reported as the write
/// failing.
fn note_write(root: &ProjectRoot, name: &str, before: Option<&[u8]>, after: &[u8]) {
    if let Err(err) = versions::note_write(&root.dir, name, before, after) {
        tracing::warn!(file = name, error = %err, "the history of a page renamed away cannot follow a write");
    }
}

/// What is recorded by path about `path` (a draft's row, comments, the
/// entry file) is forgotten after the disk changed: an error is logged,
/// not reported as the operation failing, since the file is written or
/// gone already and a retry would not do the same (a second upload, a
/// conflict, a 404). A row left over is about a file no longer there; a
/// draft's is kept from deleting another file by its `ident`.
fn log_unforgotten(path: &str, forgot: Result<()>) {
    if let Err(err) = forgot {
        tracing::warn!(file = path, error = %err, "what was recorded at a path changed on disk cannot be forgotten");
    }
}

/// The history of `name` closes after its file went (or a file that is
/// not it came): the disk changed already, so a history that cannot close
/// is logged, not reported as the operation failing.
fn close_history(root: &ProjectRoot, name: &str) {
    if let Err(err) = versions::mark_deleted(&root.dir, name) {
        tracing::warn!(file = name, error = %err, "the history of a file gone cannot close");
    }
}

/// The version every HTML save records ([`versions::ensure_current_version`]),
/// after the write: a linked history folder was refused before it, so what
/// fails here is logged, not reported as the write failing.
fn snapshot_written(root: &ProjectRoot, name: &str, bytes: &[u8]) {
    if is_html_name(name)
        && let Ok(text) = std::str::from_utf8(bytes)
        && let Err(err) = versions::ensure_current_version(&root.dir, name, text, manual())
    {
        tracing::warn!(file = name, error = %err, "a page written cannot be recorded in its history");
    }
}

/// The project's entry file is `path` (or under folder `path`, `folder`),
/// gone: it no longer names one, so the cover falls back
/// ([`files::ProjectRoot::cover_html`]) rather than to the next file put
/// at that name.
fn forget_entry(
    conn: &rusqlite::Connection,
    project_id: &str,
    path: &str,
    folder: bool,
) -> Result<()> {
    let pattern = if folder {
        format!("{}/", path.trim_end_matches('/'))
    } else {
        path.to_owned()
    };
    // `length` of SQLite itself: `substr` counts characters, Rust's `len`
    // bytes (`Ảnh/` is 4 of one, 6 of the other).
    conn.execute(
        "UPDATE projects SET metadata_json = json_remove(metadata_json, '$.entryFile') \
         WHERE id = ?1 AND json_valid(metadata_json) AND CASE WHEN ?3 \
         THEN substr(json_extract(metadata_json, '$.entryFile'), 1, length(?2)) = ?2 \
         ELSE json_extract(metadata_json, '$.entryFile') = ?2 END",
        rusqlite::params![project_id, pattern, folder],
    )?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use od_contracts::CreateProjectRequest;

    /// The undo of a page renamed away, its history closed by a delete
    /// (a page made again outside the app since): the history stays closed.
    #[cfg(unix)]
    #[test]
    fn undoing_a_rename_away_does_not_open_a_deleted_pages_history() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let s = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, _) = s
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let p = project.id.clone();
        for (name, text) in [("a.html", "<p>old</p>"), ("b.html", "<p>b</p>")] {
            s.write_file(&p, name, text.as_bytes(), true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let root = s.project_root(&project).unwrap_or_else(|e| panic!("{e}"));
        s.delete_file(&p, "a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        fs::write(root.dir.join("a.html"), b"<p>agent new</p>").unwrap_or_else(|e| panic!("{e}"));
        let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b"b.html"));
        // Its folder read-only: the manifest (written through a temp file
        // and a rename) cannot be replaced.
        let history = root.dir.join(versions::VERSION_ROOT).join(&digest[..24]);
        let mode = |m| fs::set_permissions(&history, fs::Permissions::from_mode(m));
        mode(0o555).unwrap_or_else(|e| panic!("{e}"));
        let pairs = [
            ("a.html".to_owned(), "a.md".to_owned()),
            ("b.html".to_owned(), "y.html".to_owned()),
        ];
        let res = s.follow_renames(&root, &p, &pairs);
        mode(0o755).unwrap_or_else(|e| panic!("{e}"));
        assert!(res.is_err());
        let v = versions::ensure_current_version(&root.dir, "a.html", "<p>agent new</p>", manual())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(v.version, 1, "the deleted page's history came back");
    }

    /// The second history cannot follow: the first one, moved already,
    /// goes back, so no history sits under a name its file does not have.
    #[cfg(unix)]
    #[test]
    fn histories_moved_before_one_that_cannot_follow_go_back() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let s = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, _) = s
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let p = project.id.clone();
        for name in ["a.html", "b.html"] {
            s.write_file(&p, name, b"<p>x</p>", true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let root = s.project_root(&project).unwrap_or_else(|e| panic!("{e}"));
        // Its folder read-only: the manifest (written through a temp file
        // and a rename) cannot be replaced.
        let history = |name: &str| {
            let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(name.as_bytes()));
            root.dir.join(versions::VERSION_ROOT).join(&digest[..24])
        };
        let mode = |m| fs::set_permissions(history("b.html"), fs::Permissions::from_mode(m));
        mode(0o555).unwrap_or_else(|e| panic!("{e}"));
        let pairs = [
            ("a.html".to_owned(), "x.html".to_owned()),
            ("b.html".to_owned(), "y.html".to_owned()),
        ];
        let res = s.follow_renames(&root, &p, &pairs);
        mode(0o755).unwrap_or_else(|e| panic!("{e}"));
        assert!(res.is_err());
        for (name, len) in [("a.html", 1), ("b.html", 1), ("x.html", 0), ("y.html", 0)] {
            let list = versions::list_versions(&root.dir, name).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(list.len(), len, "{name}");
        }
    }
}
