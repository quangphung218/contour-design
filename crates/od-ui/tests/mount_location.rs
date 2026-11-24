//! A page reads its address from the router (`use_location`,
//! `use_query_map`, `lib_ui::query_param`), never from `window.location`.
//! On a move inside the app the router builds the new page before it
//! pushes the new address, so the window still has the last page's query
//! and hash: `/projects/A?preview=stopped` to project B showed B's
//! preview stopped, `/plugins?skill=x` to the Plugins opened a skill.
//!
//! This test scans every `.rs` under `src` for `.location()` (whitespace
//! ignored, so a call split over lines counts): any way to the window's
//! `Location`, whether the read follows in the same chain or goes through
//! a variable (`let l = w.location(); l.search()`). The router's
//! `use_location()` has no dot before `location` and does not count.
//! Each file found is listed in [`KEPT`] with how many and why it may use
//! the window's `Location`.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::Path;

/// The call looked for, whitespace removed.
const CALL: &str = ".location()";

/// `(file under src, times, why)`.
const KEPT: &[(&str, usize, &str)] = &[
    (
        "app/desktop.rs",
        1,
        "`href` as the base to resolve a link the user opens, on click, after the address is pushed",
    ),
    (
        "app/export_extra.rs",
        1,
        "`set_href`: goes to a download, reads nothing",
    ),
    (
        "app/home.rs",
        1,
        "`set_href`: goes to Settings, reads nothing",
    ),
    (
        "app/integrations.rs",
        2,
        "`reload` reads nothing; `origin` is the same for every page of the app",
    ),
    (
        "app/leave_guard.rs",
        2,
        "read in the popstate handler (the address the Back went to) and on a link click, both after the page was built",
    ),
    (
        "app/mod.rs",
        2,
        "`assign` and `reload`: full loads, read nothing",
    ),
    ("app/palette.rs", 1, "`assign`: a full load, reads nothing"),
    (
        "app/project_actions.rs",
        1,
        "`set_href`: a full load to the copy, reads nothing",
    ),
    (
        "app/settings_page.rs",
        1,
        "`hash` read in the scroll handler, long after the page was built and the address pushed",
    ),
    (
        "app/terminal.rs",
        1,
        "`protocol` and `host`, the same for every page, when a terminal connects",
    ),
];

fn scan(root: &Path, dir: &Path, found: &mut BTreeMap<String, usize>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(root, &path, found);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text: String = std::fs::read_to_string(&path)
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let n = text.matches(CALL).count();
        if n > 0 {
            found.insert(rel, n);
        }
    }
}

#[test]
fn a_page_reads_its_address_from_the_router() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeMap::new();
    scan(&src, &src, &mut found);
    let kept: BTreeMap<String, usize> = KEPT
        .iter()
        .map(|(file, n, _)| ((*file).to_owned(), *n))
        .collect();
    let mut problems = Vec::new();
    for (file, n) in &found {
        match kept.get(file) {
            None => problems.push(format!(
                "{file}: {n} × `{CALL}`: read the router's address (`use_location`, `query_param`), or list it in KEPT with why"
            )),
            Some(k) if k != n => problems.push(format!("{file}: {n} × `{CALL}`, KEPT says {k}")),
            Some(_) => {}
        }
    }
    for file in kept.keys().filter(|k| !found.contains_key(*k)) {
        problems.push(format!(
            "{file}: listed in KEPT for `{CALL}` but has none now; drop it"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The scan counts a `Location` kept in a variable, which the old
/// `location().search(` match missed, and not the router's.
#[test]
fn the_scan_counts_a_location_read_through_a_variable() {
    let n = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .matches(CALL)
            .count()
    };
    assert_eq!(n("let l = w.location();\nl.search()"), 1);
    assert_eq!(
        n("web_sys::window()\n    .map(|w| w\n        .location())"),
        1
    );
    assert_eq!(n("let loc = use_location();"), 0);
}
