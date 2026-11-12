//! A failed call shown to the user says what failed (QA-088): the text
//! goes through `api::failure` / `failure_text` / `StudioCtx::failed_to`
//! with a template such as "Could not delete: {0}", never bare
//! `e.to_string()` ("Contour could not finish this. Try again.") and never
//! `tf(key, &[&e.to_string()])`, which keeps the cause of an internal
//! failure inside the sentence.
//!
//! This scans `src/` for a line that puts `e.to_string()` into a signal,
//! a `say`/`fail` helper or a `…Failed(` state. A line that needs it (a
//! list load under its own heading) goes in [`KEPT`] with its reason.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::path::{Path, PathBuf};

/// `(file, trimmed line)` allowed to show `e.to_string()` as it is.
const KEPT: &[(&str, &str)] = &[
    // List loads: the page has no rows to act on, the banner is the page.
    (
        "app/plugins_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
    ),
    (
        "app/templates_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
    ),
    (
        "app/media_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
    ),
    (
        "app/projects.rs",
        "Err(e) => self.error.set(Some(e.to_string())),",
    ),
    (
        "app/ds_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
    ),
    (
        "app/automations.rs",
        "Err(e) => error.set(Some(e.to_string())),",
    ),
    // A skill's detail pane, read only: its error is the pane.
    (
        "app/plugins_page.rs",
        "detail.set(Some(res.map_err(|e| e.to_string())));",
    ),
    // A JSON decode error (not an `HttpError`) becoming one.
    (
        "app/api.rs",
        "message: tf(Msg::UnexpectedResponse, &[&e.to_string()]),",
    ),
    // Under the "Could not create the design system" heading.
    (
        "app/project_actions.rs",
        "Err(e) => return fail(e.to_string()),",
    ),
    // Under the "This project could not be opened." heading.
    ("app/studio.rs", "self.load_error.set(Some(e.to_string()));"),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// The line shows a call's bare error to the user.
fn shows_bare_error(line: &str) -> bool {
    if line.contains("&[&e.to_string()]") {
        return true;
    }
    line.contains("e.to_string()")
        && [".set(", "say(", "fail(", "Failed("]
            .iter()
            .any(|sink| line.contains(sink))
}

#[test]
fn a_failure_shown_to_the_user_says_what_failed() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 50, "scanned only {} files", files.len());
    let mut found = Vec::new();
    let mut used = vec![false; KEPT.len()];
    for path in files {
        let rel = path
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if !shows_bare_error(line) {
                continue;
            }
            match KEPT.iter().position(|&(f, l)| f == rel && l == line) {
                Some(i) => used[i] = true,
                None => found.push(format!("{rel}:{}: {line}", n + 1)),
            }
        }
    }
    assert!(
        found.is_empty(),
        "say what failed with api::failure(T::…, &e) (QA-088):\n{}",
        found.join("\n")
    );
    let stale: Vec<_> = KEPT
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(k, _)| format!("{}: {}", k.0, k.1))
        .collect();
    assert!(
        stale.is_empty(),
        "KEPT entries no longer in src:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_scan_catches_the_qa088_shapes() {
    assert!(shows_bare_error("ctx.notice.set(Some(e.to_string()));"));
    assert!(shows_bare_error("Err(e) => say(Err(e.to_string())),"));
    assert!(shows_bare_error("phase.set(Phase::Failed(e.to_string()));"));
    assert!(shows_bare_error(
        "Err(e) => self.notice.set(Some(tf(T::StopFailed, &[&e.to_string()]))),"
    ));
    assert!(!shows_bare_error(
        "Err(e) => notice.set(Some(api::failure(T::DeleteFailed, &e))),"
    ));
    assert!(!shows_bare_error(
        "content: serde_json::to_string(sketch).map_err(|e| e.to_string())?,"
    ));
}
