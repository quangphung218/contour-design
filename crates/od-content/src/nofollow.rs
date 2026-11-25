//! Reading files in a folder an agent can write (the user skills, plugins,
//! prompt templates under `<data>`), without following a link and without
//! waiting on a FIFO, even one swapped in between a check and the read.
//!
//! An agent can change these folders while the daemon reads them, from
//! outside its sandbox (loopback) or from inside it. A check by path
//! (`file_type`, `symlink_metadata`) and then an open by path leaves a gap:
//! a link planted between the two is followed (the daemon reads what it
//! points at), a FIFO blocks the reader for good. Here every folder is
//! opened from the one above it by its descriptor and every file the same
//! way (unix `openat` with `O_NOFOLLOW`; files also `O_NONBLOCK`), and the
//! kind is checked on the descriptor opened. Elsewhere (Windows: no FIFO,
//! links need a privilege) it opens by path and checks the opened handle.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A file read: its bytes and, on unix, its permission bits (`& 0o777`;
/// 0 elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFile {
    pub bytes: Vec<u8>,
    pub mode: u32,
}

/// Why a read stopped.
#[derive(Debug)]
pub enum TreeError {
    /// Past the budget, counted in bytes really read.
    TooLarge,
    Io(std::io::Error),
}

impl From<std::io::Error> for TreeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A folder read by [`read_tree`]: its folders (parents first) and its
/// regular files, by path under it.
#[derive(Debug, Default)]
pub struct Tree {
    pub dirs: Vec<PathBuf>,
    pub files: Vec<(PathBuf, ReadFile)>,
}

/// Reads `file` (opened already) when it is a regular file, at most
/// `limit` bytes; `Ok(None)` for anything else (a FIFO, a device).
fn read_opened(file: File, limit: u64) -> Result<Option<ReadFile>, TreeError> {
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Ok(None);
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = 0;
    let mut bytes = Vec::new();
    // One byte past the limit tells a file too large, however its size
    // changed since `metadata`.
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(TreeError::TooLarge);
    }
    Ok(Some(ReadFile { bytes, mode }))
}

/// Reads the regular file at `path`, at most `limit` bytes, without
/// following a link at its last component or waiting on a FIFO.
/// `Ok(None)` when it is a link, a FIFO, missing, or not a regular file.
///
/// # Errors
/// [`TreeError::TooLarge`] past `limit`; [`TreeError::Io`] when it cannot
/// be read (permission).
pub fn read_file(path: &Path, limit: u64) -> Result<Option<ReadFile>, TreeError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Ok(None);
    }
    match options.open(path) {
        Ok(file) => read_opened(file, limit),
        Err(e) if skipped(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// An open that met a link (`ELOOP`), nothing (gone meanwhile), or a
/// socket: not a file to read, skipped.
fn skipped(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::NotFound {
        return true;
    }
    #[cfg(unix)]
    {
        use nix::libc;
        matches!(
            e.raw_os_error(),
            Some(libc::ELOOP | libc::ENXIO | libc::EOPNOTSUPP | libc::ENOTDIR)
        )
    }
    #[cfg(not(unix))]
    false
}

/// Reads the folder `root` (not a link itself) and everything under it
/// that `keep(at_top, name)` keeps: folders and regular files only, no
/// link at any depth whenever it was planted, at most `budget` bytes in
/// all (counted as read).
///
/// # Errors
/// [`TreeError::TooLarge`] past `budget`; [`TreeError::Io`] when `root`
/// or a file in it cannot be read.
pub fn read_tree(
    root: &Path,
    keep: impl Fn(bool, &OsStr) -> bool,
    budget: u64,
) -> Result<Tree, TreeError> {
    let mut tree = Tree::default();
    let mut used = 0u64;
    #[cfg(unix)]
    {
        use nix::fcntl::OFlag;
        use nix::sys::stat::Mode;
        let dir = nix::dir::Dir::open(
            root,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        walk_unix(dir, Path::new(""), &keep, budget, &mut used, &mut tree)?;
    }
    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(root)?.file_type().is_symlink() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        walk_path(root, Path::new(""), &keep, budget, &mut used, &mut tree)?;
    }
    tree.dirs.sort();
    tree.files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(tree)
}

/// Reads the folder `dir` (its own descriptor): every entry is listed and
/// opened through it, never by a path a link could redirect.
#[cfg(unix)]
fn walk_unix(
    mut dir: nix::dir::Dir,
    rel: &Path,
    keep: &impl Fn(bool, &OsStr) -> bool,
    budget: u64,
    used: &mut u64,
    tree: &mut Tree,
) -> Result<(), TreeError> {
    use nix::errno::Errno;
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;
    use std::os::unix::ffi::OsStrExt;
    let top = rel.as_os_str().is_empty();
    let mut names = Vec::new();
    for entry in dir.iter() {
        let entry = match entry {
            Ok(e) => e,
            // The folder was removed while it was read (macOS): what is
            // listed so far is all there is.
            Err(Errno::ENOENT) => break,
            Err(e) => return Err(std::io::Error::from(e).into()),
        };
        let name = OsStr::from_bytes(entry.file_name().to_bytes()).to_os_string();
        if name != "." && name != ".." && keep(top, &name) {
            names.push(name);
        }
    }
    for name in names {
        let path = rel.join(&name);
        let as_dir = openat(
            &dir,
            name.as_os_str(),
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        );
        match as_dir {
            Ok(fd) => {
                match nix::dir::Dir::from_fd(fd) {
                    Ok(sub) => {
                        tree.dirs.push(path.clone());
                        walk_unix(sub, &path, keep, budget, used, tree)?;
                    }
                    // Removed since it was opened (macOS `fdopendir`).
                    Err(Errno::ENOENT) => {}
                    Err(e) => return Err(std::io::Error::from(e).into()),
                }
                continue;
            }
            // Not a folder: maybe a file, opened below.
            Err(Errno::ENOTDIR) => {}
            // A link (ELOOP), gone meanwhile.
            Err(Errno::ELOOP | Errno::ENOENT) => continue,
            Err(e) => return Err(std::io::Error::from(e).into()),
        }
        let as_file = openat(
            &dir,
            name.as_os_str(),
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::empty(),
        );
        let file = match as_file {
            Ok(fd) => File::from(fd),
            Err(e) if skipped(&std::io::Error::from(e)) => continue,
            Err(e) => return Err(std::io::Error::from(e).into()),
        };
        if let Some(read) = read_opened(file, budget.saturating_sub(*used))? {
            *used += read.bytes.len() as u64;
            tree.files.push((path, read));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn walk_path(
    dir: &Path,
    rel: &Path,
    keep: &impl Fn(bool, &OsStr) -> bool,
    budget: u64,
    used: &mut u64,
    tree: &mut Tree,
) -> Result<(), TreeError> {
    let top = rel.as_os_str().is_empty();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if !keep(top, &name) {
            continue;
        }
        let kind = entry.file_type()?;
        let path = rel.join(&name);
        if kind.is_dir() {
            tree.dirs.push(path.clone());
            walk_path(&entry.path(), &path, keep, budget, used, tree)?;
        } else if kind.is_file()
            && let Some(read) = read_file(&entry.path(), budget.saturating_sub(*used))?
        {
            *used += read.bytes.len() as u64;
            tree.files.push((path, read));
        }
    }
    Ok(())
}

/// Writes `tree` into the folder `to` (made already): its folders, then
/// its files with their permission bits (unix), the file `last` (a path
/// under `to`, if there) after all the others.
///
/// # Errors
/// The first write that fails.
pub fn write_tree(tree: &Tree, to: &Path, last: Option<&Path>) -> std::io::Result<()> {
    for d in &tree.dirs {
        std::fs::create_dir(to.join(d))?;
    }
    let write = |p: &Path, f: &ReadFile| -> std::io::Result<()> {
        let dest = to.join(p);
        std::fs::write(&dest, &f.bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(f.mode))?;
        }
        Ok(())
    };
    for (p, f) in tree.files.iter().filter(|(p, _)| Some(p.as_path()) != last) {
        write(p, f)?;
    }
    if let Some((p, f)) = tree.files.iter().find(|(p, _)| Some(p.as_path()) == last) {
        write(p, f)?;
    }
    Ok(())
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn keep_all(_: bool, name: &OsStr) -> bool {
        !name.to_string_lossy().starts_with('.')
    }

    /// A thread that swaps `at` back and forth between a regular file and
    /// what `other` makes (a link, a FIFO), each made under a hidden name
    /// (skipped by the reader) and renamed into place, until stopped.
    fn swapper(
        dir: PathBuf,
        at: &'static str,
        other: impl Fn(&Path) + Send + 'static,
    ) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let (file, swap) = (dir.join(".swap-file"), dir.join(".swap-other"));
            while !done.load(Ordering::SeqCst) {
                std::fs::write(&file, "plain").unwrap();
                std::fs::rename(&file, dir.join(at)).unwrap();
                let _ = std::fs::remove_file(&swap);
                let _ = std::fs::remove_dir_all(&swap);
                other(&swap);
                std::fs::rename(&swap, dir.join(at)).unwrap();
            }
        });
        (stop, handle)
    }

    /// Reads `root` over and over for `for_ms`, counting the reads that
    /// held `secret`. Each read must come back (a FIFO never blocks it).
    fn reads_with(root: &Path, secret: &[u8], for_ms: u64) -> (usize, usize) {
        let (root, secret) = (root.to_path_buf(), secret.to_vec());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_millis(for_ms);
            let (mut reads, mut leaked) = (0, 0);
            while Instant::now() < until {
                let tree = read_tree(&root, keep_all, u64::MAX).unwrap();
                reads += 1;
                if tree.files.iter().any(|(_, f)| f.bytes == secret) {
                    leaked += 1;
                }
            }
            tx.send((reads, leaked)).unwrap();
        });
        rx.recv_timeout(Duration::from_millis(for_ms + 10_000))
            .expect("a read never came back (blocked on a FIFO?)")
    }

    /// A file swapped for a link to a file outside, between the listing
    /// and the open, is never read through the link.
    #[test]
    fn a_link_swapped_in_for_a_file_is_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let secret = tmp.path().join("fake-secret.txt");
        std::fs::write(&secret, "s3cret").unwrap();
        let root = tmp.path().join("skill");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        let (stop, handle) = swapper(root.join("assets"), "x", move |p| {
            std::os::unix::fs::symlink(&secret, p).unwrap();
        });
        let (reads, leaked) = reads_with(&root, b"s3cret", 1500);
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap();
        assert!(reads > 100, "{reads} reads");
        assert_eq!(leaked, 0, "{leaked} of {reads} reads followed the link");
    }

    /// A folder swapped for a link to a folder outside is never walked.
    #[test]
    fn a_link_swapped_in_for_a_folder_is_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("x"), "s3cret").unwrap();
        let root = tmp.path().join("skill");
        std::fs::create_dir_all(&root).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let (root, stop) = (root.clone(), Arc::clone(&stop));
            std::thread::spawn(move || {
                let (dir, link) = (root.join(".swap-dir"), root.join(".swap-link"));
                while !stop.load(Ordering::SeqCst) {
                    let _ = std::fs::remove_file(root.join("assets"));
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join("x"), "plain").unwrap();
                    std::fs::rename(&dir, root.join("assets")).unwrap();
                    let _ = std::fs::remove_file(&link);
                    std::os::unix::fs::symlink(&outside, &link).unwrap();
                    let _ = std::fs::remove_dir_all(root.join("assets"));
                    std::fs::rename(&link, root.join("assets")).unwrap();
                }
            })
        };
        let (reads, leaked) = reads_with(&root, b"s3cret", 1500);
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap();
        assert!(reads > 100, "{reads} reads");
        assert_eq!(leaked, 0, "{leaked} of {reads} reads walked the link");
    }

    /// A FIFO swapped in for a file never blocks the reader, and is not
    /// read as a file.
    #[test]
    fn a_fifo_swapped_in_for_a_file_never_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("skill");
        std::fs::create_dir_all(&root).unwrap();
        let (stop, handle) = swapper(root.clone(), "x", |p| {
            nix::unistd::mkfifo(p, nix::sys::stat::Mode::from_bits_truncate(0o644)).unwrap();
        });
        let (reads, _) = reads_with(&root, b"", 1500);
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap();
        assert!(reads > 100, "{reads} reads");
        // One there all along: skipped, the read comes back.
        nix::unistd::mkfifo(
            &root.join("fifo"),
            nix::sys::stat::Mode::from_bits_truncate(0o644),
        )
        .unwrap();
        let tree = read_tree(&root, keep_all, u64::MAX).unwrap();
        assert!(tree.files.iter().all(|(p, _)| p != Path::new("fifo")));
        assert_eq!(read_file(&root.join("fifo"), 10).unwrap(), None);
    }

    /// A file holding more than its size says (a `/proc` file says 0) is
    /// read up to the limit only: the limit holds on the bytes read, as it
    /// does for a file that grows between `metadata` and the read.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_larger_than_its_size_says_stops_at_the_limit() {
        let status = Path::new("/proc/self/status");
        assert_eq!(std::fs::metadata(status).unwrap().len(), 0);
        assert!(matches!(read_file(status, 4), Err(TreeError::TooLarge)));
    }

    /// One file by path: a link at that name reads nothing.
    #[test]
    fn a_file_read_by_path_is_never_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("secret"), "s3cret").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("secret"), tmp.path().join("x")).unwrap();
        assert_eq!(read_file(&tmp.path().join("x"), 100).unwrap(), None);
        let read = read_file(&tmp.path().join("secret"), 100).unwrap().unwrap();
        assert_eq!(read.bytes, b"s3cret");
    }

    /// The budget counts the bytes read; a file is read up to it only.
    #[test]
    fn the_budget_counts_what_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), "12345").unwrap();
        std::fs::write(tmp.path().join("b"), "67890").unwrap();
        assert_eq!(read_tree(tmp.path(), keep_all, 10).unwrap().files.len(), 2);
        assert!(matches!(
            read_tree(tmp.path(), keep_all, 9),
            Err(TreeError::TooLarge)
        ));
        assert!(matches!(
            read_file(&tmp.path().join("a"), 4),
            Err(TreeError::TooLarge)
        ));
    }
}
