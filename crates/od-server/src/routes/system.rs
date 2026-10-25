use crate::error::ApiJson;
use axum::http::header;
use axum::response::IntoResponse;
use od_contracts::{AppVersionResponse, BrowserResponse, HealthResponse};

const NO_STORE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "no-store")];

pub async fn health() -> impl IntoResponse {
    (
        NO_STORE,
        ApiJson(HealthResponse {
            ok: true,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
    )
}

pub async fn version() -> impl IntoResponse {
    (
        NO_STORE,
        ApiJson(AppVersionResponse {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            name: "contour".to_owned(),
            platform: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
        }),
    )
}

/// Onboarding warns early when export would have no browser to drive.
pub async fn browser() -> impl IntoResponse {
    let found = tokio::task::spawn_blocking(|| od_render_cdp::find_chrome().is_ok())
        .await
        .unwrap_or(false);
    tracing::info!(found, "export browser check");
    (NO_STORE, ApiJson(BrowserResponse { found }))
}
