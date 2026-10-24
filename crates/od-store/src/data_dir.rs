use std::path::{Path, PathBuf};

/// Environment variable naming the data root.
pub const DATA_DIR_ENV: &str = "OD_DATA_DIR";

/// Root of all daemon data. Every data path is derived from it, resolved once
/// at startup (spec 3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `OD_DATA_DIR`, else `<platform data dir>/contour` ([`DATA_DIR_NAME`]),
    /// else `./.od`. The default root first takes over the data of the
    /// pre-rename folder ([`migrate_legacy_root`]); `OD_DATA_DIR` is used as is.
    #[must_use]
    pub fn from_env() -> Self {
        Self::resolve(std::env::var_os(DATA_DIR_ENV), default_root())
    }

    /// [`Self::from_env`] with the `OD_DATA_DIR` value and the platform
    /// default passed in.
    fn resolve(data_dir: Option<std::ffi::OsString>, default: PathBuf) -> Self {
        if let Some(dir) = data_dir.filter(|v| !v.is_empty()) {
            return Self::new(dir);
        }
        let legacy = default.with_file_name(LEGACY_DATA_DIR_NAME);
        Self::new(migrate_legacy_root(&default, &legacy))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        self.root.join("app.sqlite")
    }

    #[must_use]
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// `app-config.json`: user settings such as `agent_permission_mode`.
    #[must_use]
    pub fn app_config_path(&self) -> PathBuf {
        self.root.join("app-config.json")
    }

    /// One top-level value of `app-config.json`; `None` when the file or
    /// key is missing or unreadable.
    #[must_use]
    pub fn app_setting(&self, key: &str) -> Option<serde_json::Value> {
        let bytes = std::fs::read(self.app_config_path()).ok()?;
        let mut config: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        config.get_mut(key).map(serde_json::Value::take)
    }

    #[must_use]
    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    /// User-installed skills; shadow bundled skills with the same id.
    #[must_use]
    pub fn user_skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    #[must_use]
    pub fn user_design_templates_dir(&self) -> PathBuf {
        self.root.join("design-templates")
    }

    #[must_use]
    pub fn user_design_systems_dir(&self) -> PathBuf {
        self.root.join("design-systems")
    }

    /// Creates the root and the directories the daemon writes into.
    ///
    /// # Errors
    /// Returns the first filesystem error.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.root.clone(), self.projects_dir(), self.runs_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Folder name of the default data root, under the platform data dir.
pub const DATA_DIR_NAME: &str = "contour";

/// The data root's name before the app became Contour (docs/decisions.md §5).
pub const LEGACY_DATA_DIR_NAME: &str = "open-design-rs";

fn default_root() -> PathBuf {
    default_root_for(
        std::env::consts::OS,
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
    )
}

fn default_root_for(
    os: &str,
    home: Option<PathBuf>,
    xdg_data: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> PathBuf {
    match (os, home, appdata) {
        ("macos", Some(home), _) => home.join("Library/Application Support").join(DATA_DIR_NAME),
        ("windows", _, Some(appdata)) => appdata.join(DATA_DIR_NAME),
        (_, Some(home), _) => xdg_data
            .unwrap_or_else(|| home.join(".local/share"))
            .join(DATA_DIR_NAME),
        (_, None, _) => PathBuf::from(".od"),
    }
}

/// Moves the pre-rename data folder `legacy` to `root` when `root` does not
/// exist yet, and returns the folder to use.
///
/// The move is a rename within the same parent, so projects, runs and
/// settings keep their relative layout (the store derives every path from
/// the root). On Unix `legacy` becomes a symlink to `root`, so an older build
/// or a daemon that was already running keeps finding its files. When the
/// rename fails, the data stays where it is and `legacy` is used: nothing is
/// copied, nothing is deleted.
#[must_use]
pub fn migrate_legacy_root(root: &Path, legacy: &Path) -> PathBuf {
    if root.symlink_metadata().is_ok() {
        return root.to_path_buf();
    }
    let legacy_is_dir = legacy
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_dir());
    if !legacy_is_dir {
        return root.to_path_buf();
    }
    if let Err(e) = std::fs::rename(legacy, root) {
        tracing::warn!(error = %e, legacy = %legacy.display(), "keeping the data folder from before the rename");
        return legacy.to_path_buf();
    }
    tracing::info!(from = %legacy.display(), to = %root.display(), "moved the data folder to its new name");
    #[cfg(unix)]
    if let Err(e) = std::os::unix::fs::symlink(root, legacy) {
        tracing::warn!(error = %e, legacy = %legacy.display(), "no compatibility link at the old data folder");
    }
    root.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_follows_each_platform() {
        let home = || Some(PathBuf::from("/Users/a"));
        assert_eq!(
            default_root_for("macos", home(), None, None),
            Path::new("/Users/a/Library/Application Support/contour")
        );
        assert_eq!(
            default_root_for("linux", home(), None, None),
            Path::new("/Users/a/.local/share/contour")
        );
        assert_eq!(
            default_root_for("linux", home(), Some("/x".into()), None),
            Path::new("/x/contour")
        );
        assert_eq!(
            default_root_for(
                "windows",
                None,
                None,
                Some(r"C:\Users\a\AppData\Roaming".into())
            ),
            Path::new(r"C:\Users\a\AppData\Roaming").join("contour")
        );
        assert_eq!(
            default_root_for("linux", None, None, None),
            Path::new(".od")
        );
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn first_run_takes_over_the_old_data_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        write(&legacy.join("app.sqlite"), "db");
        write(&legacy.join("projects/p1/index.html"), "<h1>mine</h1>");

        assert_eq!(migrate_legacy_root(&root, &legacy), root);
        assert_eq!(
            std::fs::read_to_string(root.join("projects/p1/index.html")).unwrap(),
            "<h1>mine</h1>"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
            "db"
        );
        // Old builds still find the data through the old name.
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_to_string(legacy.join("app.sqlite")).unwrap(),
            "db"
        );

        // Later runs leave both alone.
        assert_eq!(migrate_legacy_root(&root, &legacy), root);
        assert_eq!(
            std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
            "db"
        );
    }

    #[test]
    fn an_existing_new_folder_wins_and_the_old_one_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        write(&root.join("app.sqlite"), "new");
        write(&legacy.join("app.sqlite"), "old");

        assert_eq!(migrate_legacy_root(&root, &legacy), root);
        assert_eq!(
            std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(legacy.join("app.sqlite")).unwrap(),
            "old"
        );
    }

    #[test]
    fn the_default_root_migrates_and_od_data_dir_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        write(&legacy.join("app.sqlite"), "db");

        let chosen = tmp.path().join("elsewhere");
        let explicit = DataDir::resolve(Some(chosen.clone().into_os_string()), root.clone());
        assert_eq!(explicit.root(), chosen);
        assert!(!root.exists(), "OD_DATA_DIR leaves the old folder alone");

        let empty = DataDir::resolve(Some("".into()), root.clone());
        assert_eq!(empty.root(), root);
        assert_eq!(
            std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
            "db"
        );
    }

    #[test]
    fn no_old_folder_means_nothing_to_move() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        assert_eq!(migrate_legacy_root(&root, &legacy), root);
        assert!(!root.exists() && !legacy.exists());
    }

    #[test]
    fn a_failed_move_keeps_using_the_old_folder() {
        let tmp = tempfile::tempdir().unwrap();
        // The new root's parent does not exist, so the rename fails.
        let root = tmp.path().join("missing-parent/contour");
        let legacy = tmp.path().join("open-design-rs");
        write(&legacy.join("app.sqlite"), "db");
        assert_eq!(migrate_legacy_root(&root, &legacy), legacy);
        assert_eq!(
            std::fs::read_to_string(legacy.join("app.sqlite")).unwrap(),
            "db"
        );
    }
}
