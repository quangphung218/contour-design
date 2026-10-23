//! The `PATH` agents are found on.
//!
//! An app started from Finder or the Dock gets launchd's bare `PATH`
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), so the agent CLIs a user installed
//! under `~/.local/bin`, Homebrew or npm are "not found". At startup the
//! desktop asks the user's login shell for its `PATH` and appends the usual
//! install folders as a fallback, keeping what the process already had
//! first.

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
    let mut cmd = tokio::process::Command::new(shell);
    cmd.args(["-ilc", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(SHELL_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (_, rest) = text.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    Some(path.to_owned()).filter(|p| !p.trim().is_empty())
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

/// The agent search path for this process.
pub async fn agent_path() -> OsString {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let login = if cfg!(windows) {
        None
    } else {
        login_shell_path(&shell).await
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    merge(&current, login.as_deref(), home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let path = login_shell_path(shell.as_os_str()).await;
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(path.as_deref(), Some("/opt/tools/bin:/usr/bin"));
        assert_eq!(login_shell_path(OsStr::new("/no/such/shell")).await, None);
    }
}
