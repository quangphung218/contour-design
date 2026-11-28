//! Windows: an entry that is a link to a folder (a directory symlink, a
//! junction) is removed as a link; the folder it points at stays whole.
//! `DeleteFileW` refuses both, so the remove takes the folder's way
//! (`RemoveDirectoryW`, which never goes into what the link points at).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use super::UserRoot;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
enum Kind {
    Symlink,
    Junction,
}

/// Makes `link` a link of `kind` to the folder `to`. A directory symlink
/// needs a right this machine may not give: then `false`, logged (the
/// junction, which needs none, still runs the branch).
fn link_folder(kind: Kind, to: &Path, link: &Path) -> bool {
    match kind {
        Kind::Symlink => match std::os::windows::fs::symlink_dir(to, link) {
            Ok(()) => true,
            Err(e) => {
                eprintln!(
                    "skipped the directory symlink, not allowed here ({e}); the junction runs"
                );
                false
            }
        },
        Kind::Junction => {
            let out = std::process::Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(link)
                .arg(to)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "mklink /J: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            true
        }
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

fn own_is_whole(own: &Path, at: &str) {
    assert_eq!(std::fs::read(own.join("SKILL.md")).unwrap(), b"own", "{at}");
    assert_eq!(
        std::fs::read(own.join("assets").join("a.css")).unwrap(),
        b"css",
        "{at}"
    );
}

#[test]
fn an_entry_linked_to_a_folder_goes_as_a_link_alone() {
    for kind in [Kind::Symlink, Kind::Junction] {
        let tmp = tempfile::tempdir().unwrap();
        let own = tmp.path().join("own");
        std::fs::create_dir_all(own.join("assets")).unwrap();
        std::fs::write(own.join("SKILL.md"), "own").unwrap();
        std::fs::write(own.join("assets").join("a.css"), "css").unwrap();
        let root = UserRoot::open(&tmp.path().join("root"), 1024);
        std::fs::create_dir_all(root.path().join(".previous")).unwrap();
        if !link_folder(kind, &own, &root.path().join("tea")) {
            continue;
        }
        let at = format!("{kind:?}");

        // Guard::remove: journaled, renamed `.removing-<tag>`, removed.
        root.lock().remove(Path::new("tea")).unwrap();
        assert_eq!(
            names_in(root.path()),
            [".previous"],
            "{at}: the link, its journal or its staged name is left"
        );
        own_is_whole(&own, &at);

        // Guard::remove_hidden (`remove_gone`, as `recover` and a put's clean-up).
        assert!(link_folder(
            kind,
            &own,
            &root.path().join(".previous").join("tea")
        ));
        root.lock()
            .remove_hidden(Path::new(".previous/tea"))
            .unwrap();
        assert!(
            std::fs::symlink_metadata(root.path().join(".previous").join("tea")).is_err(),
            "{at}: the kept link is left"
        );
        own_is_whole(&own, &at);
    }
}
