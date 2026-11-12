//! A failed call shown to the user says what failed (QA-088): the text
//! goes through `api::failure` / `failure_text` / `StudioCtx::failed_to`
//! with a template such as "Could not delete: {0}", never bare
//! `e.to_string()` ("Contour could not finish this. Try again.") and never
//! `tf(key, &[&e.to_string()])`, which keeps the cause of an internal
//! failure inside the sentence.
//!
//! Every `e.to_string()` under `src/app` is listed in [`KEPT`] with how
//! many times its line occurs and why it may stay. A new one, one more of
//! a listed line, or a listed line no longer there fails the test: each
//! new site is a decision, not a sink the scan forgot.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `(file under src/app, trimmed line, times, why)`.
const KEPT: &[(&str, &str, usize, &str)] = &[
    // List loads: the page has no rows to act on, the banner is the page.
    (
        "plugins_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
        2,
        "plugin list loads",
    ),
    (
        "templates_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
        1,
        "template list load",
    ),
    (
        "media_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
        1,
        "media list load",
    ),
    (
        "projects.rs",
        "Err(e) => self.error.set(Some(e.to_string())),",
        1,
        "project list load",
    ),
    (
        "ds_page.rs",
        "Err(e) => error.set(Some(e.to_string())),",
        1,
        "design system list load",
    ),
    (
        "automations.rs",
        "Err(e) => error.set(Some(e.to_string())),",
        1,
        "automation list load",
    ),
    // Read-only panes and loads: the error stands in for the content.
    (
        "plugins_page.rs",
        "res.map(|v| v[\"plugin\"].clone()).map_err(|e| e.to_string()),",
        1,
        "a plugin's page: the error is the page",
    ),
    (
        "plugins_page.rs",
        "detail.set(Some(res.map_err(|e| e.to_string())));",
        1,
        "a skill's detail pane, read only",
    ),
    (
        "settings_page.rs",
        ".map_err(|e| e.to_string())",
        1,
        "Your usage panel load",
    ),
    (
        "settings_page.rs",
        ".map_err(|e| e.to_string());",
        1,
        "a license text, read only",
    ),
    (
        "studio.rs",
        "Some(api::get_text(&raw).await.map_err(|e| e.to_string()))",
        1,
        "a file's source view load",
    ),
    (
        "live_view.rs",
        "async move { api::get::<Value>(&url).await.map_err(|e| e.to_string()) }",
        2,
        "live artifact detail and refresh history loads",
    ),
    (
        "live_view.rs",
        "async move { api::get_text(&raw).await.map_err(|e| e.to_string()) }",
        1,
        "live artifact template load",
    ),
    (
        "mod.rs",
        ".map_err(|e| e.to_string());",
        2,
        "agent list load and rescan, shown as the list",
    ),
    (
        "ds_page.rs",
        ".map_err(|e| e.to_string());",
        1,
        "design system kit load",
    ),
    // Under their own heading, which says what failed.
    (
        "project_actions.rs",
        "Err(e) => return fail(e.to_string()),",
        1,
        "under \"Could not create the design system\"",
    ),
    (
        "project_actions.rs",
        "Err((e, false)) => fail(e.to_string()),",
        1,
        "under \"Could not create the design system\"",
    ),
    (
        "studio.rs",
        "self.load.set(Load::Failed(e.to_string()));",
        1,
        "under \"This project could not be opened.\"",
    ),
    // Not an `HttpError`: no internal cause to hide.
    (
        "lib_ui.rs",
        ".map_err(|e| e.to_string())?",
        1,
        "gloo request build error",
    ),
    (
        "lib_ui.rs",
        ".map_err(|e| e.to_string())?;",
        1,
        "gloo transport error",
    ),
    (
        "lib_ui.rs",
        "res.json::<T>().await.map_err(|e| e.to_string())",
        1,
        "gloo JSON decode error",
    ),
    (
        "sketch.rs",
        "content: serde_json::to_string(sketch).map_err(|e| e.to_string())?,",
        1,
        "serde encode of an in-memory sketch",
    ),
    (
        "sketch.rs",
        "serde_json::from_str(text).map_err(|e| e.to_string())",
        1,
        "serde decode, shown under SketchReadFailed",
    ),
    (
        "api.rs",
        "message: tf(Msg::UnexpectedResponse, &[&e.to_string()]),",
        1,
        "JSON decode error becoming an HttpError",
    ),
    // The helpers that build the sentence.
    (
        "api.rs",
        "template.replace(\"{0}\", &e.to_string())",
        1,
        "failure_text fills the template",
    ),
    (
        "studio.rs",
        "e.to_string()",
        1,
        "StudioCtx::failed, for loads under their own heading",
    ),
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

/// How many times `line` turns an error named `e` into text as it is
/// (`name.to_string()` is not one).
fn bare_errors(line: &str) -> usize {
    const NEEDLE: &str = "e.to_string()";
    line.match_indices(NEEDLE)
        .filter(|&(at, _)| {
            !line[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .count()
}

/// What in `sources` (`(file, text)`) does not match `kept`, one line per
/// problem; empty when every bare error is listed with its exact count.
fn unlisted(sources: &[(String, String)], kept: &[(&str, &str, usize, &str)]) -> Vec<String> {
    let mut found: BTreeMap<(&str, &str), (usize, usize)> = BTreeMap::new();
    for (file, text) in sources {
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            let times = bare_errors(line);
            if times > 0 {
                let seen = found.entry((file, line)).or_insert((0, n + 1));
                seen.0 += times;
            }
        }
    }
    let mut problems = Vec::new();
    let mut listed: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for &(file, line, times, _) in kept {
        *listed.entry((file, line)).or_default() += times;
    }
    for (&(file, line), &(times, first)) in &found {
        match listed.get(&(file, line)) {
            None => problems.push(format!("{file}:{first}: {line} (not in KEPT)")),
            Some(&allowed) if times > allowed => problems.push(format!(
                "{file}:{first}: {line} ({times} times, KEPT allows {allowed})"
            )),
            Some(_) => {}
        }
    }
    for (&(file, line), &allowed) in &listed {
        let times = found.get(&(file, line)).map_or(0, |f| f.0);
        if times < allowed {
            problems.push(format!(
                "{file}: {line} (KEPT says {allowed}, src has {times}: update KEPT)"
            ));
        }
    }
    problems
}

#[test]
fn a_failure_shown_to_the_user_says_what_failed() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 50, "scanned only {} files", files.len());
    let sources: Vec<(String, String)> = files
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(path).unwrap())
        })
        .collect();
    let problems = unlisted(&sources, KEPT);
    assert!(
        problems.is_empty(),
        "say what failed with api::failure(T::…, &e) or ctx.failed_to (QA-088), \
         or list the line in KEPT with its reason:\n{}",
        problems.join("\n")
    );
}

#[test]
fn bare_errors_counts_an_error_named_e_only() {
    assert_eq!(bare_errors("Err(e) => Err(e.to_string()),"), 1);
    assert_eq!(bare_errors(".map_err(|e| e.to_string())?"), 1);
    assert_eq!(bare_errors("(e.to_string(), e.to_string())"), 2);
    assert_eq!(
        bare_errors("Err(e) => notice.set(Some(tf(T::StopFailed, &[&e.to_string()]))),"),
        1
    );
    assert_eq!(bare_errors("e.to_string()"), 1);
    assert_eq!(bare_errors("let n = name.to_string();"), 0);
    assert_eq!(bare_errors("frame.to_string()"), 0);
    assert_eq!(bare_errors("api::failure(T::DeleteFailed, &e)"), 0);
}

fn source(file: &str, lines: &[&str]) -> (String, String) {
    (file.to_owned(), lines.join("\n"))
}

#[test]
fn the_scan_catches_a_new_bare_error() {
    let kept = [(
        "a.rs",
        "Err(e) => error.set(Some(e.to_string())),",
        1,
        "load",
    )];
    let listed = source("a.rs", &["    Err(e) => error.set(Some(e.to_string())),"]);
    assert!(unlisted(std::slice::from_ref(&listed), &kept).is_empty());

    // Turning a fixed site back into `Err(e.to_string())` is caught.
    let new = source(
        "chat_ops.rs",
        &["fn f() {", "    Err(e) => Err(e.to_string()),", "}"],
    );
    let problems = unlisted(&[listed.clone(), new], &kept);
    assert_eq!(
        problems,
        ["chat_ops.rs:2: Err(e) => Err(e.to_string()), (not in KEPT)"]
    );

    // The same line once more than listed is caught.
    let twice = source(
        "a.rs",
        &[
            "Err(e) => error.set(Some(e.to_string())),",
            "Err(e) => error.set(Some(e.to_string())),",
        ],
    );
    assert_eq!(unlisted(&[twice], &kept).len(), 1);

    // A listed line gone from the code is caught.
    let gone = source(
        "a.rs",
        &["Err(e) => error.set(Some(api::failure(T::X, &e))),"],
    );
    let problems = unlisted(&[gone], &kept);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("update KEPT"), "{problems:?}");
}
