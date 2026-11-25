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
//! or listed in [`KEPT_ELSEWHERE`] with where its calls are: so is a
//! `navigate` used as a value in any way (handed on, returned, given by a
//! closure) and a hook imported `as` another name or kept uncalled.

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
/// clone of one) without naming it `navigate`, wraps it, or hands it on.
/// `prev` is the code of the line before (rustfmt puts a long call's
/// arguments, or the value of a long `let`, on lines of their own).
fn renamed(prev: &str, line: &str, importing: bool) -> bool {
    let code = code(line).trim();
    if importing {
        return hook_as_value(code, true);
    }
    if code.contains("fn use_leave") {
        return false;
    }
    for hook in ["use_navigate();", "use_leave();", "navigate.clone();"] {
        if let Some(head) = code.strip_suffix(hook) {
            // `let x =` on the line before: the value moved down.
            let joined = format!("{} {}", prev.trim(), head.trim());
            let head = if prev.trim_end().ends_with('=') && !head.contains('=') {
                joined.as_str()
            } else {
                head
            };
            // `let navigate = <path::>hook`: the scan sees its calls.
            let path = head
                .strip_prefix("let navigate =")
                .or_else(|| head.strip_prefix("let mut navigate ="))
                .map(str::trim);
            return !path.is_some_and(|p| {
                p.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            });
        }
    }
    code.contains("use_navigate()")
        || code.contains("use_leave()")
        || hook_as_value(code, importing)
        || moved(prev, code)
}

/// Whether `code` takes `use_navigate` or `use_leave` without calling it:
/// imported under another name (`use_navigate as nav`, also inside a
/// `use` group over several lines) or kept as a value
/// (`let hook = use_navigate;`). `importing`: `code` is part of a `use`.
fn hook_as_value(code: &str, importing: bool) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    ["use_navigate", "use_leave"].iter().any(|hook| {
        code.match_indices(hook).any(|(i, word)| {
            let rest = &code[i + word.len()..];
            let after = rest.trim_start();
            if code[..i].chars().next_back().is_some_and(ident)
                || rest.starts_with(ident)
                || code[..i].trim_end().ends_with("fn")
                || after.starts_with('(')
                || after.starts_with("::")
            {
                return false; // another word, its declaration, a call, a path
            }
            after.starts_with("as ") || !importing
        })
    })
}

/// Whether `code` uses `navigate` (or `navigate.clone()`) as a value
/// rather than calling it: `let go = navigate;`, `open_now(navigate.clone(),
/// path)`, `Ctx { navigate }`, `move || navigate.clone()`, a fn's last
/// `navigate`, `return navigate`, `_ => navigate`. Whatever receives it
/// calls it under another name. Not a value: a call (and `fn navigate(`),
/// `let navigate = …` or `navigate = …`, a field `navigate: T`, and the
/// `navigate` of
/// `let navigate = navigate.clone();`, which keeps the name. At the start
/// of a line, what comes before it is the end of `prev`.
fn moved(prev: &str, code: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    code.match_indices("navigate").any(|(i, word)| {
        let own = code[..i].trim_end();
        let before = if own.is_empty() { prev.trim_end() } else { own };
        let after = &code[i + word.len()..];
        if code[..i]
            .chars()
            .next_back()
            .is_some_and(|c| ident(c) || c == '.')
            || after.starts_with(ident)
        {
            return false; // part of another word, or a field
        }
        let value = after.strip_prefix(".clone()").unwrap_or(after).trim_start();
        let field = after.trim_start().starts_with(':') && !after.trim_start().starts_with("::");
        let assigned = {
            let a = after.trim_start();
            a.starts_with('=') && !a.starts_with("==") && !a.starts_with("=>")
        };
        !(value.starts_with('(')
            || field
            || assigned
            || before.ends_with("let navigate =")
            || before.ends_with("let mut navigate ="))
    })
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
        let mut prev = String::new();
        // Inside a `use` (or `pub use`) until its `;`, over several lines.
        let mut importing = false;
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let opens = ["use ", "pub use ", "pub(crate) use ", "pub(super) use "];
            importing |= opens.iter().any(|u| trimmed.starts_with(u));
            if let Some(name) = declared_fn(line) {
                current = name.to_owned();
            }
            let n = if importing { 0 } else { calls(line) };
            if n > 0 {
                *found.entry((rel.clone(), current.clone())).or_default() += n;
            }
            if renamed(&prev, line, importing) {
                renames.push((rel.clone(), current.clone()));
            }
            if importing && code(line).contains(';') {
                importing = false;
            }
            if !code(line).trim().is_empty() {
                code(line).trim().clone_into(&mut prev);
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
    assert!(renamed("", "let go = use_leave();", false));
    assert!(renamed(
        "",
        "let nav = StoredValue::new_local(use_navigate());",
        false
    ));
    assert!(!renamed(
        "",
        "let navigate = super::leave_guard::use_leave();",
        false
    ));
    assert!(!renamed("", "let navigate = use_navigate();", false));
    assert!(!renamed(
        "",
        "        let navigate = navigate.clone();",
        false
    ));
    assert!(renamed("", "    let nav_docs = navigate.clone();", false));
    assert!(renamed(
        "",
        "let navigate = StoredValue::new_local(use_navigate());",
        false
    ));
    assert!(!renamed(
        "",
        "pub fn use_leave() -> impl Fn(&str) + Clone {",
        false
    ));
    assert!(renamed("", "    let go = navigate;", false));
    assert!(renamed(
        "",
        "on:click=move |_| open_now(navigate.clone(), path.clone())>",
        false
    ));
    assert!(renamed("", "let kept = StoredValue::new(navigate);", false));
    assert!(renamed(
        "",
        "let (navigate, path) = (navigate.clone(), path.to_owned());",
        false
    ));
    assert!(!renamed("", "let navigate = navigate;", false));
    assert!(!renamed(
        "",
        "let open_page = super::leave_guard::use_open_after_click();",
        false
    ));
    assert!(!renamed(
        "",
        "Ok(v) => navigate(&path, Default::default()),",
        false
    ));
    // rustfmt puts long arguments and values on lines of their own.
    assert!(renamed("open_project_now(", "    navigate.clone(),", false));
    assert!(renamed("let go =", "    navigate;", false));
    assert!(renamed("let ctx = Ctx {", "    navigate,", false));
    assert!(renamed("let nav =", "    use_navigate();", false));
    assert!(!renamed("let navigate =", "    navigate.clone();", false));
    assert!(!renamed(
        "let navigate =",
        "    super::leave_guard::use_leave();",
        false
    ));
    assert!(!renamed(
        "move |_| {",
        "    navigate(&path, Default::default());",
        false
    ));
    // A value however it is written: a fn's last expression, `return`, an
    // arm, a closure that gives it, a reference, a field, an alias.
    assert!(renamed(
        "let navigate = use_navigate();",
        "    navigate",
        false
    ));
    assert!(renamed("", "    return navigate;", false));
    assert!(renamed("", "        _ => navigate,", false));
    assert!(renamed("", "let make = move || navigate.clone();", false));
    assert!(renamed("", "let go = Clone::clone(&navigate);", false));
    assert!(renamed("", "    navigate: navigate.clone(),", false));
    assert!(renamed(
        "",
        "let hook = leptos_router::hooks::use_navigate;",
        false
    ));
    assert!(renamed(
        "",
        "let go = leptos_router::hooks::use_navigate();",
        false
    ));
    assert!(renamed(
        "",
        "use leptos_router::hooks::use_navigate as nav;",
        true
    ));
    assert!(renamed(
        "use leptos_router::hooks::{",
        "    use_location, use_navigate as nav,",
        true
    ));
    assert!(!renamed(
        "",
        "use leptos_router::hooks::{use_location, use_navigate};",
        true
    ));
    assert!(!renamed(
        "use leptos_router::hooks::{",
        "    use_navigate,",
        true
    ));
    assert!(!renamed("", "    navigate: F,", false));
    assert!(!renamed("", "let mut navigate = use_navigate();", false));
    assert!(!renamed("", "pub fn navigate(&self, path: &str) {", false));
    assert!(!renamed(
        "",
        "on:click=move |_| navigate.clone()(&path)>",
        false
    ));
}
