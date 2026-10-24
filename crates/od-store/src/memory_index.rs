//! Settings ▸ Memory, as files next to the database (track C2-chat-home):
//! `memory/MEMORY.md` is the index the user edits, `memory/config.json` the
//! master switch and the memory hooks. Entries stay in `global_memories`.
//!
//! The index links each entry as `- [label](<id>.md) — <kind>`. Only linked
//! entries reach the prompt, so deleting a line turns a fact off without
//! deleting it. A missing index means every entry is linked.

use crate::manage::GlobalMemory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Folder under the data dir.
pub const MEMORY_DIR: &str = "memory";
pub const INDEX_FILE: &str = "MEMORY.md";
pub const CONFIG_FILE: &str = "config.json";
/// Largest index accepted from Settings (bytes).
pub const MAX_INDEX_BYTES: usize = 64 * 1024;
/// Most entries one import proposes.
pub const MAX_IMPORT: usize = 50;
/// Characters of an entry's text shown as its index label.
const LABEL_CHARS: usize = 60;

const INDEX_HEAD: &str = "# Memory\n\nFacts every new chat carries. Lines you delete here stop being injected; the entry itself stays in Settings ▸ Memory.\n";

/// The master switch and the memory hooks (the original's `.config.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoryConfig {
    /// Off: nothing is injected and nothing is learned.
    pub enabled: bool,
    /// "Learn from chats": facts in chat messages become global memory.
    pub chat_extraction_enabled: bool,
    /// "Use my profile": profile entries reach the prompt.
    pub profile_enabled: bool,
    /// "Rewrite short asks": a short request is expanded into a brief.
    pub rewrite_enabled: bool,
    /// "Self-verify output": the result is checked against the rules.
    pub verify_enabled: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            chat_extraction_enabled: false,
            profile_enabled: true,
            rewrite_enabled: true,
            verify_enabled: true,
        }
    }
}

/// A partial update of [`MemoryConfig`] (`PATCH /api/memory/config`).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryConfigPatch {
    pub enabled: Option<bool>,
    pub chat_extraction_enabled: Option<bool>,
    pub profile_enabled: Option<bool>,
    pub rewrite_enabled: Option<bool>,
    pub verify_enabled: Option<bool>,
}

impl MemoryConfig {
    #[must_use]
    pub fn apply(mut self, p: MemoryConfigPatch) -> Self {
        let set = |slot: &mut bool, v: Option<bool>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut self.enabled, p.enabled);
        set(&mut self.chat_extraction_enabled, p.chat_extraction_enabled);
        set(&mut self.profile_enabled, p.profile_enabled);
        set(&mut self.rewrite_enabled, p.rewrite_enabled);
        set(&mut self.verify_enabled, p.verify_enabled);
        self
    }
}

/// Paths of the memory files under a data dir.
#[derive(Debug, Clone)]
pub struct MemoryFiles {
    dir: PathBuf,
}

impl MemoryFiles {
    #[must_use]
    pub fn in_data_dir(root: &Path) -> Self {
        Self {
            dir: root.join(MEMORY_DIR),
        }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The config; defaults when missing or unreadable.
    #[must_use]
    pub fn config(&self) -> MemoryConfig {
        std::fs::read(self.dir.join(CONFIG_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// One writer at a time per memory folder: `link`/`unlink` read, edit
    /// and write the index, and two runs may learn a fact at once.
    fn guard(&self) -> std::sync::MutexGuard<'static, ()> {
        type Locks = std::sync::Mutex<HashMap<PathBuf, &'static std::sync::Mutex<()>>>;
        static LOCKS: std::sync::OnceLock<Locks> = std::sync::OnceLock::new();
        let lock = *LOCKS
            .get_or_init(Locks::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(self.dir.clone())
            .or_insert_with(|| Box::leak(Box::default()));
        lock.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// # Errors
    /// I/O errors.
    pub fn write_config(&self, config: &MemoryConfig) -> std::io::Result<()> {
        let _g = self.guard();
        std::fs::create_dir_all(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(config).map_err(std::io::Error::other)?;
        write_atomic(&self.dir.join(CONFIG_FILE), &bytes)
    }

    /// The stored index, `None` when the user never saved one.
    #[must_use]
    pub fn stored_index(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join(INDEX_FILE)).ok()
    }

    /// The index to show: the stored one, else one linking every entry.
    #[must_use]
    pub fn index(&self, entries: &[GlobalMemory]) -> String {
        self.stored_index()
            .unwrap_or_else(|| default_index(entries))
    }

    /// # Errors
    /// I/O errors.
    pub fn write_index(&self, index: &str) -> std::io::Result<()> {
        let _g = self.guard();
        self.write_index_locked(index)
    }

    fn write_index_locked(&self, index: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        write_atomic(&self.dir.join(INDEX_FILE), index.as_bytes())
    }

    /// Links a new entry in a stored index (a missing one links it anyway).
    ///
    /// # Errors
    /// I/O errors.
    pub fn link(&self, entry: &GlobalMemory) -> std::io::Result<()> {
        let _g = self.guard();
        match self.stored_index() {
            Some(index) => self.write_index_locked(&with_line(&index, entry)),
            None => Ok(()),
        }
    }

    /// Drops a deleted entry's line from a stored index.
    ///
    /// # Errors
    /// I/O errors.
    pub fn unlink(&self, id: &str) -> std::io::Result<()> {
        let _g = self.guard();
        match self.stored_index() {
            Some(index) => self.write_index_locked(&without_line(&index, id)),
            None => Ok(()),
        }
    }
}

/// Writes through a temp file of its own (pid + counter), so a writer
/// never renames another one's half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{n}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// The index line of `entry`.
#[must_use]
pub fn index_line(entry: &GlobalMemory) -> String {
    let label: String = entry
        .text
        .chars()
        .map(|c| match c {
            '[' | ']' | '\n' | '\r' => ' ',
            c => c,
        })
        .take(LABEL_CHARS)
        .collect();
    format!("- [{}]({}.md) — {}", label.trim(), entry.id, entry.kind)
}

/// An index linking every entry.
#[must_use]
pub fn default_index(entries: &[GlobalMemory]) -> String {
    let mut out = INDEX_HEAD.to_owned();
    if !entries.is_empty() {
        out.push('\n');
    }
    for e in entries {
        out.push_str(&index_line(e));
        out.push('\n');
    }
    out
}

/// The entry id a line links, if it is an index line.
#[must_use]
pub fn linked_id(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("- [")?;
    let (_, after) = rest.split_once("](")?;
    let (target, _) = after.split_once(".md)")?;
    (!target.is_empty()
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(target)
}

/// `index` with `entry`'s line replaced, or appended when absent.
#[must_use]
pub fn with_line(index: &str, entry: &GlobalMemory) -> String {
    let line = index_line(entry);
    let mut found = false;
    let mut out: Vec<String> = index
        .lines()
        .map(|l| {
            if linked_id(l) == Some(entry.id.as_str()) {
                found = true;
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        out.push(line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// `index` without the line linking `id`.
#[must_use]
pub fn without_line(index: &str, id: &str) -> String {
    let mut s: String = index
        .lines()
        .filter(|l| linked_id(l) != Some(id))
        .collect::<Vec<_>>()
        .join("\n");
    s.push('\n');
    s
}

/// The entries that reach the prompt: linked from the index (all of them
/// when there is none), minus profile entries when the profile hook is off.
/// Empty when memory is off.
#[must_use]
pub fn active<'a>(
    config: &MemoryConfig,
    index: Option<&str>,
    entries: &'a [GlobalMemory],
) -> Vec<&'a GlobalMemory> {
    if !config.enabled {
        return Vec::new();
    }
    let linked: Option<Vec<&str>> = index.map(|i| i.lines().filter_map(linked_id).collect());
    entries
        .iter()
        .filter(|e| linked.as_ref().is_none_or(|l| l.contains(&e.id.as_str())))
        .filter(|e| config.profile_enabled || e.kind != "profile")
        .collect()
}

/// Prompt guidance for the PRE/POST hooks that are on.
#[must_use]
pub fn hook_instructions(config: &MemoryConfig, has_rules: bool) -> Option<String> {
    if !config.enabled {
        return None;
    }
    let mut parts = Vec::new();
    if config.rewrite_enabled {
        parts.push("- Rewrite short asks: when the request is only a few words, first expand it into a full task brief using the personal memory (audience, tone, brand, constraints), then work from that brief. Say in one line what you assumed.");
    }
    if config.verify_enabled && has_rules {
        parts.push("- Self-verify: before you finish, check the result against every rule in the personal memory and end with a short scorecard, one line per rule: pass, fail (and what you fixed) or not applicable.");
    }
    (!parts.is_empty()).then(|| format!("## Memory hooks\n\n{}", parts.join("\n")))
}

/// A fact proposed by an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub kind: String,
    pub text: String,
}

/// Facts in an imported Markdown file: a memory file with frontmatter
/// (`type:` and a body), or the bullets of a list such as a MEMORY.md,
/// CLAUDE.md or notes file. `kind` comes from the frontmatter `type`, from
/// a `## User` / `## Rules`… heading above the bullets, else `user`.
#[must_use]
pub fn parse_import(text: &str) -> Vec<Suggestion> {
    let text = text.replace("\r\n", "\n");
    let (front_kind, body) = split_frontmatter(&text);
    let mut kind = front_kind.unwrap_or_else(|| "user".to_owned());
    let mut out: Vec<Suggestion> = Vec::new();
    let mut push = |kind: &str, fact: &str| {
        let fact = fact.trim();
        if fact.is_empty()
            || fact.chars().count() > crate::manage::MAX_GLOBAL_MEMORY_CHARS
            || out.iter().any(|s| s.text == fact)
            || out.len() >= MAX_IMPORT
        {
            return;
        }
        out.push(Suggestion {
            kind: kind.to_owned(),
            text: fact.to_owned(),
        });
    };
    let mut bullets = 0usize;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix('#') {
            if let Some(k) = kind_of_heading(heading.trim_start_matches('#').trim()) {
                kind = k.to_owned();
            }
            continue;
        }
        let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        else {
            continue;
        };
        bullets += 1;
        push(&kind, &bullet_text(item));
    }
    // A memory file without bullets: its body is the fact.
    if bullets == 0 && front_kind_present(&text) {
        let fact: String = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ");
        push(&kind, &fact);
    }
    out
}

fn front_kind_present(text: &str) -> bool {
    split_frontmatter(text).0.is_some()
}

/// The `type:` of a frontmatter block, and the text after it.
fn split_frontmatter(text: &str) -> (Option<String>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (None, text);
    };
    let Some((head, body)) = rest.split_once("\n---") else {
        return (None, text);
    };
    let kind = head.lines().find_map(|l| {
        let v = l.strip_prefix("type:")?.trim();
        crate::manage::MEMORY_KINDS
            .contains(&v)
            .then(|| v.to_owned())
    });
    (Some(kind.unwrap_or_else(|| "user".to_owned())), body)
}

fn kind_of_heading(heading: &str) -> Option<&'static str> {
    let h = heading.to_lowercase();
    [
        ("profile", "profile"),
        ("user", "user"),
        ("feedback", "feedback"),
        ("project", "project"),
        ("reference", "reference"),
        ("rule", "rule"),
    ]
    .into_iter()
    .find(|(word, _)| h.contains(word))
    .map(|(_, k)| k)
}

/// The fact of a bullet: an index link becomes its label, bold names lose
/// their stars.
fn bullet_text(item: &str) -> String {
    let item = item.trim();
    if let Some(rest) = item.strip_prefix('[') {
        if let Some((label, _)) = rest.split_once("](") {
            return label.trim().to_owned();
        }
    }
    item.replace("**", "")
}

/// Lead-ins of a stated preference ("I prefer…", "always…"), lowercase.
const PREFERENCE_LEAD_INS: [&str; 6] = [
    "i prefer ",
    "i always ",
    "please always ",
    "always ",
    "never ",
    "tôi thích ",
];

/// What "Learn from chats" keeps from a message: a "remember…" fact
/// (`user`), or a stated preference such as "I prefer serif headings" or
/// "never use emoji" (`feedback`). Only the first line counts.
#[must_use]
pub fn learned_from_message(message: &str) -> Option<Suggestion> {
    if let Some(fact) = crate::memory::memory_from_message(message) {
        return Some(Suggestion {
            kind: "user".to_owned(),
            text: fact,
        });
    }
    let first = message.trim().lines().next()?.trim();
    let lower = first.to_lowercase();
    PREFERENCE_LEAD_INS
        .iter()
        .any(|lead| lower.starts_with(lead))
        .then(|| first.trim_end_matches(['.', '!']).trim())
        .filter(|f| f.split_whitespace().count() >= 3)
        .filter(|f| f.chars().count() <= crate::manage::MAX_GLOBAL_MEMORY_CHARS)
        .map(|f| Suggestion {
            kind: "feedback".to_owned(),
            text: f.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, kind: &str, text: &str) -> GlobalMemory {
        GlobalMemory {
            id: id.to_owned(),
            kind: kind.to_owned(),
            text: text.to_owned(),
            created_at: 0,
        }
    }

    #[test]
    fn concurrent_links_all_land_in_the_index() {
        let tmp = tempfile::tempdir().unwrap();
        let files = MemoryFiles::in_data_dir(tmp.path());
        files.write_index("# Memory\n").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let (files, barrier) = (files.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..5)
                        .map(|j| files.link(&entry(&format!("e{i}-{j}"), "user", "fact")))
                        .collect::<std::io::Result<Vec<()>>>()
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap().unwrap();
        }
        let index = files.stored_index().unwrap();
        let linked = index.lines().filter_map(linked_id).count();
        assert_eq!(linked, 40, "{index}");
    }

    #[test]
    fn the_index_links_entries_and_only_linked_ones_are_active() {
        let entries = vec![
            entry("a1", "user", "Prefers [dark] mode"),
            entry("b2", "rule", "No emoji icons"),
        ];
        let index = default_index(&entries);
        assert!(
            index.contains("- [Prefers  dark  mode](a1.md) — user"),
            "{index}"
        );
        let config = MemoryConfig::default();
        assert_eq!(active(&config, Some(&index), &entries).len(), 2);
        let trimmed = without_line(&index, "a1");
        let ids: Vec<&str> = active(&config, Some(&trimmed), &entries)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, ["b2"]);
        // No index at all: everything is active.
        assert_eq!(active(&config, None, &entries).len(), 2);
    }

    #[test]
    fn the_master_switch_and_the_profile_hook_gate_injection() {
        let entries = vec![
            entry("p", "profile", "Role: designer"),
            entry("u", "user", "x"),
        ];
        let off = MemoryConfig {
            enabled: false,
            ..MemoryConfig::default()
        };
        assert!(active(&off, None, &entries).is_empty());
        assert!(hook_instructions(&off, true).is_none());
        let no_profile = MemoryConfig {
            profile_enabled: false,
            ..MemoryConfig::default()
        };
        let ids: Vec<&str> = active(&no_profile, None, &entries)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, ["u"]);
    }

    #[test]
    fn with_line_replaces_or_appends() {
        let e = entry("a1", "user", "one");
        let index = with_line("# Memory\n", &e);
        assert_eq!(index, "# Memory\n- [one](a1.md) — user\n");
        let renamed = entry("a1", "rule", "two");
        assert_eq!(
            with_line(&index, &renamed),
            "# Memory\n- [two](a1.md) — rule\n"
        );
    }

    #[test]
    fn linked_id_reads_only_index_lines() {
        assert_eq!(linked_id("  - [x](ab_1.md) — user"), Some("ab_1"));
        assert_eq!(linked_id("- [x](../etc.md)"), None);
        assert_eq!(linked_id("plain text"), None);
    }

    #[test]
    fn hooks_add_guidance_only_when_on() {
        let c = MemoryConfig::default();
        let text = hook_instructions(&c, true).unwrap_or_default();
        assert!(text.contains("Rewrite short asks") && text.contains("Self-verify"));
        // Verify needs rules to check against.
        assert!(
            !hook_instructions(&c, false)
                .unwrap_or_default()
                .contains("Self-verify")
        );
        let none = MemoryConfig {
            rewrite_enabled: false,
            verify_enabled: false,
            ..c
        };
        assert!(hook_instructions(&none, true).is_none());
    }

    #[test]
    fn import_reads_bullets_headings_and_memory_files() {
        let list = "# Notes\n\n## Rules\n- **No** emoji icons\n- [Teal brand](x.md) — user\n\n## Profile\n* Role: designer\n- No emoji icons\n";
        assert_eq!(
            parse_import(list),
            vec![
                Suggestion {
                    kind: "rule".into(),
                    text: "No emoji icons".into()
                },
                Suggestion {
                    kind: "rule".into(),
                    text: "Teal brand".into()
                },
                Suggestion {
                    kind: "profile".into(),
                    text: "Role: designer".into()
                },
            ]
        );
        let file =
            "---\nname: tone\ntype: feedback\n---\n\nKeep copy short.\nNo exclamation marks.\n";
        assert_eq!(
            parse_import(file),
            vec![Suggestion {
                kind: "feedback".into(),
                text: "Keep copy short. No exclamation marks.".into()
            }]
        );
        assert!(parse_import("just prose, no list").is_empty());
    }

    #[test]
    fn chats_teach_remembered_facts_and_preferences() {
        assert_eq!(
            learned_from_message("Remember that the brand colour is teal"),
            Some(Suggestion {
                kind: "user".into(),
                text: "the brand colour is teal".into()
            })
        );
        assert_eq!(
            learned_from_message("I prefer serif headings.\nNow make a page"),
            Some(Suggestion {
                kind: "feedback".into(),
                text: "I prefer serif headings".into()
            })
        );
        assert_eq!(learned_from_message("Always"), None);
        assert_eq!(learned_from_message("Make a landing page"), None);
    }

    #[test]
    fn config_patch_changes_only_named_fields() {
        let c = MemoryConfig::default().apply(MemoryConfigPatch {
            chat_extraction_enabled: Some(true),
            ..MemoryConfigPatch::default()
        });
        assert!(c.chat_extraction_enabled && c.enabled && c.verify_enabled);
    }
}
