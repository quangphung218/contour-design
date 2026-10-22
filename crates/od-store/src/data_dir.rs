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

    /// `OD_DATA_DIR`, else `<platform data dir>/open-design-rs`, else `./.od`.
    #[must_use]
    pub fn from_env() -> Self {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Self::new(dir);
        }
        Self::new(default_root())
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

fn default_root() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (std::env::consts::OS, home) {
        ("macos", Some(home)) => home.join("Library/Application Support/open-design-rs"),
        (_, Some(home)) => std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("open-design-rs"),
        (_, None) => PathBuf::from(".od"),
    }
}
