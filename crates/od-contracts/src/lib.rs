//! Wire types shared by the daemon (`od-server`), the CLI (`od-cli`) and the UI.
//!
//! Field names follow the original app's JSON (camelCase) so existing clients
//! and fixtures keep working. Every type here is plain data: no I/O, no
//! validation beyond what serde enforces.

pub mod content;
pub mod error;
pub mod files;
pub mod projects;
pub mod system;

pub use content::*;
pub use error::*;
pub use files::*;
pub use projects::*;
pub use system::*;

/// Deserializes a field that distinguishes "absent" from "explicit null".
///
/// Use with `#[serde(default, deserialize_with = "od_contracts::double_option")]`
/// on an `Option<Option<T>>`: absent → `None`, `null` → `Some(None)`,
/// value → `Some(Some(v))`. PATCH bodies need this to clear a column.
///
/// # Errors
/// Returns the inner deserializer's error when the value has the wrong type.
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}
