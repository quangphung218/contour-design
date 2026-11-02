//! The HyperFrames CLI (`lint`, `validate`, `inspect`, `transcribe`,
//! `tts`) the HyperFrames design template tells an agent to run as
//! `"$OD_NODE_BIN" "$OD_HYPERFRAMES_BIN" …`. Contour renders films itself
//! and ships no copy: the agent gets one only when it is installed.

use std::ffi::OsString;
use std::path::PathBuf;

/// Environment variable naming the HyperFrames CLI, in place of the one
/// found on `PATH`; also what the agent reads.
pub const HYPERFRAMES_BIN_ENV: &str = "OD_HYPERFRAMES_BIN";

/// Finds the HyperFrames CLI: `OD_HYPERFRAMES_BIN`, else `hyperframes` in
/// the Homebrew and system folders, then on `PATH`. `None` when there is
/// none, or when `OD_HYPERFRAMES_BIN` names no file (logged).
#[must_use]
pub fn find_hyperframes() -> Option<PathBuf> {
    find_hyperframes_with(
        std::env::var_os(HYPERFRAMES_BIN_ENV),
        std::env::var_os("PATH"),
    )
}

/// [`find_hyperframes`] with `configured` and `path` in place of the
/// environment's `OD_HYPERFRAMES_BIN` and `PATH`.
#[must_use]
pub fn find_hyperframes_with(
    configured: Option<OsString>,
    path: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(bin) = configured.filter(|v| !v.is_empty()) {
        let bin = PathBuf::from(bin);
        if bin.is_file() {
            return Some(bin);
        }
        tracing::warn!(path = %bin.display(), "{HYPERFRAMES_BIN_ENV} names no file; HyperFrames CLI off");
        return None;
    }
    let name = if cfg!(windows) {
        "hyperframes.cmd"
    } else {
        "hyperframes"
    };
    let dirs = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from);
    dirs.into_iter()
        .chain(path.iter().flat_map(std::env::split_paths))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_variable_wins_over_path_and_a_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let on_path = tmp.path().join("bin");
        std::fs::create_dir_all(&on_path).unwrap();
        let name = if cfg!(windows) {
            "hyperframes.cmd"
        } else {
            "hyperframes"
        };
        std::fs::write(on_path.join(name), "").unwrap();
        let mine = tmp.path().join("my-hyperframes");
        std::fs::write(&mine, "").unwrap();
        let path = Some(on_path.clone().into_os_string());
        assert_eq!(
            find_hyperframes_with(Some(mine.clone().into()), path.clone()),
            Some(mine)
        );
        assert_eq!(
            find_hyperframes_with(Some(tmp.path().join("gone").into()), path.clone()),
            None,
            "a wrong path is never swapped for the default"
        );
        let found = find_hyperframes_with(Some("".into()), path).unwrap();
        assert!(
            found.ends_with(name),
            "default: PATH, got {}",
            found.display()
        );
    }
}
