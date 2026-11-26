//! Writing user content (spec 1.4 "Thư viện skill", "Plugin"): skills the
//! user creates, edits or overrides under `<data>/skills`, and plugins
//! installed from a local folder into `<data>/plugins`. The index reloads
//! afterwards; user roots shadow bundled ones by id.

use crate::frontmatter;
use crate::nofollow;
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
    /// The file is no longer the one the form opened on (its tag, sent
    /// as `If-Match`): nothing written.
    #[error("changed since it was opened")]
    Stale,
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

/// The tag of the `SKILL.md` in skill folder `dir`: sha256 of its bytes,
/// hex; `"missing"` when it cannot be read (no file, a link, a FIFO).
/// The skill editor opens on it and saves only over the same tag
/// ([`save_skill`]).
#[must_use]
pub fn skill_tag(dir: &Path) -> String {
    match nofollow::read_file(&dir.join("SKILL.md"), MAX_PLUGIN_BYTES) {
        Ok(Some(file)) => tag_of(&file.bytes),
        _ => "missing".to_owned(),
    }
}

/// The folder a save of skill `id` writes over: the user's override in
/// `<user_root>/<id>` once its `SKILL.md` is on disk (an agent may write
/// one the index has not read yet), else `entry_dir`, the folder the
/// index has for it. A form's `GET` tags this folder and its `PUT`
/// compares this folder ([`save_skill`]), so both see the same file.
/// The override is a folder named `id` byte for byte: on macOS and
/// Windows `<user_root>/<id>` also opens `Ad-Creative` for `ad-creative`,
/// another skill.
#[must_use]
pub fn skill_write_dir(user_root: &Path, id: &str, entry_dir: &Path, is_user: bool) -> PathBuf {
    let over = user_root.join(id);
    if !is_user
        && crate::design_systems::is_valid_id(id)
        && named_exactly(user_root, id)
        && over.join("SKILL.md").exists()
    {
        over
    } else {
        entry_dir.to_path_buf()
    }
}

/// Whether `dir` lists an entry named `name`, byte for byte (not one that
/// differs in case or Unicode form, which macOS and Windows open as it).
fn named_exactly(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir)
        .is_ok_and(|entries| entries.flatten().any(|e| e.file_name() == name))
}

/// The tag of content `bytes`: sha256, hex.
#[must_use]
pub fn tag_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Rewrites skill `id`. A user skill is edited in place; a bundled one
/// gets a user override in `<user_root>/<id>` that first copies the
/// bundled folder's side files (assets, references…).
///
/// With `expected` (the [`skill_tag`] the form opened on), the skill is
/// written only if its `SKILL.md` still has that tag, compared under the
/// lock a replace, restore or install over it takes.
///
/// # Errors
/// [`WriteError::BadRequest`], [`WriteError::Stale`], [`WriteError::Io`].
pub fn save_skill(
    user_root: &Path,
    id: &str,
    current_dir: &Path,
    is_user: bool,
    draft: &SkillDraft,
    expected: Option<&str>,
) -> Result<PathBuf, WriteError> {
    draft.check()?;
    if !crate::design_systems::is_valid_id(id) {
        return Err(WriteError::BadRequest(format!("invalid skill id {id:?}")));
    }
    let _one = REPLACE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A user skill named like it in another case or Unicode form is the
    // folder `<user_root>/<id>` opens on macOS and Windows: an override
    // there would write over that skill.
    if !is_user
        && !named_exactly(user_root, id)
        && let Some(twin) = entry_like(user_root, id)?
    {
        return Err(WriteError::Exists(format!("skill {twin:?}")));
    }
    if let Some(expected) = expected
        && skill_tag(&skill_write_dir(user_root, id, current_dir, is_user)) != expected
    {
        return Err(WriteError::Stale);
    }
    // Not through a link or from a FIFO an agent put there.
    let previous = nofollow::read_file(&current_dir.join("SKILL.md"), MAX_PLUGIN_BYTES)
        .ok()
        .flatten()
        .and_then(|f| String::from_utf8(f.bytes).ok());
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
    write_replacing(&dir.join("SKILL.md"), text.as_bytes()).map_err(io)?;
    Ok(dir)
}

/// Writes `bytes` as `path` through a hidden file of its own beside it,
/// renamed over `path`: a read never finds it half written (the index, a
/// form's `GET`), and a link put there is replaced, never written through.
fn write_replacing(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
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

/// Copies `from` into a new folder `to` (folders and regular files only,
/// with their permission bits; no link at any depth, no FIFO, no hidden
/// entry but `.claude-plugin`: [`nofollow::read_tree`]). Fails past
/// `budget` bytes, counted as read. `from` itself may be a link the
/// caller accepted (a folder the user picked, an indexed design system):
/// it is resolved once, and only what lies under it is read link-free.
///
/// # Errors
/// [`WriteError::BadRequest`] when too large, [`WriteError::Io`].
pub fn copy_dir(from: &Path, to: &Path, budget: u64) -> Result<u64, WriteError> {
    let from = std::fs::canonicalize(from).map_err(io)?;
    let tree = nofollow::read_tree(&from, keep_entry(true), budget).map_err(tree_error)?;
    std::fs::create_dir_all(to).map_err(io)?;
    nofollow::write_tree(&tree, to, None).map_err(io)?;
    Ok(tree.files.iter().map(|(_, f)| f.bytes.len() as u64).sum())
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
/// the copy writes these very bytes and permission bits after it compared
/// the hash the user saw, so nothing changed between the two is written
/// (an agent can edit a Personal skill over loopback while the user reads
/// the preview).
///
/// What is read ([`nofollow::read_tree`]): folders and regular files, no
/// link at any depth and no FIFO, even one swapped in while it reads; no
/// hidden entry at the top (Claude Code would load a hidden folder there
/// as a skill), and below it only `.claude-plugin`, as [`copy_dir`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFiles {
    /// Folders under the skill folder, parents first.
    dirs: Vec<PathBuf>,
    /// Files by path: bytes and permission bits.
    files: Vec<(PathBuf, nofollow::ReadFile)>,
}

/// Hidden entries a copy keeps: `.claude-plugin`, below the top only when
/// `top_too` is false.
fn keep_entry(top_too: bool) -> impl Fn(bool, &std::ffi::OsStr) -> bool {
    move |top, name| {
        !name.to_string_lossy().starts_with('.') || (name == ".claude-plugin" && (top_too || !top))
    }
}

/// `p` with `/` between its parts, on every platform.
fn slash_path(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn tree_error(e: nofollow::TreeError) -> WriteError {
    match e {
        nofollow::TreeError::TooLarge => {
            WriteError::BadRequest("the folder is larger than 50 MB".to_owned())
        }
        nofollow::TreeError::Io(e) => io(e),
    }
}

impl ExportFiles {
    /// Reads the skill folder `source` (at most [`MAX_PLUGIN_BYTES`],
    /// counted as read).
    ///
    /// # Errors
    /// [`WriteError::NotFound`] without a `SKILL.md` file,
    /// [`WriteError::BadRequest`] when too large, [`WriteError::Io`].
    pub fn read(source: &Path) -> Result<Self, WriteError> {
        Self::read_any(source, false)?.with_skill_md()
    }

    /// Reads the skill folder `source` as an install copies it
    /// ([`copy_dir`]: `.claude-plugin` kept at the top too), at most
    /// [`MAX_PLUGIN_BYTES`]: an imported skill and its source compare
    /// equal by [`Self::hash`] until one of them changes.
    ///
    /// # Errors
    /// As [`Self::read`].
    pub fn read_copy(source: &Path) -> Result<Self, WriteError> {
        Self::read_any(source, true)?.with_skill_md()
    }

    /// The folder `source` read link-free, `SKILL.md` or not.
    fn read_any(source: &Path, top_too: bool) -> Result<Self, WriteError> {
        let tree = nofollow::read_tree(source, keep_entry(top_too), MAX_PLUGIN_BYTES)
            .map_err(tree_error)?;
        Ok(Self {
            dirs: tree.dirs,
            files: tree.files,
        })
    }

    fn with_skill_md(self) -> Result<Self, WriteError> {
        if self.file(Path::new("SKILL.md")).is_none() {
            return Err(WriteError::NotFound("the skill has no SKILL.md".to_owned()));
        }
        Ok(self)
    }

    /// Names this content: every folder and file path, every file's
    /// permission bits and bytes (sha256, hex). A `chmod` changes it too.
    #[must_use]
    pub fn hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for d in &self.dirs {
            h.update(b"d\0");
            h.update(d.to_string_lossy().as_bytes());
            h.update(b"\0");
        }
        for (p, f) in &self.files {
            h.update(b"f\0");
            h.update(p.to_string_lossy().as_bytes());
            h.update(b"\0");
            h.update(f.mode.to_le_bytes());
            h.update((f.bytes.len() as u64).to_le_bytes());
            h.update(&f.bytes);
        }
        hex::encode(h.finalize())
    }

    /// The files copied, by path under the skill folder (`/` between
    /// folders).
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.files.iter().map(|(p, _)| slash_path(p)).collect()
    }

    /// The files that differ between `self` and `other`, by path (`/`
    /// between folders), sorted: in one only, or other bytes or
    /// permission bits.
    #[must_use]
    pub fn changed_files(&self, other: &Self) -> Vec<String> {
        let mut out: Vec<String> = self
            .files
            .iter()
            .filter(|(p, f)| other.files.iter().find(|(q, _)| q == p).map(|(_, g)| g) != Some(f))
            .chain(
                other
                    .files
                    .iter()
                    .filter(|(p, _)| !self.files.iter().any(|(q, _)| q == p)),
            )
            .map(|(p, _)| slash_path(p))
            .collect();
        out.sort();
        out.dedup();
        out
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
            .map(|(_, f)| f.bytes.as_slice())
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
                .filter(|(p, f)| {
                    p.starts_with("scripts") && f.bytes.len() as u64 <= MAX_CHECKED_SCRIPT_BYTES
                })
                .take(MAX_CHECKED_SCRIPTS)
                .any(|(_, f)| {
                    od_contracts::calls_contour_tools(&String::from_utf8_lossy(&f.bytes))
                });
        ExportCheck {
            contour_tools,
            has_description,
        }
    }

    /// Writes this content into the folder `to` (made already), with each
    /// file's permission bits, the top `SKILL.md` last.
    fn write_into(&self, to: &Path) -> Result<(), WriteError> {
        let tree = nofollow::Tree {
            dirs: self.dirs.clone(),
            files: self.files.clone(),
        };
        nofollow::write_tree(&tree, to, Some(Path::new("SKILL.md"))).map_err(io)
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

/// Copies `source` into `<user_root>/<id>`, replacing an older copy (a
/// plugin, or what is no skill folder: a skill folder goes through
/// [`install_skill_with`], which keeps it): built
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
    install_skill_with(source, user_root, &id, &mut |from, to| {
        std::fs::rename(from, to)
    })?;
    Ok(id)
}

/// [`install_skill`] once the id is checked. A user skill `id` already
/// there is kept as a replace keeps it ([`swap_in`]): in `.previous/<id>`,
/// for one restore.
fn install_skill_with(
    source: &Path,
    user_root: &Path,
    id: &str,
    rename: Rename<'_>,
) -> Result<(), WriteError> {
    let _one = REPLACE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(current) = user_skill_dir(user_root, id) else {
        return place_copy(source, user_root, id);
    };
    let held = previous_root(user_root)?;
    let incoming = held.join(format!(".incoming-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&incoming);
    if let Err(e) = copy_dir(source, &incoming, MAX_PLUGIN_BYTES) {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(e);
    }
    swap_in(&held, id, &current, &incoming, rename)
}

/// The id and front matter of the skill in `source`: the front matter's
/// `name`, else the folder name, as the index reads it. Install and the
/// agent CLI listing share it, so a listed id is the id installed.
fn read_skill(
    source: &Path,
) -> Result<(String, serde_json::Map<String, serde_json::Value>), WriteError> {
    let skill_md = source.join("SKILL.md");
    // A link is refused here, even one swapped in after a check, and a
    // FIFO is no file (`nofollow::read_file`): `copy_dir` skips both, so
    // the copy would have no SKILL.md.
    let raw = match nofollow::read_file(&skill_md, MAX_PLUGIN_BYTES) {
        Ok(Some(f)) => String::from_utf8(f.bytes)
            .map_err(|e| WriteError::BadRequest(format!("SKILL.md cannot be read: {e}")))?,
        Ok(None) => {
            return Err(WriteError::BadRequest(
                "the folder has no SKILL.md".to_owned(),
            ));
        }
        Err(e) => {
            let e = tree_error(e);
            return Err(WriteError::BadRequest(format!(
                "SKILL.md cannot be read: {e}"
            )));
        }
    };
    let folder = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    skill_id(&raw, &folder)
}

/// The id and front matter of the `SKILL.md` text `raw` in the folder
/// named `folder` ([`read_skill`]).
fn skill_id(
    raw: &str,
    folder: &str,
) -> Result<(String, serde_json::Map<String, serde_json::Value>), WriteError> {
    let front = skill_front_matter(raw)?;
    let id = match front.get("name") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
        _ => folder.to_owned(),
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
    /// Imported, and the user skill is no longer what this folder would
    /// install ([`ExportFiles::read_copy`] hashes differ): either side
    /// changed. False when either cannot be read.
    pub differs: bool,
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
            .filter_map(|entry| local_skill(agent, &entry.path(), user_root))
            .collect();
        found.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(found);
    }
    out
}

/// The agent CLI skill folder `path` as [`scan_agent_skills`] lists it:
/// `path` itself is a folder, not a link, its name not hidden, with a
/// `SKILL.md` entry; else `None`.
fn local_skill(agent: &str, path: &Path, user_root: &Path) -> Option<LocalSkill> {
    let folder = path.file_name()?.to_string_lossy().into_owned();
    if folder.starts_with('.') || !std::fs::symlink_metadata(path).ok()?.is_dir() {
        return None;
    }
    std::fs::symlink_metadata(path.join("SKILL.md")).ok()?;
    Some(match read_skill(path) {
        Ok((id, front)) => {
            let imported = user_skill_exists(user_root, &id);
            LocalSkill {
                imported,
                differs: imported
                    && user_skill_dir(user_root, &id).is_some_and(|c| differs(path, &c)),
                description: front
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                path: path.to_path_buf(),
                agent: agent.to_owned(),
                id,
                error: None,
            }
        }
        Err(e) => LocalSkill {
            path: path.to_path_buf(),
            agent: agent.to_owned(),
            id: folder,
            description: String::new(),
            imported: false,
            differs: false,
            error: Some(e.to_string()),
        },
    })
}

/// Whether the user skill in `current` is no longer what `source` would
/// install; false when either cannot be read.
fn differs(source: &Path, current: &Path) -> bool {
    match (
        ExportFiles::read_copy(source),
        ExportFiles::read_copy(current),
    ) {
        (Ok(a), Ok(b)) => a.hash() != b.hash(),
        _ => false,
    }
}

/// The agent CLI skill at `path` when it is one [`scan_agent_skills`]
/// lists under `roots` (right under a root, by the same path), else
/// `None`: a route takes only a folder the listing showed.
#[must_use]
pub fn agent_skill_at(
    roots: &[(String, PathBuf)],
    path: &Path,
    user_root: &Path,
) -> Option<LocalSkill> {
    let parent = path.parent()?;
    let (agent, _) = roots.iter().find(|(_, root)| root == parent)?;
    local_skill(agent, path, user_root)
}

/// The user skill folder of `id` in `user_root` (its name on disk may
/// differ in case or Unicode form): a folder, not a link. Hidden entries
/// do not count.
fn user_skill_dir(user_root: &Path, id: &str) -> Option<PathBuf> {
    let name = entry_like(user_root, id).ok().flatten()?;
    let dir = user_root.join(name);
    std::fs::symlink_metadata(&dir)
        .is_ok_and(|m| m.is_dir())
        .then_some(dir)
}

/// The hidden folder of `<data>/skills` that keeps, per id, the copy a
/// "Replace from source" took out ([`replace_skill_from_source`]). The
/// index lists no hidden folder, and this one has no `SKILL.md` at its
/// top either.
pub const PREVIOUS_DIR: &str = ".previous";

/// One replace or restore at a time in this process.
static REPLACE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What "Review and replace" shows: the agent CLI folder as an install
/// would copy it, the user skill as it is, and the files that differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePreview {
    pub source: ExportFiles,
    pub current: ExportFiles,
    pub changed_files: Vec<String>,
}

/// Reads the agent CLI skill folder `source` and the user skill `id`
/// ([`ExportFiles::read_copy`], link-free, bounded). Writes nothing.
///
/// # Errors
/// [`WriteError::NotFound`] without a user skill `id` or a `SKILL.md` on
/// either side, [`WriteError::BadRequest`] when too large,
/// [`WriteError::Io`].
pub fn source_preview(
    source: &Path,
    user_root: &Path,
    id: &str,
) -> Result<SourcePreview, WriteError> {
    let current = user_skill_dir(user_root, id)
        .ok_or_else(|| WriteError::NotFound(format!("no user skill {id:?}")))?;
    let source = ExportFiles::read_copy(source)?;
    let current = ExportFiles::read_copy(&current)?;
    let changed_files = source.changed_files(&current);
    Ok(SourcePreview {
        source,
        current,
        changed_files,
    })
}

/// Replaces the user skill `id` with the agent CLI skill folder `source`,
/// if the folder is still what the user reviewed: `expected_hash` is the
/// [`ExportFiles::hash`] of the preview ([`source_preview`]). `source` is
/// read once; the bytes compared are the bytes written.
///
/// The copy in Contour is kept, not deleted: it moves to
/// `<user_root>/.previous/<id>` (an older one kept there goes), and
/// [`restore_previous_skill`] brings it back. The new copy is built in a
/// hidden folder inside `.previous` (never listed) and renamed into place;
/// on a failure the copy in Contour stays as it was.
///
/// # Errors
/// [`WriteError::Changed`] when `source` changed (nothing written),
/// [`WriteError::BadRequest`] for an invalid id, a `SKILL.md` that names
/// another id now, or a folder too large, [`WriteError::NotFound`]
/// without a user skill `id`, [`WriteError::Io`].
pub fn replace_skill_from_source(
    source: &Path,
    user_root: &Path,
    id: &str,
    expected_hash: &str,
) -> Result<ExportFiles, WriteError> {
    replace_skill_from_source_with(source, user_root, id, expected_hash, &mut |from, to| {
        std::fs::rename(from, to)
    })
}

/// How a replace, an install or a restore moves a folder: `std::fs::rename`,
/// or in a test one that fails or stops (the app quitting) at a step.
type Rename<'a> = &'a mut dyn FnMut(&Path, &Path) -> std::io::Result<()>;

fn replace_skill_from_source_with(
    source: &Path,
    user_root: &Path,
    id: &str,
    expected_hash: &str,
    rename: Rename<'_>,
) -> Result<ExportFiles, WriteError> {
    if !crate::design_systems::is_valid_id(id) || id.starts_with(['.', '_']) {
        return Err(WriteError::BadRequest(format!("invalid skill id {id:?}")));
    }
    let files = ExportFiles::read_any(source, true)?;
    if files.hash() != expected_hash {
        return Err(WriteError::Changed(format!(
            "the source of skill {id:?} changed since it was shown: review it again"
        )));
    }
    let files = files.with_skill_md()?;
    let raw = String::from_utf8(
        files
            .file(Path::new("SKILL.md"))
            .unwrap_or_default()
            .to_vec(),
    )
    .map_err(|e| WriteError::BadRequest(format!("SKILL.md cannot be read: {e}")))?;
    let folder = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (named, _) = skill_id(&raw, &folder)?;
    if named != id {
        return Err(WriteError::BadRequest(format!(
            "the source is the skill {named:?} now, not {id:?}"
        )));
    }
    let _one = REPLACE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current = user_skill_dir(user_root, id)
        .ok_or_else(|| WriteError::NotFound(format!("no user skill {id:?}")))?;
    let held = previous_root(user_root)?;
    let incoming = held.join(format!(".incoming-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&incoming);
    let built = std::fs::create_dir(&incoming)
        .map_err(io)
        .and_then(|()| files.write_into(&incoming));
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(e);
    }
    swap_in(&held, id, &current, &incoming, rename)?;
    Ok(files)
}

/// Puts `incoming` (a new copy built in `held`) in place of the user skill
/// folder `current`, which moves to `held/<id>`. An older copy kept there
/// is set aside (`.old-<id>-<pid>`) first and removed only once both have
/// landed; on a failure everything goes back, so a failed replace keeps
/// the copy a restore offers. Cut short (the app quit), the next replace,
/// install or restore sorts it out ([`recover_cut_short`]). `incoming` is
/// removed on a failure.
fn swap_in(
    held: &Path,
    id: &str,
    current: &Path,
    incoming: &Path,
    rename: Rename<'_>,
) -> Result<(), WriteError> {
    let undo_incoming = || {
        let _ = std::fs::remove_dir_all(incoming);
    };
    let previous = held.join(id);
    let old = held.join(format!(".old-{id}-{}", std::process::id()));
    if std::fs::symlink_metadata(&old).is_ok() {
        undo_incoming();
        return Err(WriteError::Io(format!("{} is in the way", old.display())));
    }
    let had_previous = std::fs::symlink_metadata(&previous).is_ok();
    if had_previous && let Err(e) = rename(&previous, &old) {
        undo_incoming();
        return Err(io(e));
    }
    if let Err(e) = rename(current, &previous) {
        if had_previous {
            let _ = rename(&old, &previous);
        }
        undo_incoming();
        return Err(io(e));
    }
    if let Err(e) = rename(incoming, current) {
        let _ = rename(&previous, current);
        if had_previous {
            let _ = rename(&old, &previous);
        }
        undo_incoming();
        return Err(io(e));
    }
    // Landed: the older copy goes, as one level is kept. Left there, the
    // next run removes it (both the skill and `held/<id>` are there).
    if let Err(e) = remove_any(&old) {
        tracing::warn!(path = %old.display(), error = %e, "older kept copy not removed");
    }
    Ok(())
}

/// Whether a copy taken out by [`replace_skill_from_source`] is kept for
/// the user skill `id`.
#[must_use]
pub fn has_previous_skill(user_root: &Path, id: &str) -> bool {
    // An id is a folder name: never a path (`/etc`, `..`) asked about.
    valid_skill_id(id)
        && std::fs::symlink_metadata(user_root.join(PREVIOUS_DIR)).is_ok_and(|m| m.is_dir())
        && std::fs::symlink_metadata(user_root.join(PREVIOUS_DIR).join(id))
            .is_ok_and(|m| m.is_dir())
}

/// Swaps the user skill `id` and the copy kept in `.previous/<id>`: one
/// level, so a second restore undoes the first. Both are moved whole
/// (renamed), byte for byte. Without a user skill `id` (deleted since) the
/// kept copy comes back as it.
///
/// # Errors
/// [`WriteError::NotFound`] when no copy is kept, [`WriteError::BadRequest`]
/// for an invalid id, [`WriteError::Io`].
pub fn restore_previous_skill(user_root: &Path, id: &str) -> Result<(), WriteError> {
    restore_previous_skill_with(user_root, id, &mut |from, to| std::fs::rename(from, to))
}

fn restore_previous_skill_with(
    user_root: &Path,
    id: &str,
    rename: Rename<'_>,
) -> Result<(), WriteError> {
    if !crate::design_systems::is_valid_id(id) || id.starts_with(['.', '_']) {
        return Err(WriteError::BadRequest(format!("invalid skill id {id:?}")));
    }
    let _one = REPLACE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A restore cut short may have left the copy to restore in a swap.
    if std::fs::symlink_metadata(user_root.join(PREVIOUS_DIR)).is_ok_and(|m| m.is_dir()) {
        previous_root(user_root)?;
    }
    if !has_previous_skill(user_root, id) {
        return Err(WriteError::NotFound(format!(
            "no previous version of skill {id:?}"
        )));
    }
    let held = previous_root(user_root)?;
    let previous = held.join(id);
    let Some(current) = user_skill_dir(user_root, id) else {
        refuse_case_twin(user_root, id, "skill")?;
        return rename(&previous, &user_root.join(id)).map_err(io);
    };
    // Never removed: a swap left there is someone's only copy, and
    // `previous_root` has just put every one back.
    let swap = held.join(format!(".swap-{id}-{}", std::process::id()));
    if std::fs::symlink_metadata(&swap).is_ok() {
        return Err(WriteError::Io(format!("{} is in the way", swap.display())));
    }
    rename(&current, &swap).map_err(io)?;
    if let Err(e) = rename(&previous, &current) {
        let _ = rename(&swap, &current);
        return Err(io(e));
    }
    // The user's copy is only in the swap until this lands: tried twice,
    // and left for the next replace or restore to put back if both fail.
    if let Err(e) = rename(&swap, &previous) {
        rename(&swap, &previous).map_err(|_| io(e))?;
    }
    Ok(())
}

/// `<user_root>/.previous`, made if missing; a link or file there is
/// refused, never followed.
/// What a replace or restore cut short (the app quit between its steps)
/// left there is dealt with first ([`recover_cut_short`]).
fn previous_root(user_root: &Path) -> Result<PathBuf, WriteError> {
    let held = user_root.join(PREVIOUS_DIR);
    match std::fs::symlink_metadata(&held) {
        Ok(m) if m.is_dir() => {
            recover_cut_short(user_root, &held)?;
        }
        Ok(_) => {
            return Err(WriteError::Io(format!(
                "{} is not a folder",
                held.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&held).map_err(io)?;
        }
        Err(e) => return Err(io(e)),
    }
    Ok(held)
}

/// What a replace, install or restore leaves in `.previous` between its
/// steps, by name, with the skill id it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Left<'a> {
    /// `.incoming-<id>-<pid>`: a new copy being put in place (its id
    /// `None` when the name does not parse).
    Incoming(Option<&'a str>),
    /// `.old-<id>-<pid>`: the copy kept before, set aside by [`swap_in`].
    Old(&'a str),
    /// `.swap-<id>-<pid>`: the user skill a restore took out.
    Swap(&'a str),
    /// `.swap-<id>-<pid>-kept`, `…-kept2`…: a swap with nowhere to go.
    Kept(&'a str),
}

fn left_by(name: &str) -> Option<Left<'_>> {
    // `<id>-<pid>`: an id (`-` allowed) and a pid, digits only.
    fn id_of(rest: &str) -> Option<&str> {
        let (id, pid) = rest.rsplit_once('-')?;
        (valid_skill_id(id) && !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
            .then_some(id)
    }
    if let Some(rest) = name.strip_prefix(".incoming-") {
        return Some(Left::Incoming(id_of(rest)));
    }
    if let Some(rest) = name.strip_prefix(".old-") {
        return id_of(rest).map(Left::Old);
    }
    let rest = name.strip_prefix(".swap-")?;
    if let Some((head, tail)) = rest.rsplit_once('-')
        && let Some(n) = tail.strip_prefix("kept")
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        return id_of(head).map(Left::Kept);
    }
    id_of(rest).map(Left::Swap)
}

/// Deals with what a replace, install or restore cut short left in `held`
/// (`.previous`), never deleting a copy that may be the user's only one:
///
/// - `.incoming-<id>-<pid>`: a copy of a source being put in place. The
///   source is still there: removed. With no skill `<id>` and a
///   `.previous/<id>` folder, the replace or install stopped between
///   moving the skill aside and putting the copy in: `.previous/<id>`
///   goes back as the skill first (an `.old-<id>-…` then goes back to
///   `.previous/<id>`, whichever is dealt with first).
/// - `.old-<id>-<pid>`: the copy kept before a replace ([`swap_in`]). With
///   the skill and `.previous/<id>` both there, the replace landed and it
///   goes. With no skill, the replace stopped after moving the skill
///   aside: `.previous/<id>` goes back as the skill. Then it goes back to
///   `.previous/<id>`.
/// - `.swap-<id>-<pid>`: the user skill a restore took out, between its
///   steps the only copy of it. Moved back to the user skill `<id>` when
///   there is none (the restore stopped after its first step), else to
///   `.previous/<id>` when that is free (it stopped after its second);
///   with both there it is kept, renamed `….-kept` (`-kept2`… when taken),
///   never removed. A swap that is no folder is left alone.
///
/// An entry that cannot be dealt with is logged and left: it never stops
/// a replace or restore of any skill.
fn recover_cut_short(user_root: &Path, held: &Path) -> Result<(), WriteError> {
    for entry in std::fs::read_dir(held).map_err(io)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let done = match left_by(&name) {
            Some(Left::Incoming(id)) => recover_incoming(user_root, held, id, &path),
            Some(Left::Old(id)) => recover_old(user_root, held, id, &path),
            Some(Left::Swap(id)) => recover_swap(user_root, held, id, &name, &path),
            Some(Left::Kept(_)) | None => Ok(()),
        };
        if let Err(e) = done {
            tracing::warn!(path = %path.display(), error = %e, "left from a replace cut short, kept as is");
        }
    }
    Ok(())
}

fn recover_incoming(
    user_root: &Path,
    held: &Path,
    id: Option<&str>,
    incoming: &Path,
) -> std::io::Result<()> {
    if let Some(id) = id
        && entry_like(user_root, id).ok().flatten().is_none()
        && std::fs::symlink_metadata(held.join(id)).is_ok_and(|m| m.is_dir())
    {
        std::fs::rename(held.join(id), user_root.join(id))?;
    }
    remove_any(incoming)
}

fn recover_old(user_root: &Path, held: &Path, id: &str, old: &Path) -> std::io::Result<()> {
    let previous = held.join(id);
    let skill = entry_like(user_root, id).ok().flatten().is_some();
    let kept = std::fs::symlink_metadata(&previous).is_ok();
    match (skill, kept) {
        (true, true) => remove_any(old),
        (false, true) => {
            std::fs::rename(&previous, user_root.join(id))?;
            std::fs::rename(old, &previous)
        }
        (_, false) => std::fs::rename(old, &previous),
    }
}

fn recover_swap(
    user_root: &Path,
    held: &Path,
    id: &str,
    name: &str,
    swap: &Path,
) -> std::io::Result<()> {
    if !std::fs::symlink_metadata(swap)?.is_dir() {
        return Err(std::io::Error::other("not a folder"));
    }
    let to = if entry_like(user_root, id).ok().flatten().is_none() {
        user_root.join(id)
    } else if std::fs::symlink_metadata(held.join(id)).is_err() {
        held.join(id)
    } else {
        (1..=100)
            .map(|n| {
                let n = if n == 1 { String::new() } else { n.to_string() };
                held.join(format!("{name}-kept{n}"))
            })
            .find(|p| std::fs::symlink_metadata(p).is_err())
            .ok_or_else(|| std::io::Error::other("no free -kept name"))?
    };
    std::fs::rename(swap, to)
}

/// Drops the copy kept for the user skill `id` (the skill is deleted: a
/// new skill of that id must not offer to restore it), and what a
/// replace or restore of it cut short left (`.swap-<id>-…`, `.old-<id>-…`,
/// `.incoming-<id>-…`:
/// else the next run would bring the deleted skill back). Nothing kept is
/// fine; a `.previous` that is a link is left alone.
///
/// # Errors
/// [`WriteError::Io`], once every entry of `id` was tried.
pub fn forget_previous_skill(user_root: &Path, id: &str) -> Result<(), WriteError> {
    if !valid_skill_id(id) {
        return Ok(());
    }
    let _one = REPLACE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let held = user_root.join(PREVIOUS_DIR);
    if !std::fs::symlink_metadata(&held).is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    let mut failed = remove_any(&held.join(id)).err();
    for entry in std::fs::read_dir(&held).map_err(io)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let of_id = matches!(
            left_by(&name),
            Some(Left::Old(i) | Left::Swap(i) | Left::Kept(i) | Left::Incoming(Some(i))) if i == id
        );
        if of_id && let Err(e) = remove_any(&entry.path()) {
            failed.get_or_insert(e);
        }
    }
    failed.map_or(Ok(()), |e| Err(io(e)))
}

/// An id a skill folder can have: letters, digits and `-`, not hidden.
fn valid_skill_id(id: &str) -> bool {
    crate::design_systems::is_valid_id(id) && !id.starts_with(['.', '_'])
}

/// Removes whatever is at `path` (a folder and all in it, a file, a link
/// itself); nothing there is fine.
fn remove_any(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
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
        let dir = save_skill(
            &user,
            "deck",
            &bundled,
            false,
            &draft("deck", "New body."),
            None,
        )
        .unwrap();
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

    /// A user skill `Ad-Creative` beside the bundled `ad-creative` (PR #349
    /// review): on macOS and Windows `<user>/ad-creative` is that folder.
    /// The form opens on the bundled copy, so its tag is the bundled one,
    /// and a Save of `ad-creative` is refused, never written into the
    /// other skill.
    #[test]
    fn a_save_never_lands_in_a_skill_named_alike() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled/ad-creative");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(
            bundled.join("SKILL.md"),
            "---\nname: ad-creative\n---\nBundled.\n",
        )
        .unwrap();
        let user = tmp.path().join("user");
        let twin = user.join("Ad-Creative");
        std::fs::create_dir_all(&twin).unwrap();
        let mine = "---\nname: Ad-Creative\n---\nMy own skill.\n";
        std::fs::write(twin.join("SKILL.md"), mine).unwrap();

        assert_eq!(
            skill_write_dir(&user, "ad-creative", &bundled, false),
            bundled,
            "the form tags the copy it shows"
        );
        let tag = skill_tag(&bundled);
        let saved = save_skill(
            &user,
            "ad-creative",
            &bundled,
            false,
            &draft("ad-creative", "Typed."),
            Some(&tag),
        );
        assert!(matches!(saved, Err(WriteError::Exists(_))), "{saved:?}");
        assert_eq!(std::fs::read_to_string(twin.join("SKILL.md")).unwrap(), mine);
        let names: Vec<String> = std::fs::read_dir(&user)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Ad-Creative"], "no second folder made");

        // An override of that very name is written as before.
        std::fs::remove_dir_all(&twin).unwrap();
        let over = user.join("ad-creative");
        std::fs::create_dir_all(&over).unwrap();
        std::fs::write(over.join("SKILL.md"), "---\nname: ad-creative\n---\nOver.\n").unwrap();
        assert_eq!(skill_write_dir(&user, "ad-creative", &bundled, false), over);
        let tag = skill_tag(&over);
        save_skill(
            &user,
            "ad-creative",
            &bundled,
            false,
            &draft("ad-creative", "Typed."),
            Some(&tag),
        )
        .unwrap();
        assert!(std::fs::read_to_string(over.join("SKILL.md")).unwrap().contains("Typed."));
    }

    /// A save writes a new `SKILL.md` and renames it into place (PR #349
    /// review): a link an agent put there is replaced, the file it points
    /// at is never written, and no read finds the file half written.
    #[cfg(unix)]
    #[test]
    fn a_save_replaces_a_link_and_writes_nothing_through_it() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user");
        let dir = user.join("tea");
        std::fs::create_dir_all(&dir).unwrap();
        let outside = tmp.path().join("outside.md");
        std::fs::write(&outside, "---\nname: tea\n---\nNot yours.\n").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("SKILL.md")).unwrap();
        save_skill(&user, "tea", &dir, true, &draft("tea", "Typed."), None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "---\nname: tea\n---\nNot yours.\n",
            "written through the link"
        );
        let meta = std::fs::symlink_metadata(dir.join("SKILL.md")).unwrap();
        assert!(meta.is_file(), "the link is still there");
        assert!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap().contains("Typed."));
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 1, "a temp file left: {left:?}");
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
        // The copy replaced is kept for one restore, hidden from the index.
        assert_eq!(names(&user), [".previous", "tea-house"]);
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea-house"]);
        assert!(user.join(".previous/tea-house/assets/tokens.css").is_file());
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

    /// A script keeps its permission bits (`+x`) in the copy, as the bytes:
    /// Claude Code runs `scripts/…` as the SKILL.md says. A `chmod` after
    /// the review changes the hash: refused like any other change.
    #[cfg(unix)]
    #[test]
    fn a_copy_keeps_the_permission_bits_it_reviewed() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("data/skills");
        let dest = tmp.path().join("home/.claude/skills");
        let tea = skill_dir(
            &user,
            "tea",
            "---\nname: tea\ndescription: D.\n---\nRun scripts/x.py.\n",
        );
        std::fs::create_dir_all(tea.join("scripts")).unwrap();
        let script = tea.join("scripts/x.py");
        std::fs::write(&script, "print(1)\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let hash = hash_of(&tea);
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            export_skill(&user, &tea, "tea", &dest, &hash),
            Err(WriteError::Changed(_))
        ));
        assert!(!dest.exists(), "nothing written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (written, _) = export_skill(&user, &tea, "tea", &dest, &hash).unwrap();
        assert_eq!(mode(&written.join("scripts/x.py")), 0o755);
        // An install or an override copy keeps them too.
        let copy = tmp.path().join("copy");
        copy_dir(&tea, &copy, u64::MAX).unwrap();
        assert_eq!(mode(&copy.join("scripts/x.py")), 0o755);
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

    /// A folder the user points at through a link (`~/skills/tea` → a
    /// repo) is still copied: only links under it are skipped.
    #[cfg(unix)]
    #[test]
    fn a_copy_follows_a_link_at_its_root_only() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("scripts")).unwrap();
        std::fs::write(
            real.join("SKILL.md"),
            "---\nname: tea\ndescription: Brews tea.\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("outside.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("outside.txt"), real.join("scripts/out"))
            .unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let to = tmp.path().join("copy");
        copy_dir(&link, &to, u64::MAX).unwrap();
        assert!(to.join("SKILL.md").is_file());
        assert!(!to.join("scripts/out").exists());

        let user = tmp.path().join("user");
        assert_eq!(install_skill(&link, &user, |_| false).unwrap(), "tea");
    }

    /// An agent CLI folder `tea` (with `scripts/run.sh`) imported as the
    /// user skill `tea`: `(agent roots, source, user root)`.
    fn imported_tea(tmp: &Path) -> (Vec<(String, PathBuf)>, PathBuf, PathBuf) {
        let home = tmp.join("home");
        let user = tmp.join("data/skills");
        let roots = agent_skill_roots(&home);
        let source = skill_dir(
            &home.join(".claude/skills"),
            "tea",
            "---\nname: tea\ndescription: Calm.\n---\nUse cream.\n",
        );
        std::fs::create_dir_all(source.join("scripts")).unwrap();
        std::fs::write(source.join("scripts/run.sh"), "echo tea\n").unwrap();
        assert_eq!(install_skill(&source, &user, |_| false).unwrap(), "tea");
        (roots, source, user)
    }

    fn tea_row(roots: &[(String, PathBuf)], user: &Path) -> LocalSkill {
        scan_agent_skills(roots, user)
            .into_iter()
            .find(|s| s.id == "tea")
            .unwrap()
    }

    /// Every file under `dir` with its bytes and permission bits, hidden
    /// ones too: what a restore must bring back exactly.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, nofollow::ReadFile)> {
        nofollow::read_tree(dir, |_, _| true, u64::MAX)
            .unwrap()
            .files
    }

    #[test]
    fn an_imported_skill_differs_once_either_side_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let (roots, source, user) = imported_tea(tmp.path());
        let row = tea_row(&roots, &user);
        assert!(row.imported);
        assert!(!row.differs, "just imported: the same");
        // The source's SKILL.md edited, then put back.
        let md = std::fs::read(source.join("SKILL.md")).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nUse milk.\n").unwrap();
        assert!(tea_row(&roots, &user).differs, "SKILL.md edited");
        std::fs::write(source.join("SKILL.md"), &md).unwrap();
        assert!(!tea_row(&roots, &user).differs);
        // A side file only.
        std::fs::write(source.join("scripts/run.sh"), "echo green tea\n").unwrap();
        assert!(tea_row(&roots, &user).differs, "a side file edited");
        std::fs::write(source.join("scripts/run.sh"), "echo tea\n").unwrap();
        assert!(!tea_row(&roots, &user).differs);
        // Its permission bits only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let run = source.join("scripts/run.sh");
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(tea_row(&roots, &user).differs, "chmod +x");
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!tea_row(&roots, &user).differs);
        }
        // What an install leaves out (a hidden file) is no difference.
        std::fs::write(source.join(".DS_Store"), "x").unwrap();
        assert!(
            !tea_row(&roots, &user).differs,
            "a hidden file is not copied"
        );
        // The copy in Contour edited.
        std::fs::write(user.join("tea/notes.md"), "mine").unwrap();
        assert!(tea_row(&roots, &user).differs, "edited in Contour");
        // Not imported: never differs.
        std::fs::remove_dir_all(user.join("tea")).unwrap();
        let row = tea_row(&roots, &user);
        assert!(!row.imported && !row.differs);
    }

    #[test]
    fn a_replace_keeps_the_copy_in_contour_for_one_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let (roots, source, user) = imported_tea(tmp.path());
        // Edited in Contour: SKILL.md, a new file, a mode.
        std::fs::write(user.join("tea/SKILL.md"), "---\nname: tea\n---\nMine.\n").unwrap();
        std::fs::write(user.join("tea/notes.md"), "mine").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let run = user.join("tea/scripts/run.sh");
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let edited = snapshot(&user.join("tea"));
        // And the source edited too.
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nUse milk.\n").unwrap();

        let preview = source_preview(&source, &user, "tea").unwrap();
        assert_eq!(
            preview.source.skill_md(),
            "---\nname: tea\n---\nUse milk.\n"
        );
        assert_eq!(preview.current.skill_md(), "---\nname: tea\n---\nMine.\n");
        let mut want = vec!["SKILL.md", "notes.md"];
        if cfg!(unix) {
            want.push("scripts/run.sh");
        }
        assert_eq!(preview.changed_files, want);
        assert_eq!(preview.source.hash(), hash_of_copy(&source));
        // Nothing written by the preview.
        assert!(!user.join(PREVIOUS_DIR).exists());

        let copied =
            replace_skill_from_source(&source, &user, "tea", &preview.source.hash()).unwrap();
        assert_eq!(copied.hash(), preview.source.hash());
        assert_eq!(snapshot(&user.join("tea")), snapshot(&source));
        assert_eq!(
            snapshot(&user.join(".previous/tea")),
            edited,
            "the edited copy is kept, byte for byte"
        );
        assert!(has_previous_skill(&user, "tea"));
        assert!(
            !tea_row(&roots, &user).differs,
            "the same as its source now"
        );
        assert_eq!(names(&user), [".previous", "tea"]);
        assert_eq!(
            names(&user.join(".previous")),
            ["tea"],
            "nothing left behind"
        );
        // Not listed as a skill, and no user skill named `.previous`.
        assert_eq!(
            crate::skills::list_skills(std::slice::from_ref(&user), true)
                .iter()
                .map(|e| e.dir.clone())
                .collect::<Vec<_>>(),
            [user.join("tea")]
        );

        // Restore: the edited copy back, exactly; the source's kept.
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), edited);
        assert_eq!(snapshot(&user.join(".previous/tea")), snapshot(&source));
        // One level: a second restore swaps them again.
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), snapshot(&source));
        assert_eq!(snapshot(&user.join(".previous/tea")), edited);
        assert_eq!(names(&user.join(".previous")), ["tea"]);

        // A second replace keeps only the copy it took out.
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nUse honey.\n").unwrap();
        let before = snapshot(&user.join("tea"));
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        assert_eq!(snapshot(&user.join(".previous/tea")), before);
    }

    /// [`ExportFiles::read_copy`]'s hash, as the preview shows it.
    fn hash_of_copy(source: &Path) -> String {
        ExportFiles::read_copy(source).unwrap().hash()
    }

    #[test]
    fn a_replace_writes_nothing_for_a_source_changed_or_renamed() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, source, user) = imported_tea(tmp.path());
        std::fs::write(user.join("tea/notes.md"), "mine").unwrap();
        let mine = snapshot(&user.join("tea"));
        let shown = hash_of_copy(&source);
        // Changed after the preview.
        std::fs::write(source.join("scripts/run.sh"), "rm -rf ~\n").unwrap();
        assert!(matches!(
            replace_skill_from_source(&source, &user, "tea", &shown),
            Err(WriteError::Changed(_))
        ));
        assert_eq!(snapshot(&user.join("tea")), mine);
        assert!(!user.join(PREVIOUS_DIR).exists(), "nothing written");
        // Its name changed: another skill now, even as reviewed.
        std::fs::write(source.join("SKILL.md"), "---\nname: tea2\n---\nOther.\n").unwrap();
        assert!(matches!(
            replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)),
            Err(WriteError::BadRequest(_))
        ));
        assert_eq!(snapshot(&user.join("tea")), mine);
        assert!(!user.join(PREVIOUS_DIR).exists(), "nothing written");
        // No user skill of that id.
        std::fs::write(source.join("SKILL.md"), "---\nname: mint\n---\nOther.\n").unwrap();
        assert!(matches!(
            replace_skill_from_source(&source, &user, "mint", &hash_of_copy(&source)),
            Err(WriteError::NotFound(_))
        ));
        // Nothing to restore.
        assert!(!has_previous_skill(&user, "tea"));
        assert!(matches!(
            restore_previous_skill(&user, "tea"),
            Err(WriteError::NotFound(_))
        ));
        assert_eq!(snapshot(&user.join("tea")), mine);
    }

    /// The source's `SKILL.md` a link, a side file swapped for a link or a
    /// FIFO: no link is followed, no FIFO waited on, nothing of them
    /// written.
    #[cfg(unix)]
    #[test]
    fn a_replace_follows_no_link_and_waits_on_no_fifo() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let (roots, source, user) = imported_tea(tmp.path());
        let secret = tmp.path().join("secret.md");
        std::fs::write(&secret, "---\nname: tea\n---\ns3cret\n").unwrap();
        let mine = snapshot(&user.join("tea"));
        // SKILL.md a link: no preview, no replace whatever the hash.
        std::fs::remove_file(source.join("SKILL.md")).unwrap();
        symlink(&secret, source.join("SKILL.md")).unwrap();
        assert!(!tea_row(&roots, &user).differs);
        assert!(source_preview(&source, &user, "tea").is_err());
        let no_md = ExportFiles::read_any(&source, true).unwrap().hash();
        for hash in [no_md.as_str(), "x"] {
            assert!(replace_skill_from_source(&source, &user, "tea", hash).is_err());
        }
        assert_eq!(snapshot(&user.join("tea")), mine);
        assert!(!user.join(PREVIOUS_DIR).exists());
        // A side file a link, another a FIFO: read and left out, at once.
        std::fs::remove_file(source.join("SKILL.md")).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
        std::fs::remove_file(source.join("scripts/run.sh")).unwrap();
        symlink(&secret, source.join("scripts/run.sh")).unwrap();
        nix::unistd::mkfifo(&source.join("assets/pipe"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let (src, usr) = (source.clone(), user.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let preview = source_preview(&src, &usr, "tea").unwrap();
            let done = replace_skill_from_source(&src, &usr, "tea", &preview.source.hash());
            tx.send((preview.changed_files, done.is_ok())).unwrap();
        });
        let (changed, done) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a FIFO never blocks the replace");
        assert_eq!(
            changed,
            ["SKILL.md", "scripts/run.sh"],
            "run.sh is gone from the copy"
        );
        assert!(done);
        let now = snapshot(&user.join("tea"));
        let paths: Vec<_> = now.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(
            paths,
            [
                PathBuf::from("SKILL.md"),
                PathBuf::from("assets/tokens.css")
            ]
        );
        assert!(
            !now.iter()
                .any(|(_, f)| f.bytes.windows(6).any(|w| w == b"s3cret"))
        );
    }

    #[test]
    fn only_a_listed_agent_folder_is_taken() {
        let tmp = tempfile::tempdir().unwrap();
        let (roots, source, user) = imported_tea(tmp.path());
        let row = agent_skill_at(&roots, &source, &user).unwrap();
        assert_eq!(row, tea_row(&roots, &user));
        // Not right under an agent root: refused.
        let elsewhere = skill_dir(tmp.path(), "tea", "---\nname: tea\n---\n");
        assert!(agent_skill_at(&roots, &elsewhere, &user).is_none());
        let deeper = skill_dir(&source, "tea", "---\nname: tea\n---\n");
        assert!(agent_skill_at(&roots, &deeper, &user).is_none());
        let dotted = source.join("..").join("tea");
        assert!(agent_skill_at(&roots, &dotted, &user).is_none());
        let hidden = skill_dir(&home_skills(tmp.path()), ".tea", "---\nname: tea\n---\n");
        assert!(agent_skill_at(&roots, &hidden, &user).is_none());
        #[cfg(unix)]
        {
            let link = home_skills(tmp.path()).join("linked");
            std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
            assert!(agent_skill_at(&roots, &link, &user).is_none());
        }
    }

    fn home_skills(tmp: &Path) -> PathBuf {
        tmp.join("home/.claude/skills")
    }

    /// The user skill deleted after a replace: Restore brings the kept
    /// copy back as the skill. An entry of that name in another case that
    /// is no skill folder (a file) is in the way: refused, both kept.
    #[test]
    fn a_restore_brings_back_a_deleted_skill_unless_a_twin_is_in_the_way() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, source, user) = imported_tea(tmp.path());
        std::fs::write(user.join("tea/notes.md"), "mine").unwrap();
        let edited = snapshot(&user.join("tea"));
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        delete_user_dir(&user, &user.join("tea")).unwrap();
        // A file named `Tea`: in the way of `tea` on every file system.
        std::fs::write(user.join("Tea"), "not a skill").unwrap();
        assert!(matches!(
            restore_previous_skill(&user, "tea"),
            Err(WriteError::Exists(_))
        ));
        assert_eq!(snapshot(&user.join(".previous/tea")), edited, "kept");
        assert_eq!(std::fs::read(user.join("Tea")).unwrap(), b"not a skill");
        std::fs::remove_file(user.join("Tea")).unwrap();
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), edited, "back as the skill");
        assert!(!has_previous_skill(&user, "tea"), "nothing kept any more");
        assert_eq!(names(&user.join(".previous")), Vec::<String>::new());
    }

    /// `.previous` a link to a folder elsewhere: a replace writes nothing
    /// there and keeps the skill; restore and forget never follow it.
    #[cfg(unix)]
    #[test]
    fn a_previous_folder_that_is_a_link_is_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, source, user) = imported_tea(tmp.path());
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(outside.join("tea")).unwrap();
        std::fs::write(outside.join("tea/keep.txt"), "theirs").unwrap();
        std::os::unix::fs::symlink(&outside, user.join(PREVIOUS_DIR)).unwrap();
        let mine = snapshot(&user.join("tea"));
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
        assert!(matches!(
            replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)),
            Err(WriteError::Io(_))
        ));
        assert_eq!(snapshot(&user.join("tea")), mine);
        assert_eq!(names(&outside), ["tea"]);
        assert_eq!(names(&outside.join("tea")), ["keep.txt"]);
        assert!(!has_previous_skill(&user, "tea"));
        assert!(matches!(
            restore_previous_skill(&user, "tea"),
            Err(WriteError::NotFound(_))
        ));
        forget_previous_skill(&user, "tea").unwrap();
        assert_eq!(names(&outside.join("tea")), ["keep.txt"]);
        assert_eq!(snapshot(&user.join("tea")), mine);
    }

    /// Only an id is asked about: a path (`/…`, `..`) is no skill.
    #[test]
    fn a_kept_copy_is_looked_up_by_id_only() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, source, user) = imported_tea(tmp.path());
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        assert!(has_previous_skill(&user, "tea"));
        let outside = tmp.path().to_string_lossy().into_owned();
        for id in [outside.as_str(), "..", "../skills", ".", ""] {
            assert!(!has_previous_skill(&user, id), "{id:?}");
            assert!(restore_previous_skill(&user, id).is_err(), "{id:?}");
            forget_previous_skill(&user, id).unwrap();
        }
        assert!(user.join("tea").is_dir() && has_previous_skill(&user, "tea"));
        // Forgotten (the skill deleted): nothing to restore.
        forget_previous_skill(&user, "tea").unwrap();
        assert!(!has_previous_skill(&user, "tea"));
    }

    /// A replace cut short leaves `.incoming-…`, a copy of a source still
    /// there: the next replace removes it.
    #[test]
    fn what_a_replace_cut_short_left_is_removed_next_time() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, source, user) = imported_tea(tmp.path());
        std::fs::create_dir_all(user.join(PREVIOUS_DIR).join(".incoming-tea-1")).unwrap();
        std::fs::write(user.join(".previous/.incoming-tea-1/SKILL.md"), "x").unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);
    }

    /// Files under a folder, as [`snapshot`] reads them.
    type Snapshot = Vec<(PathBuf, nofollow::ReadFile)>;

    /// A restore swaps in three renames: `tea` → `.swap-tea-<pid>`,
    /// `.previous/tea` → `tea`, the swap → `.previous/tea`. Cut short after
    /// either of the first two, the swap is the only copy of the skill the
    /// user had: the next replace (of any skill) or restore puts it back,
    /// never removes it. `(A, B)`: the copy restored and the user's.
    fn cut_short_restore(tmp: &Path) -> (PathBuf, PathBuf, Snapshot, Snapshot) {
        let (_, source, user) = imported_tea(tmp);
        let a = snapshot(&user.join("tea"));
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        // The user's own edit of the copy replaced: only here, not at the source.
        std::fs::write(user.join("tea/notes.md"), "the user's only copy").unwrap();
        let b = snapshot(&user.join("tea"));
        (source, user, a, b)
    }

    #[test]
    fn a_restore_cut_short_after_its_second_step_loses_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, user, a, b) = cut_short_restore(tmp.path());
        let swap = user.join(".previous/.swap-tea-424242");
        std::fs::rename(user.join("tea"), &swap).unwrap();
        std::fs::rename(user.join(".previous/tea"), user.join("tea")).unwrap();
        // Another skill replaced next: the swap goes where the restore
        // would have put it.
        let mint_src = skill_dir(
            &tmp.path().join("home/.claude/skills"),
            "mint",
            "---\nname: mint\n---\nMint.\n",
        );
        install_skill(&mint_src, &user, |_| false).unwrap();
        std::fs::write(mint_src.join("SKILL.md"), "---\nname: mint\n---\nMint 2.\n").unwrap();
        replace_skill_from_source(&mint_src, &user, "mint", &hash_of_copy(&mint_src)).unwrap();
        assert_eq!(snapshot(&user.join("tea")), a);
        assert_eq!(
            snapshot(&user.join(".previous/tea")),
            b,
            "the user's copy is kept"
        );
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["mint", "tea"]);
        // And a restore brings it back.
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), b);
    }

    /// Cut short after its second step, then Restore pressed again (no
    /// replace in between): it finds the user's copy first, then swaps.
    #[test]
    fn a_restore_asked_again_after_its_second_step_finds_the_users_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, user, a, b) = cut_short_restore(tmp.path());
        std::fs::rename(user.join("tea"), user.join(".previous/.swap-tea-424242")).unwrap();
        std::fs::rename(user.join(".previous/tea"), user.join("tea")).unwrap();
        assert!(!has_previous_skill(&user, "tea"));
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), b, "the user's copy is back");
        assert_eq!(snapshot(&user.join(".previous/tea")), a);
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);
    }

    #[test]
    fn a_restore_cut_short_after_its_first_step_loses_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, user, a, b) = cut_short_restore(tmp.path());
        std::fs::rename(user.join("tea"), user.join(".previous/.swap-tea-424242")).unwrap();
        // No user skill `tea` now; the restore asked again finds the swap
        // back as the skill first, then swaps as asked.
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), a);
        assert_eq!(
            snapshot(&user.join(".previous/tea")),
            b,
            "the user's copy is kept"
        );
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);
    }

    /// A swap with both the skill and its kept copy there is kept too,
    /// under a name no later run takes for a swap of its own.
    #[test]
    fn a_swap_with_nowhere_to_go_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let (source, user, _, b) = cut_short_restore(tmp.path());
        let swap = user.join(".previous/.swap-tea-7");
        std::fs::create_dir_all(&swap).unwrap();
        for (p, f) in &b {
            std::fs::create_dir_all(swap.join(p).parent().unwrap()).unwrap();
            std::fs::write(swap.join(p), &f.bytes).unwrap();
        }
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNewer.\n").unwrap();
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        let kept = user.join(".previous/.swap-tea-7-kept");
        assert_eq!(
            std::fs::read(kept.join("notes.md")).unwrap(),
            b"the user's only copy"
        );
        // Not a swap any more: the next run leaves it where it is.
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        restore_previous_skill(&user, "tea").unwrap();
        assert!(kept.join("notes.md").is_file());
        assert_eq!(names(&user.join(PREVIOUS_DIR)), [".swap-tea-7-kept", "tea"]);
    }

    /// A rename that fails at the steps in `fail` (1-based, counted over
    /// the run), and stops the run there as the app quitting would at
    /// step `stop` (before it is done).
    fn steps(
        fail: &'static [usize],
        stop: usize,
    ) -> impl FnMut(&Path, &Path) -> std::io::Result<()> {
        let mut n = 0;
        move |from, to| {
            n += 1;
            assert!(n != stop, "quit at step {stop}");
            if fail.contains(&n) {
                return Err(std::io::Error::other(format!("step {n} failed")));
            }
            std::fs::rename(from, to)
        }
    }

    /// `tea` replaced once: `.previous/tea` is the first copy (`A`), and
    /// `tea` is edited after (`B`, the user's only copy of it). The source
    /// then changed (`N`).
    fn replaced_and_edited(tmp: &Path) -> (PathBuf, PathBuf, Snapshot, Snapshot) {
        let (source, user, a, b) = cut_short_restore(tmp);
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNewer.\n").unwrap();
        (source, user, a, b)
    }

    fn tea_and_kept(user: &Path) -> (Snapshot, Snapshot) {
        (
            snapshot(&user.join("tea")),
            snapshot(&user.join(".previous/tea")),
        )
    }

    /// A replace that fails at any of its renames leaves both copies as
    /// they were: the kept one too (it was removed first, before #346).
    #[test]
    fn a_replace_that_fails_keeps_the_copy_kept_before() {
        for step in [1, 2, 3] {
            let tmp = tempfile::tempdir().unwrap();
            let (source, user, a, b) = replaced_and_edited(tmp.path());
            let fail: &'static [usize] = match step {
                1 => &[1],
                2 => &[2],
                _ => &[3],
            };
            let hash = hash_of_copy(&source);
            assert!(
                replace_skill_from_source_with(&source, &user, "tea", &hash, &mut steps(fail, 0))
                    .is_err(),
                "step {step}"
            );
            assert_eq!(tea_and_kept(&user), (b, a), "step {step}");
            assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"], "step {step}");
        }
    }

    /// The same on a real file system: the skill cannot move out of
    /// `<data>/skills` (read-only), `.previous` can be written.
    #[cfg(unix)]
    #[test]
    fn a_replace_that_cannot_move_the_skill_keeps_the_copy_kept_before() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let (source, user, a, b) = replaced_and_edited(tmp.path());
        std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o555)).unwrap();
        let done = replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source));
        std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(done, Err(WriteError::Io(_))));
        assert_eq!(tea_and_kept(&user), (b, a));
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);
    }

    /// The app quits before any step of a replace: what the next run finds
    /// is the state before the replace or after it, nothing in between and
    /// nothing left over.
    #[test]
    fn a_replace_cut_short_at_any_step_loses_nothing() {
        for stop in 1..=4 {
            let tmp = tempfile::tempdir().unwrap();
            let (source, user, a, b) = replaced_and_edited(tmp.path());
            let hash = hash_of_copy(&source);
            let quit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                replace_skill_from_source_with(&source, &user, "tea", &hash, &mut steps(&[], stop))
                    .unwrap();
            }));
            assert_eq!(quit.is_err(), stop <= 3, "step {stop}");
            previous_root(&user).unwrap();
            let now = tea_and_kept(&user);
            let after = (snapshot(&source), b.clone());
            assert!(now == (b.clone(), a.clone()) || now == after, "step {stop}");
            assert_eq!(now == after, stop > 3, "step {stop}");
            assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"], "step {stop}");
            assert_eq!(names(&user), [".previous", "tea"], "step {stop}");
        }
        // Landed, then quit before the older copy went: the next run
        // removes it, and only it.
        let tmp = tempfile::tempdir().unwrap();
        let (source, user, _, b) = replaced_and_edited(tmp.path());
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        std::fs::create_dir_all(user.join(".previous/.old-tea-9/x")).unwrap();
        previous_root(&user).unwrap();
        assert_eq!(tea_and_kept(&user), (snapshot(&source), b));
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);
    }

    /// Install again over a user skill of that id: the copy there is kept
    /// for one restore, as a replace keeps it; cut short at any step, the
    /// next run finds it before or after.
    #[test]
    fn an_install_over_a_skill_keeps_it_for_one_restore() {
        for stop in 1..=4 {
            let tmp = tempfile::tempdir().unwrap();
            let (source, user, a, b) = replaced_and_edited(tmp.path());
            let quit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                install_skill_with(&source, &user, "tea", &mut steps(&[], stop)).unwrap();
            }));
            assert_eq!(quit.is_err(), stop <= 3, "step {stop}");
            previous_root(&user).unwrap();
            let now = tea_and_kept(&user);
            let after = (snapshot(&source), b.clone());
            assert_eq!(
                now,
                if stop > 3 {
                    after
                } else {
                    (b.clone(), a.clone())
                },
                "step {stop}"
            );
            assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"], "step {stop}");
        }
        // Through `install_skill`, with no copy kept yet.
        let tmp = tempfile::tempdir().unwrap();
        let (_, source, user) = imported_tea(tmp.path());
        std::fs::write(user.join("tea/notes.md"), "mine").unwrap();
        let mine = snapshot(&user.join("tea"));
        install_skill(&source, &user, |_| false).unwrap();
        assert_eq!(snapshot(&user.join("tea")), snapshot(&source));
        assert_eq!(snapshot(&user.join(".previous/tea")), mine);
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(snapshot(&user.join("tea")), mine);
    }

    /// The first replace or install of a skill (nothing kept yet), the app
    /// quitting before any step: the next run finds the skill as it was,
    /// or the new copy with the old one kept; never the skill hidden in
    /// `.previous` alone.
    #[test]
    fn a_first_replace_or_install_cut_short_at_any_step_loses_nothing() {
        for install in [false, true] {
            for stop in 1..=3 {
                let tmp = tempfile::tempdir().unwrap();
                let (_, source, user) = imported_tea(tmp.path());
                std::fs::write(user.join("tea/notes.md"), "mine").unwrap();
                let mine = snapshot(&user.join("tea"));
                std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNew.\n").unwrap();
                let hash = hash_of_copy(&source);
                let quit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if install {
                        install_skill_with(&source, &user, "tea", &mut steps(&[], stop)).unwrap();
                    } else {
                        replace_skill_from_source_with(
                            &source,
                            &user,
                            "tea",
                            &hash,
                            &mut steps(&[], stop),
                        )
                        .unwrap();
                    }
                }));
                let at = format!("install {install}, step {stop}");
                assert_eq!(quit.is_err(), stop <= 2, "{at}");
                previous_root(&user).unwrap();
                assert_eq!(names(&user), [".previous", "tea"], "{at}");
                if stop <= 2 {
                    assert_eq!(snapshot(&user.join("tea")), mine, "{at}");
                    assert_eq!(
                        names(&user.join(PREVIOUS_DIR)),
                        Vec::<String>::new(),
                        "{at}"
                    );
                } else {
                    assert_eq!(tea_and_kept(&user), (snapshot(&source), mine), "{at}");
                    assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"], "{at}");
                }
                assert_eq!(
                    crate::skills::list_skills(std::slice::from_ref(&user), true).len(),
                    1,
                    "{at}: listed"
                );
            }
        }
    }

    /// A first install cut short left the skill in `.previous` with its
    /// `.incoming` mark; bringing it back fails (no write permission, a
    /// file held open on Windows): the mark stays, so the next run tries
    /// again and the skill comes back, never left hidden in `.previous`.
    #[cfg(unix)]
    #[test]
    fn a_bring_back_that_fails_keeps_its_mark() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("skills");
        let held = user.join(PREVIOUS_DIR);
        std::fs::create_dir_all(held.join("tea")).unwrap();
        std::fs::write(held.join("tea/SKILL.md"), "---\nname: tea\n---\nMine.\n").unwrap();
        std::fs::create_dir(held.join(".incoming-tea-7")).unwrap();
        let mode = |m: u32| {
            std::fs::set_permissions(&user, std::fs::Permissions::from_mode(m)).unwrap();
        };
        mode(0o555);
        let _ = previous_root(&user);
        mode(0o755);
        assert_eq!(names(&held), [".incoming-tea-7", "tea"], "the mark stays");
        previous_root(&user).unwrap();
        assert_eq!(names(&user), [".previous", "tea"]);
        assert_eq!(names(&held), Vec::<String>::new());
    }

    /// The app quits before any step of a restore: the next run finds the
    /// state before or after it.
    #[test]
    fn a_restore_cut_short_at_any_step_loses_nothing() {
        for stop in 1..=4 {
            let tmp = tempfile::tempdir().unwrap();
            let (_, user, a, b) = cut_short_restore(tmp.path());
            let quit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                restore_previous_skill_with(&user, "tea", &mut steps(&[], stop)).unwrap();
            }));
            assert_eq!(quit.is_err(), stop <= 3, "step {stop}");
            previous_root(&user).unwrap();
            let now = tea_and_kept(&user);
            assert!(
                now == (b.clone(), a.clone()) || now == (a.clone(), b.clone()),
                "step {stop}"
            );
            assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"], "step {stop}");
        }
    }

    /// The last step of a restore (the user's copy into `.previous`) is
    /// tried twice: one failure is no failure; two leave the swap for the
    /// next run, which puts it where it was going.
    #[test]
    fn a_restore_tries_its_last_step_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, user, a, b) = cut_short_restore(tmp.path());
        restore_previous_skill_with(&user, "tea", &mut steps(&[3], 0)).unwrap();
        assert_eq!(tea_and_kept(&user), (a.clone(), b.clone()));
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);

        let tmp = tempfile::tempdir().unwrap();
        let (_, user, a, b) = cut_short_restore(tmp.path());
        assert!(restore_previous_skill_with(&user, "tea", &mut steps(&[3, 4], 0)).is_err());
        assert_eq!(snapshot(&user.join("tea")), a);
        assert!(!has_previous_skill(&user, "tea"));
        previous_root(&user).unwrap();
        assert_eq!(tea_and_kept(&user), (a, b));
        assert_eq!(names(&user.join(PREVIOUS_DIR)), ["tea"]);
    }

    /// A restore cut short after its second step, then the skill deleted:
    /// what it left goes with it, and no later run brings it back.
    #[test]
    fn a_deleted_skill_never_comes_back_from_what_a_restore_left() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, user, _, _) = cut_short_restore(tmp.path());
        let held = user.join(PREVIOUS_DIR);
        std::fs::rename(user.join("tea"), held.join(".swap-tea-424242")).unwrap();
        std::fs::rename(held.join("tea"), user.join("tea")).unwrap();
        // Older leftovers of `tea` too, and one of another skill kept.
        std::fs::create_dir_all(held.join(".swap-tea-7-kept/x")).unwrap();
        std::fs::create_dir_all(held.join(".old-tea-8")).unwrap();
        std::fs::create_dir_all(held.join(".incoming-tea-3")).unwrap();
        std::fs::create_dir_all(held.join(".swap-tea-house-9-kept")).unwrap();
        delete_user_dir(&user, &user.join("tea")).unwrap();
        forget_previous_skill(&user, "tea").unwrap();
        assert_eq!(names(&held), [".swap-tea-house-9-kept"]);
        previous_root(&user).unwrap();
        assert!(!user.join("tea").exists());
        assert!(!has_previous_skill(&user, "tea"));
    }

    /// A swap whose `-kept` name is taken gets the next one, and one that
    /// cannot move (or is no folder) is left with a warning: either way
    /// every other skill still replaces and restores.
    #[test]
    fn one_leftover_that_cannot_move_stops_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let (source, user, a, b) = cut_short_restore(tmp.path());
        let held = user.join(PREVIOUS_DIR);
        for kept in [".swap-tea-5", ".swap-tea-5-kept"] {
            std::fs::create_dir_all(held.join(kept)).unwrap();
            std::fs::write(held.join(kept).join("notes.md"), kept).unwrap();
        }
        // No folder: a file, and on Unix a link to a folder elsewhere.
        std::fs::write(held.join(".swap-mint-6"), "not a folder").unwrap();
        #[cfg(unix)]
        {
            let outside = tmp.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, held.join(".swap-lime-7")).unwrap();
        }
        previous_root(&user).unwrap();
        assert_eq!(
            std::fs::read(held.join(".swap-tea-5-kept2/notes.md")).unwrap(),
            b".swap-tea-5"
        );
        assert!(!user.join("mint").exists() && !user.join("lime").exists());
        assert!(held.join(".swap-mint-6").is_file());
        std::fs::write(source.join("SKILL.md"), "---\nname: tea\n---\nNewer.\n").unwrap();
        replace_skill_from_source(&source, &user, "tea", &hash_of_copy(&source)).unwrap();
        restore_previous_skill(&user, "tea").unwrap();
        assert_eq!(tea_and_kept(&user), (b, snapshot(&source)));
        let _ = a;
    }
}
