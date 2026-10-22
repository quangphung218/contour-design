use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use od_contracts::{ApiError, ApiErrorResponse, ErrorCode};
use od_store::StoreError;

/// An HTTP error with the standard `{ "error": { code, message } }` body.
#[derive(Debug)]
pub struct ApiErr {
    pub status: StatusCode,
    pub code: ErrorCode,
    pub message: String,
}

pub type ApiResult<T> = Result<T, ApiErr>;

impl ApiErr {
    pub fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, ErrorCode::BadRequest, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, ErrorCode::Forbidden, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::InternalError,
            message,
        )
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            error: ApiError {
                code: self.code,
                message: self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<StoreError> for ApiErr {
    fn from(err: StoreError) -> Self {
        use StatusCode as S;
        let (status, code) = match &err {
            StoreError::InvalidPath
            | StoreError::ReservedPath
            | StoreError::InvalidId
            | StoreError::NotHtml
            | StoreError::BadRequest(_) => (S::BAD_REQUEST, ErrorCode::BadRequest),
            StoreError::HiddenSegment => (S::FORBIDDEN, ErrorCode::Forbidden),
            StoreError::PathEscape => (S::FORBIDDEN, ErrorCode::PathEscape),
            StoreError::ProjectNotFound => (S::NOT_FOUND, ErrorCode::ProjectNotFound),
            StoreError::FileNotFound => (S::NOT_FOUND, ErrorCode::FileNotFound),
            StoreError::VersionNotFound => (S::NOT_FOUND, ErrorCode::VersionNotFound),
            StoreError::FileExists => (S::CONFLICT, ErrorCode::FileExists),
            StoreError::ProjectFolderMissing => (S::CONFLICT, ErrorCode::Conflict),
            _ => {
                tracing::error!(error = %err, "store error");
                (S::INTERNAL_SERVER_ERROR, ErrorCode::InternalError)
            }
        };
        Self::new(status, code, err.to_string())
    }
}

impl From<JsonRejection> for ApiErr {
    fn from(rejection: JsonRejection) -> Self {
        Self::bad_request(rejection.body_text())
    }
}

/// `Json` extractor whose rejections use the standard error body.
#[derive(Debug, Clone, Copy, Default, axum::extract::FromRequest)]
#[from_request(via(axum::Json), rejection(ApiErr))]
pub struct ApiJson<T>(pub T);

impl<T: serde::Serialize> IntoResponse for ApiJson<T> {
    fn into_response(self) -> Response {
        Json(self.0).into_response()
    }
}
