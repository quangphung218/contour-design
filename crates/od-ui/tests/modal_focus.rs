//! A modal dialog keeps the focus while it is open and gives it back when
//! it closes: `lib_ui::Modal` makes the page behind it `inert`, sends Tab
//! round its own controls, and returns the focus to the opener (e2e
//! `library.e2e.mjs` "dialog focus"). A dialog built by hand with
//! `aria-modal="true"` gets none of that: Tab walks out to the page behind
//! the backdrop, where Esc no longer closes it.
//!
//! Every `aria-modal="true"` under `src/app` outside `lib_ui.rs` is listed
//! in [`KEPT`] with how many times its file has one and why it may stay. A
//! new one, one more in a listed file, or a listed one gone fails the test:
//! a new modal uses `Modal`, or is a decision written here.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::Path;

/// `(file under src/app, times, why)`.
const KEPT: &[(&str, usize, &str)] = &[
    (
        "chat_ops.rs",
        2,
        "Studio: delete conversation and link folder dialogs; not moved to Modal yet (studio area)",
    ),
    (
        "composer.rs",
        1,
        "image preview over the composer; not moved to Modal yet (composer area)",
    ),
    (
        "export_extra.rs",
        2,
        "save as template and handoff dialogs; not moved to Modal yet",
    ),
    (
        "files_panel.rs",
        1,
        "rename, move and delete file dialog; not moved to Modal yet",
    ),
    (
        "new_project.rs",
        1,
        "New project panel; not moved to Modal yet",
    ),
    (
        "palette.rs",
        1,
        "command palette: a combobox whose list takes arrow keys, Tab is not its way round",
    ),
    (
        "projects.rs",
        1,
        "project instructions dialog; not moved to Modal yet",
    ),
    (
        "shortcuts.rs",
        1,
        "keyboard shortcuts sheet: no controls but its close button",
    ),
];

fn scan(dir: &Path, found: &mut BTreeMap<String, usize>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, found);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name == "lib_ui.rs" {
            continue;
        }
        let n = std::fs::read_to_string(&path)
            .unwrap()
            .matches("aria-modal=\"true\"")
            .count();
        if n > 0 {
            *found.entry(name).or_default() += n;
        }
    }
}

#[test]
fn every_modal_dialog_is_the_shared_one_or_listed() {
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
    let mut found = BTreeMap::new();
    scan(&app, &mut found);
    let kept: BTreeMap<String, usize> = KEPT
        .iter()
        .map(|(file, n, _)| ((*file).to_owned(), *n))
        .collect();
    let mut problems = Vec::new();
    for (file, n) in &found {
        match kept.get(file) {
            None => problems.push(format!(
                "{file}: {n} aria-modal dialog(s) built by hand; use lib_ui::Modal (focus stays in it) or list it in KEPT with why"
            )),
            Some(k) if k != n => problems.push(format!("{file}: {n} aria-modal, KEPT says {k}")),
            Some(_) => {}
        }
    }
    for file in kept.keys().filter(|f| !found.contains_key(*f)) {
        problems.push(format!(
            "{file}: listed in KEPT but has no aria-modal now; drop it"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_shared_modal_keeps_the_focus() {
    // The three parts the e2e checks, so a refactor of `Modal` keeps them.
    let src =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/lib_ui.rs"))
            .unwrap();
    let modal = &src[src.find("pub fn Modal(").unwrap()..src.find("pub fn from_dialog").unwrap()];
    for part in [
        "return_focus_on_cleanup()",
        "ModalStack::open(",
        "ModalStack::close(",
        "tab_round(&el",
    ] {
        assert!(modal.contains(part), "lib_ui::Modal lost {part}");
    }
}
