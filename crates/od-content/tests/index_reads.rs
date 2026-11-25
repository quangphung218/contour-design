//! The index reads folders an agent can write (`<data>/skills`,
//! `<data>/plugins`, craft): a FIFO named `SKILL.md` must not keep it
//! waiting, a file past the limit is not read, a link at a file's name is
//! not followed. An entry that is itself a link (the user's own symlink)
//! is still listed.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::craft::{list_craft, load_craft_sections};
/// `od_content::nofollow::MAX_INDEX_FILE_BYTES`, written out so the test builds on code without it.
const MAX_INDEX_FILE_BYTES: u64 = 4 * 1024 * 1024;
use od_content::plugin_validate::validate_plugin_dir;
use od_content::plugins::list_plugins;
use od_content::skills::list_skills;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Runs `f` on its own thread; panics when it has not come back in 10 s
/// (a read blocked on a FIFO).
fn returns<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("{what} never came back (blocked on a FIFO?)"))
}

fn fifo(path: &Path) {
    nix::unistd::mkfifo(path, nix::sys::stat::Mode::from_bits_truncate(0o644)).unwrap();
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: A test skill.\n---\n\n# {name}\n")
}

/// `root` with a folder per case, each meant to hold `file`:
/// `fine` (a regular file from `text`), `fifo`, `big` (past the limit),
/// `link` (a link to a file outside holding `text`), and `linked` (the
/// folder itself a link to one outside holding a regular file).
fn cases(root: &Path, file: &str, text: impl Fn(&str) -> String) -> PathBuf {
    let outside = root.join("outside");
    std::fs::create_dir_all(outside.join("linked")).unwrap();
    std::fs::write(outside.join("linked").join(file), text("linked")).unwrap();
    std::fs::write(outside.join("secret"), text("link")).unwrap();
    let entries = root.join("entries");
    for name in ["fine", "fifo", "big", "link"] {
        std::fs::create_dir_all(entries.join(name)).unwrap();
    }
    std::fs::write(entries.join("fine").join(file), text("fine")).unwrap();
    fifo(&entries.join("fifo").join(file));
    let mut big = text("big");
    big.push_str(&" ".repeat(usize::try_from(MAX_INDEX_FILE_BYTES).unwrap()));
    std::fs::write(entries.join("big").join(file), big).unwrap();
    std::os::unix::fs::symlink(outside.join("secret"), entries.join("link").join(file)).unwrap();
    std::os::unix::fs::symlink(outside.join("linked"), entries.join("linked")).unwrap();
    entries
}

#[test]
fn the_skill_index_skips_a_fifo_a_large_file_and_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let root = cases(tmp.path(), "SKILL.md", skill_md);
    let mut ids: Vec<String> = returns("list_skills", move || {
        list_skills(&[root], true)
            .into_iter()
            .map(|s| s.info.id)
            .collect()
    });
    ids.sort();
    assert_eq!(ids, ["fine", "linked"]);
}

#[test]
fn the_plugin_index_skips_a_fifo_a_large_file_and_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    std::fs::create_dir_all(&bundled).unwrap();
    for file in ["SKILL.md", "open-design.json"] {
        let case = tmp.path().join(file);
        std::fs::create_dir_all(&case).unwrap();
        let user = cases(&case, file, |n| {
            if file == "SKILL.md" {
                skill_md(n)
            } else {
                format!(r#"{{"name": "{n}", "title": "{n}", "version": "1.0.0"}}"#)
            }
        });
        let (b, u) = (bundled.clone(), user.clone());
        let mut ids: Vec<String> = returns("list_plugins", move || {
            list_plugins(&b, Some(&u))
                .into_iter()
                .map(|p| p.id)
                .collect()
        });
        ids.sort();
        assert_eq!(ids, ["fine", "linked"], "{file}");
        let checked = returns("validate_plugin_dir", move || {
            validate_plugin_dir(&user.join("fifo"))
        });
        assert!(!checked.errors.is_empty(), "{file}: {checked:?}");
    }
}

#[test]
fn craft_skips_a_fifo_a_large_file_and_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("craft");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("fine.md"), "# Fine\n\nrule").unwrap();
    fifo(&dir.join("fifo.md"));
    std::fs::write(
        dir.join("big.md"),
        "x".repeat(usize::try_from(MAX_INDEX_FILE_BYTES).unwrap() + 1),
    )
    .unwrap();
    std::fs::write(tmp.path().join("secret.md"), "# Secret\n\ns3cret").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("secret.md"), dir.join("link.md")).unwrap();
    let d = dir.clone();
    let listed: Vec<String> = returns("list_craft", move || {
        list_craft(&d).into_iter().map(|c| c.id).collect()
    });
    assert_eq!(listed, ["fine"]);
    let requested: Vec<String> = ["fine", "fifo", "big", "link"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let (body, slugs) = returns("load_craft_sections", move || {
        load_craft_sections(&dir, &requested)
    });
    assert_eq!(slugs, ["fine"]);
    assert!(!body.contains("s3cret"), "{body}");
}
