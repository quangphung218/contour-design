//! The `PATH` agents are found on.
//!
//! An app started from Finder or the Dock gets launchd's bare `PATH`
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), so the agent CLIs a user installed
//! under `~/.local/bin`, Homebrew or npm are "not found". At startup the
//! desktop asks the user's login shell for its `PATH` and appends the usual
//! install folders as a fallback, keeping what the process already had
//! first. [`crate::RunEngine::rescan_path`] asks again later.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the login shell may take (rc files can be slow).
const SHELL_TIMEOUT: Duration = Duration::from_secs(3);
/// Brackets the value in the shell's output, which rc files may pollute.
const MARK: &str = "__OD_PATH__";

/// Install folders of common agent CLIs, relative to `$HOME` or absolute.
const FALLBACK_DIRS: &[&str] = &[
    "~/.local/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "~/.npm-global/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
    "~/.volta/bin",
];

/// `$PATH` as the user's login shell sets it; `None` when the shell is
/// missing, slow or prints nothing usable.
pub async fn login_shell_path(shell: &OsStr) -> Option<String> {
    use tokio::io::AsyncReadExt;
    let mut cmd = tokio::process::Command::new(shell);
    cmd.args(["-ilc", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read up to the closing mark, not to EOF: a background job the rc
    // files started can hold stdout open long after the shell is done.
    let read = async {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stdout.read(&mut buf).await.ok()?;
            if n == 0 {
                return None;
            }
            out.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&out);
            if let Some((_, rest)) = text.split_once(MARK) {
                if let Some((path, _)) = rest.split_once(MARK) {
                    return Some(path.to_owned());
                }
            }
        }
    };
    let path = tokio::time::timeout(SHELL_TIMEOUT, read).await.ok()??;
    Some(path).filter(|p| !p.trim().is_empty())
}

/// `current`, then the login shell's entries, then the fallback folders;
/// each folder once, in that order.
pub fn merge(current: &OsStr, login: Option<&str>, home: Option<&Path>) -> OsString {
    let fallback = FALLBACK_DIRS
        .iter()
        .filter_map(|d| match d.strip_prefix("~/") {
            Some(rel) => home.map(|h| h.join(rel)),
            None => Some(PathBuf::from(d)),
        });
    let mut seen = Vec::<PathBuf>::new();
    for dir in std::env::split_paths(current)
        .chain(login.into_iter().flat_map(|l| std::env::split_paths(l)))
        .chain(fallback)
    {
        if !dir.as_os_str().is_empty() && !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    std::env::join_paths(seen).unwrap_or_else(|_| current.to_owned())
}

/// The shell the desktop asks: `$SHELL`, else zsh; none on Windows.
#[must_use]
pub fn default_shell() -> Option<OsString> {
    (!cfg!(windows)).then(|| std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into()))
}

/// The agent search path for this process.
pub async fn agent_path() -> OsString {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let login = match default_shell() {
        Some(shell) => login_shell_path(&shell).await,
        None => None,
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    merge(&current, login.as_deref(), home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new binary's first exec can take seconds on a loaded Mac (it
    /// scans it), past the 3 s login shell limit; run a fresh fake shell
    /// once before timing it.
    #[cfg(unix)]
    fn warm_up(shell: &Path) {
        let _ = std::process::Command::new(shell)
            .args(["-ilc", "true"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    #[test]
    fn keeps_order_and_drops_repeats() {
        let merged = merge(
            OsStr::new("/usr/bin:/bin"),
            // nvm's folder is in no fallback list: only the login shell knows it.
            Some("/Users/u/.nvm/versions/node/v22/bin:/usr/bin:/opt/homebrew/bin"),
            Some(Path::new("/Users/u")),
        );
        let dirs: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(
            dirs[..4],
            [
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/Users/u/.nvm/versions/node/v22/bin"),
                PathBuf::from("/opt/homebrew/bin"),
            ]
        );
        assert!(dirs.contains(&PathBuf::from("/Users/u/.local/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/u/.bun/bin")));
        let unique: std::collections::HashSet<_> = dirs.iter().collect();
        assert_eq!(unique.len(), dirs.len(), "{dirs:?}");
    }

    #[test]
    fn falls_back_without_a_login_shell() {
        let merged = merge(OsStr::new("/usr/bin"), None, Some(Path::new("/h")));
        let dirs: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(dirs[0], PathBuf::from("/usr/bin"));
        assert!(dirs.contains(&PathBuf::from("/h/.local/bin")));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reads_the_login_shell_path_past_rc_noise() {
        let dir = std::env::temp_dir().join(format!("od-login-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shell = dir.join("fake-shell");
        // A shell whose rc files print a banner before running the command.
        std::fs::write(
            &shell,
            "#!/bin/sh\necho 'welcome banner'\nPATH=/opt/tools/bin:/usr/bin\nexport PATH\nshift\neval \"$1\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        warm_up(&shell);
        let path = login_shell_path(shell.as_os_str()).await;
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(path.as_deref(), Some("/opt/tools/bin:/usr/bin"));
        assert_eq!(login_shell_path(OsStr::new("/no/such/shell")).await, None);
    }

    /// An rc file that starts a background job keeps the shell's stdout
    /// open after the shell exits; the path is read as soon as it is out.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_rc_background_job_does_not_hold_the_path_back() {
        let secs = (600_000 + std::process::id() % 100_000).to_string();
        let dir = std::env::temp_dir().join(format!("od-login-bg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shell = dir.join("fake-shell");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\nsleep {secs} &\nPATH=/opt/tools/bin\nexport PATH\nshift\neval \"$1\"\n"
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        warm_up(&shell);
        let started = std::time::Instant::now();
        let path = login_shell_path(shell.as_os_str()).await;
        let took = started.elapsed();
        let _ = std::process::Command::new("pkill")
            .args(["-f", &format!("^sleep {secs}$")])
            .status();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(path.as_deref(), Some("/opt/tools/bin"));
        assert!(took < Duration::from_secs(2), "{took:?}");
    }

    /// Check again in onboarding: the login shell's new `PATH` (an nvm
    /// install made while the app runs) reaches the engine; a shell that
    /// does not answer leaves the path alone.
    #[cfg(unix)]
    #[tokio::test]
    async fn rescan_takes_the_login_shells_new_path() {
        let dir = std::env::temp_dir().join(format!("od-login-rescan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shell = dir.join("fake-shell");
        let write_shell = |path: &str| {
            std::fs::write(
                &shell,
                format!("#!/bin/sh\nPATH={path}\nexport PATH\nshift\neval \"$1\"\n"),
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let nvm = "/Users/u/.nvm/versions/node/v24/bin";
        let has_nvm = |p: &OsStr| std::env::split_paths(p).any(|d| d == Path::new(nvm));
        let engine = crate::RunEngine::new(crate::EngineConfig {
            path: OsString::from("/usr/bin"),
            login_shell: Some(shell.clone().into_os_string()),
            ..crate::EngineConfig::from_env()
        });
        assert!(!has_nvm(&engine.config().path));
        write_shell(&format!("{nvm}:/usr/bin"));
        warm_up(&shell);
        assert!(engine.rescan_path().await, "the new folder is not a change");
        let path = engine.config().path.clone();
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        // The shell's PATH comes first, as in a new Terminal: its Node runs
        // the agent's `#!/usr/bin/env node`, not an older one already here.
        assert_eq!(
            dirs[..2],
            [PathBuf::from(nvm), PathBuf::from("/usr/bin")],
            "{dirs:?}"
        );
        let unique: std::collections::HashSet<_> = dirs.iter().collect();
        assert_eq!(unique.len(), dirs.len(), "{dirs:?}");
        // Same answer again: nothing changes.
        assert!(!engine.rescan_path().await);
        // A shell that fails now keeps what the last rescan found.
        std::fs::write(&shell, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(!engine.rescan_path().await);
        assert!(has_nvm(&engine.config().path), "{:?}", engine.config().path);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    /// A daemon started from a terminal (no login shell): rescan keeps its
    /// `PATH` first and adds the usual install folders, so `claude` from
    /// the native installer (`~/.local/bin`) is found.
    async fn rescan_without_a_login_shell_adds_the_install_folders() {
        let engine = crate::RunEngine::new(crate::EngineConfig {
            path: OsString::from("/only/here"),
            login_shell: None,
            rescan_install_dirs: true,
            ..crate::EngineConfig::from_env()
        });
        assert!(engine.rescan_path().await);
        let path = engine.config().path.clone();
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(dirs[0], PathBuf::from("/only/here"));
        assert!(
            dirs.contains(&PathBuf::from("/opt/homebrew/bin")),
            "{dirs:?}"
        );
        if let Some(home) = std::env::var_os("HOME") {
            assert!(
                dirs.contains(&Path::new(&home).join(".local/bin")),
                "{dirs:?}"
            );
        }
        // Nothing new the second time.
        assert!(!engine.rescan_path().await);
    }

    /// Tests turn the install folders off so the machine's agents stay out.
    #[tokio::test]
    async fn rescan_can_keep_the_path_as_started() {
        let engine = crate::RunEngine::new(crate::EngineConfig {
            path: OsString::from("/only/here"),
            login_shell: None,
            rescan_install_dirs: false,
            ..crate::EngineConfig::from_env()
        });
        assert!(!engine.rescan_path().await);
        assert_eq!(engine.config().path, OsString::from("/only/here"));
    }
}
