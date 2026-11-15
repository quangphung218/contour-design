//! Every `Store` operation that touches a page's version history, in every
//! state that history can be in ([`crate::versions`]): one assert per cell,
//! so a fix for one cell cannot leave its neighbour broken (QA-102, QA-103).

#![cfg(unix)]

use crate::data_dir::DataDir;
use crate::error::StoreError;
use crate::store::Store;
use crate::versions::{self, VERSION_ROOT};
use od_contracts::{CreateProjectRequest, CreateVersionRequest};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const PAGE: &str = "page.html";

/// The state of `page.html`'s history before the operation.
#[derive(Clone, Copy, Debug)]
enum State {
    /// The page put there outside the app: no history yet.
    Absent,
    /// Two versions; the page holds the second.
    Open,
    /// Two versions; the page changed outside the app since.
    OpenChanged,
    /// The page deleted in the app, then made again outside it.
    ClosedDeleted,
    /// Renamed to `page.md`, replaced there outside the app, renamed back.
    ClosedReplaced,
    /// `.file-versions` is a link the agent made.
    LinkedRoot,
    /// The page's own history folder is a link.
    LinkedPage,
}

const STATES: [State; 7] = [
    State::Absent,
    State::Open,
    State::OpenChanged,
    State::ClosedDeleted,
    State::ClosedReplaced,
    State::LinkedRoot,
    State::LinkedPage,
];

/// What a cell expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    /// Succeeds; then the op's history lists this many versions (`None`:
    /// not checked, it cannot be read).
    Ok(Option<usize>),
    FileNotFound,
    VersionNotFound,
    /// Refused as a linked history folder.
    Refused,
}

use Want::{FileNotFound as Fnf, Ok as Y, Refused as R, VersionNotFound as Vnf};

struct Case {
    store: Store,
    project: String,
    dir: PathBuf,
    elsewhere: PathBuf,
    /// The id of the page's first version, when it had one.
    old: Option<String>,
    _tmp: tempfile::TempDir,
}

fn unwrap<T, E: std::fmt::Display>(res: Result<T, E>) -> T {
    res.unwrap_or_else(|e| panic!("{e}"))
}

impl Case {
    fn new(state: State) -> Self {
        let tmp = unwrap(tempfile::tempdir());
        let store = unwrap(Store::open(DataDir::new(tmp.path())));
        let (project, _) = unwrap(store.create_project(CreateProjectRequest {
            name: "p".into(),
            ..CreateProjectRequest::default()
        }));
        let dir = unwrap(store.project_root(&project)).dir;
        let elsewhere = tmp.path().join("elsewhere");
        unwrap(fs::create_dir(&elsewhere));
        let mut case = Self {
            store,
            project: project.id,
            dir,
            elsewhere,
            old: None,
            _tmp: tmp,
        };
        case.setup(state);
        case
    }

    fn write(&self, name: &str, text: &str) {
        unwrap(
            self.store
                .write_file(&self.project, name, text.as_bytes(), true, None),
        );
    }

    fn shell(&self, name: &str, text: &str) {
        unwrap(fs::write(self.dir.join(name), text));
    }

    fn setup(&mut self, state: State) {
        self.write("style.css", "a{}");
        self.write("pages/p.html", "<p>p</p>");
        self.write("pages/n.txt", "n");
        let two = |case: &mut Self| {
            case.write(PAGE, "<p>one</p>");
            case.write(PAGE, "<p>two</p>");
            case.old = Some(
                unwrap(versions::list_versions(&case.dir, PAGE))[0]
                    .id
                    .clone(),
            );
        };
        match state {
            State::Absent => self.shell(PAGE, "<p>shell</p>"),
            State::Open => two(self),
            State::OpenChanged => {
                two(self);
                self.shell(PAGE, "<p>shell</p>");
            }
            State::ClosedDeleted => {
                two(self);
                unwrap(self.store.delete_file(&self.project, PAGE));
                self.shell(PAGE, "<p>shell</p>");
            }
            State::ClosedReplaced => {
                two(self);
                unwrap(self.store.rename_file(&self.project, PAGE, "page.md"));
                self.shell("page.md", "<p>shell</p>");
                unwrap(self.store.rename_file(&self.project, "page.md", PAGE));
            }
            State::LinkedRoot => {
                two(self);
                let root = self.dir.join(VERSION_ROOT);
                unwrap(fs::remove_dir_all(&root));
                unwrap(std::os::unix::fs::symlink(&self.elsewhere, &root));
            }
            State::LinkedPage => {
                two(self);
                let history = self.history_dir(PAGE);
                unwrap(fs::remove_dir_all(&history));
                unwrap(std::os::unix::fs::symlink(&self.elsewhere, &history));
            }
        }
    }

    fn history_dir(&self, name: &str) -> PathBuf {
        let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(name.as_bytes()));
        self.dir.join(VERSION_ROOT).join(&digest[..24])
    }

    /// The project's files and their bytes, histories left out.
    fn disk(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in unwrap(fs::read_dir(at)) {
                let path = unwrap(entry).path();
                if path.file_name().is_some_and(|n| n == VERSION_ROOT) {
                    continue;
                }
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    let rel = unwrap(path.strip_prefix(base))
                        .to_string_lossy()
                        .into_owned();
                    out.insert(rel, unwrap(fs::read(&path)));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.dir, &self.dir, &mut out);
        out
    }

    fn old_id(&self) -> String {
        self.old
            .clone()
            .unwrap_or_else(|| "0000-no-such-version".to_owned())
    }
}

type Run = fn(&Case) -> Result<(), StoreError>;

struct Op {
    name: &'static str,
    /// Done outside the app just before the op (not part of what an error
    /// must leave unchanged).
    before: Option<fn(&Case)>,
    run: Run,
    /// The history counted after a success.
    check: Option<&'static str>,
    /// Per state, in [`STATES`] order.
    want: [Want; 7],
}

fn ops() -> Vec<Op> {
    vec![
        Op {
            name: "write page",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.write_file(p, PAGE, b"<p>new</p>", true, None).map(drop)
            },
            check: Some(PAGE),
            want: [
                Y(Some(1)),
                Y(Some(3)),
                Y(Some(3)),
                Y(Some(1)),
                Y(Some(1)),
                R,
                R,
            ],
        },
        Op {
            name: "write new txt",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.write_file(p, "notes.txt", b"n", true, None).map(drop)
            },
            check: None,
            want: [Y(None); 7],
        },
        Op {
            name: "create page",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.create_file_exact(p, "other.html", b"<p>o</p>").map(drop)
            },
            check: Some("other.html"),
            want: [
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                R,
                Y(Some(1)),
            ],
        },
        Op {
            name: "create txt",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.create_file_exact(p, "c.txt", b"c").map(drop)
            },
            check: None,
            want: [Y(None); 7],
        },
        Op {
            name: "upload page",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.upload_file(p, None, "up.html", b"<p>u</p>").map(drop)
            },
            check: Some("up.html"),
            want: [
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                R,
                Y(Some(1)),
            ],
        },
        Op {
            name: "upload image",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.upload_file(p, None, "img.png", b"\x89PNG\r\n\x1a\n")
                    .map(drop)
            },
            check: None,
            want: [Y(None); 7],
        },
        Op {
            name: "replace page",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.replace_file(p, PAGE, b"<p>rep</p>", "Edit").map(drop)
            },
            check: Some(PAGE),
            want: [
                Y(Some(2)),
                Y(Some(3)),
                Y(Some(4)),
                Y(Some(2)),
                Y(Some(2)),
                R,
                R,
            ],
        },
        Op {
            name: "replace txt",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.replace_file(p, "style.css", b"b{}", "Edit").map(drop)
            },
            check: None,
            want: [Y(None); 7],
        },
        Op {
            name: "rename page",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.rename_file(p, PAGE, "moved.html").map(drop)
            },
            check: Some("moved.html"),
            want: [
                Y(Some(0)),
                Y(Some(2)),
                Y(Some(2)),
                Y(Some(0)),
                Y(Some(0)),
                R,
                R,
            ],
        },
        Op {
            name: "move folder",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.move_folder(p, "pages", "site/pages").map(drop)
            },
            check: Some("site/pages/p.html"),
            want: [
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                Y(Some(1)),
                R,
                Y(Some(1)),
            ],
        },
        Op {
            name: "delete page",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.delete_file(p, PAGE)
            },
            check: Some(PAGE),
            want: [
                Y(Some(0)),
                Y(Some(0)),
                Y(Some(0)),
                Y(Some(0)),
                Y(Some(0)),
                Y(None),
                Y(None),
            ],
        },
        Op {
            name: "delete page gone",
            before: Some(|c| unwrap(fs::remove_file(c.dir.join(PAGE)))),
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.delete_file(p, PAGE)
            },
            check: None,
            want: [Fnf; 7],
        },
        Op {
            name: "delete folder",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.delete_folder(p, "pages").map(drop)
            },
            check: Some("pages/p.html"),
            want: [
                Y(Some(0)),
                Y(Some(0)),
                Y(Some(0)),
                Y(Some(0)),
                Y(Some(0)),
                Y(None),
                Y(Some(0)),
            ],
        },
        Op {
            name: "list",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.list_versions(p, PAGE).map(drop)
            },
            check: Some(PAGE),
            want: [
                Y(Some(1)),
                Y(Some(2)),
                Y(Some(2)),
                Y(Some(1)),
                Y(Some(1)),
                R,
                R,
            ],
        },
        Op {
            name: "read old",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.read_version(p, PAGE, &c.old_id()).map(drop)
            },
            check: None,
            want: [Vnf, Y(None), Y(None), Vnf, Vnf, R, R],
        },
        Op {
            name: "restore old",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.restore_version(p, PAGE, &c.old_id(), None).map(drop)
            },
            check: Some(PAGE),
            want: [Vnf, Y(Some(3)), Y(Some(4)), Vnf, Vnf, R, R],
        },
        Op {
            name: "create version",
            before: None,
            run: |c| {
                let (st, p) = (&c.store, c.project.as_str());
                st.create_version(p, PAGE, CreateVersionRequest::default())
                    .map(drop)
            },
            check: Some(PAGE),
            want: [
                Y(Some(1)),
                Y(Some(3)),
                Y(Some(3)),
                Y(Some(1)),
                Y(Some(1)),
                R,
                R,
            ],
        },
    ]
}

fn got(res: &Result<(), StoreError>) -> Option<Want> {
    match res {
        Ok(()) => None,
        Err(StoreError::FileNotFound) => Some(Fnf),
        Err(StoreError::VersionNotFound) => Some(Vnf),
        Err(StoreError::BadRequest(msg)) if msg.contains(VERSION_ROOT) => Some(R),
        Err(_) => Some(Y(Some(usize::MAX))),
    }
}

#[test]
fn every_operation_in_every_history_state() {
    let mut failures = Vec::new();
    for op in ops() {
        for (state, want) in STATES.iter().zip(op.want) {
            let case = Case::new(*state);
            if let Some(before) = op.before {
                before(&case);
            }
            let disk = case.disk();
            let res = (op.run)(&case);
            let cell = format!("{} × {state:?}", op.name);
            let fail = match (want, got(&res)) {
                (Y(count), None) => count.zip(op.check).and_then(|(n, name)| {
                    let len = versions::list_versions(&case.dir, name).map(|l| l.len());
                    (len.as_ref().ok() != Some(&n)).then(|| format!("{len:?} versions, want {n}"))
                }),
                (_, Some(err)) if err == want => {
                    (case.disk() != disk).then(|| "refused, yet the disk changed".to_owned())
                }
                _ => Some(format!("{res:?}, want {want:?}")),
            };
            let leaked = unwrap(fs::read_dir(&case.elsewhere)).next().is_some();
            if let Some(fail) = fail {
                failures.push(format!("{cell}: {fail}"));
            }
            if leaked {
                failures.push(format!("{cell}: written through the link"));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A page changed outside the app since its last version, then restored:
/// what it held is kept in a version, not lost to the restore.
#[test]
fn a_restore_keeps_what_the_page_held() {
    let case = Case::new(State::OpenChanged);
    unwrap(
        case.store
            .restore_version(&case.project, PAGE, &case.old_id(), None),
    );
    let kept = unwrap(versions::list_versions(&case.dir, PAGE))
        .iter()
        .filter_map(|v| versions::read_version(&case.dir, PAGE, &v.id).ok())
        .any(|(_, text)| text == "<p>shell</p>");
    assert!(kept, "the change made outside the app is in no version");
    assert_eq!(
        unwrap(fs::read_to_string(case.dir.join(PAGE))),
        "<p>one</p>"
    );
}

/// A closed history is not the page's now: listed, it holds only the page
/// as it is (QA-102).
#[test]
fn a_closed_history_lists_only_the_page_now() {
    for state in [State::ClosedDeleted, State::ClosedReplaced] {
        let case = Case::new(state);
        let list = unwrap(case.store.list_versions(&case.project, PAGE)).versions;
        assert_eq!(list.len(), 1, "{state:?}");
        let (_, text) = unwrap(case.store.read_version(&case.project, PAGE, &list[0].id));
        assert_eq!(text, "<p>shell</p>", "{state:?}");
    }
}

/// The history cannot be written (its folder read-only) after the disk
/// changed: the write or delete is reported done, not failed, since the
/// file is written or gone already.
#[test]
fn a_history_that_cannot_update_does_not_fail_what_is_done() {
    use std::os::unix::fs::PermissionsExt;
    let case = Case::new(State::Open);
    let history = case.history_dir(PAGE);
    let mode = |m| unwrap(fs::set_permissions(&history, fs::Permissions::from_mode(m)));
    mode(0o555);
    let write = case
        .store
        .write_file(&case.project, PAGE, b"<p>three</p>", true, None);
    let delete = case.store.delete_file(&case.project, PAGE);
    mode(0o755);
    assert!(write.is_ok(), "write: {write:?}");
    assert!(delete.is_ok(), "delete: {delete:?}");
    assert!(!case.dir.join(PAGE).exists());
}
