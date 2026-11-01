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
    RunNotFound,
    AgentNotFound,
    AgentUnavailable,
    ConversationNotFound,
    /// `clientRequestId` reused for a different request.
    IdempotencyConflict,
    /// Steer: the runtime has no steering channel.
    RuntimeUnsupported,
    /// Steer: the run already ended.
    RunTerminal,
    /// Steer: the stdin of the agent is already closed.
    StdinClosed,
    /// Export: the source cannot be bundled (missing dependency, bad HTML).
    ValidationFailed,
    /// Export: the entry file is not HTML.
    UnsupportedMediaType,
    /// Export: Chrome is missing or the render failed.
    UpstreamUnavailable,
    /// The OS refused a write (read-only folder or volume); the message
    /// names the folder when known.
    NoWritePermission,
    /// The disk is full.
    StorageFull,
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
