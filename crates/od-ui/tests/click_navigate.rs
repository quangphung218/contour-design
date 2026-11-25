//! A page opened inside a click (or a key, or a pick) takes the screen
//! away while the same event still passes through its listeners, and wasm
//! throws `closure invoked recursively or after being dropped` (PR #332,
//! seen on CI only). A handler opens a page with
//! `leave_guard::use_open_after_click`, which leaves on the next task.
//!
//! This test finds every call of a `navigate` under `src` and checks each
//! against [`KEPT`]: a call that may stay is one that never runs inside an
//! event (after an `await`, in an effect), or one that leaves no listener
//! of the event behind. A new `navigate(…)` in a handler fails here, and
//! so does a listed site that went away. The scan only sees the name
//! `navigate`, so every `use_navigate()` and `use_leave()` is bound to it,
//! or listed in [`KEPT_ELSEWHERE`] with where its calls are.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `(file under src, enclosing fn, times, why it may navigate there)`.
type Kept = (&'static str, &'static str, usize, &'static str);

/// After an `await`: the call runs on a task of its own, not in a handler.
const AFTER_AWAIT: &str = "after an `await` (a request, a save, a picker): not inside the event";

const KEPT: &[Kept] = &[
    (
        "app/leave_guard.rs",
        "use_open_after_click",
        1,
        "the way a handler opens a page: on the next task, once the event is done",
    ),
    (
        "app/leave_guard.rs",
        "use_leave",
        1,
        "the leave guard itself; each caller names it `navigate`, so its calls are scanned here",
    ),
    (
        "app/leave_guard.rs",
        "LeaveGuard",
        1,
        "Leave in the leave question: the dialog is outside `<Routes>`, so the page it \
         leaves holds no listener of that click",
    ),
    (
        "app/palette.rs",
        "CommandPalette",
        3,
        "the palette is outside `<Routes>`: the page it leaves holds no listener of the \
         click or key, and the palette closes itself on the next task",
    ),
    (
        "app/studio.rs",
        "PreviewStopped",
        1,
        "Run again replaces the address of the same Studio (only the query changes): no \
         screen goes",
    ),
    (
        "app/mod.rs",
        "OnboardingGate",
        1,
        "an effect on the config and the path, not an event handler",
    ),
    ("app/ds_page.rs", "DsDetail", 1, AFTER_AWAIT),
    ("app/home.rs", "FromFolderForm", 1, AFTER_AWAIT),
    ("app/home.rs", "Home", 1, AFTER_AWAIT),
    ("app/home.rs", "HomeActions", 1, AFTER_AWAIT),
    ("app/media_page.rs", "LibraryView", 1, AFTER_AWAIT),
    ("app/new_project.rs", "NewProjectPanel", 1, AFTER_AWAIT),
    ("app/onboarding.rs", "Onboarding", 1, AFTER_AWAIT),
    ("app/plugins_page.rs", "CreateDialog", 1, AFTER_AWAIT),
    ("app/plugins_page.rs", "PluginDetail", 1, AFTER_AWAIT),
    ("app/projects.rs", "Projects", 2, AFTER_AWAIT),
    ("app/settings_page.rs", "About", 1, AFTER_AWAIT),
    ("app/templates_page.rs", "MyTemplates", 1, AFTER_AWAIT),
    ("app/templates_page.rs", "TemplatePreview", 1, AFTER_AWAIT),
];

/// `(file under src, enclosing fn, why its navigate is not named `navigate`)`.
const KEPT_ELSEWHERE: &[(&str, &str, &str)] = &[
    (
        "app/plugins_page.rs",
        "LibraryPage",
        "`go` wraps it (the router's navigate is not `Send`); called from `on_created` \
         and `on_imported`, which run after a create or an import answered",
    ),
    (
        "app/projects.rs",
        "Projects",
        "`nav` and `nav_zip` are clones for the folder and zip imports, called after \
         their request answered",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The name of a `fn` declared on `line`, if any.
fn declared_fn(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    let at = rest.find("fn ")?;
    let head = &rest[..at];
    // Only qualifiers before `fn`: `pub`, `pub(crate)`, `async`, `const`.
    if !head
        .split_whitespace()
        .all(|w| w == "pub" || w.starts_with("pub(") || w == "async" || w == "const")
    {
        return None;
    }
    let name: String = rest[at + 3..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then(|| &rest[at + 3..at + 3 + name.len()])
}

/// The code of `line`, without a `//` comment.
fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// How many times `line` calls `navigate(` or `navigate.clone()(` (not
/// `use_navigate(`, not a `fn navigate(`).
fn calls(line: &str) -> usize {
    let code = code(line);
    code.match_indices("navigate(")
        .filter(|(i, _)| {
            let before = &code[..*i];
            let joined = before
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
            !joined && !before.trim_end().ends_with("fn")
        })
        .count()
        + code.matches("navigate.clone()(").count()
}

/// Whether `line` takes a navigate (`use_navigate()`, `use_leave()`, or a
/// clone of one) without naming it `navigate`, or wraps it.
fn renamed(line: &str) -> bool {
    let code = code(line).trim();
    if code.contains("fn use_leave") {
        return false;
    }
    for hook in ["use_navigate();", "use_leave();", "navigate.clone();"] {
        if let Some(head) = code.strip_suffix(hook) {
            // `let navigate = <path::>hook`: the scan sees its calls.
            let path = head.strip_prefix("let navigate =").map(str::trim);
            return !path.is_some_and(|p| {
                p.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            });
        }
    }
    code.contains("use_navigate()") || code.contains("use_leave()")
}

/// `(file, fn) → calls`, and the `(file, fn)` sites that rename it.
type Scan = (BTreeMap<(String, String), usize>, Vec<(String, String)>);

fn scan(src: &Path) -> Scan {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    let mut found = BTreeMap::new();
    let mut renames = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut current = String::from("(top)");
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("use ") {
                continue;
            }
            if let Some(name) = declared_fn(line) {
                current = name.to_owned();
            }
            let n = calls(line);
            if n > 0 {
                *found.entry((rel.clone(), current.clone())).or_default() += n;
            }
            if renamed(line) {
                renames.push((rel.clone(), current.clone()));
            }
        }
    }
    (found, renames)
}

#[test]
fn a_handler_opens_a_page_on_the_next_task() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (found, renames) = scan(&src);
    let kept: BTreeMap<_, _> = KEPT
        .iter()
        .map(|(file, f, n, _)| (((*file).to_owned(), (*f).to_owned()), *n))
        .collect();
    let mut wrong = Vec::new();
    for (site, n) in &found {
        match kept.get(site) {
            Some(k) if k == n => {}
            Some(k) => wrong.push(format!("{site:?}: {n} calls, KEPT lists {k}")),
            None => wrong.push(format!(
                "{site:?}: {n} navigate call(s) not in KEPT; a handler opens a page \
                 with `use_open_after_click`"
            )),
        }
    }
    for site in kept.keys() {
        if !found.contains_key(site) {
            wrong.push(format!("{site:?}: in KEPT but gone; drop the entry"));
        }
    }
    let elsewhere: Vec<_> = KEPT_ELSEWHERE
        .iter()
        .map(|(file, f, _)| ((*file).to_owned(), (*f).to_owned()))
        .collect();
    for site in &renames {
        if !elsewhere.contains(site) {
            wrong.push(format!(
                "{site:?}: takes a navigate not named `navigate`, which this scan \
                 does not see; name it `navigate` or list it in KEPT_ELSEWHERE"
            ));
        }
    }
    for site in &elsewhere {
        if !renames.contains(site) {
            wrong.push(format!(
                "{site:?}: in KEPT_ELSEWHERE but gone; drop the entry"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_kept_site_says_why() {
    for (file, f, n, why) in KEPT {
        assert!(*n > 0 && why.len() > 20, "{file} {f}: say why it may stay");
    }
    for (file, f, why) in KEPT_ELSEWHERE {
        assert!(why.len() > 20, "{file} {f}: say why it may stay");
    }
}

#[test]
fn the_scan_sees_a_call_of_navigate_only() {
    assert_eq!(
        calls("on:click=move |_| navigate(&go, Default::default())>"),
        1
    );
    assert_eq!(calls("Ok(v) => navigate(&a), Err(_) => navigate(&b),"), 2);
    assert_eq!(calls("let navigate = use_navigate();"), 0);
    assert_eq!(calls("pub fn navigate(&self, path: &str) {"), 0);
    assert_eq!(calls("cx.navigate(path, options)"), 0);
    assert_eq!(calls("on:click=move |_| navigate.clone()(&path)>"), 1);
    assert_eq!(calls("open(path); // then navigate(path)"), 0);
    assert!(renamed("let go = use_leave();"));
    assert!(renamed("let nav = StoredValue::new_local(use_navigate());"));
    assert!(!renamed("let navigate = super::leave_guard::use_leave();"));
    assert!(!renamed("let navigate = use_navigate();"));
    assert!(!renamed("        let navigate = navigate.clone();"));
    assert!(renamed("    let nav_docs = navigate.clone();"));
    assert!(renamed(
        "let navigate = StoredValue::new_local(use_navigate());"
    ));
    assert!(!renamed("pub fn use_leave() -> impl Fn(&str) + Clone {"));
}
