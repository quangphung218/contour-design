//! Persistent state for open-design-rs: the SQLite database, the data
//! directory layout, project folders, file version history, and the safe
//! path resolution every file operation goes through.

pub mod chat;
pub mod comments;
pub mod data_dir;
pub mod db;
pub mod error;
pub mod files;
pub mod paths;
pub mod store;
pub mod versions;

pub use chat::{AgentSession, NewTurn};
pub use data_dir::{DATA_DIR_ENV, DataDir};
pub use error::{Result, StoreError};
pub use files::ProjectRoot;
pub use store::{PROJECT_MANIFEST_PATH, Store};

/// Current time as Unix epoch milliseconds.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
