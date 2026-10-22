use std::io;

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
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
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
            Self::Io(err)
        }
    }
}
