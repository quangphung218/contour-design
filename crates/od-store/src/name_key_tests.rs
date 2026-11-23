//! A project's file names compare one way on every disk: as macOS and
//! Windows do by default (case ignored, NFC and NFD one name), also where
//! the disk tells them apart (Linux, a case-sensitive volume). A file the
//! user has is never written over, nor joined by a second file that is
//! the same one on a Mac (#225, #242, #94).
//!
//! The asserts hold on every disk: no test sets up two names that one
//! disk folds, and each looks at what is on disk, not at what came back.

use crate::data_dir::DataDir;
use crate::error::StoreError;
use crate::paths::{as_on_disk, name_key};
use crate::store::Store;
use od_contracts::{CommentRect, CreateCommentRequest, CreateProjectRequest};
use std::fs;
use std::path::PathBuf;

/// `mẩu.png` composed (NFC), as Windows and the browser send it.
const MAU_NFC: &str = "m\u{1ea9}u.png";
/// `mẩu.png` decomposed (NFD), as macOS may send it.
const MAU_NFD: &str = "ma\u{302}\u{309}u.png";

struct Project {
    _tmp: tempfile::TempDir,
    store: Store,
    id: String,
    conversation: String,
    dir: PathBuf,
}

fn project() -> Project {
    let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
    let (project, conversation) = store
        .create_project(CreateProjectRequest {
            name: "p".into(),
            ..CreateProjectRequest::default()
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let dir = store
        .project_root(&project)
        .unwrap_or_else(|e| panic!("{e}"))
        .dir;
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    Project {
        _tmp: tmp,
        store,
        id: project.id,
        conversation,
        dir,
    }
}

impl Project {
    /// A file put there outside the app (Finder, the agent's shell).
    fn put(&self, rel: &str, bytes: &[u8]) {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| panic!("{e}"));
        }
        fs::write(path, bytes).unwrap_or_else(|e| panic!("{e}"));
    }

    /// The names in folder `rel` ("" for the project's), as on disk,
    /// sorted, hidden ones left out.
    fn names(&self, rel: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir.join(rel))
            .unwrap_or_else(|e| panic!("{e}"))
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();
        names
    }

    fn bytes(&self, rel: &str) -> Vec<u8> {
        fs::read(self.dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }
}

#[test]
fn the_key_ignores_case_and_unicode_form() {
    assert_eq!(name_key("Logo.PNG"), name_key("logo.png"));
    assert_eq!(name_key(MAU_NFC), name_key(MAU_NFD));
    assert_eq!(name_key("MẨU.png"), name_key(MAU_NFD));
    assert_eq!(name_key("Assets/Logo.png"), "assets/logo.png");
    assert_ne!(name_key("màu.png"), name_key(MAU_NFC));
    assert_ne!(name_key("logo.png"), name_key("logo-1.png"));
}

#[test]
fn a_name_is_spelled_as_the_entry_on_disk() {
    let p = project();
    p.put("Assets/Logo.png", b"old");
    p.put(MAU_NFD, b"nfd");
    assert_eq!(as_on_disk(&p.dir, "assets/logo.png"), "Assets/Logo.png");
    assert_eq!(as_on_disk(&p.dir, MAU_NFC), MAU_NFD);
    assert_eq!(as_on_disk(&p.dir, "ASSETS/new.png"), "Assets/new.png");
    assert_eq!(as_on_disk(&p.dir, "other/Logo.png"), "other/Logo.png");
}

#[test]
fn an_upload_never_takes_a_name_that_differs_only_in_case() {
    let p = project();
    p.put("Logo.png", b"old");
    let up = p
        .store
        .upload_file(&p.id, None, "logo.png", b"new")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(up.path, "logo-1.png");
    assert_eq!(p.names(""), ["Logo.png", "logo-1.png"]);
    assert_eq!(p.bytes("Logo.png"), b"old");
}

#[test]
fn an_upload_never_takes_a_name_that_differs_only_in_unicode_form() {
    let p = project();
    p.put(MAU_NFD, b"old");
    let up = p
        .store
        .upload_file(&p.id, None, MAU_NFC, b"new")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(name_key(&up.path), name_key("mẩu-1.png"));
    assert_eq!(p.names("").len(), 2, "{:?}", p.names(""));
    assert_eq!(p.bytes(MAU_NFD), b"old");
}

#[test]
fn one_batch_with_two_names_alike_keeps_both() {
    let p = project();
    let up = p
        .store
        .upload_files(
            &p.id,
            None,
            &[
                ("Logo.png", b"one".as_slice()),
                ("logo.png", b"two"),
                (MAU_NFC, b"three"),
            ],
            None,
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let paths: Vec<&str> = up.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths[..2], ["Logo.png", "logo-1.png"]);
    assert_eq!(p.bytes("Logo.png"), b"one");
    assert_eq!(p.bytes("logo-1.png"), b"two");
    // A second batch: `mẩu.png` sent decomposed is a new file.
    let again = p
        .store
        .upload_files(&p.id, None, &[(MAU_NFD, b"four")], None)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(name_key(&again[0].path), name_key("mẩu-1.png"));
    assert_eq!(p.bytes(&up[2].path), b"three");
}

#[test]
fn an_upload_into_a_folder_of_another_case_goes_into_that_folder() {
    let p = project();
    p.put("assets/a.png", b"a");
    let up = p
        .store
        .upload_file(&p.id, Some("Assets"), "b.png", b"b")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(up.path, "assets/b.png");
    assert_eq!(p.names(""), ["assets"]);
}

#[test]
fn a_new_file_named_as_another_in_other_case_is_refused() {
    let p = project();
    p.put("Logo.png", b"old");
    p.put(MAU_NFD, b"nfd");
    for name in ["logo.png", MAU_NFC] {
        let res = p.store.write_file(&p.id, name, b"new", false, None);
        assert!(
            matches!(res, Err(StoreError::FileExists)),
            "{name}: {res:?}"
        );
        let res = p.store.create_file_exact(&p.id, name, b"new");
        assert!(
            matches!(res, Err(StoreError::FileExists)),
            "{name}: {res:?}"
        );
    }
    assert_eq!(p.bytes("Logo.png"), b"old");
    assert_eq!(p.bytes(MAU_NFD), b"nfd");
    assert_eq!(p.names("").len(), 2);
}

/// An overwrite of `logo.png` writes `Logo.png`, as the Mac does, and
/// says so: the history and comments are those of the file on disk.
#[test]
fn an_overwrite_writes_the_file_of_that_name_on_disk() {
    let p = project();
    p.put("Page.html", b"<p>old</p>");
    let file = p
        .store
        .write_file(&p.id, "page.html", b"<p>new</p>", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(file.path, "Page.html");
    assert_eq!(p.names(""), ["Page.html"]);
    assert_eq!(p.bytes("Page.html"), b"<p>new</p>");
}

/// The overwrite is of the file there: its comments stay (a new file at
/// a name drops those left by a file gone from it).
#[test]
fn an_overwrite_keeps_the_comments_of_the_file_on_disk() {
    let p = project();
    p.store
        .write_file(&p.id, "Page.html", b"<h1>old</h1>", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    p.store
        .create_comment(
            &p.id,
            &p.conversation,
            CreateCommentRequest {
                file_path: "Page.html".into(),
                selector: "h1".into(),
                label: "h1".into(),
                position: CommentRect {
                    x: 1.0,
                    y: 2.0,
                    width: 3.0,
                    height: 4.0,
                },
                note: "Make it red".into(),
                ..CreateCommentRequest::default()
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
    p.store
        .write_file(&p.id, "page.html", b"<h1>new</h1>", true, None)
        .unwrap_or_else(|e| panic!("{e}"));
    let comments = p
        .store
        .list_comments(&p.id, &p.conversation)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(comments.len(), 1, "the comment on Page.html went");
}

#[test]
fn a_rename_onto_a_name_alike_is_refused() {
    let p = project();
    p.put("Logo.png", b"old");
    p.put("b.png", b"b");
    p.put(MAU_NFD, b"nfd");
    for to in ["logo.png", "LOGO.PNG", MAU_NFC] {
        let res = p.store.rename_file(&p.id, "b.png", to);
        assert!(matches!(res, Err(StoreError::FileExists)), "{to}: {res:?}");
    }
    assert_eq!(p.bytes("Logo.png"), b"old");
    assert_eq!(p.bytes("b.png"), b"b");
    assert_eq!(p.names("").len(), 3);
}

#[test]
fn a_file_renamed_to_its_own_name_in_other_case_takes_it() {
    let p = project();
    p.put("sub/a.html", b"<p>a</p>");
    let file = p
        .store
        .rename_file(&p.id, "sub/a.html", "SUB/A.html")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(file.path, "sub/A.html");
    assert_eq!(p.names("sub"), ["A.html"]);
    assert_eq!(p.bytes("sub/A.html"), b"<p>a</p>");
}

#[test]
fn a_folder_moved_to_its_own_name_in_other_case_takes_it() {
    let p = project();
    p.put("assets/a.png", b"a");
    let (folder, moved) = p
        .store
        .move_folder(&p.id, "assets", "Assets")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((folder.as_str(), moved), ("Assets", 1));
    assert_eq!(p.names(""), ["Assets"]);
    assert_eq!(p.bytes("Assets/a.png"), b"a");
}

#[test]
fn a_folder_moved_onto_a_name_alike_is_refused() {
    let p = project();
    p.put("assets/a.png", b"a");
    p.put("img/b.png", b"b");
    let res = p.store.move_folder(&p.id, "img", "Assets");
    assert!(matches!(res, Err(StoreError::FileExists)), "{res:?}");
    let res = p.store.move_folder(&p.id, "assets", "ASSETS/inner");
    assert!(
        matches!(res, Err(StoreError::BadRequest(_))),
        "into itself: {res:?}"
    );
    assert_eq!(p.names(""), ["assets", "img"]);
    assert_eq!(p.names("assets"), ["a.png"]);
}

#[test]
fn a_folder_made_as_another_in_other_case_is_that_folder() {
    let p = project();
    p.put("assets/a.png", b"a");
    let folder = p
        .store
        .create_folder(&p.id, "Assets")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(folder, "assets");
    assert_eq!(p.names(""), ["assets"]);
}
