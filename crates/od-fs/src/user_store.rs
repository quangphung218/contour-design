//! One door to a user's files in a folder an agent can write (`<data>/skills`,
//! `<data>/design-systems`, `<data>/memory`): [`UserRoot`] names the folder and
//! holds the one lock every writer of it takes; [`Guard`] (the lock held) reads
//! and writes a file under it by a plain relative path.
//!
//! - Every folder below the root is opened from the one above it with
//!   `O_NOFOLLOW` and every file through its folder's descriptor (unix
//!   `openat`/`renameat`): a link anywhere below the root is never followed.
//!   The root itself is followed (resolved once per call).
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
        for dir in dirs {
            fd = match openat(&fd, *dir, DIR | OFlag::O_NOFOLLOW, Mode::empty()) {
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
    split(rel).map_err(Unreadable::Io)?;
    match nofollow::read_file_under(root, rel, limit) {
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
    for dir in dirs {
        at.push(dir);
        if !std::fs::symlink_metadata(&at)?.is_dir() {
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
