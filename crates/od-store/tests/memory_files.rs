//! The memory files (`<data>/memory/MEMORY.md`, `config.json`) sit in a
//! folder an agent can write (its shell, or a loopback call from outside
//! its sandbox). They are written through a new file of a random name made
//! `O_EXCL|O_NOFOLLOW` and renamed over them, and read without following a
//! link and without waiting on a FIFO (`od_fs::nofollow`;
//! `crates/od-server/CLAUDE.md`, invariants 15 and 16). One test per
//! question of the six a writer of a user's file answers.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_store::manage::GlobalMemory;
use od_store::memory_index::{self, CONFIG_FILE, INDEX_FILE, MemoryFiles, Stored};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;
use std::time::Duration;

fn files_in(tmp: &tempfile::TempDir) -> MemoryFiles {
    let files = MemoryFiles::in_data_dir(tmp.path());
    std::fs::create_dir_all(files.dir()).unwrap();
    files
}

fn entry(id: &str) -> GlobalMemory {
    GlobalMemory {
        id: id.to_owned(),
        kind: "user".to_owned(),
        text: format!("fact {id}"),
        created_at: 0,
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Runs `read` on its own thread; whether it returned within 3 s. A FIFO
/// it waits on is then opened for writing (non-blocking: no reader, no
/// open) so the reader goes on and the test process can end.
fn returns_in_time(fifo: &Path, read: impl FnOnce() + Send + 'static) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        read();
        let _ = tx.send(());
    });
    let done = rx.recv_timeout(Duration::from_secs(3)).is_ok();
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(fifo);
    done
}

fn text(index: &str) -> Stored {
    Stored::Text(index.to_owned())
}

fn make_fifo(path: &Path) {
    nix::unistd::mkfifo(path, nix::sys::stat::Mode::from_bits_truncate(0o644)).unwrap();
}

// Question 3: a link at the temp name or at the file.

#[test]
fn a_save_never_writes_through_a_link_planted_at_a_temp_name() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let victim = tmp.path().join("outside.txt");
    std::fs::write(&victim, "the user's own file").unwrap();
    // The names the old writer used: pid and a counter from 0.
    let pid = std::process::id();
    for n in 0..64 {
        symlink(&victim, files.dir().join(format!("MEMORY.{pid}.{n}.tmp"))).unwrap();
        symlink(&victim, files.dir().join(format!("config.{pid}.{n}.tmp"))).unwrap();
    }
    files.write_index("# index saved by the user\n").unwrap();
    files
        .write_config(&memory_index::MemoryConfig::default())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "the user's own file"
    );
    for name in [INDEX_FILE, CONFIG_FILE] {
        let meta = std::fs::symlink_metadata(files.dir().join(name)).unwrap();
        assert!(meta.is_file(), "{name} is a regular file of its own");
    }
    assert_eq!(
        files.stored_index(),
        Stored::Text("# index saved by the user\n".to_owned())
    );
}

#[test]
fn a_link_at_the_index_or_the_config_is_replaced_never_written_through() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let victim = tmp.path().join("outside.txt");
    std::fs::write(&victim, "the user's own file").unwrap();
    symlink(&victim, files.dir().join(INDEX_FILE)).unwrap();
    symlink(&victim, files.dir().join(CONFIG_FILE)).unwrap();
    files.write_index("# saved\n").unwrap();
    files
        .write_config(&memory_index::MemoryConfig::default())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "the user's own file"
    );
    assert!(
        std::fs::symlink_metadata(files.dir().join(INDEX_FILE))
            .unwrap()
            .is_file()
    );
    assert_eq!(files.stored_index(), text("# saved\n"));
}

#[test]
fn the_index_read_never_follows_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "SECRET OUTSIDE THE DATA FOLDER").unwrap();
    symlink(&secret, files.dir().join(INDEX_FILE)).unwrap();
    // There but not read: never taken for a missing index.
    assert_eq!(files.stored_index(), Stored::Unreadable);
    // Question 6: the index shown (and tagged by `GET /api/memory/index`)
    // is the default one, from this one read.
    let entries = [entry("a1")];
    assert_eq!(files.index(&entries), memory_index::default_index(&entries));
    // Fail closed: no entry reaches the prompt.
    let config = memory_index::MemoryConfig::default();
    let stored = files.stored_index();
    assert!(memory_index::active(&config, &stored, &entries).is_empty());
    // `link` leaves it as it is; a Save puts a file of its own there.
    files.link(&entry("b2")).unwrap();
    assert!(
        std::fs::symlink_metadata(files.dir().join(INDEX_FILE))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let saved = files.write_index_if("# saved\n", |s| {
        Ok::<_, std::io::Error>(s == Stored::Unreadable)
    });
    assert!(saved.unwrap());
    assert_eq!(files.stored_index(), text("# saved\n"));
    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "SECRET OUTSIDE THE DATA FOLDER"
    );
}

#[test]
fn the_index_read_never_waits_on_a_fifo() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let fifo = files.dir().join(INDEX_FILE);
    make_fifo(&fifo);
    let reader = files.clone();
    let done = returns_in_time(&fifo, move || {
        assert_eq!(reader.stored_index(), Stored::Unreadable);
    });
    assert!(done, "stored_index waited on a FIFO for 3 s");
}

#[test]
fn the_config_read_never_follows_a_link_or_waits_on_a_fifo() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let off = tmp.path().join("off.json");
    std::fs::write(&off, r#"{"enabled":false}"#).unwrap();
    symlink(&off, files.dir().join(CONFIG_FILE)).unwrap();
    assert!(files.config().enabled, "a linked config is not read");

    std::fs::remove_file(files.dir().join(CONFIG_FILE)).unwrap();
    let fifo = files.dir().join(CONFIG_FILE);
    make_fifo(&fifo);
    let reader = files.clone();
    let done = returns_in_time(&fifo, move || {
        assert!(reader.config().enabled);
    });
    assert!(done, "config waited on a FIFO for 3 s");
}

#[test]
fn an_index_past_the_read_limit_is_not_read() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let big = "x".repeat(usize::try_from(memory_index::MAX_INDEX_READ_BYTES).unwrap() + 1);
    std::fs::write(files.dir().join(INDEX_FILE), big).unwrap();
    assert_eq!(
        files.stored_index(),
        Stored::Unreadable,
        "read past the limit"
    );
}

// Question 1: the app stops between two steps.

#[test]
fn a_save_that_fails_leaves_the_index_it_had_and_nothing_beside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    files.write_index("# old\n").unwrap();
    let before = names_in(files.dir());
    std::fs::set_permissions(files.dir(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let res = files.write_index("# new\n");
    std::fs::set_permissions(files.dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(res.is_err(), "a read-only folder takes no new file");
    assert_eq!(files.stored_index(), text("# old\n"));
    assert_eq!(names_in(files.dir()), before);
}

// Question 2: someone reads while a save writes.

#[test]
fn a_reader_never_finds_the_index_half_written() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let size = 256 * 1024;
    let (a, b) = ("a".repeat(size), "b".repeat(size));
    files.write_index(&a).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (files, stop) = (files.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut reads = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Stored::Text(shown) = files.stored_index() else {
                    panic!("no index")
                };
                assert_eq!(shown.len(), size, "read half written");
                assert!(shown == shown[..1].repeat(size), "read two saves mixed");
                reads += 1;
            }
            reads
        })
    };
    for i in 0..40 {
        files.write_index(if i % 2 == 0 { &b } else { &a }).unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(reader.join().unwrap() > 0);
}

// Question 4: the file was deleted elsewhere.

#[test]
fn an_index_deleted_elsewhere_is_not_made_again() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    files.write_index("# Memory\n").unwrap();
    std::fs::remove_file(files.dir().join(INDEX_FILE)).unwrap();
    files.link(&entry("a1")).unwrap();
    files.unlink("a1").unwrap();
    assert!(!files.dir().join(INDEX_FILE).exists());
    let entries = [entry("a1")];
    assert_eq!(files.index(&entries), memory_index::default_index(&entries));
}

// Question 5: a name alike in another case.

#[test]
fn a_save_beside_a_name_alike_reads_back_what_it_wrote() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    std::fs::write(files.dir().join("memory.md"), "lower case, not ours").unwrap();
    files.write_index("# saved\n").unwrap();
    // macOS and Windows: one file under either name; Linux: two files,
    // and the index is the one named `MEMORY.md`. Either way the read
    // finds what the save wrote.
    assert_eq!(files.stored_index(), text("# saved\n"));
}

// Question 2 again: an agent writes the index between the compare and the
// write (from its own process, so the lock does not hold it).

#[test]
fn a_save_over_an_index_an_agent_wrote_meanwhile_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    files.write_index("# opened\n").unwrap();
    let at = files.dir().join(INDEX_FILE);
    let saved = files.write_index_if("# form\n", |stored| {
        std::fs::write(&at, "# agent\n").unwrap();
        Ok::<_, std::io::Error>(stored == text("# opened\n"))
    });
    assert!(!saved.unwrap(), "written over the agent's index");
    assert_eq!(files.stored_index(), text("# agent\n"));
}

/// Missing and unreadable are told apart: a missing index links every
/// entry, a FIFO there links none.
#[test]
fn a_missing_index_links_all_and_an_unreadable_one_none() {
    let tmp = tempfile::tempdir().unwrap();
    let files = files_in(&tmp);
    let entries = [entry("a1"), entry("b2")];
    let config = memory_index::MemoryConfig::default();
    let stored = files.stored_index();
    assert_eq!(stored, Stored::Missing);
    assert_eq!(memory_index::active(&config, &stored, &entries).len(), 2);
    make_fifo(&files.dir().join(INDEX_FILE));
    let stored = files.stored_index();
    assert_eq!(stored, Stored::Unreadable);
    assert!(memory_index::active(&config, &stored, &entries).is_empty());
}
