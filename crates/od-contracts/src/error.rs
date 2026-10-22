use serde::{Deserialize, Serialize};

/// Machine-readable error code carried in every non-2xx JSON body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ErrorCode {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    ProjectNotFound,
    FileNotFound,
    VersionNotFound,
    FileExists,
    Conflict,
    PathEscape,
    PayloadTooLarge,
    InternalError,
}

/// Error payload: `{ "error": { "code": "...", "message": "..." } }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}
