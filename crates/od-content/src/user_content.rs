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
    /// The skill is no longer what the user reviewed ("Copy to Claude
    /// Code", [`export_skill`]).
    #[error("{0}")]
    Changed(String),
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

/// Whether `user_root` holds a skill folder named `id`, in any case or
/// Unicode form (one folder on macOS and Windows). Hidden folders (an
/// install in progress) do not count.
#[must_use]
pub fn user_skill_exists(user_root: &Path, id: &str) -> bool {
    let key = od_contracts::name_key(id);
    std::fs::read_dir(user_root).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            !name.starts_with('.') && e.path().is_dir() && od_contracts::name_key(&name) == key
        })
    })
}

/// The first entry of `dir` whose name is `id` in any case or Unicode
/// form, of any kind (folder, file, link, broken link); hidden entries
/// (a copy in progress) do not count.
fn entry_like(dir: &Path, id: &str) -> Result<Option<String>, WriteError> {
    let key = od_contracts::name_key(id);
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
    };
    Ok(entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|name| !name.starts_with('.') && od_contracts::name_key(name) == key))
}

/// One export into an agent's skills at a time in this process.
static EXPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The file a copy into an agent's skills holds until its `SKILL.md` is
/// there: a folder with it and no `SKILL.md` is a copy of Contour's that
/// was cut short (the app quit), which the next copy replaces. The next
/// copy removes that whole folder, with anything put in it since (a file
/// the user added to a copy cut short goes too): only Contour writes the
/// mark, and such a folder is no skill for Claude Code.
pub const EXPORT_MARK: &str = ".contour-copy";

/// What stands at a skill's name in an agent's skills already.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingEntry {
    /// Its name on disk (`TEA` for the skill `tea`).
    pub name: String,
    pub path: PathBuf,
    pub kind: ExistingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExistingKind {
    /// A folder with a `SKILL.md`: a skill.
    Skill,
    /// Contour's own copy, cut short ([`EXPORT_MARK`], no `SKILL.md`).
    Unfinished,
    /// Anything else: a file, a link, a folder with no `SKILL.md`.
    Other,
}

/// The entry of `dest_root` at the name `id` (any case or Unicode form),
/// as [`export_skill`] sees it. Read only.
///
/// # Errors
/// [`WriteError::Io`] when `dest_root` cannot be read.
pub fn existing_export(dest_root: &Path, id: &str) -> Result<Option<ExistingEntry>, WriteError> {
    let Some(name) = entry_like(dest_root, id)? else {
        return Ok(None);
    };
    let path = dest_root.join(&name);
    let is = |p: &Path| std::fs::symlink_metadata(p).is_ok();
    let kind = match std::fs::symlink_metadata(&path) {
        Ok(m) if m.is_dir() && is(&path.join("SKILL.md")) => ExistingKind::Skill,
        Ok(m) if m.is_dir() && is(&path.join(EXPORT_MARK)) => ExistingKind::Unfinished,
        _ => ExistingKind::Other,
    };
    Ok(Some(ExistingEntry { name, path, kind }))
}

/// What "Copy to Claude Code" checks before it copies a skill. Warnings
/// only: the user decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportCheck {
    /// The skill calls Contour's own tools (`od media`, `$OD_BIN`, the
    /// daemon's API), in its `SKILL.md` (front matter too) or a file under
    /// `scripts/`: it may not work outside Contour.
    pub contour_tools: bool,
    /// Its front matter has a `description`. Without one Claude Code still
    /// loads the skill and shows its model the first line of the body
    /// (checked with Claude Code 2.1.332).
    pub has_description: bool,
}

/// Most script files read, and largest one, for [`ExportFiles::check`].
const MAX_CHECKED_SCRIPTS: usize = 500;
const MAX_CHECKED_SCRIPT_BYTES: u64 = 1024 * 1024;

/// A skill folder as "Copy to Claude Code" copies it, read into memory
/// once: the preview shows it and names it by [`ExportFiles::hash`], and
/// the copy writes these very bytes after it compared the hash the user
/// saw, so nothing changed between the two is written (an agent can edit a
/// Personal skill over loopback while the user reads the preview).
///
/// What is read: folders and files, not links; no hidden entry at the top
/// (Claude Code would load a hidden folder there as a skill), and below it
/// only `.claude-plugin`, as [`copy_dir`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFiles {
    /// Folders under the skill folder, parents first.
    dirs: Vec<PathBuf>,
    /// Files and their bytes, by path.
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl ExportFiles {
    /// Reads the skill folder `source` (at most [`MAX_PLUGIN_BYTES`]).
    ///
    /// # Errors
    /// [`WriteError::NotFound`] without a `SKILL.md` file,
    /// [`WriteError::BadRequest`] when too large, [`WriteError::Io`].
    pub fn read(source: &Path) -> Result<Self, WriteError> {
        let mut out = Self {
            dirs: Vec::new(),
            files: Vec::new(),
        };
        let mut used = 0u64;
        out.read_dir(source, Path::new(""), &mut used)?;
        out.dirs.sort();
        out.files.sort_by(|a, b| a.0.cmp(&b.0));
        if !out.files.iter().any(|(p, _)| p == Path::new("SKILL.md")) {
            return Err(WriteError::NotFound("the skill has no SKILL.md".to_owned()));
        }
        Ok(out)
    }

    fn read_dir(&mut self, dir: &Path, rel: &Path, used: &mut u64) -> Result<(), WriteError> {
        let top = rel.as_os_str().is_empty();
        for entry in std::fs::read_dir(dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') && (top || name != ".claude-plugin") {
                continue;
            }
            let kind = entry.file_type().map_err(io)?;
            let path = rel.join(&name);
            if kind.is_dir() {
                self.dirs.push(path.clone());
                self.read_dir(&entry.path(), &path, used)?;
            } else if kind.is_file() {
                *used += entry.metadata().map_err(io)?.len();
                if *used > MAX_PLUGIN_BYTES {
                    return Err(WriteError::BadRequest(
                        "the folder is larger than 50 MB".to_owned(),
                    ));
                }
                let bytes = std::fs::read(entry.path()).map_err(io)?;
                self.files.push((path, bytes));
            }
        }
        Ok(())
    }

    /// Names this content: every folder and file path and every file's
    /// bytes (sha256, hex).
    #[must_use]
    pub fn hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for d in &self.dirs {
            h.update(b"d\0");
            h.update(d.to_string_lossy().as_bytes());
            h.update(b"\0");
        }
        for (p, bytes) in &self.files {
            h.update(b"f\0");
            h.update(p.to_string_lossy().as_bytes());
            h.update(b"\0");
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(bytes);
        }
        hex::encode(h.finalize())
    }

    /// The files copied, by path under the skill folder (`/` between
    /// folders).
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|(p, _)| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect()
    }

    /// The `SKILL.md` as text.
    #[must_use]
    pub fn skill_md(&self) -> String {
        self.file(Path::new("SKILL.md"))
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    }

    fn file(&self, path: &Path) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, b)| b.as_slice())
    }

    /// [`ExportCheck`] of this content. The one place the warnings are
    /// decided: the preview the app shows and the copy read the same bytes.
    #[must_use]
    pub fn check(&self) -> ExportCheck {
        let raw = self.skill_md();
        let has_description = skill_front_matter(&raw).is_ok_and(|front| {
            front
                .get("description")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|d| !d.trim().is_empty())
        });
        let contour_tools = od_contracts::calls_contour_tools(&raw)
            || self
                .files
                .iter()
                .filter(|(p, b)| {
                    p.starts_with("scripts") && b.len() as u64 <= MAX_CHECKED_SCRIPT_BYTES
                })
                .take(MAX_CHECKED_SCRIPTS)
                .any(|(_, b)| od_contracts::calls_contour_tools(&String::from_utf8_lossy(b)));
        ExportCheck {
            contour_tools,
            has_description,
        }
    }

    /// Writes this content into the folder `to` (made already), the top
    /// `SKILL.md` last.
    fn write_into(&self, to: &Path) -> Result<(), WriteError> {
        for d in &self.dirs {
            std::fs::create_dir(to.join(d)).map_err(io)?;
        }
        let skill_md = Path::new("SKILL.md");
        for (p, bytes) in self.files.iter().filter(|(p, _)| p != skill_md) {
            std::fs::write(to.join(p), bytes).map_err(io)?;
        }
        if let Some(bytes) = self.file(skill_md) {
            std::fs::write(to.join(skill_md), bytes).map_err(io)?;
        }
        Ok(())
    }
}

/// Copies the user skill `id` in `source` (a folder right in `user_root`:
/// its `SKILL.md` and side files as [`ExportFiles`] reads them) into
/// `<dest_root>/<id>`, an agent CLI's skills (`~/.claude/skills`), if its
/// content is still the one the user reviewed: `hash` is the
/// [`ExportFiles::hash`] the preview showed, else [`WriteError::Changed`]
/// and nothing is written. The bytes compared are the bytes written.
///
/// Never writes over anything: an entry of `dest_root` named `id` in any
/// case or Unicode form, of any kind, is [`WriteError::Exists`] with its
/// name on disk, except a copy of Contour's cut short (see
/// [`EXPORT_MARK`]), which is replaced; the
/// folder is then made with `create_dir`, which fails if another program
/// made it meanwhile (also [`WriteError::Exists`]). Nothing hidden is
/// written in `dest_root` (Claude Code would load it as a skill), and the
/// `SKILL.md` is copied last: a copy cut short is no skill. On a failure
/// the folder made here is removed. Returns the folder written and the
/// content copied.
///
/// # Errors
/// [`WriteError::BadRequest`] for an invalid id or a skill too large,
/// [`WriteError::NotFound`] when `source` is no user skill folder (outside
/// `user_root`, a link, or its `SKILL.md` missing or a link),
/// [`WriteError::Changed`], [`WriteError::Exists`], [`WriteError::Io`].
pub fn export_skill(
    user_root: &Path,
    source: &Path,
    id: &str,
    dest_root: &Path,
    hash: &str,
) -> Result<(PathBuf, ExportFiles), WriteError> {
    export_skill_with(user_root, source, id, dest_root, hash, || {})
}

/// [`export_skill`], running `before_claim` right before the folder is
/// made: a test puts another program's folder there at that moment.
fn export_skill_with(
    user_root: &Path,
    source: &Path,
    id: &str,
    dest_root: &Path,
    hash: &str,
    before_claim: impl FnOnce(),
) -> Result<(PathBuf, ExportFiles), WriteError> {
    if !crate::design_systems::is_valid_id(id) || id.starts_with(['.', '_']) {
        return Err(WriteError::BadRequest(format!("invalid skill id {id:?}")));
    }
    let not_found = || WriteError::NotFound(format!("no user skill {id:?}"));
    // The folder itself and its SKILL.md, not what a link points at; and
    // the folder right in the user skills, not one a link leads to.
    let in_root = user_root.canonicalize().ok().is_some_and(|root| {
        source
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .is_some_and(|p| p == root)
    });
    if !in_root
        || !std::fs::symlink_metadata(source).is_ok_and(|m| m.is_dir())
        || !std::fs::symlink_metadata(source.join("SKILL.md")).is_ok_and(|m| m.is_file())
    {
        return Err(not_found());
    }
    let files = ExportFiles::read(source)?;
    if files.hash() != hash {
        return Err(WriteError::Changed(format!(
            "skill {id:?} changed since it was shown: review it again"
        )));
    }
    std::fs::create_dir_all(dest_root).map_err(io)?;
    let _one = EXPORT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match existing_export(dest_root, id)? {
        None => {}
        // Ours, cut short: never a skill, replaced.
        Some(e) if e.kind == ExistingKind::Unfinished => {
            std::fs::remove_dir_all(&e.path).map_err(io)?;
        }
        Some(e) => return Err(WriteError::Exists(e.name)),
    }
    let dest = dest_root.join(id);
    before_claim();
    // The folder is ours only if this makes it.
    match std::fs::create_dir(&dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(WriteError::Exists(id.to_owned()));
        }
        Err(e) => return Err(io(e)),
    }
    let mark = dest.join(EXPORT_MARK);
    let copied = std::fs::write(&mark, "")
        .map_err(io)
        .and_then(|()| files.write_into(&dest))
        .and_then(|()| std::fs::remove_file(&mark).map_err(io));
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    Ok((dest, files))
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
    let (id, _) = read_skill(source)?;
    if taken(&id) {
        return Err(WriteError::Exists(format!("skill {id:?}")));
    }
    refuse_case_twin(user_root, &id, "skill")?;
    place_copy(source, user_root, &id)?;
    Ok(id)
}

/// The id and front matter of the skill in `source`: the front matter's
/// `name`, else the folder name, as the index reads it. Install and the
/// agent CLI listing share it, so a listed id is the id installed.
fn read_skill(
    source: &Path,
) -> Result<(String, serde_json::Map<String, serde_json::Value>), WriteError> {
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
    Ok((id, front))
}

/// Whether `source` is a bare skill: a `SKILL.md` that installs as a
/// skill (front matter, a valid id) and no plugin manifest
/// (`open-design.json`, `.claude-plugin/`). "Install from folder" puts
/// one in the skills; anything else stays a plugin, as before.
#[must_use]
pub fn is_bare_skill(source: &Path) -> bool {
    !source.join("open-design.json").exists()
        && !source.join(".claude-plugin").exists()
        && read_skill(source).is_ok()
}

/// Where agent CLIs keep their skills, under `home`: `(agent, folder)`.
/// Only these are listed; nothing else in the home folder is read.
#[must_use]
pub fn agent_skill_roots(home: &Path) -> Vec<(String, PathBuf)> {
    [
        ("claude", ".claude/skills"),
        ("agents", ".agents/skills"),
        ("codex", ".codex/skills"),
        ("gemini", ".gemini/skills"),
        ("opencode", ".config/opencode/skill"),
        ("opencode", ".config/opencode/skills"),
    ]
    .into_iter()
    .map(|(agent, rel)| (agent.to_owned(), home.join(rel)))
    .collect()
}

/// A skill folder found in an agent CLI's skills.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LocalSkill {
    pub path: PathBuf,
    pub agent: String,
    /// The id it installs as; the folder name when `error` is set.
    pub id: String,
    pub description: String,
    /// A user skill with this id exists (any case or Unicode form).
    pub imported: bool,
    /// Why it cannot be imported (front matter, id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The skill folders one level under each root, by agent then id. A root
/// that does not exist is skipped. Hidden entries and links are skipped
/// (`~/.claude/skills` often links into `~/.agents/skills`), so are
/// folders without a `SKILL.md`; a `SKILL.md` that would not install is
/// listed with its `error`. Nothing is written.
#[must_use]
pub fn scan_agent_skills(roots: &[(String, PathBuf)], user_root: &Path) -> Vec<LocalSkill> {
    let mut out = Vec::new();
    for (agent, root) in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut found: Vec<LocalSkill> = entries
            .flatten()
            .filter_map(|entry| {
                let folder = entry.file_name().to_string_lossy().into_owned();
                let kind = entry.file_type().ok()?;
                if folder.starts_with('.') || !kind.is_dir() {
                    return None;
                }
                let path = entry.path();
                std::fs::symlink_metadata(path.join("SKILL.md")).ok()?;
                Some(match read_skill(&path) {
                    Ok((id, front)) => LocalSkill {
                        imported: user_skill_exists(user_root, &id),
                        description: front
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                        path,
                        agent: agent.clone(),
                        id,
                        error: None,
                    },
                    Err(e) => LocalSkill {
                        path,
                        agent: agent.clone(),
                        id: folder,
                        description: String::new(),
                        imported: false,
                        error: Some(e.to_string()),
                    },
                })
            })
            .collect();
        found.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(found);
    }
    out
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

    #[test]
    fn a_user_skill_exists_in_any_case_and_not_while_it_installs() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("skills");
        assert!(!user_skill_exists(&user, "tea-house"), "no folder yet");
        std::fs::create_dir_all(user.join("tea-house")).unwrap();
        std::fs::create_dir_all(user.join(".installing-brew-1")).unwrap();
        std::fs::write(user.join("notes"), "x").unwrap();
        assert!(user_skill_exists(&user, "tea-house"));
        assert!(user_skill_exists(&user, "Tea-House"));
        assert!(!user_skill_exists(&user, "tea-house-2"));
        assert!(!user_skill_exists(&user, ".installing-brew-1"));
        assert!(!user_skill_exists(&user, "notes"), "a file is no skill");
    }

    #[test]
    fn agent_skills_are_listed_one_level_deep_by_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let user = tmp.path().join("user");
        let roots = agent_skill_roots(&home);
        // No agent folder at all: nothing, no error.
        assert!(scan_agent_skills(&roots, &user).is_empty());

        let claude = home.join(".claude/skills");
        let codex = home.join(".codex/skills");
        skill_dir(
            &claude,
            "tea",
            "---\nname: tea-house\ndescription: Calm.\n---\nBody.\n",
        );
        skill_dir(&claude, "plain", "---\ndescription: x\n---\nBody.\n");
        skill_dir(&codex, "brew", "---\nname: brew\n---\nBody.\n");
        // Not listed: hidden, no SKILL.md, a file, deeper than one level.
        skill_dir(&claude, ".hidden", "---\nname: hidden\n---\n");
        std::fs::create_dir_all(claude.join("notes")).unwrap();
        std::fs::write(claude.join("README.md"), "x").unwrap();
        skill_dir(&claude.join("group"), "deep", "---\nname: deep\n---\n");
        // Listed with an error: no front matter, an id that cannot install.
        skill_dir(&codex, "loose", "Just text.\n");
        skill_dir(&codex, "bad", "---\nname: \"a b\"\n---\nBody.\n");
        #[cfg(unix)]
        std::os::unix::fs::symlink(codex.join("brew"), claude.join("linked")).unwrap();
        // Imported: a user skill with the same id in another case.
        std::fs::create_dir_all(user.join("Brew")).unwrap();

        let got = scan_agent_skills(&roots, &user);
        let rows: Vec<(&str, &str, bool, bool)> = got
            .iter()
            .map(|s| {
                (
                    s.agent.as_str(),
                    s.id.as_str(),
                    s.imported,
                    s.error.is_some(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("claude", "plain", false, false),
                ("claude", "tea-house", false, false),
                ("codex", "bad", false, true),
                ("codex", "brew", true, false),
                ("codex", "loose", false, true),
            ]
        );
        assert_eq!(got[1].description, "Calm.");
        assert_eq!(got[1].path, claude.join("tea"));
        // What is listed installs under the id listed.
        let id = install_skill(&got[1].path, &user, |_| false).unwrap();
        assert_eq!(id, got[1].id);
        assert!(scan_agent_skills(&roots, &user)[1].imported);
        // Nothing was written in the agent folders.
        let mut left = names(&claude);
        left.retain(|n| n != "linked");
        assert_eq!(
            left,
            [".hidden", "README.md", "group", "notes", "plain", "tea"]
        );
    }

    #[test]
    fn a_bare_skill_has_no_plugin_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = skill_dir(tmp.path(), "bare", "---\nname: bare\n---\n");
        assert!(is_bare_skill(&bare));
        let od = skill_dir(tmp.path(), "od", "---\nname: od\n---\n");
        std::fs::write(od.join("open-design.json"), "{}").unwrap();
        assert!(!is_bare_skill(&od));
        let cc = skill_dir(tmp.path(), "cc", "---\nname: cc\n---\n");
        std::fs::create_dir_all(cc.join(".claude-plugin")).unwrap();
        assert!(!is_bare_skill(&cc));
        let none = tmp.path().join("none");
        std::fs::create_dir_all(&none).unwrap();
        assert!(!is_bare_skill(&none));
        // A SKILL.md the skills would refuse stays a plugin, as before.
        let loose = skill_dir(tmp.path(), "loose", "Just text.\n");
        assert!(!is_bare_skill(&loose));
        let bad = skill_dir(tmp.path(), "bad", "---\nname: \"a b\"\n---\n");
        assert!(!is_bare_skill(&bad));
    }

    /// The hash the preview shows for `source` (empty when it cannot be
    /// read, which the copy refuses anyway).
    fn hash_of(source: &Path) -> String {
        ExportFiles::read(source)
            .map(|f| f.hash())
            .unwrap_or_default()
    }

    /// [`export_skill`] of the content the user reviewed just now.
    fn export(user: &Path, source: &Path, id: &str, dest: &Path) -> Result<PathBuf, WriteError> {
        export_skill(user, source, id, dest, &hash_of(source)).map(|(p, _)| p)
    }

    /// The copy writes only the content whose hash the user saw: an edit
    /// of the SKILL.md, of a side file, or a file added since is refused
    /// and nothing is written; a hidden file, never copied, changes
    /// nothing. The bytes written are the ones compared.
    #[test]
    fn an_export_copies_only_the_content_reviewed() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let tea = skill_dir(
            &user,
            "tea",
            "---\nname: tea\ndescription: D.\n---\nUse cream.\n",
        );
        let shown = ExportFiles::read(&tea).unwrap();
        assert_eq!(shown.paths(), ["SKILL.md", "assets/tokens.css"]);
        assert_eq!(
            shown.skill_md(),
            "---\nname: tea\ndescription: D.\n---\nUse cream.\n"
        );
        let hash = shown.hash();
        assert_eq!(hash.len(), 64);
        let changed = |what: &str| {
            let got = export_skill(&user, &tea, "tea", &dest, &hash);
            assert!(
                matches!(got, Err(WriteError::Changed(_))),
                "{what}: {got:?}"
            );
            assert!(!dest.exists(), "{what}: nothing written");
        };
        std::fs::write(
            tea.join("SKILL.md"),
            "---\nname: tea\ndescription: D.\n---\nPRINT PING\n",
        )
        .unwrap();
        changed("SKILL.md edited");
        std::fs::write(
            tea.join("SKILL.md"),
            "---\nname: tea\ndescription: D.\n---\nUse cream.\n",
        )
        .unwrap();
        std::fs::write(tea.join("assets/tokens.css"), "a{color:red}").unwrap();
        changed("side file edited");
        std::fs::write(tea.join("assets/tokens.css"), ":root{--accent:#2f6b3a}").unwrap();
        std::fs::create_dir_all(tea.join("scripts")).unwrap();
        changed("folder added");
        std::fs::write(tea.join("scripts/run.sh"), "od files list\n").unwrap();
        changed("file added");
        std::fs::remove_dir_all(tea.join("scripts")).unwrap();
        // Back as it was shown; a hidden file is never copied.
        std::fs::write(tea.join(".env"), "KEY=1").unwrap();
        let (written, files) = export_skill(&user, &tea, "tea", &dest, &hash).unwrap();
        assert_eq!(files, shown);
        assert_eq!(names(&written), ["SKILL.md", "assets"]);
        assert_eq!(
            std::fs::read_to_string(written.join("assets/tokens.css")).unwrap(),
            ":root{--accent:#2f6b3a}"
        );
    }

    /// The bytes written are the bytes whose hash was compared: a skill
    /// edited right after the compare (before the folder is made) is
    /// copied as it was when compared, never as it is on disk by then.
    #[test]
    fn an_export_writes_the_bytes_it_compared() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let md = "---\nname: tea\ndescription: D.\n---\nUse cream.\n";
        let tea = skill_dir(&user, "tea", md);
        let hash = hash_of(&tea);
        let (written, _) = export_skill_with(&user, &tea, "tea", &dest, &hash, || {
            std::fs::write(tea.join("SKILL.md"), "---\nname: tea\n---\nPRINT PING\n").unwrap();
            std::fs::write(tea.join("assets/tokens.css"), "body{}").unwrap();
            std::fs::write(tea.join("assets/late.css"), "a{}").unwrap();
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(written.join("SKILL.md")).unwrap(),
            md
        );
        assert_eq!(
            std::fs::read_to_string(written.join("assets/tokens.css")).unwrap(),
            ":root{--accent:#2f6b3a}"
        );
        assert_eq!(
            names(&written.join("assets")),
            ["tokens.css"],
            "no file added since"
        );
    }

    /// The hash counts each file's length: without it, one file `a`
    /// holding the next entry's header (`f\0b\0…`) would hash as two
    /// files `a` and `b`, and the copy would take one for the other.
    #[test]
    fn the_export_hash_tells_one_file_from_two() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = |name: &str, files: &[(&str, &[u8])]| {
            let dir = tmp.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), "---\nname: x\n---\nX.\n").unwrap();
            for (f, bytes) in files {
                std::fs::write(dir.join(f), bytes).unwrap();
            }
            ExportFiles::read(&dir).unwrap()
        };
        let one = folder("one", &[("a", b"Xf\0b\0Y")]);
        let two = folder("two", &[("a", b"X"), ("b", b"Y")]);
        assert_ne!(one.paths(), two.paths());
        assert_ne!(one.hash(), two.hash());
    }

    /// A skill with no description is copied: Claude Code 2.1.332 still
    /// loads it (the first line of its body stands in); the app warns.
    #[test]
    fn a_skill_with_no_description_is_copied() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let bare = skill_dir(&user, "bare", "---\nname: bare\n---\nShort notes.\n");
        assert!(!ExportFiles::read(&bare).unwrap().check().has_description);
        export(&user, &bare, "bare", &dest).unwrap();
        assert!(dest.join("bare/SKILL.md").is_file());
    }

    /// "Copy to Claude Code": the skill and its side files land in the
    /// agent's folder; links are not followed, nothing is written over,
    /// and nothing is left behind when it is refused.
    #[test]
    fn export_copies_a_user_skill_and_never_writes_over() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let tea = skill_dir(
            &user,
            "tea",
            "---\nname: tea\ndescription: D.\n---\nUse cream.\n",
        );
        let written = export(&user, &tea, "tea", &dest).unwrap();
        assert_eq!(written, dest.join("tea"));
        assert_eq!(
            std::fs::read_to_string(dest.join("tea/SKILL.md")).unwrap(),
            "---\nname: tea\ndescription: D.\n---\nUse cream.\n"
        );
        assert!(dest.join("tea/assets/tokens.css").is_file());
        // Again, or in another case or Unicode form: refused, the copy
        // there stays as it was.
        std::fs::write(dest.join("tea/SKILL.md"), "mine").unwrap();
        // Ids are ASCII; a name there may not be: the Kelvin sign `K`
        // folds to `k`, one folder with `kiwi` on a Mac.
        let kiwi = skill_dir(&user, "kiwi", "---\nname: kiwi\ndescription: D.\n---\n");
        std::fs::create_dir_all(dest.join("\u{212a}iwi")).unwrap();
        for (dir, id) in [(&tea, "tea"), (&tea, "TEA"), (&kiwi, "kiwi")] {
            let got = export(&user, dir, id, &dest);
            assert!(matches!(got, Err(WriteError::Exists(_))), "{id}: {got:?}");
        }
        assert_eq!(
            std::fs::read_to_string(dest.join("tea/SKILL.md")).unwrap(),
            "mine"
        );
        assert_eq!(names(&dest), ["tea", "\u{212a}iwi"]);
        // A file or a link of that name is no place to write either.
        let mint = skill_dir(&user, "mint", "---\nname: mint\ndescription: D.\n---\n");
        std::fs::write(dest.join("Mint"), "a file").unwrap();
        assert!(matches!(
            export(&user, &mint, "mint", &dest),
            Err(WriteError::Exists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(dest.join("Mint")).unwrap(),
            "a file"
        );
        // A bad id: refused before anything is read or written.
        for id in ["", "..", "a/b", ".hidden", "_x", "a b"] {
            let got = export(&user, &tea, id, &dest);
            assert!(
                matches!(got, Err(WriteError::BadRequest(_))),
                "{id:?}: {got:?}"
            );
        }
        // A folder outside the user skills, or missing its SKILL.md.
        let outside = skill_dir(
            tmp.path(),
            "outside",
            "---\nname: outside\ndescription: D.\n---\n",
        );
        assert!(matches!(
            export(&user, &outside, "outside", &dest),
            Err(WriteError::NotFound(_))
        ));
        let bare = user.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        assert!(matches!(
            export(&user, &bare, "bare", &dest),
            Err(WriteError::NotFound(_))
        ));
        assert_eq!(
            names(&dest),
            ["Mint", "tea", "\u{212a}iwi"],
            "nothing left behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_follows_no_link() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let secret = tmp.path().join("secret");
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("key.txt"), "s3cret").unwrap();
        // A skill holding links: they are not copied.
        let tea = skill_dir(&user, "tea", "---\nname: tea\ndescription: D.\n---\n");
        symlink(&secret, tea.join("assets/linked")).unwrap();
        symlink(secret.join("key.txt"), tea.join("key.txt")).unwrap();
        export(&user, &tea, "tea", &dest).unwrap();
        assert!(!dest.join("tea/assets/linked").exists());
        assert!(!dest.join("tea/key.txt").exists());
        assert!(dest.join("tea/assets/tokens.css").is_file());
        // A skill folder that is a link, or whose SKILL.md is one.
        let away = skill_dir(
            tmp.path(),
            "away",
            "---\nname: away\ndescription: D.\n---\n",
        );
        symlink(&away, user.join("away")).unwrap();
        assert!(matches!(
            export(&user, &user.join("away"), "away", &dest),
            Err(WriteError::NotFound(_))
        ));
        let md = user.join("md");
        std::fs::create_dir_all(&md).unwrap();
        symlink(away.join("SKILL.md"), md.join("SKILL.md")).unwrap();
        assert!(matches!(
            export(&user, &md, "md", &dest),
            Err(WriteError::NotFound(_))
        ));
        // A broken link of that name in the agent's folder: refused, and
        // nothing is written where it points.
        let mint = skill_dir(&user, "mint", "---\nname: mint\ndescription: D.\n---\n");
        symlink(tmp.path().join("nowhere"), dest.join("mint")).unwrap();
        assert!(matches!(
            export(&user, &mint, "mint", &dest),
            Err(WriteError::Exists(_))
        ));
        assert!(!tmp.path().join("nowhere").exists());
        assert_eq!(names(&dest), ["mint", "tea"]);
    }

    /// A skill with a file it cannot read is refused before anything is
    /// made in the agent's folder.
    #[cfg(unix)]
    #[test]
    fn an_export_that_fails_leaves_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let tea = skill_dir(&user, "tea", "---\nname: tea\ndescription: D.\n---\n");
        let locked = tea.join("assets/locked.css");
        std::fs::write(&locked, "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&locked).is_ok() {
            return; // root reads it anyway
        }
        let got = export(&user, &tea, "tea", &dest);
        assert!(matches!(got, Err(WriteError::Io(_))), "{got:?}");
        assert!(!dest.exists(), "nothing made there");
    }

    /// A skill with many files, so a copy takes a while.
    fn big_skill(user: &Path, id: &str) -> PathBuf {
        let dir = skill_dir(
            user,
            id,
            &format!("---\nname: {id}\ndescription: Big.\n---\n"),
        );
        for i in 0..600 {
            std::fs::write(dir.join("assets").join(format!("f{i}.css")), "a{}").unwrap();
        }
        dir
    }

    /// Claude Code loads a hidden folder in its skills as a skill: the
    /// copy never puts one there, not even while it runs.
    #[test]
    fn an_export_never_shows_a_hidden_folder_in_the_agents_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        std::fs::create_dir_all(&dest).unwrap();
        let tea = big_skill(&user, "tea");
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watch = {
            let (dest, done) = (dest.clone(), std::sync::Arc::clone(&done));
            std::thread::spawn(move || {
                let mut seen = Vec::new();
                while !done.load(std::sync::atomic::Ordering::SeqCst) {
                    for e in std::fs::read_dir(&dest).unwrap().flatten() {
                        let n = e.file_name().to_string_lossy().into_owned();
                        if n.starts_with('.') && !seen.contains(&n) {
                            seen.push(n);
                        }
                    }
                }
                seen
            })
        };
        export(&user, &tea, "tea", &dest).unwrap();
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(watch.join().unwrap(), Vec::<String>::new());
        assert!(dest.join("tea/assets/f599.css").is_file());
    }

    /// A folder of that name that another program makes while the copy
    /// runs is never replaced: either it came first (refused) or the
    /// copy's own folder did (theirs fails).
    #[test]
    fn a_folder_made_elsewhere_during_an_export_is_not_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        std::fs::create_dir_all(&dest).unwrap();
        let tea = big_skill(&user, "tea");
        let theirs = {
            let dest = dest.clone();
            std::thread::spawn(move || {
                loop {
                    // As soon as anything shows up there, make `tea` too.
                    if std::fs::read_dir(&dest).unwrap().next().is_some() {
                        return std::fs::create_dir(dest.join("tea")).is_ok();
                    }
                }
            })
        };
        let ours = export(&user, &tea, "tea", &dest);
        let made = theirs.join().unwrap();
        if made {
            assert!(matches!(ours, Err(WriteError::Exists(_))), "{ours:?}");
        } else {
            assert!(ours.is_ok(), "{ours:?}");
            assert!(dest.join("tea/SKILL.md").is_file());
        }
    }

    /// The warning reads the whole SKILL.md and every file under
    /// `scripts/`, but no link; a description must be in the front matter.
    #[test]
    fn the_export_check_reads_the_whole_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let check = |md: &str, script: Option<(&str, &str)>| {
            let dir = tmp.path().join(format!("s{}", names(tmp.path()).len()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), md).unwrap();
            if let Some((rel, text)) = script {
                let f = dir.join("scripts").join(rel);
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(f, text).unwrap();
            }
            let c = ExportFiles::read(&dir).unwrap().check();
            (c.contour_tools, c.has_description)
        };
        let plain = "---\nname: a\ndescription: Tea.\n---\nUse cream.\n";
        assert_eq!(check(plain, None), (false, true));
        assert_eq!(
            check(
                "---\nname: a\ndescription: Wraps od media generate.\n---\nBody.\n",
                None
            ),
            (true, true),
            "the front matter counts"
        );
        assert_eq!(
            check(plain, Some(("deep/run.sh", "\"$OD_BIN\" files list\n"))),
            (true, true)
        );
        assert_eq!(
            check(plain, Some(("run.mjs", "fetch(process.env.OD_DAEMON_URL)"))),
            (true, true)
        );
        assert_eq!(check(plain, Some(("run.sh", "echo tea\n"))), (false, true));
        assert_eq!(
            check(plain, Some((".hidden.sh", "od files list\n"))),
            (false, true)
        );
        assert_eq!(check("---\nname: a\n---\nBody.\n", None), (false, false));
        assert_eq!(
            check("---\nname: a\ndescription: \"  \"\n---\nBody.\n", None),
            (false, false)
        );
        assert_eq!(
            check("No front matter. od files list\n", None),
            (true, false)
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_export_check_follows_no_link() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("x.sh"), "od files list\n").unwrap();
        let dir = skill_dir(tmp.path(), "a", "---\nname: a\ndescription: D.\n---\n");
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("scripts/linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("x.sh"), dir.join("scripts/x.sh")).unwrap();
        assert!(!ExportFiles::read(&dir).unwrap().check().contour_tools);
    }

    /// Another program makes the folder between the check and the copy:
    /// it is kept as it is and the copy is refused.
    #[test]
    fn a_folder_made_right_before_the_copy_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let tea = skill_dir(&user, "tea", "---\nname: tea\ndescription: D.\n---\n");
        let got = export_skill_with(&user, &tea, "tea", &dest, &hash_of(&tea), || {
            std::fs::create_dir_all(dest.join("tea")).unwrap();
        });
        assert!(matches!(got, Err(WriteError::Exists(_))), "{got:?}");
        assert!(
            names(&dest.join("tea")).is_empty(),
            "their folder stays empty"
        );
    }

    /// Claude Code loads a folder once it has a SKILL.md: that comes last,
    /// so a copy cut short is no skill.
    #[test]
    fn the_skill_md_lands_after_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        std::fs::create_dir_all(&dest).unwrap();
        let tea = big_skill(&user, "tea");
        let watch = {
            let dest = dest.clone();
            std::thread::spawn(move || {
                let mut marked_while_copying = false;
                loop {
                    if dest.join("tea/SKILL.md").exists() {
                        let files =
                            std::fs::read_dir(dest.join("tea/assets")).map_or(0, Iterator::count);
                        return (files, marked_while_copying);
                    }
                    if dest.join("tea/assets").exists()
                        && dest.join("tea").join(EXPORT_MARK).exists()
                    {
                        marked_while_copying = true;
                    }
                }
            })
        };
        export(&user, &tea, "tea", &dest).unwrap();
        let (files, marked) = watch.join().unwrap();
        assert_eq!(files, 601, "every side file was there first");
        assert!(marked, "a copy cut short is told by its mark");
    }

    /// What stands at the name already: told by its name on disk; a copy of
    /// Contour's cut short is replaced, anything else is kept.
    #[test]
    fn an_export_names_what_is_there_and_replaces_only_its_own_unfinished_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let skill = |id: &str| {
            skill_dir(
                &user,
                id,
                &format!("---\nname: {id}\ndescription: D.\n---\n"),
            )
        };
        // Another case: refused by the name it has.
        let tea = skill("tea");
        std::fs::create_dir_all(dest.join("TEA")).unwrap();
        std::fs::write(dest.join("TEA/SKILL.md"), "theirs").unwrap();
        assert_eq!(
            export(&user, &tea, "tea", &dest),
            Err(WriteError::Exists("TEA".to_owned()))
        );
        let there = existing_export(&dest, "tea").unwrap().unwrap();
        assert_eq!(
            (there.name.as_str(), there.kind),
            ("TEA", ExistingKind::Skill)
        );
        assert_eq!(there.path, dest.join("TEA"));
        // Ours, cut short: replaced by a whole copy, with no mark left.
        let mint = skill("mint");
        std::fs::create_dir_all(dest.join("mint/assets")).unwrap();
        std::fs::write(dest.join("mint").join(EXPORT_MARK), "").unwrap();
        std::fs::write(dest.join("mint/assets/half.css"), "a{").unwrap();
        assert_eq!(
            existing_export(&dest, "mint").unwrap().unwrap().kind,
            ExistingKind::Unfinished
        );
        export(&user, &mint, "mint", &dest).unwrap();
        assert!(dest.join("mint/SKILL.md").is_file());
        assert!(dest.join("mint/assets/tokens.css").is_file());
        assert!(
            !dest.join("mint/assets/half.css").exists(),
            "the half copy went"
        );
        assert!(!dest.join("mint").join(EXPORT_MARK).exists());
        // A folder with no SKILL.md and no mark is not ours: kept.
        let sage = skill("sage");
        std::fs::create_dir_all(dest.join("sage")).unwrap();
        std::fs::write(dest.join("sage/notes.txt"), "mine").unwrap();
        assert_eq!(
            existing_export(&dest, "sage").unwrap().unwrap().kind,
            ExistingKind::Other
        );
        assert_eq!(
            export(&user, &sage, "sage", &dest),
            Err(WriteError::Exists("sage".to_owned()))
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("sage/notes.txt")).unwrap(),
            "mine"
        );
        // A whole copy leaves no mark.
        let rue = skill("rue");
        export(&user, &rue, "rue", &dest).unwrap();
        assert_eq!(names(&dest.join("rue")), ["SKILL.md", "assets"]);
        assert_eq!(
            existing_export(&dest, "rue").unwrap().unwrap().kind,
            ExistingKind::Skill
        );
        assert_eq!(existing_export(&dest, "none").unwrap(), None);
    }
}
