use crate::error::ApiJson;
use axum::http::header;
use axum::response::IntoResponse;
use od_contracts::{AppVersionResponse, HealthResponse};

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
            name: "open-design-rs".to_owned(),
            platform: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
        }),
    )
}
