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

/// `GET /api/browser`: whether the Chrome, Edge or Chromium that PDF, PPTX
/// and image export drive is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserResponse {
    pub found: bool,
}
