//! In-place edits of an HTML file (preview inspect and text edits, spec
//! 1.2): one read-modify-write under the version lock, recorded as a
//! manual version.

use crate::error::{Result, StoreError};
use crate::files;
use crate::paths::validate_project_path;
use crate::store::{Store, manual, utf8};
use crate::versions::{self, NewVersion};
use od_contracts::{FileVersion, ProjectFile, PromptSource, VersionSource};

impl Store {
    /// Rewrites `name` with `edit(current text)` and records the result as
    /// a manual version labelled `label`. The text before the edit is kept
    /// in the history first, when it is not the current version already.
    ///
    /// # Errors
    /// Project, path and I/O errors; [`StoreError::NotHtml`] (from the
    /// history, before anything is read into `edit`); whatever `edit`
    /// returns.
    pub fn edit_html<E: From<StoreError>>(
        &self,
        project_id: &str,
        name: &str,
        label: &str,
        edit: impl FnOnce(&str) -> Result<String, E>,
    ) -> Result<(ProjectFile, FileVersion), E> {
        self.edit_html_tracked(project_id, name, label, edit)
            .map(|(file, version, _)| (file, version))
    }

    /// [`Self::edit_html`], also returning the version that holds the text
    /// from before the edit (manual edit's undo restores it).
    ///
    /// # Errors
    /// As [`Self::edit_html`].
    pub fn edit_html_tracked<E: From<StoreError>>(
        &self,
        project_id: &str,
        name: &str,
        label: &str,
        edit: impl FnOnce(&str) -> Result<String, E>,
    ) -> Result<(ProjectFile, FileVersion, FileVersion), E> {
        let root = self.root_for(project_id)?;
        let safe = validate_project_path(name)?;
        let _guard = self.versions_guard();
        let (_, bytes) = files::read_file(&root, &safe)?;
        let before = utf8(&bytes)?;
        let previous = versions::ensure_current_version(&root.dir, &safe, before, manual())?;
        let after = edit(before)?;
        let file = files::write_file_exact(&root, &safe, after.as_bytes())?;
        let version = versions::create_version(
            &root.dir,
            &file.name,
            &after,
            NewVersion {
                source: Some(VersionSource::Manual),
                prompt_source: Some(PromptSource::Manual),
                label: Some(label.to_owned()),
                ..NewVersion::default()
            },
        )?;
        Ok((file, version, previous))
    }
}
