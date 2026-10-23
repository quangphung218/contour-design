//! Management screens (Projects, Settings, Automations, MCP, Home rail):
//! routes the original app has beyond the core API. Mounted with one
//! `merge` in [`crate::router`].

pub mod diagnostics;
pub mod import_zip;
pub mod locations;
pub mod mcp;
pub mod memory;
pub mod projects;
pub mod settings;

use crate::state::AppState;
use axum::Router;
use axum::routing::{delete, get, post};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/search", get(projects::search))
        .route("/api/project-status", get(projects::project_status))
        .route("/api/projects/{id}/viewed", post(projects::mark_viewed))
        .route("/api/recently-viewed", get(projects::recently_viewed))
        .route("/api/import/claude-design", post(import_zip::import_route))
        .route("/api/test/connection", post(settings::test_connection))
        .route("/api/provider/models", post(settings::provider_models))
        .route(
            "/api/agent-cli-env",
            get(settings::get_agent_env).put(settings::put_agent_env),
        )
        .route("/api/memory", get(memory::list).post(memory::add))
        .route("/api/memory/{id}", delete(memory::remove))
        .route(
            "/api/project-locations",
            get(locations::get).put(locations::put),
        )
        .route("/api/project-locations/scan", post(locations::scan_route))
        .route("/api/mcp/install-info", get(mcp::install_info))
        .route("/api/mcp/install/{agent}", post(mcp::install_route))
        .route("/api/mcp/catalog", get(mcp::catalog_route))
        .route("/api/diagnostics/export", get(diagnostics::export))
}
