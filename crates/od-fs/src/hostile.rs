//! A root as an agent leaves it at its worst, for any test of a writer of
//! user files: [`Hostile::new`] makes a root with a skill `tea` and a
//! folder outside the root that nothing may change ([`Hostile::outside`]),
//! and [`Hostile::plant`] puts one [`Hazard`] in it: the entry a link, a
//! link below the entry, a link at the file, a FIFO, a file too large, a
//! read-only folder, a folder named like the entry in another case.
//!
//! `every_guard_call_holds_up_in_every_hazard` runs every call of
//! [`Guard`] (and [`UserRoot::recover`]) in each one. Another crate's
//! tests take it with the feature `test-support` (a dev-dependency only),
//! to run their own writer in each [`Hazard`] ([`finishes`]).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use crate::nofollow::{ReadFile, Tree};
use crate::user_store::{Expect, Found, Guard, UserRoot};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The read limit of a hostile root.
pub const LIMIT: u64 = 1024;

/// What an agent may have left in the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hazard {
    /// Nothing: the root as the app left it.
    None,
    /// `tea` is a link to a skill folder elsewhere (the user's own, which
    /// the index lists and a write may follow; a remove never does).
    LinkedEntry,
    /// `tea/assets` is a link to the folder outside.
    LinkBelowEntry,
    /// `tea/SKILL.md` is a link to the file outside.
    LinkAtFile,
    /// `tea/SKILL.md` is a FIFO.
    Fifo,
    /// `tea/SKILL.md` is past [`LIMIT`].
    TooLarge,
    /// `tea` is read-only (`0555`).
    ReadOnly,
    /// `Tea`, not `tea`, is there: another skill on a disk that keeps case.
    CaseTwin,
}

impl Hazard {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::LinkedEntry,
        Self::LinkBelowEntry,
        Self::LinkAtFile,
        Self::Fifo,
        Self::TooLarge,
        Self::ReadOnly,
        Self::CaseTwin,
    ];
}

/// A root under a temp folder, with a folder outside it.
pub struct Hostile {
    tmp: tempfile::TempDir,
    pub root: UserRoot,
}

impl Hostile {
    /// `root/tea` (a `SKILL.md`, `assets/a.css`), `root/.previous/tea` (a
    /// kept copy), `outside/secret.txt`, `own/` (a skill folder of the
    /// user's outside the root).
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("root");
        std::fs::create_dir_all(path.join("tea/assets")).unwrap();
        std::fs::write(path.join("tea/SKILL.md"), "tea").unwrap();
        std::fs::write(path.join("tea/assets/a.css"), "css").unwrap();
        std::fs::create_dir_all(path.join(".previous/tea")).unwrap();
        std::fs::write(path.join(".previous/tea/SKILL.md"), "kept").unwrap();
        std::fs::create_dir_all(tmp.path().join("outside")).unwrap();
        std::fs::write(tmp.path().join("outside/secret.txt"), "secret").unwrap();
        std::fs::create_dir_all(tmp.path().join("own")).unwrap();
        std::fs::write(tmp.path().join("own/SKILL.md"), "own").unwrap();
        let root = UserRoot::open(&path, LIMIT);
        Self { tmp, root }
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }

    /// The folder no call may change.
    pub fn outside(&self) -> PathBuf {
        self.tmp.path().join("outside")
    }

    /// The user's own skill folder outside the root, which
    /// [`Hazard::LinkedEntry`] links as `tea`.
    pub fn own(&self) -> PathBuf {
        self.tmp.path().join("own")
    }

    /// Puts `hazard` in the root.
    pub fn plant(&self, hazard: Hazard) {
        let tea = self.path().join("tea");
        let secret = self.outside().join("secret.txt");
        match hazard {
            Hazard::None => {}
            Hazard::LinkedEntry => {
                std::fs::remove_dir_all(&tea).unwrap();
                symlink(self.own(), &tea).unwrap();
            }
            Hazard::LinkBelowEntry => {
                std::fs::remove_dir_all(tea.join("assets")).unwrap();
                symlink(self.outside(), tea.join("assets")).unwrap();
            }
            Hazard::LinkAtFile => {
                std::fs::remove_file(tea.join("SKILL.md")).unwrap();
                symlink(&secret, tea.join("SKILL.md")).unwrap();
            }
            Hazard::Fifo => {
                std::fs::remove_file(tea.join("SKILL.md")).unwrap();
                nix::unistd::mkfifo(
                    &tea.join("SKILL.md"),
                    nix::sys::stat::Mode::from_bits_truncate(0o644),
                )
                .unwrap();
            }
            Hazard::TooLarge => {
                let big = vec![b'x'; usize::try_from(LIMIT).unwrap() + 1];
                std::fs::write(tea.join("SKILL.md"), big).unwrap();
            }
            Hazard::ReadOnly => {
                std::fs::set_permissions(&tea, std::fs::Permissions::from_mode(0o555)).unwrap();
            }
            Hazard::CaseTwin => {
                std::fs::rename(&tea, self.path().join("Tea")).unwrap();
            }
        }
    }

    /// Every file and link under `dir` with its bytes (where a link
    /// points), no link followed.
    pub fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(at) = todo.pop() {
            let Ok(list) = std::fs::read_dir(&at) else {
                continue;
            };
            for e in list.flatten() {
                let p = e.path();
                let m = std::fs::symlink_metadata(&p).unwrap();
                let rel = p.strip_prefix(dir).unwrap().to_path_buf();
                if m.file_type().is_symlink() {
                    let to = std::fs::read_link(&p).unwrap();
                    out.push((rel, to.to_string_lossy().as_bytes().to_vec()));
                } else if m.is_dir() {
                    out.push((rel, Vec::new()));
                    todo.push(p);
                } else if m.is_file() {
                    out.push((rel, std::fs::read(&p).unwrap()));
                } else {
                    out.push((rel, b"<special>".to_vec()));
                }
            }
        }
        out.sort();
        out
    }

    /// Whether this disk takes `Tea` for `tea` (macOS, Windows).
    pub fn folds_case(&self) -> bool {
        let probe = self.tmp.path().join("CaseProbe");
        std::fs::write(&probe, "").unwrap();
        let folds = self.tmp.path().join("caseprobe").exists();
        std::fs::remove_file(probe).unwrap();
        folds
    }

    /// Opens the FIFO of [`Hazard::Fifo`] for writing (non-blocking: no
    /// reader, no open), so a reader stuck on it goes on before the test
    /// fails.
    pub fn release_fifo(&self) {
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(self.path().join("tea/SKILL.md"));
    }

    /// The temp files a [`Guard::write`] cut short would leave
    /// (`.<file>.<tag>.tmp`) anywhere in the root.
    pub fn temp_files(&self) -> Vec<PathBuf> {
        Self::snapshot(self.path())
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.') && n.ends_with(".tmp"))
            })
            .collect()
    }
}

impl Default for Hostile {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Hostile {
    /// A read-only folder left would stop the temp folder going.
    fn drop(&mut self) {
        for dir in Self::snapshot(self.tmp.path())
            .iter()
            .map(|(p, _)| self.tmp.path().join(p))
            .chain([self.path().to_path_buf()])
        {
            if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
}

/// A call of [`Guard`] on `tea`, as a route makes it.
#[derive(Debug, Clone, Copy)]
pub enum Call {
    Read,
    ReadBelow,
    Write,
    WriteBelow,
    Remove,
    PutTree,
    PutCopy,
    Restore,
    MakeDir,
    RemoveHidden,
    PutNew,
    MakeTree,
    RemoveFile,
    Recover,
}

impl Call {
    pub const ALL: [Self; 14] = [
        Self::Read,
        Self::ReadBelow,
        Self::Write,
        Self::WriteBelow,
        Self::Remove,
        Self::PutTree,
        Self::PutCopy,
        Self::Restore,
        Self::MakeDir,
        Self::RemoveHidden,
        Self::PutNew,
        Self::MakeTree,
        Self::RemoveFile,
        Self::Recover,
    ];

    /// Runs the call on `root`; what it found, for the test to check.
    pub fn run(self, root: &UserRoot) -> Outcome {
        let tree = Tree {
            dirs: vec![PathBuf::from("assets")],
            files: vec![
                (PathBuf::from("assets/a.css"), file(b"new css")),
                (PathBuf::from("SKILL.md"), file(b"new")),
            ],
        };
        if let Self::Recover = self {
            return Outcome::Done(root.recover().map(drop));
        }
        let g: Guard<'_> = root.lock();
        let tea = Path::new("tea");
        match self {
            Self::Read => Outcome::Read(g.read(Path::new("tea/SKILL.md"))),
            Self::ReadBelow => Outcome::Read(g.read(Path::new("tea/assets/a.css"))),
            Self::Write => Outcome::Done(
                g.write(Path::new("tea/SKILL.md"), b"new", Expect::Any)
                    .map(drop)
                    .map_err(io),
            ),
            Self::WriteBelow => Outcome::Done(
                g.write(Path::new("tea/assets/a.css"), b"new", Expect::Any)
                    .map(drop)
                    .map_err(io),
            ),
            Self::Remove => Outcome::Done(g.remove(tea)),
            Self::PutTree => {
                Outcome::Done(g.put_tree(tea, Some(Path::new(".previous/tea")), |at| {
                    let made = crate::nofollow::NewDir::create(at)?;
                    crate::nofollow::write_tree(&tree, &made, None)
                }))
            }
            Self::PutCopy => {
                Outcome::Done(g.put_copy(tea, None, &tree, Some(Path::new("SKILL.md"))))
            }
            Self::Restore => Outcome::Done(g.restore(tea, Path::new(".previous/tea"))),
            Self::MakeDir => Outcome::Done(g.make_dir(tea)),
            Self::RemoveHidden => Outcome::Done(g.remove_hidden(Path::new(".previous/tea"))),
            Self::PutNew => {
                Outcome::Done(g.put_new(["tea".to_owned()], |_| Ok(tree), None).map(drop))
            }
            Self::MakeTree => Outcome::Done(g.make_tree(
                tea,
                &tree,
                Some(Path::new("SKILL.md")),
                Path::new(".mark"),
            )),
            Self::RemoveFile => Outcome::Done(g.remove_file(Path::new("tea/assets/a.css"))),
            Self::Recover => unreachable!(),
        }
    }
}

fn file(bytes: &[u8]) -> ReadFile {
    ReadFile {
        bytes: bytes.to_vec(),
        mode: 0o644,
    }
}

fn io(e: crate::user_store::WriteError) -> std::io::Error {
    match e {
        crate::user_store::WriteError::Io(e) => e,
        crate::user_store::WriteError::Stale => std::io::Error::other("stale"),
    }
}

/// What a call found.
#[derive(Debug)]
pub enum Outcome {
    Read(Found),
    Done(std::io::Result<()>),
}

/// Runs `call` on a thread of its own: `None` when it is still running
/// after 3 s (it waits on a FIFO).
pub fn within(root: &UserRoot, call: Call) -> Option<Outcome> {
    let root = root.clone();
    finishes(move || call.run(&root))
}

/// Runs `f` on a thread of its own: `None` when it is still running after
/// 3 s (it waits on a FIFO). [`Hostile::release_fifo`] then lets it go.
pub fn finishes<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(3)).ok()
}

/// Every call of [`Guard`] in every hazard: none waits on a FIFO, none
/// changes a byte outside the root, none leaves a temp file, a read
/// tells a file it cannot read from one not there, and a name alike in
/// another case is reached only where the disk takes it for the same.
#[cfg(test)]
#[test]
fn every_guard_call_holds_up_in_every_hazard() {
    use crate::user_store::{Tag, Unreadable};
    for hazard in Hazard::ALL {
        for call in Call::ALL {
            let h = Hostile::new();
            h.plant(hazard);
            let outside = Hostile::snapshot(&h.outside());
            let twin = Hostile::snapshot(&h.path().join("Tea"));
            let at = format!("{call:?} in {hazard:?}");
            let Some(outcome) = within(&h.root, call) else {
                h.release_fifo();
                panic!("{at}: still running after 3 s");
            };
            assert_eq!(
                Hostile::snapshot(&h.outside()),
                outside,
                "{at}: wrote outside the root"
            );
            assert_eq!(
                h.temp_files(),
                Vec::<PathBuf>::new(),
                "{at}: left a temp file"
            );
            if hazard == Hazard::CaseTwin && !h.folds_case() {
                assert_eq!(
                    Hostile::snapshot(&h.path().join("Tea")),
                    twin,
                    "{at}: reached the twin"
                );
            }
            match (hazard, call, &outcome) {
                // Read: what cannot be read is never taken for missing.
                (Hazard::LinkAtFile, Call::Read, Outcome::Read(f)) => {
                    assert!(
                        matches!(f, Found::Unreadable(Unreadable::Link)),
                        "{at}: {f:?}"
                    );
                }
                (Hazard::Fifo, Call::Read, Outcome::Read(f)) => {
                    assert!(
                        matches!(f, Found::Unreadable(Unreadable::NotAFile)),
                        "{at}: {f:?}"
                    );
                }
                (Hazard::TooLarge, Call::Read, Outcome::Read(f)) => {
                    assert!(
                        matches!(f, Found::Unreadable(Unreadable::TooLarge)),
                        "{at}: {f:?}"
                    );
                }
                (Hazard::LinkBelowEntry, Call::ReadBelow, Outcome::Read(f)) => {
                    assert!(
                        matches!(f, Found::Unreadable(Unreadable::Link)),
                        "{at}: {f:?}"
                    );
                }
                (Hazard::LinkedEntry, Call::Read, Outcome::Read(Found::File(r))) => {
                    assert_eq!(r.tag, Tag::parse(&tag_of(b"own")), "{at}");
                }
                // The user's own linked entry goes as a link, never its folder.
                (Hazard::LinkedEntry, Call::Remove, Outcome::Done(r)) => {
                    assert!(r.is_ok(), "{at}: {r:?}");
                    assert!(
                        h.own().join("SKILL.md").is_file(),
                        "{at}: removed the folder linked"
                    );
                }
                // A write through a link below the entry is refused.
                (Hazard::LinkBelowEntry, Call::WriteBelow, Outcome::Done(r)) => {
                    assert!(r.is_err(), "{at}: wrote through a link");
                }
                // A link or FIFO at the file is replaced, never written through.
                (Hazard::LinkAtFile | Hazard::Fifo, Call::Write, Outcome::Done(r)) => {
                    assert!(r.is_ok(), "{at}: {r:?}");
                    assert_eq!(
                        std::fs::read(h.path().join("tea/SKILL.md")).unwrap(),
                        b"new"
                    );
                }
                // A read-only folder keeps its file, whole.
                (Hazard::ReadOnly, Call::Write, Outcome::Done(r)) => {
                    assert!(r.is_err(), "{at}: wrote in a read-only folder");
                    assert_eq!(
                        std::fs::read(h.path().join("tea/SKILL.md")).unwrap(),
                        b"tea"
                    );
                }
                // A new folder is never one there already, whatever its case.
                (Hazard::None | Hazard::LinkedEntry, Call::MakeDir, Outcome::Done(r)) => {
                    assert_eq!(
                        r.as_ref().map_err(std::io::Error::kind),
                        Err(std::io::ErrorKind::AlreadyExists),
                        "{at}"
                    );
                }
                (Hazard::CaseTwin, Call::MakeDir | Call::MakeTree, Outcome::Done(r)) => {
                    assert_eq!(r.is_err(), h.folds_case(), "{at}: {r:?}");
                }
                // A new entry is never one there, nor one named alike.
                (_, Call::PutNew | Call::MakeTree, Outcome::Done(r)) => {
                    assert_eq!(
                        r.as_ref().map_err(std::io::Error::kind),
                        Err(std::io::ErrorKind::AlreadyExists),
                        "{at}"
                    );
                }
                // A file below the entry goes only where no link is on the way.
                (Hazard::LinkBelowEntry, Call::RemoveFile, Outcome::Done(r)) => {
                    assert!(r.is_err(), "{at}: removed through a link");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
fn tag_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
