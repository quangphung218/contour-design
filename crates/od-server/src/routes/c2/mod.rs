//! Track C2-chat-home routes, mounted with one `merge` in
//! [`crate::router`]:
//!
//! - [`memory`]: Settings ▸ Memory config, hooks, `MEMORY.md`, import,
//!   change events (memory toast).
//! - [`library`]: "Import from library" in the composer.
//! - [`jury`]: Design Jury ▸ Interrupt.
//! - [`media`]: media generation progress on the run's stream.
//! - [`help`]: Help ▸ Report Issue (diagnostics text, log folder).

pub mod help;
pub mod jury;
pub mod library;
pub mod media;
pub mod memory;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/memory/config",
            get(memory::get_config).patch(memory::patch_config),
        )
        .route(
            "/api/memory/index",
            get(memory::get_index).put(memory::put_index),
        )
        .route("/api/memory/import", post(memory::import))
        .route("/api/memory/events", get(memory::events))
        .route("/api/library/assets/apply", post(library::apply))
        .route(
            "/api/projects/{id}/critique/{run_id}/interrupt",
            post(jury::interrupt),
        )
        .route("/api/diagnostics/summary", get(help::summary))
        .route("/api/diagnostics/open-logs", post(help::open_logs))
}
