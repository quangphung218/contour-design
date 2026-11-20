//! A task started straight with `tokio::spawn` or `spawn_blocking` runs
//! outside the request's span, so its log lines lose the request id
//! (`crates/od-server/CLAUDE.md`, invariant 10). Code under `src/` goes
//! through `od_server::spawn`; the few spawns that belong to no request
//! are listed here with the reason.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Calls that start a task without the caller's span.
const DIRECT: &[&str] = &[
    "tokio::spawn(",
    "tokio::task::spawn(",
    "spawn_blocking(",
    "spawn_local(",
];

/// The helper module itself (relative to `src/`).
const HELPER: &str = "spawn.rs";

/// Direct spawns that belong to no request: file (relative to `src/`),
/// how many, and why.
const ALLOWED: &[(&str, usize, &str)] = &[
    (
        "lib.rs",
        3,
        "daemon start-up (`open_runs`, `serve`): runs before any request",
    ),
    (
        "routes/manage/updates.rs",
        1,
        "unit test's stub releases server, not a route",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Direct spawn calls per file (relative to `src/`, `/`-separated),
/// with the lines, comments left out.
fn direct_spawns(src: &Path) -> BTreeMap<String, Vec<String>> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    assert!(files.len() > 20, "no sources under {}", src.display());
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let rel = file
            .strip_prefix(src)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if rel == HELPER {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if DIRECT.iter().any(|d| code.contains(d)) {
                found.entry(rel.clone()).or_default().push(format!(
                    "{rel}:{}: {}",
                    n + 1,
                    line.trim()
                ));
            }
        }
    }
    found
}

#[test]
fn every_task_spawned_in_src_keeps_the_callers_span() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(src.join(HELPER).is_file(), "the helper module is missing");
    let mut found = direct_spawns(&src);
    let mut wrong = Vec::new();
    for (file, count, why) in ALLOWED {
        let lines = found.remove(*file).unwrap_or_default();
        if lines.len() != *count {
            wrong.push(format!(
                "{file}: {count} direct spawn(s) allowed ({why}), found {}: {lines:#?}",
                lines.len()
            ));
        }
    }
    wrong.extend(found.into_values().flatten());
    assert!(
        wrong.is_empty(),
        "use od_server::spawn::{{spawn_in_span, spawn_blocking_in_span}}, \
         or list the spawn in ALLOWED with why it belongs to no request:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn the_scan_sees_a_direct_spawn_and_skips_comments() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path();
    for i in 0..21 {
        std::fs::write(src.join(format!("f{i}.rs")), "fn f() {}\n").unwrap();
    }
    std::fs::create_dir(src.join("routes")).unwrap();
    std::fs::write(
        src.join("routes/a.rs"),
        "// tokio::spawn(x)\nlet h = tokio::task::spawn_blocking(move || 1);\n",
    )
    .unwrap();
    std::fs::write(src.join(HELPER), "tokio::spawn(fut)\n").unwrap();
    let found = direct_spawns(src);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found["routes/a.rs"].len(), 1);
    assert!(found["routes/a.rs"][0].starts_with("routes/a.rs:2:"));
}
