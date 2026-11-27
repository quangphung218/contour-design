//! [`Guard`]: one test (or more) per question of the six a writer of a
//! user's file answers (CoE `roles/_code.md` rule 5), then the read's
//! states and the lock.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use super::*;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::time::Duration;

const LIMIT: u64 = 1024;

fn root(tmp: &tempfile::TempDir) -> UserRoot {
    let path = tmp.path().join("root");
    std::fs::create_dir_all(path.join("tea")).unwrap();
    UserRoot::open(&path, LIMIT)
}

fn rel() -> &'static Path {
    Path::new("tea/SKILL.md")
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn tag_read(g: &Guard<'_>) -> Tag {
    match g.read(rel()) {
        Found::File(r) => r.tag,
        other => panic!("no file: {other:?}"),
    }
}

// Question 1: the app stops (or the write fails) between two steps.

/// A rename that fails (a folder took the name) leaves what was there and
/// removes its temp file: no copy of the bytes lies beside it.
#[test]
fn a_write_that_fails_at_the_rename_leaves_nothing_beside_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let dir = root.path().join("tea");
    std::fs::create_dir_all(dir.join("SKILL.md/held")).unwrap();
    let res = root.lock().write(rel(), b"new", Expect::Any);
    assert!(matches!(res, Err(WriteError::Io(_))), "{res:?}");
    assert_eq!(names_in(&dir), ["SKILL.md"]);
    assert!(dir.join("SKILL.md/held").is_dir());
}

/// Up to the rename the old file is the one there, whole.
#[test]
fn a_write_refused_by_the_folder_keeps_the_old_file_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let dir = root.path().join("tea");
    std::fs::write(dir.join("SKILL.md"), "old").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let res = root.lock().write(rel(), b"new", Expect::Any);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(res.is_err());
    assert_eq!(std::fs::read(dir.join("SKILL.md")).unwrap(), b"old");
    assert_eq!(names_in(&dir), ["SKILL.md"]);
}

// Question 2: an agent writes the same file meanwhile.

/// A write sent with the tag of an earlier read finds the agent's bytes
/// and writes nothing over them.
#[test]
fn a_write_over_a_file_an_agent_changed_since_is_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let g = root.lock();
    g.write(rel(), b"form", Expect::Absent).unwrap();
    let opened = g.read(rel()).expect();
    std::fs::write(root.path().join("tea/SKILL.md"), "agent").unwrap();
    let res = g.write(rel(), b"form again", opened);
    assert!(matches!(res, Err(WriteError::Stale)), "{res:?}");
    assert_eq!(
        std::fs::read(root.path().join("tea/SKILL.md")).unwrap(),
        b"agent"
    );
}

/// A reader never finds a write half done.
#[test]
fn a_reader_never_finds_a_write_half_done() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("root");
    std::fs::create_dir_all(path.join("tea")).unwrap();
    let size = 256 * 1024;
    let root = UserRoot::open(&path, 2 * size as u64);
    let (a, b) = (vec![b'a'; size], vec![b'b'; size]);
    root.lock().write(rel(), &a, Expect::Any).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (file, stop) = (path.join("tea/SKILL.md"), stop.clone());
        std::thread::spawn(move || {
            let mut reads = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let shown = std::fs::read(&file).unwrap();
                assert_eq!(shown.len(), size, "read half written");
                assert!(shown.iter().all(|c| *c == shown[0]), "two writes mixed");
                reads += 1;
            }
            reads
        })
    };
    for i in 0..40 {
        let bytes = if i % 2 == 0 { &b } else { &a };
        root.lock().write(rel(), bytes, Expect::Any).unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(reader.join().unwrap() > 0);
}

/// Many writers from one copy (each with its tag): one lands.
#[test]
fn writers_from_one_copy_land_once() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("root");
    std::fs::create_dir_all(path.join("tea")).unwrap();
    let root = UserRoot::open(&path, LIMIT);
    root.lock().write(rel(), b"v0", Expect::Absent).unwrap();
    let opened = tag_read(&root.lock());
    let landed: usize = (0..16)
        .map(|n| {
            let (path, opened) = (path.clone(), opened.clone());
            std::thread::spawn(move || {
                let root = UserRoot::open(&path, LIMIT);
                let bytes = format!("v{}", n + 1);
                root.lock()
                    .write(rel(), bytes.as_bytes(), Expect::Tag(opened))
                    .is_ok()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| usize::from(t.join().unwrap()))
        .sum();
    assert_eq!(landed, 1);
}

// Question 3: a link at the file, at a folder on its way, at the temp name.

#[test]
fn a_link_at_the_file_is_replaced_never_written_through() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "the user's own").unwrap();
    symlink(&victim, root.path().join("tea/SKILL.md")).unwrap();
    let g = root.lock();
    assert!(matches!(g.read(rel()), Found::Unreadable(Unreadable::Link)));
    g.write(rel(), b"form", Expect::Tag(Tag::parse(Tag::UNREADABLE)))
        .unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"the user's own");
    let meta = std::fs::symlink_metadata(root.path().join("tea/SKILL.md")).unwrap();
    assert!(meta.is_file());
}

#[test]
fn a_link_at_a_folder_below_the_entry_is_never_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let away = tmp.path().join("away");
    std::fs::create_dir(&away).unwrap();
    std::fs::write(away.join("SKILL.md"), "outside").unwrap();
    symlink(&away, root.path().join("tea/away")).unwrap();
    let rel = Path::new("tea/away/SKILL.md");
    let g = root.lock();
    assert!(matches!(g.read(rel), Found::Unreadable(Unreadable::Link)));
    assert!(g.write(rel, b"form", Expect::Any).is_err());
    assert_eq!(names_in(&away), ["SKILL.md"]);
    assert_eq!(std::fs::read(away.join("SKILL.md")).unwrap(), b"outside");
}

/// The entry (a skill's folder) may be the user's own link, as the index
/// takes it: read and written where it points, with the same tag a read
/// gives. Below it, a link (folder or file) is still never followed.
#[test]
fn the_entry_may_be_a_link_and_nothing_below_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let real = tmp.path().join("elsewhere/mine");
    std::fs::create_dir_all(real.join("assets")).unwrap();
    std::fs::write(real.join("SKILL.md"), "old").unwrap();
    symlink(&real, root.path().join("mine")).unwrap();
    let rel = Path::new("mine/SKILL.md");
    let g = root.lock();
    let opened = g.read(rel).expect();
    assert_eq!(opened, Expect::Tag(Tag::of(b"old")));
    let tag = g.write(rel, b"new", opened).unwrap();
    assert_eq!(std::fs::read(real.join("SKILL.md")).unwrap(), b"new");
    assert_eq!(tag_read_at(&g, rel), tag);
    assert!(
        std::fs::symlink_metadata(root.path().join("mine"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // A link at the file inside the linked entry: not followed.
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "the user's own").unwrap();
    symlink(&victim, real.join("assets/SKILL.md")).unwrap();
    let below = Path::new("mine/assets/SKILL.md");
    assert!(matches!(g.read(below), Found::Unreadable(Unreadable::Link)));
    // A folder below the linked entry that is a link: not followed.
    symlink(tmp.path(), real.join("up")).unwrap();
    let up = Path::new("mine/up/victim");
    assert!(matches!(g.read(up), Found::Unreadable(Unreadable::Link)));
    assert!(g.write(up, b"x", Expect::Any).is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"the user's own");
    // A file at the root (`MEMORY.md`) that is a link: never followed.
    symlink(&victim, root.path().join("MEMORY.md")).unwrap();
    assert!(matches!(
        g.read(Path::new("MEMORY.md")),
        Found::Unreadable(Unreadable::Link)
    ));
}

fn tag_read_at(g: &Guard<'_>, rel: &Path) -> Tag {
    match g.read(rel) {
        Found::File(r) => r.tag,
        other => panic!("no file: {other:?}"),
    }
}

/// The temp file is made new: a link put at every name a counter or the
/// pid gives is never opened, and the temp name is not one of them.
#[test]
fn a_link_at_a_guessable_temp_name_is_never_written_through() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "the user's own").unwrap();
    let pid = std::process::id();
    for n in 0..16 {
        let at = root.path().join(format!("tea/.SKILL.md.{pid}.{n}.tmp"));
        symlink(&victim, at).unwrap();
    }
    root.lock().write(rel(), b"form", Expect::Any).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"the user's own");
    assert_eq!(names_in(&root.path().join("tea")).len(), 17);
}

/// The temp file is opened only if new: a link (or a file) at its name,
/// planted by an agent that guessed it, is refused and never written.
#[test]
fn the_temp_file_is_never_one_already_there() {
    use nix::sys::stat::Mode;
    let tmp = tempfile::tempdir().unwrap();
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "the user's own").unwrap();
    symlink(&victim, tmp.path().join(".SKILL.md.guessed.tmp")).unwrap();
    let dir = at::folder(tmp.path(), &[]).unwrap().unwrap();
    let res = new_file_at(
        &dir,
        ".SKILL.md.guessed.tmp",
        Mode::from_bits_truncate(0o644),
    );
    assert_eq!(res.err(), Some(nix::errno::Errno::EEXIST));
    assert_eq!(std::fs::read(&victim).unwrap(), b"the user's own");
}

/// A plain relative path only.
#[test]
fn a_path_out_of_the_root_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let g = root.lock();
    for rel in ["../x", "/x", "", "tea/../../x"] {
        let res = g.write(Path::new(rel), b"x", Expect::Any);
        assert!(matches!(res, Err(WriteError::Io(_))), "{rel}: {res:?}");
    }
    assert_eq!(names_in(tmp.path()), ["root"]);
}

// Question 4: the file was deleted elsewhere.

#[test]
fn a_file_deleted_elsewhere_is_missing_and_not_made_again_over_a_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let g = root.lock();
    let tag = g.write(rel(), b"v1", Expect::Absent).unwrap();
    std::fs::remove_file(root.path().join("tea/SKILL.md")).unwrap();
    assert!(matches!(g.read(rel()), Found::Missing));
    assert!(matches!(
        g.write(rel(), b"v2", Expect::Tag(tag)),
        Err(WriteError::Stale)
    ));
    assert!(!root.path().join("tea/SKILL.md").exists());
    // Its folder gone too: still missing, nothing made.
    std::fs::remove_dir(root.path().join("tea")).unwrap();
    assert!(matches!(g.read(rel()), Found::Missing));
    assert!(g.write(rel(), b"v2", Expect::Absent).is_err());
    assert!(!root.path().join("tea").exists());
}

#[test]
fn absent_is_refused_over_a_file_there() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    std::fs::write(root.path().join("tea/SKILL.md"), "agent").unwrap();
    let res = root.lock().write(rel(), b"form", Expect::Absent);
    assert!(matches!(res, Err(WriteError::Stale)));
}

// Question 5: a name alike in another case.

#[test]
fn a_write_beside_a_name_alike_reads_back_what_it_wrote() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    std::fs::write(root.path().join("tea/skill.md"), "lower case").unwrap();
    let g = root.lock();
    // macOS and Windows: one file under either name, so `Absent` is
    // refused; Linux: two files. Either way a read finds what was written.
    let tag = g.write(rel(), b"form", Expect::Any).unwrap();
    assert_eq!(tag_read(&g), tag);
}

// Question 6: the tag is of the bytes read or written.

#[test]
fn a_tag_is_of_the_bytes_it_came_with() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let g = root.lock();
    let written = g.write(rel(), b"form", Expect::Absent).unwrap();
    let Found::File(read) = g.read(rel()) else {
        panic!("no file")
    };
    assert_eq!(read.bytes, b"form");
    assert_eq!(read.tag, written);
    assert_eq!(read.tag, Tag::of(b"form"));
    let next = g
        .write(rel(), b"next", Expect::Tag(written.clone()))
        .unwrap();
    assert_ne!(next, written);
    assert!(matches!(
        g.write(rel(), b"late", Expect::Tag(written)),
        Err(WriteError::Stale)
    ));
}

// The read's states: missing is not unreadable.

#[test]
fn a_fifo_or_a_file_too_large_is_unreadable_never_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let at = root.path().join("tea/SKILL.md");
    nix::unistd::mkfifo(&at, nix::sys::stat::Mode::from_bits_truncate(0o644)).unwrap();
    // Read on a thread of its own: one that waited on the FIFO would hang
    // the test; it is let go by opening the FIFO for writing.
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = root.clone();
    std::thread::spawn(move || {
        let found = reader.lock().read(rel());
        let _ = tx.send(matches!(found, Found::Unreadable(Unreadable::NotAFile)));
    });
    let read = rx.recv_timeout(Duration::from_secs(3));
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(&at);
    assert_eq!(read, Ok(true), "waited on a FIFO, or took it for a file");
    let g = root.lock();
    std::fs::remove_file(&at).unwrap();
    std::fs::write(&at, vec![b'x'; usize::try_from(LIMIT).unwrap() + 1]).unwrap();
    assert!(matches!(
        g.read(rel()),
        Found::Unreadable(Unreadable::TooLarge)
    ));
    // A write's compare reads to the same limit.
    assert!(matches!(
        g.write(rel(), b"x", Expect::Absent),
        Err(WriteError::Stale)
    ));
    g.write(rel(), b"x", Expect::Tag(Tag::parse(Tag::UNREADABLE)))
        .unwrap();
}

#[test]
fn a_file_keeps_its_bits_and_a_read_only_one_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(&tmp);
    let at = root.path().join("tea/SKILL.md");
    std::fs::write(&at, "old").unwrap();
    std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o600)).unwrap();
    let g = root.lock();
    g.write(rel(), b"new", Expect::Any).unwrap();
    let mode = std::fs::metadata(&at).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o444)).unwrap();
    let res = g.write(rel(), b"newer", Expect::Any);
    assert!(
        matches!(&res, Err(WriteError::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert_eq!(std::fs::read(&at).unwrap(), b"new");
}

// The lock: one per root path.

#[test]
fn two_roots_opened_on_one_path_share_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("shared");
    let held = UserRoot::open(&path, LIMIT);
    let g = held.lock();
    let (tx, rx) = std::sync::mpsc::channel();
    let other = std::thread::spawn({
        let path = path.clone();
        move || {
            let root = UserRoot::open(&path, LIMIT);
            let _g = root.lock();
            tx.send(()).unwrap();
        }
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "not held"
    );
    drop(g);
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    other.join().unwrap();
    // Another path: its own lock.
    let _g = held.lock();
    let _h = UserRoot::open(&tmp.path().join("other"), LIMIT).lock();
}

// Remove and recover: an entry goes whole, a link goes as a link, and an
// app stopped at any step loses no byte of another entry.

/// Every file under `dir` (no link followed) with its bytes, and every
/// link with where it points.
fn tree_of(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(at) = todo.pop() {
        for e in std::fs::read_dir(&at).unwrap() {
            let p = e.unwrap().path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            let rel = p.strip_prefix(dir).unwrap().to_path_buf();
            if m.file_type().is_symlink() {
                out.push((
                    rel,
                    std::fs::read_link(&p).unwrap().into_os_string().into_vec(),
                ));
            } else if m.is_dir() {
                out.push((rel, Vec::new()));
                todo.push(p);
            } else {
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

use std::os::unix::ffi::OsStringExt;

/// A root of three skills (`tea` with side files, `kept`, and `linked`, a
/// link to `kept`) and a file outside it that a link in `tea` points at.
fn root_of_three(tmp: &tempfile::TempDir) -> UserRoot {
    let root = root(tmp);
    let p = root.path();
    std::fs::write(p.join("tea/SKILL.md"), "tea").unwrap();
    std::fs::create_dir_all(p.join("tea/assets/deep")).unwrap();
    std::fs::write(p.join("tea/assets/deep/style.css"), "css").unwrap();
    std::fs::create_dir_all(p.join("kept")).unwrap();
    std::fs::write(p.join("kept/SKILL.md"), "kept").unwrap();
    std::fs::write(tmp.path().join("outside.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("outside.txt"), p.join("tea/out")).unwrap();
    std::os::unix::fs::symlink(tmp.path(), p.join("tea/assets/up")).unwrap();
    symlink(p.join("kept"), p.join("linked")).unwrap();
    root
}

/// Lỗi 5: a skill that is a link to another skill goes as a link; the
/// skill it points at stays whole.
#[test]
fn removing_a_linked_entry_removes_the_link_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root_of_three(&tmp);
    let kept = tree_of(&root.path().join("kept"));
    root.lock().remove(Path::new("linked")).unwrap();
    assert!(std::fs::symlink_metadata(root.path().join("linked")).is_err());
    assert_eq!(tree_of(&root.path().join("kept")), kept);
    assert_eq!(names_in(root.path()), ["kept", "tea"]);
}

/// A folder goes with all it holds; a link in it goes as a link, never
/// followed (the file outside, the folder above, stay).
#[test]
fn removing_a_folder_follows_no_link_in_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root_of_three(&tmp);
    let before = tree_of(tmp.path());
    root.lock().remove(Path::new("tea")).unwrap();
    assert_eq!(names_in(root.path()), ["kept", "linked"]);
    let tea = Path::new("root/tea");
    let left: Vec<_> = before
        .into_iter()
        .filter(|(p, _)| !p.starts_with(tea))
        .collect();
    assert_eq!(tree_of(tmp.path()), left);
}

/// Only one plain visible name under the root is removed: a path, `..`,
/// a hidden name (a journal, a staging folder) and nothing there are
/// refused with nothing changed.
#[test]
fn remove_takes_one_visible_name_there() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root_of_three(&tmp);
    std::fs::create_dir(root.path().join(".previous")).unwrap();
    let before = tree_of(tmp.path());
    for rel in ["tea/assets", "..", "../root", ".previous", "", "gone"] {
        let res = root.lock().remove(Path::new(rel));
        assert!(res.is_err(), "{rel:?} removed");
    }
    assert_eq!(tree_of(tmp.path()), before);
}

/// Stopped after each step, then recovered: the other entries keep every
/// byte, the entry is whole (stopped before the rename) or gone (after),
/// and nothing hidden is left. A second recover changes nothing.
#[test]
fn a_remove_cut_short_at_any_step_loses_nothing_of_another_entry() {
    for (at, gone) in [
        (Step::Journaled, false),
        (Step::Staged, true),
        (Step::Removed, true),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = root_of_three(&tmp);
        let before = tree_of(tmp.path());
        let res = root.lock().remove_steps(Path::new("tea"), |step| {
            if step == at {
                Err(std::io::ErrorKind::Interrupted.into())
            } else {
                Ok(())
            }
        });
        assert!(res.is_err(), "{at:?}");
        if at == Step::Staged {
            // Stopped halfway through the tree, too.
            let staged = std::fs::read_dir(root.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.to_string_lossy().contains(REMOVING))
                .unwrap();
            std::fs::remove_file(staged.join("SKILL.md")).unwrap();
        }
        assert!(root.recover().unwrap() >= 1, "{at:?}");
        let tea = Path::new("root/tea");
        let want: Vec<_> = before
            .iter()
            .filter(|(p, _)| !gone || !p.starts_with(tea))
            .cloned()
            .collect();
        assert_eq!(tree_of(tmp.path()), want, "{at:?}");
        assert_eq!(root.recover().unwrap(), 0, "{at:?}");
    }
}

/// A rename that fails (the entry went meanwhile) leaves no journal:
/// nothing is left for a recover to take for a remove under way.
#[test]
fn a_remove_whose_rename_fails_leaves_no_journal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root_of_three(&tmp);
    let p = root.path().to_path_buf();
    let res = root.lock().remove_steps(Path::new("tea"), |step| {
        if step == Step::Journaled {
            std::fs::remove_dir_all(p.join("tea")).unwrap();
        }
        Ok(())
    });
    assert!(res.is_err());
    assert_eq!(names_in(root.path()), ["kept", "linked"]);
}

/// Recover removes what a cut-short write left (in the root and in an
/// entry) and a `.removing-<tag>` with no journal; it keeps a journal of
/// another kind (an operation 4b adds), a hidden file that is no temp
/// file, a folder at a temp name, and every entry.
#[test]
fn recover_removes_only_what_it_made() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root_of_three(&tmp);
    let p = root.path();
    let tag = nofollow::random_tag();
    std::fs::write(p.join(format!(".MEMORY.md.{tag}.tmp")), "half").unwrap();
    std::fs::write(p.join(format!("tea/.SKILL.md.{tag}.tmp")), "half").unwrap();
    std::fs::write(p.join(format!("linked/.SKILL.md.{tag}.tmp")), "half").unwrap();
    std::fs::create_dir_all(p.join(format!("{REMOVING}{tag}/x"))).unwrap();
    let before = tree_of(tmp.path());
    let other = nofollow::random_tag();
    std::fs::write(p.join(format!("{JOURNAL}{other}")), "install\ntea").unwrap();
    std::fs::write(p.join(".notes.tmp"), "mine").unwrap();
    std::fs::write(p.join("tea/.style.v2.tmp"), "mine").unwrap();
    std::fs::create_dir(p.join(format!(".dir.{tag}.tmp"))).unwrap();
    // A folder in an entry with a temp-like name is the user's, not a temp.
    std::fs::create_dir(p.join(format!("tea/.dir.{tag}.tmp"))).unwrap();
    std::fs::create_dir(p.join(format!("{REMOVING}not-a-tag"))).unwrap();
    let kept = tree_of(tmp.path())
        .into_iter()
        .filter(|e| !before.contains(e))
        .collect::<Vec<_>>();
    assert_eq!(root.recover().unwrap(), 4);
    let now = tree_of(tmp.path());
    for e in &kept {
        assert!(now.contains(e), "{e:?} removed");
    }
    for name in ["kept", "linked", "tea"] {
        assert!(p.join(name).exists());
    }
    assert!(!p.join(format!("{REMOVING}{tag}")).exists());
    assert!(!p.join(format!("tea/.SKILL.md.{tag}.tmp")).exists());
    assert!(!p.join(format!("kept/.SKILL.md.{tag}.tmp")).exists());
}

/// A root not made yet has nothing to recover.
#[test]
fn recover_of_a_root_not_there_is_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = UserRoot::open(&tmp.path().join("none"), LIMIT);
    assert_eq!(root.recover().unwrap(), 0);
}
