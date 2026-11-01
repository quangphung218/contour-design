use std::io;
use std::path::{Path, PathBuf};

/// Errors from the store. Each variant maps to one HTTP status in `od-server`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("invalid file name")]
    InvalidPath,
    #[error("reserved project path")]
    ReservedPath,
    #[error("hidden path segments are not accessible in imported folders")]
    HiddenSegment,
    #[error("path escapes project dir")]
    PathEscape,
    #[error("invalid project id")]
    InvalidId,
    #[error("project not found")]
    ProjectNotFound,
    #[error("conversation not found")]
    ConversationNotFound,
    #[error("file not found")]
    FileNotFound,
    #[error("version not found")]
    VersionNotFound,
    #[error("comment not found")]
    CommentNotFound,
    #[error("file already exists")]
    FileExists,
    /// An imported project's folder is gone (e.g. an unplugged drive). The
    /// daemon never recreates a user folder.
    #[error("project folder is missing")]
    ProjectFolderMissing,
    #[error("versions are only available for HTML files")]
    NotHtml,
    #[error("{0}")]
    BadRequest(String),
    /// The file moved on since the client read it.
    #[error("{0}")]
    Conflict(String),
    /// The OS refused a write (`EACCES`, `EPERM`, `EROFS`): a read-only
    /// folder or volume. Carries the folder when the caller knows it.
    #[error("{}", no_write_message(.0.as_deref()))]
    NoWritePermission(Option<PathBuf>),
    /// The disk is full (`ENOSPC`, `SQLITE_FULL`).
    #[error("The disk is full. Free some space, then try again.")]
    StorageFull,
    /// Any other I/O error. Built through `From<io::Error>`, which sorts
    /// out the two above.
    #[error("io error: {0}")]
    Io(io::Error),
    #[error("database error: {0}")]
    Db(rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

impl StoreError {
    /// Maps a filesystem `NotFound` to [`StoreError::FileNotFound`].
    pub(crate) fn from_file_io(err: io::Error) -> Self {
        if err.kind() == io::ErrorKind::NotFound {
            Self::FileNotFound
        } else {
            Self::from(err)
        }
    }

    /// Names `dir` in a [`StoreError::NoWritePermission`] that has no
    /// folder yet; other errors pass through.
    #[must_use]
    pub fn in_folder(self, dir: &Path) -> Self {
        match self {
            Self::NoWritePermission(None) => Self::NoWritePermission(Some(dir.to_path_buf())),
            other => other,
        }
    }
}

fn no_write_message(dir: Option<&Path>) -> String {
    match dir {
        Some(dir) => format!(
            "Contour has no permission to write to the folder {}. Pick a folder you can write to, or change its permissions.",
            dir.display()
        ),
        None => "Contour has no permission to write a file it needs. Check the permissions of the project folder and of Contour's data folder.".to_owned(),
    }
}

impl From<io::Error> for StoreError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
                Self::NoWritePermission(None)
            }
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Self::StorageFull,
            _ => Self::Io(err),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode as C;
        match err.sqlite_error_code() {
            Some(C::DiskFull) => Self::StorageFull,
            Some(C::ReadOnly | C::PermissionDenied) => Self::NoWritePermission(None),
            _ => Self::Db(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_refusals_and_full_disks_get_their_own_variant() {
        let denied = StoreError::from(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(matches!(denied, StoreError::NoWritePermission(None)));
        // Raw errno 1 is EPERM only on Unix; Windows numbers differ.
        #[cfg(unix)]
        assert!(matches!(
            StoreError::from(io::Error::from_raw_os_error(1)), // EPERM
            StoreError::NoWritePermission(None)
        ));
        let full = StoreError::from(io::Error::from(io::ErrorKind::StorageFull));
        assert!(matches!(full, StoreError::StorageFull));
        let named = denied.in_folder(Path::new("/Users/me/ro site"));
        let text = named.to_string();
        assert!(
            text.contains("/Users/me/ro site") && !text.contains("os error"),
            "{text}"
        );
        assert!(!full.to_string().contains("os error"));
        let other = StoreError::from(io::Error::other("boom"));
        assert!(matches!(other, StoreError::Io(_)));
    }
}
