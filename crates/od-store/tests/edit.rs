//! `Store::edit_html`: in-place edits recorded as manual versions.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_contracts::{CreateProjectRequest, VersionSource};
use od_store::{DataDir, Store, StoreError};

fn project(store: &Store) -> String {
    let (p, _) = store
        .create_project(CreateProjectRequest {
            name: "Edit".into(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    p.id
}

#[test]
fn edit_writes_file_and_manual_version_keeping_the_old_text() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(DataDir::new(tmp.path())).unwrap();
    let id = project(&store);
    store
        .write_file(&id, "index.html", b"<h1>Old</h1>", true, None)
        .unwrap();
    // Written behind the store's back (an agent): not in the history yet.
    let root = store
        .project_root(&store.get_project(&id).unwrap())
        .unwrap();
    std::fs::write(root.dir.join("index.html"), "<h1>Agent</h1>").unwrap();

    let (file, version) = store
        .edit_html::<StoreError>(&id, "index.html", "Edited in preview", |text| {
            assert_eq!(text, "<h1>Agent</h1>");
            Ok(text.replace("Agent", "New"))
        })
        .unwrap();
    assert_eq!(file.name, "index.html");
    assert_eq!(version.source, VersionSource::Manual);
    assert_eq!(version.label, "Edited in preview");
    assert!(version.current);
    let (_, bytes) = store.read_file(&id, "index.html").unwrap();
    assert_eq!(bytes, b"<h1>New</h1>");
    let history = store.list_versions(&id, "index.html").unwrap().versions;
    let texts: Vec<String> = history
        .iter()
        .map(|v| store.read_version(&id, "index.html", &v.id).unwrap().1)
        .collect();
    assert_eq!(texts, ["<h1>Old</h1>", "<h1>Agent</h1>", "<h1>New</h1>"]);
}

#[test]
fn edit_refuses_bad_paths_and_keeps_file_on_error() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(DataDir::new(tmp.path())).unwrap();
    let id = project(&store);
    store
        .write_file(&id, "index.html", b"<p>x</p>", true, None)
        .unwrap();
    store.write_file(&id, "a.css", b"p{}", true, None).unwrap();
    let ok = |t: &str| Ok::<_, StoreError>(t.to_owned());
    for bad in ["../index.html", "/etc/passwd.html", "a/../../x.html", ""] {
        assert!(
            matches!(
                store.edit_html(&id, bad, "x", ok),
                Err(StoreError::InvalidPath | StoreError::PathEscape | StoreError::BadRequest(_))
            ),
            "{bad}"
        );
    }
    assert!(matches!(
        store.edit_html(&id, "a.css", "x", ok),
        Err(StoreError::NotHtml)
    ));
    assert!(matches!(
        store.edit_html(&id, "missing.html", "x", ok),
        Err(StoreError::FileNotFound)
    ));
    let err = store
        .edit_html(&id, "index.html", "x", |_| {
            Err::<String, _>(StoreError::BadRequest("no".into()))
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::BadRequest(_)));
    assert_eq!(store.read_file(&id, "index.html").unwrap().1, b"<p>x</p>");
}
