//! A reload that returns shows every write made before it was called, also
//! while another reload (started earlier, before the write) is still
//! scanning: that one must not swap its older snapshot in last.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::{ContentRoots, ContentStore};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn a_reload_is_never_undone_by_an_older_one() {
    let data = tempfile::tempdir().unwrap();
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let store = Arc::new(ContentStore::load(
        ContentRoots::bundled(&content).with_user_root(data.path()),
    ));
    // Another request's reload, over and over.
    let stop = Arc::new(AtomicBool::new(false));
    let other = {
        let (store, stop) = (Arc::clone(&store), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                store.reload();
            }
        })
    };
    let mut missing = Vec::new();
    for i in 0..20 {
        let id = format!("race-{i}");
        let dir = data.path().join("skills").join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: x\n---\n\nBody.\n"),
        )
        .unwrap();
        store.reload();
        if store.snapshot().find_skill(&id).is_none() {
            missing.push(id);
        }
    }
    stop.store(true, Ordering::Relaxed);
    other.join().unwrap();
    assert!(missing.is_empty(), "lost to an older reload: {missing:?}");
}
