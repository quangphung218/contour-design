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
    refuse_case_twin(user_root, &id, "plugin")?;
    place_copy(source, user_root, &id)?;
    let dest = user_root.join(&id);
    load_plugin(&dest, &id, "user")
        .ok_or_else(|| WriteError::Io("the installed plugin did not load".to_owned()))
}

/// Refuses `id` when `user_root` holds a folder whose name differs from
/// it only in case or Unicode form. macOS and Windows treat `Foo` and
/// `foo` as one folder: installing `Foo` would delete the `foo` already
/// there (Linux would keep both, which collide once synced). The same id
/// is no clash: reinstalling replaces it.
fn refuse_case_twin(user_root: &Path, id: &str, what: &str) -> Result<(), WriteError> {
    std::fs::create_dir_all(user_root).map_err(io)?;
    let key = od_contracts::name_key(id);
    for entry in std::fs::read_dir(user_root).map_err(io)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != id && od_contracts::name_key(&name) == key {
            return Err(WriteError::Exists(format!("{what} {name:?}")));
        }
    }
    Ok(())
}

/// Copies `source` into `<user_root>/<id>`, replacing an older copy: built
/// in a hidden folder, then renamed into place. Nothing is left behind on
/// failure.
fn place_copy(source: &Path, user_root: &Path, id: &str) -> Result<(), WriteError> {
    let tmp = user_root.join(format!(".installing-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = copy_dir(source, &tmp, MAX_PLUGIN_BYTES) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let dest = user_root.join(id);
    if dest.exists()
        && let Err(e) = std::fs::remove_dir_all(&dest)
    {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(io(e));
    }
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        io(e)
    })
}

/// Installs the skill in `source` (a local folder with a `SKILL.md` that
/// has front matter) as `<user_root>/<id>`, side files (`assets/`…)
/// included, replacing a user skill with the same id. The id is the
/// front matter's `name`, else the folder name, as the index reads it.
/// `taken` says an id belongs to a bundled skill.
///
/// # Errors
/// [`WriteError::BadRequest`] for a folder that is not a skill or is too
/// large, [`WriteError::Exists`] for a bundled id or a user skill whose
/// name differs only in case, [`WriteError::Io`].
pub fn install_skill(
    source: &Path,
    user_root: &Path,
    taken: impl Fn(&str) -> bool,
) -> Result<String, WriteError> {
    if !source.is_dir() {
        return Err(WriteError::BadRequest(format!(
            "{} is not a folder",
            source.display()
        )));
    }
    let skill_md = source.join("SKILL.md");
    // A link is refused here: `copy_dir` skips links, so the copy would
    // have no SKILL.md.
    let raw = match std::fs::symlink_metadata(&skill_md) {
        Ok(m) if m.is_file() => std::fs::read_to_string(&skill_md)
            .map_err(|e| WriteError::BadRequest(format!("SKILL.md cannot be read: {e}")))?,
        _ => {
            return Err(WriteError::BadRequest(
                "the folder has no SKILL.md".to_owned(),
            ));
        }
    };
    let front = skill_front_matter(&raw)?;
    let id = match front.get("name") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
        _ => source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if !crate::design_systems::is_valid_id(&id) || id.starts_with(['.', '_']) {
        return Err(WriteError::BadRequest(format!(
            "invalid skill id {id:?}: use letters, digits and -"
        )));
    }
    if taken(&id) {
        return Err(WriteError::Exists(format!("skill {id:?}")));
    }
    refuse_case_twin(user_root, &id, "skill")?;
    place_copy(source, user_root, &id)?;
    Ok(id)
}

/// The front matter of a `SKILL.md`; text without one, or with one that
/// does not close or parse, is refused (the index would skip it or read
/// it all as body).
fn skill_front_matter(raw: &str) -> Result<serde_json::Map<String, serde_json::Value>, WriteError> {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let (front, body) = frontmatter::parse(text)
        .map_err(|e| WriteError::BadRequest(format!("SKILL.md front matter: {e}")))?;
    // No block, or one that does not close, parses as all body.
    if front.is_empty() && body.len() == text.len() {
        return Err(WriteError::BadRequest(
            "SKILL.md needs front matter between --- lines".to_owned(),
        ));
    }
    Ok(front)
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
        let id = create_skill(tmp.path(), &draft("Brand Voice", "Write warmly."), |_| {
            false
        })
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
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: tea-plugin\n---\nDo tea.\n",
        )
        .unwrap();
        let user = tmp.path().join("plugins");
        let p = install_plugin(&src, &user).unwrap();
        assert_eq!(p.id, "tea-plugin");
        assert_eq!(p.source, "user");
        assert!(user.join("tea-plugin/SKILL.md").is_file());
        assert!(
            !user.join("tea-plugin/.git").exists(),
            "hidden folders are not copied"
        );
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: tea-plugin\n---\nDo more tea.\n",
        )
        .unwrap();
        let p = install_plugin(&src, &user).unwrap();
        assert_eq!(p.body, "Do more tea.");
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(matches!(
            install_plugin(&empty, &user),
            Err(WriteError::BadRequest(_))
        ));
    }

    #[test]
    fn install_refuses_an_id_that_differs_only_in_case() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("plugins");
        let installed = user.join("tea-plugin");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(
            installed.join("SKILL.md"),
            "---\nname: tea-plugin\n---\nOld.\n",
        )
        .unwrap();
        let src = tmp.path().join("src/Tea");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "---\nname: Tea-Plugin\n---\nNew.\n").unwrap();
        // macOS and Windows see one folder; installing must not delete the
        // plugin already there, and Linux must not end up with two.
        assert!(matches!(
            install_plugin(&src, &user),
            Err(WriteError::Exists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(installed.join("SKILL.md")).unwrap(),
            "---\nname: tea-plugin\n---\nOld.\n"
        );
        assert_eq!(std::fs::read_dir(&user).unwrap().count(), 1);
    }

    fn skill_dir(root: &Path, folder: &str, skill_md: &str) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("SKILL.md"), skill_md).unwrap();
        std::fs::write(dir.join("assets/tokens.css"), ":root{--accent:#2f6b3a}").unwrap();
        dir
    }

    /// Names in `root`, hidden ones included (a leftover `.installing-*`).
    fn names(root: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn install_skill_copies_side_files_and_replaces_its_own_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let src = skill_dir(
            &tmp.path().join("src"),
            "folder-name",
            "---\nname: tea-house\ndescription: Calm tea pages.\n---\n\nUse cream.\n",
        );
        std::fs::create_dir_all(src.join(".git")).unwrap();
        let user = tmp.path().join("skills");
        assert_eq!(install_skill(&src, &user, |_| false).unwrap(), "tea-house");
        assert!(user.join("tea-house/assets/tokens.css").is_file());
        assert!(!user.join("tea-house/.git").exists());
        // The index reads it back under the same id, as a user skill.
        let listed = crate::skills::list_skills(std::slice::from_ref(&user), true);
        assert_eq!(listed[0].info.id, "tea-house");
        // Saving again replaces the copy; a file gone from the source goes.
        std::fs::remove_file(src.join("assets/tokens.css")).unwrap();
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: tea-house\n---\n\nUse green.\n",
        )
        .unwrap();
        install_skill(&src, &user, |_| false).unwrap();
        let text = std::fs::read_to_string(user.join("tea-house/SKILL.md")).unwrap();
        assert!(text.contains("Use green."));
        assert!(!user.join("tea-house/assets/tokens.css").exists());
        assert_eq!(names(&user), ["tea-house"]);
    }

    #[test]
    fn install_skill_takes_the_folder_name_without_a_name() {
        let tmp = tempfile::tempdir().unwrap();
        let src = skill_dir(
            tmp.path(),
            "brand-voice",
            "---\ndescription: x\n---\nBody.\n",
        );
        let user = tmp.path().join("skills");
        assert_eq!(
            install_skill(&src, &user, |_| false).unwrap(),
            "brand-voice"
        );
    }

    #[test]
    fn install_skill_refuses_what_is_not_a_skill_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("skills");
        let bad = |dir: &Path| {
            let res = install_skill(dir, &user, |_| false);
            assert!(
                matches!(res, Err(WriteError::BadRequest(_))),
                "{dir:?}: {res:?}"
            );
        };
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        bad(&empty);
        bad(&tmp.path().join("missing"));
        bad(&skill_dir(tmp.path(), "no-front", "Just a body.\n"));
        bad(&skill_dir(
            tmp.path(),
            "unclosed",
            "---\nname: unclosed\nBody.\n",
        ));
        bad(&skill_dir(
            tmp.path(),
            "spaced",
            "---\nname: Brand Voice\n---\nBody.\n",
        ));
        bad(&skill_dir(
            tmp.path(),
            "hidden",
            "---\nname: .hidden\n---\nBody.\n",
        ));
        let big = skill_dir(tmp.path(), "big", "---\nname: big\n---\nBody.\n");
        let f = std::fs::File::create(big.join("assets/huge.bin")).unwrap();
        f.set_len(MAX_PLUGIN_BYTES + 1).unwrap();
        bad(&big);
        assert!(names(&user).is_empty(), "left behind: {:?}", names(&user));
    }

    #[cfg(unix)]
    #[test]
    fn install_skill_refuses_a_skill_md_that_is_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside.md");
        std::fs::write(&outside, "---\nname: linked\n---\nBody.\n").unwrap();
        let src = tmp.path().join("linked");
        std::fs::create_dir_all(&src).unwrap();
        std::os::unix::fs::symlink(&outside, src.join("SKILL.md")).unwrap();
        let user = tmp.path().join("skills");
        assert!(matches!(
            install_skill(&src, &user, |_| false),
            Err(WriteError::BadRequest(_))
        ));
        assert!(!user.join("linked").exists());
    }

    #[test]
    fn install_skill_refuses_a_bundled_id_and_a_case_twin() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("skills");
        let deck = skill_dir(
            &tmp.path().join("src"),
            "deck",
            "---\nname: deck\n---\nB.\n",
        );
        assert!(matches!(
            install_skill(&deck, &user, |id| id == "deck"),
            Err(WriteError::Exists(_))
        ));
        let mine = skill_dir(&tmp.path().join("a"), "foo", "---\nname: foo\n---\nMine.\n");
        install_skill(&mine, &user, |_| false).unwrap();
        let twin = skill_dir(&tmp.path().join("b"), "Foo", "---\nname: Foo\n---\nTwin.\n");
        assert!(matches!(
            install_skill(&twin, &user, |_| false),
            Err(WriteError::Exists(_))
        ));
        assert_eq!(names(&user), ["foo"]);
        let text = std::fs::read_to_string(user.join("foo/SKILL.md")).unwrap();
        assert!(text.contains("Mine."));
    }
}
