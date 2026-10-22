use serde::{Deserialize, Serialize};

/// `GET /api/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
}

/// `GET /api/version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppVersionResponse {
    pub version: String,
    pub name: String,
    pub platform: String,
    pub arch: String,
}
