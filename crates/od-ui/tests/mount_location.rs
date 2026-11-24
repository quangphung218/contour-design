//! A page reads its address from the router (`use_location`,
//! `use_query_map`, `lib_ui::query_param`), never from `window.location`.
//! On a move inside the app the router builds the new page before it
//! pushes the new address, so the window still has the last page's query
//! and hash: `/projects/A?preview=stopped` to project B showed B's
//! preview stopped, `/plugins?skill=x` to the Plugins opened a skill.
//!
//! This test scans every `.rs` under `src` for a read of
//! `location().search(` or `location().hash(` (whitespace ignored, so a
//! call split over lines counts). Each one found is listed in [`KEPT`]
//! with why it reads the window and not the router.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::Path;

/// The reads looked for, whitespace removed.
const READS: &[&str] = &["location().search(", "location().hash("];

/// `(file under src, read, times, why)`.
const KEPT: &[(&str, &str, usize, &str)] = &[(
    "app/settings_page.rs",
    "location().hash(",
    1,
    "read in the scroll handler, long after the page was built and the address pushed",
)];

fn scan(root: &Path, dir: &Path, found: &mut BTreeMap<(String, &'static str), usize>) {
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
        for read in READS {
            let n = text.matches(read).count();
            if n > 0 {
                *found.entry((rel.clone(), read)).or_default() += n;
            }
        }
    }
}

#[test]
fn a_page_reads_its_address_from_the_router() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeMap::new();
    scan(&src, &src, &mut found);
    let kept: BTreeMap<(String, &str), usize> = KEPT
        .iter()
        .map(|(file, read, n, _)| (((*file).to_owned(), *read), *n))
        .collect();
    let mut problems = Vec::new();
    for ((file, read), n) in &found {
        match kept.get(&(file.clone(), *read)) {
            None => problems.push(format!(
                "{file}: {n} × `{read}`: read the router's address (`use_location`, `query_param`), or list it in KEPT with why"
            )),
            Some(k) if k != n => problems.push(format!("{file}: {n} × `{read}`, KEPT says {k}")),
            Some(_) => {}
        }
    }
    for (file, read) in kept.keys().filter(|k| !found.contains_key(*k)) {
        problems.push(format!(
            "{file}: listed in KEPT for `{read}` but has none now; drop it"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
