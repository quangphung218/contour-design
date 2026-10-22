//! Bundled and user content for open-design-rs: skills, design templates,
//! design systems and craft rules.
//!
//! Loading is "discovery, not validation": unreadable entries are skipped,
//! never fatal. Normalization follows the original app so the same folders
//! produce the same catalogue (spec 3.4–3.6).

pub mod craft;
pub mod design_systems;
pub mod frontmatter;
pub mod index;
pub mod prompt;
pub mod skills;
mod text;

pub use design_systems::DesignSystemEntry;
pub use index::{ContentIndex, ContentRoots, ContentStore};
pub use skills::SkillEntry;

/// Environment variable pointing at the bundled resource folder.
pub const RESOURCE_ROOT_ENV: &str = "OD_RESOURCE_ROOT";
