//! Live artifacts (spec 1.5, 3.2, 4.2): an HTML page rendered from
//! `template.html` plus `data.json` (`html_template_v1`), refreshed by hand
//! from a local source. Pure Rust, no browser: templates that could run
//! code are refused, and every value is escaped.

pub mod model;
pub mod refresh;
pub mod render;
pub mod store;

pub use model::{CreateInput, LiveArtifact, Summary, UpdateInput};
pub use refresh::{RefreshOutcome, RefreshSummary};
pub use store::LiveStore;

#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("live artifact not found")]
    NotFound,
    #[error("{0}")]
    Validation(String),
    #[error("template: {0}")]
    Template(#[from] render::RenderError),
    #[error("a refresh of this live artifact is already running")]
    RefreshInProgress,
    #[error("{0}")]
    NotRefreshable(&'static str),
    #[error("refresh source failed: {0}")]
    Source(String),
    #[error("corrupt live artifact: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl LiveError {
    /// Stable machine code for logs and API errors.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "LIVE_ARTIFACT_NOT_FOUND",
            Self::Validation(_) | Self::Template(_) => "VALIDATION_FAILED",
            Self::RefreshInProgress => "REFRESH_IN_PROGRESS",
            Self::NotRefreshable(_) => "REFRESH_UNAVAILABLE",
            Self::Source(_) => "REFRESH_SOURCE_FAILED",
            Self::Corrupt(_) | Self::Io(_) => "INTERNAL_ERROR",
        }
    }
}

pub type Result<T> = std::result::Result<T, LiveError>;
