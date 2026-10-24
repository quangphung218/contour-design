//! Bundled and user content for Contour: skills, design templates,
//! design systems, craft rules and bundled plugins.
//!
//! Loading is "discovery, not validation": unreadable entries are skipped,
//! never fatal. Normalization follows the original app so the same folders
//! produce the same catalogue (spec 3.4–3.6).

pub mod atoms;
pub mod craft;
pub mod design_kit;
pub mod design_system_gen;
pub mod design_systems;
pub mod frontmatter;
pub mod index;
pub mod pipeline;
pub mod plugin_validate;
pub mod plugins;
pub mod prompt;
pub mod prompt_templates;
pub mod skills;
mod text;
pub mod user_content;

pub use design_systems::DesignSystemEntry;
pub use index::{ContentIndex, ContentRoots, ContentStore};
pub use plugins::PluginEntry;
pub use skills::SkillEntry;

/// Environment variable pointing at the bundled resource folder.
pub const RESOURCE_ROOT_ENV: &str = "OD_RESOURCE_ROOT";
