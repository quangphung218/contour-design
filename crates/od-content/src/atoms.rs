//! First-party atom catalogue (spec 1.4 "Atoms và pipeline"): the static
//! list of the original `plugins/atoms.ts`. A plugin pipeline may only
//! name atoms from this list; `od plugin validate` rejects the rest.

use serde::Serialize;

/// Whether the daemon can run the atom today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AtomStatus {
    Implemented,
    Planned,
}

/// One catalogue row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Atom {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub status: AtomStatus,
    pub task_kinds: &'static [&'static str],
}

const ALL_KINDS: &[&str] = &[
    "new-generation",
    "code-migration",
    "figma-migration",
    "tune-collab",
];
const GEN_TUNE: &[&str] = &["new-generation", "tune-collab"];
const CODE: &[&str] = &["code-migration"];
const CODE_FIGMA: &[&str] = &["code-migration", "figma-migration"];
const CODE_TUNE: &[&str] = &["code-migration", "tune-collab"];

const fn atom(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    task_kinds: &'static [&'static str],
) -> Atom {
    Atom {
        id,
        label,
        description,
        status: AtomStatus::Implemented,
        task_kinds,
    }
}

/// The catalogue, in the original order.
pub const ATOMS: &[Atom] = &[
    atom(
        "discovery-question-form",
        "Discovery question form",
        "Structured clarification for unresolved material requirements.",
        GEN_TUNE,
    ),
    atom(
        "direction-picker",
        "Direction picker",
        "Host-owned visual catalog when explicitly requested.",
        GEN_TUNE,
    ),
    atom(
        "todo-write",
        "Todo write",
        "TodoWrite-driven plan.",
        ALL_KINDS,
    ),
    atom("file-read", "File read", "Read project files.", ALL_KINDS),
    atom(
        "file-write",
        "File write",
        "Write project files.",
        ALL_KINDS,
    ),
    atom("file-edit", "File edit", "Edit project files.", ALL_KINDS),
    atom(
        "research-search",
        "Research search",
        "Shallow web research.",
        &["new-generation"],
    ),
    atom(
        "media-image",
        "Media image",
        "Image generation through media providers.",
        GEN_TUNE,
    ),
    atom(
        "media-video",
        "Media video",
        "Video generation through media providers.",
        GEN_TUNE,
    ),
    atom(
        "media-audio",
        "Media audio",
        "Audio generation through media providers.",
        GEN_TUNE,
    ),
    atom(
        "live-artifact",
        "Live artifact",
        "Create/refresh live artifacts.",
        GEN_TUNE,
    ),
    atom("connector", "Connector", "Connector tool calls.", GEN_TUNE),
    atom(
        "critique-theater",
        "Critique theater",
        "5-dim panel critique; devloop signal.",
        ALL_KINDS,
    ),
    atom(
        "code-import",
        "Code import",
        "Walk an existing repo into <cwd>/code/index.json.",
        CODE,
    ),
    atom(
        "design-extract",
        "Design extract",
        "Extract design tokens into <cwd>/code/tokens.json.",
        CODE_FIGMA,
    ),
    atom(
        "figma-extract",
        "Figma extract",
        "Pull Figma file tree + assets via REST.",
        &["figma-migration"],
    ),
    atom(
        "token-map",
        "Token map",
        "Crosswalk source token bag onto active design system.",
        CODE_FIGMA,
    ),
    atom(
        "rewrite-plan",
        "Rewrite plan",
        "Heuristic ownership classifier + per-leaf step list.",
        CODE_TUNE,
    ),
    atom(
        "patch-edit",
        "Patch edit",
        "Atomic unified-diff applier with shell-tier safety gate.",
        CODE_TUNE,
    ),
    atom(
        "build-test",
        "Build / test",
        "Shell-out to typecheck + tests; emits build/tests.passing signals.",
        CODE,
    ),
    atom(
        "diff-review",
        "Diff review",
        "Render rewrite as review/{diff.patch,summary.md,decision.json}.",
        CODE_TUNE,
    ),
    atom(
        "handoff",
        "Handoff",
        "Update ArtifactManifest provenance + handoffKind ladder.",
        CODE_TUNE,
    ),
];

#[must_use]
pub fn find_atom(id: &str) -> Option<&'static Atom> {
    ATOMS.iter().find(|a| a.id == id)
}

#[must_use]
pub fn is_known_atom(id: &str) -> bool {
    find_atom(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_ids_are_unique_and_findable() {
        let mut ids: Vec<&str> = ATOMS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ATOMS.len());
        assert_eq!(ATOMS.len(), 22);
        assert!(is_known_atom("critique-theater"));
        assert!(!is_known_atom("rm-rf"));
        assert_eq!(find_atom("build-test").map(|a| a.task_kinds), Some(CODE));
    }
}
