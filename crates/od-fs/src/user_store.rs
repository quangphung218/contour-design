//! One door to a user's files in a folder an agent can write (`<data>/skills`,
//! `<data>/design-systems`, `<data>/memory`): [`UserRoot`] names the folder and
//! holds the one lock every writer of it takes; [`Guard`] (the lock held) reads
//! and writes a file under it by a plain relative path.
//!
//! - The root and the entry under it (the first folder of a path: a
//!   skill's or a design system's folder, which may be the user's own
//!   link, as the index takes it) are followed, each resolved once per
//!   call. Every folder below the entry is opened from the one above it
//!   with `O_NOFOLLOW` and every file through its folder's descriptor (unix
//!   `openat`/`renameat`): no link below the entry, and no link at the file,
//!   is ever followed.
//! - A read is bounded (the root's limit), waits on no FIFO, and tells a
//!   file that is not there ([`Found::Missing`]) from one that is there but
//!   cannot be read ([`Found::Unreadable`]: a link, a FIFO, too large).
//! - A [`Tag`] is made only by a read or a write here (or parsed from the
//!   `If-Match` a form sends back): it is the tag of the bytes read with it.
//! - A write compares what is there with its [`Expect`] under the lock, then
//!   writes a new file of a random name (`O_CREAT|O_EXCL|O_NOFOLLOW`), syncs
//!   it and renames it over the file: no half-written read, no write through
//!   a link at the file or at the temp name.
//!
//! The lock holds this process's writers apart; an agent writing from its
//! own process can still land between the compare and the rename.

use crate::nofollow::{self, ReadFile};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// The tag of the content of a file read or written here: sha256 of its
/// bytes, hex; [`Tag::UNREADABLE`] for a file there that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag(String);

impl Tag {
    /// The tag of a file that is there but cannot be read (a link, a FIFO,
    /// too large): a form opened on it may write a good file over it.
    pub const UNREADABLE: &str = "unreadable";

    /// The tag a form sent back (`If-Match`, its quotes taken off).
    #[must_use]
    pub fn parse(sent: &str) -> Self {
        Self(sent.to_owned())
    }

    fn of(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self(hex::encode(Sha256::digest(bytes)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A file read: its bytes and their tag, from the same read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub bytes: Vec<u8>,
    pub tag: Tag,
}

/// Why a file that is there cannot be read.
#[derive(Debug)]
pub enum Unreadable {
    /// The file, or a folder on its way, is a link.
    Link,
    /// Not a regular file (a FIFO, a socket, a folder).
    NotAFile,
    /// Past the root's limit.
    TooLarge,
    Io(std::io::Error),
}

/// What a read found.
#[derive(Debug)]
pub enum Found {
    File(Read),
    /// Nothing at that name (nor at a folder on its way).
    Missing,
    Unreadable(Unreadable),
}

impl Found {
    /// The [`Expect`] of a write that goes only over what this read found.
    #[must_use]
    pub fn expect(&self) -> Expect {
        match self {
            Self::File(read) => Expect::Tag(read.tag.clone()),
            Self::Missing => Expect::Absent,
            Self::Unreadable(_) => Expect::Tag(Tag::parse(Tag::UNREADABLE)),
        }
    }
}

/// What a write expects to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// The file with this tag ([`Tag::UNREADABLE`]: one that cannot be read).
    Tag(Tag),
    /// No file.
    Absent,
    /// Whatever is there.
    Any,
}

/// Why a write wrote nothing.
#[derive(Debug)]
pub enum WriteError {
    /// What is there is not what was expected.
    Stale,
    Io(std::io::Error),
}

impl From<std::io::Error> for WriteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A folder of user files and its lock.
#[derive(Debug, Clone)]
pub struct UserRoot {
    path: PathBuf,
    limit: u64,
    lock: &'static Mutex<()>,
}

impl UserRoot {
    /// The folder `path` (it may not exist yet; it may be a link), its
    /// files read up to `limit` bytes. One lock per `path`, shared by every
    /// [`UserRoot`] opened on it in this process. The same limit bounds a
    /// read and a write's compare, so both agree on a file too large.
    #[must_use]
    pub fn open(path: &Path, limit: u64) -> Self {
        type Locks = Mutex<HashMap<PathBuf, &'static Mutex<()>>>;
        static LOCKS: OnceLock<Locks> = OnceLock::new();
        let lock = *LOCKS
            .get_or_init(Locks::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(path.to_path_buf())
            .or_insert_with(|| Box::leak(Box::default()));
        Self {
            path: path.to_path_buf(),
            limit,
            lock,
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Waits for the root's other writers; reads and writes go through the
    /// [`Guard`].
    #[must_use]
    pub fn lock(&self) -> Guard<'_> {
        Guard {
            root: self,
            _held: self.lock.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }
}

/// The root's lock, held.
#[derive(Debug)]
pub struct Guard<'a> {
    root: &'a UserRoot,
    _held: MutexGuard<'static, ()>,
}

impl Guard<'_> {
    /// Reads the file at `rel` (a plain relative path under the root).
    #[must_use]
    pub fn read(&self, rel: &Path) -> Found {
        match read_under(&self.root.path, rel, self.root.limit) {
            Ok(Some(file)) => Found::File(Read {
                tag: Tag::of(&file.bytes),
                bytes: file.bytes,
            }),
            Ok(None) => Found::Missing,
            Err(why) => Found::Unreadable(why),
        }
    }

    /// Writes `bytes` as the file at `rel` when what is there is what
    /// `expect` says; the folders on its way must be there. A file there
    /// keeps its permission bits; a read-only one is refused
    /// (`PermissionDenied`), as a write in place would be.
    ///
    /// # Errors
    /// [`WriteError::Stale`] with nothing written; [`WriteError::Io`] (`rel`
    /// not plain, a folder on its way a link or missing, the write's).
    pub fn write(&self, rel: &Path, bytes: &[u8], expect: Expect) -> Result<Tag, WriteError> {
        let fits = match (&expect, self.read(rel)) {
            (Expect::Any, _) | (Expect::Absent, Found::Missing) => true,
            (Expect::Tag(t), Found::File(read)) => *t == read.tag,
            (Expect::Tag(t), Found::Unreadable(_)) => t.as_str() == Tag::UNREADABLE,
            _ => false,
        };
        if !fits {
            return Err(WriteError::Stale);
        }
        replace_under(&self.root.path, rel, bytes)?;
        Ok(Tag::of(bytes))
    }

    /// Removes the entry `rel` (one plain name under the root, not hidden):
    /// a folder with all it holds, or, when the entry is a link (the user's
    /// own), the link alone, never what it points at. A link below the
    /// entry is removed as a link, never followed.
    ///
    /// Crash-safe: a journal `.journal-<tag>` is written first, then the
    /// entry is renamed to `.removing-<tag>` (hidden: no index lists it),
    /// then removed, then the journal. An app stopped on the way leaves
    /// the entry whole (before the rename) or hidden for
    /// [`UserRoot::recover`] to finish (after it).
    ///
    /// # Errors
    /// `rel` not one plain visible name, nothing there, the journal's or the
    /// rename's (the entry is left as it was). Once renamed the entry is
    /// gone from view: a failure to remove what it held is logged and left
    /// for [`UserRoot::recover`].
    pub fn remove(&self, rel: &Path) -> std::io::Result<()> {
        self.remove_steps(rel, |_| Ok(()))
    }

    /// [`Self::remove`], `stop` called after each step: an error from it
    /// ends the remove there, as an app stopped at that step would.
    pub(crate) fn remove_steps(
        &self,
        rel: &Path,
        mut stop: impl FnMut(Step) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let (dirs, name) = split(rel)?;
        if !dirs.is_empty() || name.as_encoded_bytes().starts_with(b".") {
            return Err(bad_path(rel));
        }
        let root = &self.root.path;
        std::fs::symlink_metadata(root.join(name))?;
        let tag = nofollow::random_tag();
        let journal = format!("{JOURNAL}{tag}");
        let mut record = b"remove\n".to_vec();
        record.extend_from_slice(name.as_encoded_bytes());
        replace_under(root, Path::new(&journal), &record)?;
        stop(Step::Journaled)?;
        let staged = root.join(format!("{REMOVING}{tag}"));
        // The name's last part is never followed: a link moves as a link.
        if let Err(e) = std::fs::rename(root.join(name), &staged) {
            let _ = std::fs::remove_file(root.join(&journal));
            return Err(e);
        }
        stop(Step::Staged)?;
        if let Err(e) = remove_staged(&staged) {
            tracing::warn!(at = %staged.display(), error = %e, "removed entry left for recover");
            return Ok(());
        }
        stop(Step::Removed)?;
        std::fs::remove_file(root.join(&journal))
    }

    /// Puts a new tree in place of the entry `rel` (one plain visible name
    /// under the root; nothing there is fine): `build` makes and fills the
    /// folder it is given (a new hidden `.putting-<tag>` in the root, not
    /// there yet), then it is renamed into place. The entry there before
    /// goes to `keep` (a path under a hidden folder of the root, which
    /// must be there: `.previous/<id>`), and what `keep` held goes; with no
    /// `keep` it goes.
    ///
    /// Crash-safe: a journal `.journal-<tag>` is written once the tree is
    /// built, then the entry is set aside (`.aside-<tag>`), the tree put in
    /// its place, what `keep` held set aside (`.dropping-<tag>`), the entry
    /// moved to `keep`; then what is left set aside is renamed
    /// `.removing-<tag>`, the journal removed, and that removed (a recover
    /// removes it too: never put back once the tree is in). An app stopped
    /// at any step leaves for [`UserRoot::recover`] the entry before or
    /// after, never neither; a step that fails here puts everything back.
    ///
    /// # Errors
    /// `build`'s (what it made is its to remove); `rel` or `keep` not
    /// plain, a folder on the way a link, the journal's or a rename's
    /// (all put back). Once placed it is done: a failure to remove the old
    /// copy or the journal is logged and left for [`UserRoot::recover`].
    pub fn put_tree<E: From<std::io::Error>>(
        &self,
        rel: &Path,
        keep: Option<&Path>,
        build: impl FnOnce(&Path) -> Result<(), E>,
    ) -> Result<(), E> {
        self.put_tree_steps(rel, keep, build, |_| Ok(()))
    }

    /// [`Self::put_tree`], `step` called before each step: an error from
    /// it fails that step, a panic stops there as an app quitting would.
    #[doc(hidden)]
    pub fn put_tree_steps<E: From<std::io::Error>>(
        &self,
        rel: &Path,
        keep: Option<&Path>,
        build: impl FnOnce(&Path) -> Result<(), E>,
        step: impl FnMut(PutStep) -> std::io::Result<()>,
    ) -> Result<(), E> {
        let tag = nofollow::random_tag();
        let staged = PathBuf::from(format!("{PUTTING}{tag}"));
        let put = Put::new(&tag, rel, &staged, keep)?;
        build(&self.root.path.join(&staged))?;
        Ok(self.place(&put, step)?)
    }

    /// Swaps the entry `rel` and the copy kept at `kept` (as
    /// [`Self::put_tree`] keeps one): `kept` becomes the entry, the entry
    /// (when there) is kept at `kept`. Crash-safe as [`Self::put_tree`].
    ///
    /// # Errors
    /// As [`Self::put_tree`]; nothing at `kept` is `NotFound` (the entry,
    /// set aside first, is put back).
    pub fn restore(&self, rel: &Path, kept: &Path) -> std::io::Result<()> {
        self.restore_steps(rel, kept, |_| Ok(()))
    }

    /// [`Self::restore`] with the `step` of [`Self::put_tree_steps`].
    #[doc(hidden)]
    pub fn restore_steps(
        &self,
        rel: &Path,
        kept: &Path,
        step: impl FnMut(PutStep) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let put = Put::new(&nofollow::random_tag(), rel, kept, Some(kept))?;
        self.place(&put, step)
    }

    fn place(
        &self,
        p: &Put,
        mut step: impl FnMut(PutStep) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let root = &self.root.path;
        let drop_staged = || {
            if p.staged {
                let _ = remove_staged(&root.join(&p.s));
            }
        };
        let name = format!("{JOURNAL}{}", p.tag);
        let journal = root.join(&name);
        let journaled = step(PutStep::Journal)
            .and_then(|()| replace_under(root, Path::new(&name), &p.record()));
        if let Err(e) = journaled {
            drop_staged();
            return Err(e);
        }
        let there = |rel: &Path| std::fs::symlink_metadata(root.join(rel)).is_ok();
        let had = there(&p.r);
        let undo = |e: std::io::Error, placed: bool| {
            let back = if placed && !there(&p.s) {
                move_at(root, &p.r, &p.s)
            } else {
                Ok(())
            };
            match back.and_then(|()| p.settle(root)) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&journal);
                }
                Err(u) => tracing::warn!(error = %u, "put not undone, left for recover"),
            }
            e
        };
        if had && let Err(e) = step(PutStep::Aside).and_then(|()| move_at(root, &p.r, &p.a)) {
            return Err(undo(e, false));
        }
        if let Err(e) = step(PutStep::Place).and_then(|()| move_at(root, &p.s, &p.r)) {
            return Err(undo(e, false));
        }
        if let Some(k) = p.k.as_deref().filter(|_| had) {
            let kept = if there(k) {
                step(PutStep::Drop).and_then(|()| move_at(root, k, &p.d))
            } else {
                Ok(())
            };
            if let Err(e) = kept
                .and_then(|()| step(PutStep::Keep))
                .and_then(|()| move_at(root, &p.a, k))
            {
                return Err(undo(e, true));
            }
        }
        // Placed: what was set aside (the entry, with no keep; what the
        // keep held) is no copy anyone gets back now. Renamed to a remove's
        // name before the journal goes, so no later recover takes it for
        // an entry to put back (one deleted since would come back).
        let retired = PathBuf::from(format!("{REMOVING}{}", p.tag));
        let left = [&p.a, &p.d].into_iter().find(|n| there(n));
        let retire = step(PutStep::Retire).and_then(|()| match left {
            Some(left) => move_at(root, left, &retired),
            None => Ok(()),
        });
        if let Err(e) = retire.or_else(|_| p.clean(root)) {
            tracing::warn!(error = %e, "replaced entry not removed, kept hidden");
        }
        if let Err(e) = step(PutStep::Forget).and_then(|()| std::fs::remove_file(&journal)) {
            // Done all the same: recover finds the tree placed.
            tracing::warn!(error = %e, "put journal left for recover");
        }
        if let Err(e) = step(PutStep::Clean).and_then(|()| remove_gone(&root.join(&retired))) {
            tracing::warn!(error = %e, "replaced entry left for recover");
        }
        Ok(())
    }
}

/// The steps of a [`Guard::put_tree`] or [`Guard::restore`], for a test to
/// fail or stop at; each is called before it is taken.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutStep {
    /// The journal written.
    Journal,
    /// The entry set aside (when there).
    Aside,
    /// The new tree (or the kept copy) put in its place.
    Place,
    /// What `keep` held set aside (when there).
    Drop,
    /// The entry moved to `keep`.
    Keep,
    /// What was set aside renamed to `.removing-<tag>`.
    Retire,
    /// The journal removed.
    Forget,
    /// `.removing-<tag>` removed.
    Clean,
}

/// A put under way, by the paths under the root it moves between: `r`
/// the entry, `s` the tree put there (`.putting-<tag>`, or the kept copy
/// a restore brings back), `k` where the entry is kept, `a` and `d` the
/// entry and what `k` held, set aside.
#[derive(Debug)]
struct Put {
    tag: String,
    r: PathBuf,
    s: PathBuf,
    k: Option<PathBuf>,
    a: PathBuf,
    d: PathBuf,
    staged: bool,
}

impl Put {
    /// Checks the paths: `r` one plain visible name, `k` under a hidden
    /// folder, `s` the staging of `tag` or `k` itself.
    fn new(tag: &str, r: &Path, s: &Path, k: Option<&Path>) -> std::io::Result<Self> {
        let (dirs, name) = split(r)?;
        if !dirs.is_empty() || name.as_encoded_bytes().starts_with(b".") {
            return Err(bad_path(r));
        }
        if let Some(k) = k {
            let (dirs, _) = split(k)?;
            if !dirs
                .first()
                .is_some_and(|d| d.as_encoded_bytes().starts_with(b"."))
            {
                return Err(bad_path(k));
            }
        }
        let staging = PathBuf::from(format!("{PUTTING}{tag}"));
        if s != staging && Some(s) != k {
            return Err(bad_path(s));
        }
        Ok(Self {
            tag: tag.to_owned(),
            r: r.to_path_buf(),
            s: s.to_path_buf(),
            k: k.map(Path::to_path_buf),
            a: PathBuf::from(format!("{ASIDE}{tag}")),
            d: PathBuf::from(format!("{DROPPING}{tag}")),
            staged: s == staging,
        })
    }

    /// `put\n<r>\n<s>\n<k>` (`k` empty when none).
    fn record(&self) -> Vec<u8> {
        let mut out = b"put".to_vec();
        for p in [Some(&self.r), Some(&self.s), self.k.as_ref()] {
            out.push(b'\n');
            out.extend_from_slice(p.map_or(&[][..], |p| p.as_os_str().as_encoded_bytes()));
        }
        out
    }

    /// The put of journal `.journal-<tag>` holding `bytes`, checked as
    /// [`Self::new`] checks it (a journal is a file an agent can write).
    fn read(tag: &str, bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes.strip_prefix(b"put\n")?).ok()?;
        let mut lines = text.split('\n');
        let (r, s, k) = (lines.next()?, lines.next()?, lines.next()?);
        if lines.next().is_some() {
            return None;
        }
        let k = (!k.is_empty()).then(|| Path::new(k));
        Self::new(tag, Path::new(r), Path::new(s), k).ok()
    }

    /// Puts what an app stopped (or a step failed) left in place, by what
    /// is there: the tree not placed yet (`s` there), the entry back; once
    /// placed, the entry on to `k`. The entry set aside is never removed
    /// while nothing is in its place.
    fn settle(&self, root: &Path) -> std::io::Result<()> {
        let there = |rel: &Path| std::fs::symlink_metadata(root.join(rel)).is_ok();
        let back = |from: &Path, to: &Path| {
            if !there(from) {
                Ok(())
            } else if there(to) {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} is in the way", to.display()),
                ))
            } else {
                move_at(root, from, to)
            }
        };
        if there(&self.s) {
            back(&self.a, &self.r)?;
            if let Some(k) = &self.k {
                back(&self.d, k)?;
            }
            if self.staged {
                remove_staged(&root.join(&self.s))?;
            }
            return Ok(());
        }
        if !there(&self.r) {
            return back(&self.a, &self.r);
        }
        if let Some(k) = self.k.as_deref().filter(|_| there(&self.a)) {
            if there(k) {
                move_at(root, k, &self.d)?;
            }
            move_at(root, &self.a, k)?;
        }
        self.clean(root)
    }

    /// Removes what was set aside once the entry is in place.
    fn clean(&self, root: &Path) -> std::io::Result<()> {
        for gone in [&self.d, &self.a] {
            remove_gone(&root.join(gone))?;
        }
        Ok(())
    }
}

/// Renames `from` to `to` (paths under `root`), each through its folder's
/// descriptor: no folder on the way that is a link is followed (but an
/// entry, which may be the user's own link).
#[cfg(unix)]
fn move_at(root: &Path, from: &Path, to: &Path) -> std::io::Result<()> {
    let (from_dirs, from_name) = split(from)?;
    let (to_dirs, to_name) = split(to)?;
    let from_dir = at::folder(root, &from_dirs)?.ok_or(std::io::ErrorKind::NotFound)?;
    let to_dir = at::folder(root, &to_dirs)?.ok_or(std::io::ErrorKind::NotFound)?;
    nix::fcntl::renameat(&from_dir, from_name, &to_dir, to_name).map_err(Into::into)
}

#[cfg(not(unix))]
fn move_at(root: &Path, from: &Path, to: &Path) -> std::io::Result<()> {
    for rel in [from, to] {
        let (dirs, _) = split(rel)?;
        let mut at = root.to_path_buf();
        for dir in dirs {
            at.push(dir);
            if !std::fs::symlink_metadata(&at)?.is_dir() {
                return Err(std::io::ErrorKind::NotADirectory.into());
            }
        }
    }
    std::fs::rename(root.join(from), root.join(to))
}

/// The steps of a [`Guard::remove`], for a test to stop at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Journaled,
    Staged,
    Removed,
}

/// Names [`Guard::remove`] and [`Guard::put_tree`] make in the root:
/// `<JOURNAL><tag>` holds the operation (`remove\n<entry name>`, or a
/// [`Put`]), `<REMOVING><tag>` the entry on its way out, `<PUTTING><tag>`
/// a tree being built, `<ASIDE><tag>` and `<DROPPING><tag>` what a put set
/// aside.
const JOURNAL: &str = ".journal-";
const REMOVING: &str = ".removing-";
const PUTTING: &str = ".putting-";
const ASIDE: &str = ".aside-";
const DROPPING: &str = ".dropping-";

/// Removes what is at `path`: a folder and what it holds (no link in it
/// followed), else the file or link itself.
fn remove_staged(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// [`remove_staged`], nothing there being fine.
fn remove_gone(path: &Path) -> std::io::Result<()> {
    match remove_staged(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Whether `s` is a [`nofollow::random_tag`]: 16 lowercase hex digits.
fn is_tag(s: &str) -> bool {
    s.len() == 16
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether `name` is a temp file a [`Guard::write`] left: `.<file>.<tag>.tmp`.
fn is_temp(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|n| n.strip_suffix(".tmp"))
        .and_then(|n| n.rsplit_once('.'))
        .is_some_and(|(file, tag)| !file.is_empty() && is_tag(tag))
}

impl UserRoot {
    /// Finishes what an app stopped on the way left, once at start-up,
    /// under the lock: a journaled remove is finished (its entry, hidden
    /// already, removed; one never renamed is left whole), a journaled
    /// [`Guard::put_tree`] or [`Guard::restore`] put back (tree not placed
    /// yet) or finished (placed), the entry never left missing, a
    /// `.removing-<tag>` or `.putting-<tag>` with no journal removed (an
    /// `.aside-<tag>` or `.dropping-<tag>` with none is kept), and the temp files of a
    /// [`Guard::write`] cut short (`.<file>.<tag>.tmp`, in the root and in
    /// each entry) removed. Nothing else is touched: a journal of another
    /// kind is kept. Returns how many names it removed.
    ///
    /// # Errors
    /// Listing the root (none there is `Ok(0)`). One leftover that cannot
    /// go is logged and stops nothing else.
    pub fn recover(&self) -> std::io::Result<usize> {
        let guard = self.lock();
        let names: Vec<String> = match std::fs::read_dir(&self.path) {
            Ok(list) => list
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut gone = 0;
        let mut clear = |path: PathBuf| match remove_staged(&path) {
            Ok(()) => gone += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(at = %path.display(), error = %e, "leftover not removed"),
        };
        for name in &names {
            if let Some(tag) = name.strip_prefix(JOURNAL).filter(|t| is_tag(t)) {
                let Found::File(read) = guard.read(Path::new(name)) else {
                    continue;
                };
                if read.bytes.starts_with(b"remove\n") {
                    clear(self.path.join(format!("{REMOVING}{tag}")));
                    clear(self.path.join(name));
                } else if let Some(put) = Put::read(tag, &read.bytes) {
                    match put.settle(&self.path) {
                        Ok(()) => clear(self.path.join(name)),
                        Err(e) => tracing::warn!(journal = name, error = %e, "put not settled"),
                    }
                }
            } else if name.strip_prefix(REMOVING).is_some_and(is_tag) {
                clear(self.path.join(name));
            } else if let Some(tag) = name.strip_prefix(PUTTING).filter(|t| is_tag(t))
                && std::fs::symlink_metadata(self.path.join(format!("{JOURNAL}{tag}"))).is_err()
            {
                // Built, never journaled: a copy of a source still there.
                // With its journal it is the journal's: gone, the journal
                // would take the tree for placed.
                clear(self.path.join(name));
            } else if is_temp(name) {
                if !std::fs::symlink_metadata(self.path.join(name)).is_ok_and(|m| m.is_dir()) {
                    clear(self.path.join(name));
                }
            } else if !name.starts_with('.')
                && let Ok(list) = std::fs::read_dir(self.path.join(name))
            {
                // The entry may be the user's own link; nothing below it.
                for e in list.flatten() {
                    let temp = e.file_name().to_str().is_some_and(is_temp);
                    if temp && e.file_type().is_ok_and(|t| !t.is_dir()) {
                        clear(e.path());
                    }
                }
            }
        }
        Ok(gone)
    }
}

fn bad_path(rel: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("{} is no plain relative path", rel.display()),
    )
}

/// `rel`'s folders and file name, when it is plain (no `..`, no root).
fn split(rel: &Path) -> std::io::Result<(Vec<&std::ffi::OsStr>, &std::ffi::OsStr)> {
    let mut names = Vec::new();
    for c in rel.components() {
        match c {
            std::path::Component::Normal(n) => names.push(n),
            _ => return Err(bad_path(rel)),
        }
    }
    let name = names.pop().ok_or_else(|| bad_path(rel))?;
    Ok((names, name))
}

#[cfg(unix)]
mod at {
    use super::Unreadable;
    use nix::errno::Errno;
    use nix::fcntl::{OFlag, open, openat};
    use nix::sys::stat::Mode;
    use std::ffi::OsStr;
    use std::os::fd::OwnedFd;
    use std::path::Path;

    const DIR: OFlag = OFlag::O_RDONLY
        .union(OFlag::O_DIRECTORY)
        .union(OFlag::O_CLOEXEC);

    /// The folder `dirs` under `root`, opened link-free below `root`.
    /// `Ok(None)` when one of them is not there.
    pub fn folder(root: &Path, dirs: &[&OsStr]) -> Result<Option<OwnedFd>, Errno> {
        let mut fd = match open(root, DIR, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::ENOENT) => return Ok(None),
            Err(e) => return Err(e),
        };
        for (depth, dir) in dirs.iter().enumerate() {
            // The entry may be the user's own link; nothing below it, and
            // no hidden folder of the root (`.previous`) is an entry.
            let flags = if depth == 0 && !dir.as_encoded_bytes().starts_with(b".") {
                DIR
            } else {
                DIR | OFlag::O_NOFOLLOW
            };
            fd = match openat(&fd, *dir, flags, Mode::empty()) {
                Ok(next) => next,
                Err(Errno::ENOENT) => return Ok(None),
                Err(e) => return Err(e),
            };
        }
        Ok(Some(fd))
    }

    pub fn why(e: Errno) -> Unreadable {
        match e {
            Errno::ELOOP | Errno::ENOTDIR => Unreadable::Link,
            Errno::ENXIO | Errno::EOPNOTSUPP => Unreadable::NotAFile,
            e => Unreadable::Io(e.into()),
        }
    }
}

#[cfg(unix)]
fn read_under(root: &Path, rel: &Path, limit: u64) -> Result<Option<ReadFile>, Unreadable> {
    use nix::errno::Errno;
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;
    let (dirs, name) = split(rel).map_err(Unreadable::Io)?;
    let Some(dir) = at::folder(root, &dirs).map_err(at::why)? else {
        return Ok(None);
    };
    let flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC;
    let file = match openat(&dir, name, flags, Mode::empty()) {
        Ok(fd) => std::fs::File::from(fd),
        Err(Errno::ENOENT) => return Ok(None),
        Err(e) => return Err(at::why(e)),
    };
    match nofollow::read_opened(file, limit) {
        Ok(Some(file)) => Ok(Some(file)),
        Ok(None) => Err(Unreadable::NotAFile),
        Err(nofollow::TreeError::TooLarge) => Err(Unreadable::TooLarge),
        Err(nofollow::TreeError::Io(e)) => Err(Unreadable::Io(e)),
    }
}

#[cfg(not(unix))]
fn read_under(root: &Path, rel: &Path, limit: u64) -> Result<Option<ReadFile>, Unreadable> {
    use nofollow::TreeError;
    let (dirs, _) = split(rel).map_err(Unreadable::Io)?;
    let mut at = root.to_path_buf();
    for (depth, dir) in dirs.iter().enumerate() {
        at.push(dir);
        if depth > 0 && std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Unreadable::Link);
        }
    }
    match nofollow::read_file(&root.join(rel), limit) {
        Ok(Some(file)) => Ok(Some(file)),
        Err(TreeError::TooLarge) => Err(Unreadable::TooLarge),
        Err(TreeError::Io(e)) => Err(Unreadable::Io(e)),
        Ok(None) => match std::fs::symlink_metadata(root.join(rel)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Ok(m) if m.file_type().is_symlink() => Err(Unreadable::Link),
            _ => Err(Unreadable::NotAFile),
        },
    }
}

/// Writes `bytes` as `rel` under `root` through a new file of a random
/// name in the same folder, synced, renamed over it.
#[cfg(unix)]
fn replace_under(root: &Path, rel: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use nix::errno::Errno;
    use nix::fcntl::{AtFlags, renameat};
    use nix::sys::stat::{Mode, fstatat};
    use nix::unistd::{UnlinkatFlags, unlinkat};
    use std::io::Write as _;
    let (dirs, name) = split(rel)?;
    let dir = at::folder(root, &dirs)?.ok_or(std::io::ErrorKind::NotFound)?;
    // A file there keeps its bits; a read-only one is refused, as a write
    // in place would be (the rename would replace it).
    let mode = match fstatat(&dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(st) if st.st_mode & nix::libc::S_IFMT == nix::libc::S_IFREG => {
            let mode = Mode::from_bits_truncate(st.st_mode & 0o777);
            if !mode.intersects(Mode::S_IWUSR | Mode::S_IWGRP | Mode::S_IWOTH) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("{} is read-only", rel.display()),
                ));
            }
            mode
        }
        Ok(_) | Err(Errno::ENOENT) => Mode::from_bits_truncate(0o644),
        Err(e) => return Err(e.into()),
    };
    let mut tries = 0;
    let (tmp, fd) = loop {
        let tmp = format!(".{}.{}.tmp", name.to_string_lossy(), nofollow::random_tag());
        match new_file_at(&dir, &tmp, mode) {
            Ok(fd) => break (tmp, fd),
            // A name taken (by chance, or planted): another one.
            Err(Errno::EEXIST) if tries < 3 => tries += 1,
            Err(e) => return Err(e.into()),
        }
    };
    // The umask took bits off at the open; put the kept ones back.
    let written = nix::sys::stat::fchmod(&fd, mode)
        .map_err(std::io::Error::from)
        .map(|()| std::fs::File::from(fd));
    let written = written
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| renameat(&dir, tmp.as_str(), &dir, name).map_err(Into::into));
    if written.is_err() {
        let _ = unlinkat(&dir, tmp.as_str(), UnlinkatFlags::NoRemoveDir);
    }
    written
}

/// Makes the new file `name` in folder `dir` for writing: a file or a
/// link already there (planted at a temp name) is never opened, `EEXIST`.
#[cfg(unix)]
fn new_file_at(
    dir: &std::os::fd::OwnedFd,
    name: &str,
    mode: nix::sys::stat::Mode,
) -> nix::Result<std::os::fd::OwnedFd> {
    use nix::fcntl::{OFlag, openat};
    let flags =
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    openat(dir, name, flags, mode)
}

#[cfg(not(unix))]
fn replace_under(root: &Path, rel: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let (dirs, _) = split(rel)?;
    let mut at = root.to_path_buf();
    for (depth, dir) in dirs.iter().enumerate() {
        at.push(dir);
        // The entry may be the user's own link; nothing below it.
        let meta = if depth == 0 {
            std::fs::metadata(&at)?
        } else {
            std::fs::symlink_metadata(&at)?
        };
        if !meta.is_dir() {
            return Err(std::io::ErrorKind::NotADirectory.into());
        }
    }
    let path = root.join(rel);
    let kept = match std::fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && m.permissions().readonly() => {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        Ok(m) if m.is_file() => Some(m.permissions()),
        _ => None,
    };
    nofollow::replace_file(&path, bytes, kept)
}

#[cfg(test)]
#[path = "user_store_tests.rs"]
mod tests;
