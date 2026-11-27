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
}

/// The steps of a [`Guard::remove`], for a test to stop at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Journaled,
    Staged,
    Removed,
}

/// Names [`Guard::remove`] makes in the root: `<JOURNAL><tag>` holds the
/// operation (`remove\n<entry name>`; 4b adds kinds), `<REMOVING><tag>` the
/// entry on its way out.
const JOURNAL: &str = ".journal-";
const REMOVING: &str = ".removing-";

/// Removes what is at `path`: a folder and what it holds (no link in it
/// followed), else the file or link itself.
fn remove_staged(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
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
    /// already, removed; one never renamed is left whole), a
    /// `.removing-<tag>` with no journal removed, and the temp files of a
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
                }
            } else if name.strip_prefix(REMOVING).is_some_and(is_tag) {
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
            // The entry may be the user's own link; nothing below it.
            let flags = if depth == 0 {
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
