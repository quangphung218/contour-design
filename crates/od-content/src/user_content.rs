//! Writing user content (spec 1.4 "Thư viện skill", "Plugin"): skills the
//! user creates, edits or overrides under `<data>/skills`, and plugins
//! installed from a local folder into `<data>/plugins`. The index reloads
//! afterwards; user roots shadow bundled ones by id.

use crate::frontmatter;
use crate::plugins::{PluginEntry, load_plugin};
use std::path::{Path, PathBuf};

/// Longest skill folder name (the original caps slugs at 64).
const MAX_SLUG: usize = 64;
const MAX_NAME_CHARS: usize = 80;
const MAX_BODY_BYTES: usize = 512 * 1024;
/// Largest plugin folder copied on install.
pub const MAX_PLUGIN_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0} already exists")]
    Exists(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
}

fn io(e: std::io::Error) -> WriteError {
    WriteError::Io(e.to_string())
}

/// What the skill editor sends.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillDraft {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub body: String,
}

impl SkillDraft {
    fn check(&self) -> Result<(), WriteError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(WriteError::BadRequest(format!(
                "name must be 1–{MAX_NAME_CHARS} characters"
            )));
        }
        if self.body.trim().is_empty() {
            return Err(WriteError::BadRequest("body is required".to_owned()));
        }
        if self.body.len() > MAX_BODY_BYTES {
            return Err(WriteError::BadRequest("body is too large".to_owned()));
        }
        Ok(())
    }
}

/// Folder name for a skill: lowercase, anything outside `[a-z0-9-_]`
/// becomes `-`, at most 64 characters (the original rule).
#[must_use]
pub fn skill_slug(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let slug: String = slug.chars().take(MAX_SLUG).collect();
    slug.trim_end_matches('-').to_owned()
}

/// A YAML double-quoted scalar (JSON strings are valid YAML).
fn quoted(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// Top-level keys the editor owns; every other front-matter line is kept.
const OWNED_KEYS: [&str; 3] = ["name", "description", "triggers"];

/// The front matter lines of `raw` minus the owned keys and their
/// indented continuation lines.
fn kept_front_lines(raw: &str) -> Vec<String> {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut skipping = false;
    for line in rest.lines() {
        if line.trim_end() == "---" {
            break;
        }
        let top_level = !line.starts_with([' ', '\t']) && !line.trim().is_empty();
        if top_level {
            let key = line.split(':').next().unwrap_or("").trim();
            skipping = OWNED_KEYS.contains(&key);
        }
        if !skipping {
            out.push(line.to_owned());
        }
    }
    out
}

/// `SKILL.md` text for `draft`, keeping the other front-matter keys of
/// `previous` (an override keeps the bundled `od:` block).
#[must_use]
pub fn render_skill_md(draft: &SkillDraft, id: &str, previous: Option<&str>) -> String {
    let mut lines = vec!["---".to_owned(), format!("name: {}", quoted(id))];
    let description = draft.description.trim();
    if !description.is_empty() {
        lines.push("description: |".to_owned());
        lines.extend(description.lines().map(|l| format!("  {l}")));
    }
    let triggers: Vec<&str> = draft
        .triggers
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if !triggers.is_empty() {
        lines.push("triggers:".to_owned());
        lines.extend(triggers.iter().map(|t| format!("  - {}", quoted(t))));
    }
    if let Some(prev) = previous {
        lines.extend(kept_front_lines(prev));
    }
    lines.push("---".to_owned());
    format!("{}\n\n{}\n", lines.join("\n"), draft.body.trim())
}

/// Creates `<user_root>/<slug>/SKILL.md`; refuses a folder that exists or
/// an id a bundled skill already has (`taken`).
///
/// # Errors
/// [`WriteError::BadRequest`], [`WriteError::Exists`], [`WriteError::Io`].
pub fn create_skill(
    user_root: &Path,
    draft: &SkillDraft,
    taken: impl Fn(&str) -> bool,
) -> Result<String, WriteError> {
    draft.check()?;
    let slug = skill_slug(&draft.name);
    if slug.is_empty() {
        return Err(WriteError::BadRequest(
            "name needs at least one letter or digit".to_owned(),
        ));
    }
    if taken(&slug) {
        return Err(WriteError::Exists(format!("skill {slug:?}")));
    }
    std::fs::create_dir_all(user_root).map_err(io)?;
    let dir = user_root.join(&slug);
    std::fs::create_dir(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => WriteError::Exists(format!("skill {slug:?}")),
        _ => io(e),
    })?;
    std::fs::write(dir.join("SKILL.md"), render_skill_md(draft, &slug, None)).map_err(io)?;
    Ok(slug)
}

/// Rewrites skill `id`. A user skill is edited in place; a bundled one
/// gets a user override in `<user_root>/<id>` that first copies the
/// bundled folder's side files (assets, references…).
///
/// # Errors
/// [`WriteError::BadRequest`], [`WriteError::Io`].
pub fn save_skill(
    user_root: &Path,
    id: &str,
    current_dir: &Path,
    is_user: bool,
    draft: &SkillDraft,
) -> Result<PathBuf, WriteError> {
    draft.check()?;
    if !crate::design_systems::is_valid_id(id) {
        return Err(WriteError::BadRequest(format!("invalid skill id {id:?}")));
    }
    let previous = std::fs::read_to_string(current_dir.join("SKILL.md")).ok();
    let dir = if is_user {
        current_dir.to_path_buf()
    } else {
        let dir = user_root.join(id);
        if !dir.exists() {
            copy_dir(current_dir, &dir, u64::MAX)?;
        }
        dir
    };
    let text = render_skill_md(draft, id, previous.as_deref());
    // Validate before writing: a broken file would drop the skill.
    frontmatter::parse(&text).map_err(|e| WriteError::BadRequest(e.to_string()))?;
    std::fs::write(dir.join("SKILL.md"), text).map_err(io)?;
    Ok(dir)
}

/// Removes a user skill or override folder (never a bundled one).
///
/// # Errors
/// [`WriteError::NotFound`] outside `user_root`, [`WriteError::Io`].
pub fn delete_user_dir(user_root: &Path, dir: &Path) -> Result<(), WriteError> {
    let root = user_root.canonicalize().map_err(io)?;
    let dir = dir.canonicalize().map_err(io)?;
    if dir.parent() != Some(root.as_path()) {
        return Err(WriteError::NotFound(
            "only user content can be deleted".to_owned(),
        ));
    }
    std::fs::remove_dir_all(dir).map_err(io)
}

/// Copies `from` into a new folder `to` (files and folders only; links
/// and hidden entries are skipped). Fails past `budget` bytes.
///
/// # Errors
/// [`WriteError::BadRequest`] when too large, [`WriteError::Io`].
pub fn copy_dir(from: &Path, to: &Path, budget: u64) -> Result<u64, WriteError> {
    std::fs::create_dir_all(to).map_err(io)?;
    let mut used = 0u64;
    for entry in std::fs::read_dir(from).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') && name != ".claude-plugin" {
            continue;
        }
        let kind = entry.file_type().map_err(io)?;
        let dest = to.join(&name);
        if kind.is_dir() {
            used += copy_dir(&entry.path(), &dest, budget.saturating_sub(used))?;
        } else if kind.is_file() {
            used += entry.metadata().map_err(io)?.len();
            if used > budget {
                return Err(WriteError::BadRequest(
                    "the folder is larger than 50 MB".to_owned(),
                ));
            }
            std::fs::copy(entry.path(), dest).map_err(io)?;
        }
    }
    Ok(used)
}

/// Installs the plugin in `source` (a local folder with `SKILL.md` or
/// `open-design.json`) as `<user_root>/<id>`, replacing an older copy.
/// The copy is built in a hidden folder and renamed into place.
///
/// # Errors
/// [`WriteError::BadRequest`] for a folder that is not a plugin or is too
/// large, [`WriteError::Io`].
pub fn install_plugin(source: &Path, user_root: &Path) -> Result<PluginEntry, WriteError> {
    if !source.is_dir() {
        return Err(WriteError::BadRequest(format!(
            "{} is not a folder",
            source.display()
        )));
    }
    let folder = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let probe = load_plugin(source, &folder, "user").ok_or_else(|| {
        WriteError::BadRequest("the folder has no SKILL.md or open-design.json".to_owned())
    })?;
    let id = probe.id.clone();
    if !crate::design_systems::is_valid_id(&id) || id.starts_with(['.', '_']) {
        return Err(WriteError::BadRequest(format!("invalid plugin id {id:?}")));
    }
    std::fs::create_dir_all(user_root).map_err(io)?;
    let tmp = user_root.join(format!(".installing-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = copy_dir(source, &tmp, MAX_PLUGIN_BYTES) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let dest = user_root.join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(io)?;
    }
    std::fs::rename(&tmp, &dest).map_err(io)?;
    load_plugin(&dest, &id, "user")
        .ok_or_else(|| WriteError::Io("the installed plugin did not load".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str, body: &str) -> SkillDraft {
        SkillDraft {
            name: name.to_owned(),
            description: "Line one.\nLine two.".to_owned(),
            triggers: vec!["brand".to_owned(), " ".to_owned()],
            body: body.to_owned(),
        }
    }

    #[test]
    fn slug_follows_the_original_rule() {
        assert_eq!(skill_slug("My Brand Voice!"), "my-brand-voice");
        assert_eq!(skill_slug("  a_b-c "), "a_b-c");
        assert_eq!(skill_slug("***"), "");
        assert_eq!(skill_slug(&"x".repeat(90)).len(), 64);
    }

    #[test]
    fn create_writes_a_parseable_skill_and_refuses_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let id = create_skill(tmp.path(), &draft("Brand Voice", "Write warmly."), |_| false)
            .unwrap();
        assert_eq!(id, "brand-voice");
        let text = std::fs::read_to_string(tmp.path().join("brand-voice/SKILL.md")).unwrap();
        let (front, body) = frontmatter::parse(&text).unwrap();
        assert_eq!(front["name"], "brand-voice");
        assert_eq!(front["description"], "Line one.\nLine two.");
        assert_eq!(front["triggers"], serde_json::json!(["brand"]));
        assert_eq!(body.trim(), "Write warmly.");
        let again = create_skill(tmp.path(), &draft("Brand Voice", "x"), |_| false);
        assert!(matches!(again, Err(WriteError::Exists(_))));
        let bundled = create_skill(tmp.path(), &draft("Taken", "x"), |id| id == "taken");
        assert!(matches!(bundled, Err(WriteError::Exists(_))));
        let empty = create_skill(tmp.path(), &draft("Ok", "  "), |_| false);
        assert!(matches!(empty, Err(WriteError::BadRequest(_))));
    }

    #[test]
    fn override_keeps_od_metadata_and_side_files() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled/deck");
        std::fs::create_dir_all(bundled.join("assets")).unwrap();
        std::fs::write(
            bundled.join("SKILL.md"),
            "---\nname: deck\ndescription: |\n  Old.\ntriggers:\n  - slides\nod:\n  mode: deck\n  scenario: sales\n---\n\nOld body.\n",
        )
        .unwrap();
        std::fs::write(bundled.join("assets/a.css"), "x").unwrap();
        let user = tmp.path().join("user");
        let dir = save_skill(&user, "deck", &bundled, false, &draft("deck", "New body.")).unwrap();
        assert_eq!(dir, user.join("deck"));
        assert!(user.join("deck/assets/a.css").is_file());
        let text = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        let (front, body) = frontmatter::parse(&text).unwrap();
        assert_eq!(front["od"]["mode"], "deck");
        assert_eq!(front["description"], "Line one.\nLine two.");
        assert_eq!(body.trim(), "New body.");
        // The bundled folder is untouched.
        let old = std::fs::read_to_string(bundled.join("SKILL.md")).unwrap();
        assert!(old.contains("Old body."));
        // Deleting the override only works inside the user root.
        assert!(delete_user_dir(&user, &bundled).is_err());
        delete_user_dir(&user, &dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn install_copies_a_plugin_folder_and_replaces_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src/my-plugin");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/HEAD"), "x").unwrap();
        std::fs::write(
            src.join("open-design.json"),
            r#"{"name":"tea-plugin","title":"Tea","version":"1.0.0","od":{"kind":"scenario"}}"#,
        )
        .unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: tea-plugin\n---\nDo tea.\n").unwrap();
        let user = tmp.path().join("plugins");
        let p = install_plugin(&src, &user).unwrap();
        assert_eq!(p.id, "tea-plugin");
        assert_eq!(p.source, "user");
        assert!(user.join("tea-plugin/SKILL.md").is_file());
        assert!(!user.join("tea-plugin/.git").exists(), "hidden folders are not copied");
        std::fs::write(src.join("SKILL.md"), "---\nname: tea-plugin\n---\nDo more tea.\n").unwrap();
        let p = install_plugin(&src, &user).unwrap();
        assert_eq!(p.body, "Do more tea.");
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(matches!(
            install_plugin(&empty, &user),
            Err(WriteError::BadRequest(_))
        ));
    }
}
