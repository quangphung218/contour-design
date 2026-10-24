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
        let platform = Platform::from_env();
        Self::resolve(
            std::env::var_os(DATA_DIR_ENV),
            platform.default_root(),
            platform.legacy_root(),
        )
    }

    /// [`Self::from_env`] with the `OD_DATA_DIR` value, the platform default
    /// and the pre-rename default passed in.
    fn resolve(
        data_dir: Option<std::ffi::OsString>,
        default: PathBuf,
        legacy: Option<PathBuf>,
    ) -> Self {
        if let Some(dir) = data_dir.filter(|v| !v.is_empty()) {
            return Self::new(dir);
        }
        match legacy {
            Some(legacy) => Self::new(migrate_legacy_root(&default, &legacy)),
            None => Self::new(default),
        }
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

/// What the default data roots depend on.
struct Platform {
    os: &'static str,
    home: Option<PathBuf>,
    xdg_data: Option<PathBuf>,
    appdata: Option<PathBuf>,
}

impl Platform {
    fn from_env() -> Self {
        Self {
            os: std::env::consts::OS,
            home: std::env::var_os("HOME").map(PathBuf::from),
            xdg_data: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
        }
    }

    fn default_root(&self) -> PathBuf {
        match (self.os, &self.home, &self.appdata) {
            ("macos", Some(home), _) => {
                home.join("Library/Application Support").join(DATA_DIR_NAME)
            }
            ("windows", _, Some(appdata)) => appdata.join(DATA_DIR_NAME),
            (_, Some(home), _) => self.xdg_or(home).join(DATA_DIR_NAME),
            (_, None, _) => PathBuf::from(".od"),
        }
    }

    /// The default root of builds before the rename: the same formula on
    /// macOS and Linux, and on Windows the XDG-style folder under `HOME`
    /// those builds used. Without `HOME` they used `./.od` in the working
    /// directory; that folder depends on where the app was started, so it
    /// is not migrated (`None`).
    fn legacy_root(&self) -> Option<PathBuf> {
        match (self.os, &self.home) {
            ("macos", Some(home)) => Some(
                home.join("Library/Application Support")
                    .join(LEGACY_DATA_DIR_NAME),
            ),
            (_, Some(home)) => Some(self.xdg_or(home).join(LEGACY_DATA_DIR_NAME)),
            (_, None) => None,
        }
    }

    fn xdg_or(&self, home: &Path) -> PathBuf {
        self.xdg_data
            .clone()
            .unwrap_or_else(|| home.join(".local/share"))
    }
}

/// Moves the pre-rename data folder `legacy` to `root` when `root` does not
/// exist yet, and returns the folder to use.
///
/// The move is a rename, so projects, runs and settings keep their relative
/// layout (the store derives every path from the root). A `legacy` that is a
/// symlink to the real folder is moved as the link, so `root` points at the
/// same data. On Unix `legacy` then becomes a symlink to `root`, so an older
/// build or a daemon that was already running keeps finding its files.
///
/// Processes starting together (desktop app and `od`) take turns through an
/// exclusive lock on `root`'s parent folder (Unix) and check again once they
/// hold it. A rename that still fails means another process won when `root`
/// now exists; otherwise the data stays where it is and `legacy` is used.
/// Nothing is copied, nothing is deleted.
#[must_use]
pub fn migrate_legacy_root(root: &Path, legacy: &Path) -> PathBuf {
    migrate_with(root, legacy, || {})
}

/// [`migrate_legacy_root`] with a hook that runs just before the rename,
/// where tests stand in for a second process.
fn migrate_with(root: &Path, legacy: &Path, before_rename: impl FnOnce()) -> PathBuf {
    let exists = |p: &Path| p.symlink_metadata().is_ok();
    // Follows a symlinked legacy folder to the directory it names.
    let is_dir = |p: &Path| p.metadata().is_ok_and(|m| m.is_dir());
    if exists(root) || !is_dir(legacy) {
        return root.to_path_buf();
    }
    let _turn = MigrationLock::take(root);
    if exists(root) || !is_dir(legacy) {
        return root.to_path_buf();
    }
    before_rename();
    if let Err(e) = std::fs::rename(legacy, root) {
        if exists(root) {
            return root.to_path_buf();
        }
        if is_dir(legacy) {
            tracing::warn!(error = %e, legacy = %legacy.display(), "keeping the data folder from before the rename");
            return legacy.to_path_buf();
        }
        return root.to_path_buf();
    }
    tracing::info!(from = %legacy.display(), to = %root.display(), "moved the data folder to its new name");
    #[cfg(unix)]
    if let Err(e) = std::os::unix::fs::symlink(root, legacy) {
        tracing::warn!(error = %e, legacy = %legacy.display(), "no compatibility link at the old data folder");
    }
    root.to_path_buf()
}

/// An exclusive `flock` on the folder that holds the data root, for the few
/// microseconds of a migration. Best effort: without it (no parent, other
/// platforms) the re-checks in [`migrate_with`] still pick the right folder.
struct MigrationLock {
    #[cfg(unix)]
    _held: Option<nix::fcntl::Flock<std::fs::File>>,
}

impl MigrationLock {
    fn take(root: &Path) -> Self {
        #[cfg(unix)]
        {
            use nix::fcntl::{Flock, FlockArg};
            let held = root
                .parent()
                .and_then(|parent| std::fs::File::open(parent).ok())
                .and_then(|dir| Flock::lock(dir, FlockArg::LockExclusive).ok());
            Self { _held: held }
        }
        #[cfg(not(unix))]
        {
            let _ = root;
            Self {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(
        os: &'static str,
        home: Option<&str>,
        xdg: Option<&str>,
        appdata: Option<&str>,
    ) -> Platform {
        Platform {
            os,
            home: home.map(PathBuf::from),
            xdg_data: xdg.map(PathBuf::from),
            appdata: appdata.map(PathBuf::from),
        }
    }

    #[test]
    fn default_root_follows_each_platform() {
        let home = Some("/Users/a");
        assert_eq!(
            platform("macos", home, None, None).default_root(),
            Path::new("/Users/a/Library/Application Support/contour")
        );
        assert_eq!(
            platform("linux", home, None, None).default_root(),
            Path::new("/Users/a/.local/share/contour")
        );
        assert_eq!(
            platform("linux", home, Some("/x"), None).default_root(),
            Path::new("/x/contour")
        );
        let appdata = r"C:\Users\a\AppData\Roaming";
        assert_eq!(
            platform("windows", None, None, Some(appdata)).default_root(),
            Path::new(appdata).join("contour")
        );
        assert_eq!(
            platform("linux", None, None, None).default_root(),
            Path::new(".od")
        );
    }

    /// The old folder is wherever builds before the rename put it (main at
    /// f681fa2: `<HOME or XDG>/.local/share/open-design-rs` on every OS but
    /// macOS, `./.od` without HOME), not next to the new one on Windows.
    #[test]
    fn legacy_root_is_where_old_builds_wrote() {
        let home = Some("/Users/a");
        assert_eq!(
            platform("macos", home, None, None).legacy_root().unwrap(),
            Path::new("/Users/a/Library/Application Support/open-design-rs")
        );
        assert_eq!(
            platform("linux", home, Some("/x"), None)
                .legacy_root()
                .unwrap(),
            Path::new("/x/open-design-rs")
        );
        assert_eq!(
            platform(
                "windows",
                Some(r"C:\Users\a"),
                None,
                Some(r"C:\Users\a\AppData\Roaming")
            )
            .legacy_root()
            .unwrap(),
            Path::new(r"C:\Users\a")
                .join(".local/share")
                .join("open-design-rs")
        );
        assert_eq!(
            platform("windows", None, None, Some(r"C:\AppData")).legacy_root(),
            None
        );
    }

    #[test]
    fn windows_takes_over_the_old_home_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().join("AppData/Roaming");
        std::fs::create_dir_all(&appdata).unwrap();
        let home = tmp.path().join("home");
        let p = Platform {
            os: "windows",
            home: Some(home.clone()),
            xdg_data: None,
            appdata: Some(appdata.clone()),
        };
        let legacy = p.legacy_root().unwrap();
        write(&legacy.join("app.sqlite"), "db");
        let dir = DataDir::resolve(None, p.default_root(), p.legacy_root());
        assert_eq!(dir.root(), appdata.join("contour"));
        assert_eq!(
            std::fs::read_to_string(dir.root().join("app.sqlite")).unwrap(),
            "db"
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
        let explicit = DataDir::resolve(
            Some(chosen.clone().into_os_string()),
            root.clone(),
            Some(legacy.clone()),
        );
        assert_eq!(explicit.root(), chosen);
        assert!(!root.exists(), "OD_DATA_DIR leaves the old folder alone");

        let empty = DataDir::resolve(Some("".into()), root.clone(), Some(legacy.clone()));
        assert_eq!(empty.root(), root);
        assert_eq!(
            std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
            "db"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_old_folder_that_is_a_symlink_is_taken_over() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("external-drive/data");
        write(&real.join("app.sqlite"), "db");
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        std::os::unix::fs::symlink(&real, &legacy).unwrap();

        assert_eq!(migrate_legacy_root(&root, &legacy), root);
        assert_eq!(
            std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
            "db"
        );
        assert_eq!(
            std::fs::read_to_string(legacy.join("app.sqlite")).unwrap(),
            "db"
        );
        // The data itself never moved.
        assert_eq!(
            std::fs::read_to_string(real.join("app.sqlite")).unwrap(),
            "db"
        );
    }

    #[test]
    fn losing_the_rename_race_uses_the_winners_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        write(&legacy.join("app.sqlite"), "db");
        // Another process renames between our checks and our rename.
        let chosen = migrate_with(&root, &legacy, || std::fs::rename(&legacy, &root).unwrap());
        assert_eq!(chosen, root);
        assert_eq!(
            std::fs::read_to_string(chosen.join("app.sqlite")).unwrap(),
            "db"
        );
    }

    #[test]
    fn a_new_folder_created_meanwhile_wins_over_the_old_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        write(&legacy.join("app.sqlite"), "old");
        // Another process already runs on a fresh `contour` (rename fails: not empty).
        let chosen = migrate_with(&root, &legacy, || write(&root.join("app.sqlite"), "new"));
        assert_eq!(chosen, root);
        assert_eq!(
            std::fs::read_to_string(legacy.join("app.sqlite")).unwrap(),
            "old"
        );
    }

    #[test]
    fn an_old_folder_that_vanishes_falls_back_to_the_new_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, legacy) = (
            tmp.path().join("contour"),
            tmp.path().join("open-design-rs"),
        );
        write(&legacy.join("app.sqlite"), "db");
        let moved = tmp.path().join("moved-away");
        let chosen = migrate_with(&root, &legacy, || std::fs::rename(&legacy, &moved).unwrap());
        assert_eq!(chosen, root, "never hand out a path that no longer exists");
    }

    /// Several processes starting at once (desktop app + `od`) must all end
    /// up on the one migrated folder, each followed by the daemon's
    /// `create_dir_all` of the folder it was given.
    #[test]
    fn concurrent_first_runs_agree_on_one_folder() {
        for _ in 0..300 {
            let tmp = tempfile::tempdir().unwrap();
            let (root, legacy) = (
                tmp.path().join("contour"),
                tmp.path().join("open-design-rs"),
            );
            write(&legacy.join("app.sqlite"), "db");
            let barrier = std::sync::Barrier::new(4);
            let chosen: Vec<PathBuf> = std::thread::scope(|s| {
                let handles: Vec<_> = (0..4)
                    .map(|_| {
                        s.spawn(|| {
                            barrier.wait();
                            let dir = migrate_legacy_root(&root, &legacy);
                            std::fs::create_dir_all(&dir).unwrap();
                            dir
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for dir in &chosen {
                assert_eq!(dir, &root, "every process uses the new folder");
            }
            assert_eq!(
                std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
                "db"
            );
            #[cfg(unix)]
            assert!(
                legacy.symlink_metadata().unwrap().file_type().is_symlink(),
                "the old path stays a link, not an empty folder"
            );
        }
    }

    /// Same race with a symlinked old folder: without taking turns, a late
    /// process renames the old link over the new one and `contour` ends up
    /// pointing at itself.
    #[cfg(unix)]
    #[test]
    fn concurrent_first_runs_keep_a_symlinked_folder_reachable() {
        for _ in 0..300 {
            let tmp = tempfile::tempdir().unwrap();
            let real = tmp.path().join("external/data");
            write(&real.join("app.sqlite"), "db");
            let (root, legacy) = (
                tmp.path().join("contour"),
                tmp.path().join("open-design-rs"),
            );
            std::os::unix::fs::symlink(&real, &legacy).unwrap();
            let barrier = std::sync::Barrier::new(4);
            std::thread::scope(|s| {
                for _ in 0..4 {
                    s.spawn(|| {
                        barrier.wait();
                        assert_eq!(migrate_legacy_root(&root, &legacy), root);
                    });
                }
            });
            assert_eq!(
                std::fs::read_to_string(root.join("app.sqlite")).unwrap(),
                "db"
            );
            assert_eq!(
                std::fs::read_to_string(legacy.join("app.sqlite")).unwrap(),
                "db"
            );
        }
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
