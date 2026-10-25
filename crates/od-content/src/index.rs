//! The in-memory content index: built once at startup (folders parsed in
//! parallel), swapped atomically on reload, read lock-free by requests.

use crate::craft::list_craft;
use crate::design_systems::{DesignSystemEntry, ListOptions, list_design_systems};
use crate::plugins::{PluginEntry, list_plugins};
use crate::prompt_templates::{PromptTemplate, list_prompt_templates};
use crate::skills::{SkillEntry, list_skills, resolve_skill_id};
use arc_swap::ArcSwap;
use od_contracts::CraftEntry;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where content lives. User roots shadow bundled roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentRoots {
    pub bundled_skills: PathBuf,
    pub bundled_design_templates: PathBuf,
    pub bundled_design_systems: PathBuf,
    pub craft: PathBuf,
    /// `plugins/_official` (groups of plugin folders).
    pub bundled_plugins: PathBuf,
    /// `prompt-templates/{image,video}`.
    pub prompt_templates: PathBuf,
    pub user_skills: Option<PathBuf>,
    pub user_design_templates: Option<PathBuf>,
    pub user_design_systems: Option<PathBuf>,
    pub user_plugins: Option<PathBuf>,
}

impl ContentRoots {
    /// Bundled roots under one resource folder (the repo's `content/`).
    #[must_use]
    pub fn bundled(resource_root: &Path) -> Self {
        Self {
            bundled_skills: resource_root.join("skills"),
            bundled_design_templates: resource_root.join("design-templates"),
            bundled_design_systems: resource_root.join("design-systems"),
            craft: resource_root.join("craft"),
            bundled_plugins: resource_root.join("plugins").join("_official"),
            prompt_templates: resource_root.join("prompt-templates"),
            ..Self::default()
        }
    }

    /// Adds user roots (`<data>/skills`, `<data>/design-templates`,
    /// `<data>/design-systems`).
    #[must_use]
    pub fn with_user_root(mut self, data_root: &Path) -> Self {
        self.user_skills = Some(data_root.join("skills"));
        self.user_design_templates = Some(data_root.join("design-templates"));
        self.user_design_systems = Some(data_root.join("design-systems"));
        self.user_plugins = Some(data_root.join("plugins"));
        self
    }

    fn skill_roots(user: Option<&PathBuf>, bundled: &Path) -> (Vec<PathBuf>, bool) {
        match user {
            Some(u) => (vec![u.clone(), bundled.to_path_buf()], true),
            None => (vec![bundled.to_path_buf()], false),
        }
    }
}

/// A complete snapshot of the catalogue.
#[derive(Debug, Clone, Default)]
pub struct ContentIndex {
    pub skills: Vec<SkillEntry>,
    pub design_templates: Vec<SkillEntry>,
    pub design_systems: Vec<DesignSystemEntry>,
    pub craft: Vec<CraftEntry>,
    pub craft_dir: PathBuf,
    pub plugins: Vec<PluginEntry>,
    pub prompt_templates: Vec<PromptTemplate>,
    pub build_time: Duration,
}

impl ContentIndex {
    /// Scans every root; the four catalogues load in parallel.
    #[must_use]
    pub fn build(roots: &ContentRoots) -> Self {
        let started = Instant::now();
        let ((skills, design_templates), (design_systems, craft)) = rayon::join(
            || {
                rayon::join(
                    || {
                        let (r, u) = ContentRoots::skill_roots(
                            roots.user_skills.as_ref(),
                            &roots.bundled_skills,
                        );
                        list_skills(&r, u)
                    },
                    || {
                        let (r, u) = ContentRoots::skill_roots(
                            roots.user_design_templates.as_ref(),
                            &roots.bundled_design_templates,
                        );
                        list_skills(&r, u)
                    },
                )
            },
            || {
                rayon::join(
                    || {
                        let mut all = list_design_systems(
                            &roots.bundled_design_systems,
                            ListOptions::BUILT_IN,
                        );
                        if let Some(user) = &roots.user_design_systems {
                            all.extend(list_design_systems(user, ListOptions::USER));
                        }
                        all
                    },
                    || list_craft(&roots.craft),
                )
            },
        );
        let plugins = list_plugins(&roots.bundled_plugins, roots.user_plugins.as_deref());
        let prompt_templates = list_prompt_templates(&roots.prompt_templates);
        Self {
            skills,
            design_templates,
            design_systems,
            craft,
            craft_dir: roots.craft.clone(),
            plugins,
            prompt_templates,
            build_time: started.elapsed(),
        }
    }

    /// Finds a skill by id, following renamed-id aliases.
    #[must_use]
    pub fn find_skill(&self, id: &str) -> Option<&SkillEntry> {
        find(&self.skills, id)
    }

    #[must_use]
    pub fn find_design_template(&self, id: &str) -> Option<&SkillEntry> {
        find(&self.design_templates, id)
    }

    #[must_use]
    pub fn find_design_system(&self, id: &str) -> Option<&DesignSystemEntry> {
        self.design_systems.iter().find(|d| d.summary.id == id)
    }

    #[must_use]
    pub fn find_plugin(&self, id: &str) -> Option<&PluginEntry> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

fn find<'a>(entries: &'a [SkillEntry], id: &str) -> Option<&'a SkillEntry> {
    let canonical = resolve_skill_id(id);
    entries.iter().find(|e| e.info.id == canonical)
}

/// Shared handle: lock-free reads, atomic swap on [`ContentStore::reload`].
#[derive(Debug)]
pub struct ContentStore {
    roots: ContentRoots,
    index: ArcSwap<ContentIndex>,
    /// Reloads are numbered when they start; a scan is stored only if no
    /// later-started one was stored already. Otherwise a reload that began
    /// before a write could finish last and swap the older snapshot back
    /// in, losing a skill just created (tests/reload.rs).
    started: std::sync::atomic::AtomicU64,
    stored: std::sync::Mutex<u64>,
}

impl ContentStore {
    #[must_use]
    pub fn load(roots: ContentRoots) -> Self {
        let index = ContentIndex::build(&roots);
        tracing::info!(
            skills = index.skills.len(),
            design_templates = index.design_templates.len(),
            design_systems = index.design_systems.len(),
            craft = index.craft.len(),
            plugins = index.plugins.len(),
            ms = index.build_time.as_millis(),
            "content index built"
        );
        Self {
            roots,
            index: ArcSwap::from_pointee(index),
            started: std::sync::atomic::AtomicU64::new(0),
            stored: std::sync::Mutex::new(0),
        }
    }

    /// Current snapshot; cheap to call per request.
    #[must_use]
    pub fn snapshot(&self) -> Arc<ContentIndex> {
        self.index.load_full()
    }

    /// Rescans every root and swaps the snapshot in.
    pub fn reload(&self) {
        let generation = self
            .started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let index = Arc::new(ContentIndex::build(&self.roots));
        let mut stored = self
            .stored
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if generation > *stored {
            *stored = generation;
            self.index.store(index);
        }
    }

    #[must_use]
    pub fn roots(&self) -> &ContentRoots {
        &self.roots
    }
}
