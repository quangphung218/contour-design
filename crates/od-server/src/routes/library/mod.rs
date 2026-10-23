//! Track B1-library routes (spec 1.4, 1.5), mounted with one `merge` in
//! [`crate::router`]:
//!
//! - [`skills`]: create, edit (user override of a bundled skill), delete.
//! - [`plugins`]: install from a local folder, uninstall, "Use" (a
//!   project bound to the plugin and its inputs).
//! - [`templates`]: projects saved as templates (`templates` table), and
//!   "Remix" of a design template's example.
//! - [`assets`]: the media library across projects, uploads, "Chat to
//!   design".
//! - [`design_systems`]: import, copy, edit, status, default, archive,
//!   assets, token score, revisions, "Edit with agent".

pub mod assets;
pub mod design_systems;
pub mod plugins;
pub mod skills;
pub mod templates;

use crate::error::ApiErr;
use crate::state::AppState;
use axum::Router;
use axum::http::StatusCode;
use od_content::user_content::WriteError;
use od_contracts::ErrorCode;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(skills::routes())
        .merge(plugins::routes())
        .merge(templates::routes())
        .merge(assets::routes())
        .merge(design_systems::routes())
}

impl From<WriteError> for ApiErr {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::BadRequest(m) => ApiErr::bad_request(m),
            WriteError::Exists(m) => ApiErr::new(
                StatusCode::CONFLICT,
                ErrorCode::Conflict,
                format!("{m} already exists"),
            ),
            WriteError::NotFound(m) => ApiErr::not_found(m),
            WriteError::Io(m) => ApiErr::internal(m),
        }
    }
}

/// Runs blocking file work off the async runtime, then reloads the content
/// index so the change is listed right away.
pub(crate) async fn write_and_reload<T, F>(state: &AppState, f: F) -> Result<T, ApiErr>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiErr> + Send + 'static,
{
    let content = std::sync::Arc::clone(&state.content);
    tokio::task::spawn_blocking(move || {
        let out = f()?;
        content.reload();
        Ok(out)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
}

/// An optional JSON body: empty means the defaults.
pub(crate) fn optional_json<T: serde::de::DeserializeOwned + Default>(
    body: &axum::body::Bytes,
) -> Result<T, ApiErr> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(body).map_err(|e| ApiErr::bad_request(e.to_string()))
}
