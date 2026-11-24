//! A modal dialog keeps the focus while it is open and gives it back when
//! it closes: `lib_ui::Modal` makes the page behind it `inert`, sends Tab
//! round its own controls, and returns the focus to the opener (e2e
//! `library.e2e.mjs` "dialog focus"). A dialog built by hand with
//! `aria-modal="true"` gets none of that: Tab walks out to the page behind
//! the backdrop, where Esc no longer closes it.
//!
//! Every dialog over the page under `src/app` outside `lib_ui.rs` is
//! listed in [`KEPT`] with how many its file has and why it may stay. A
//! dialog over the page is found by its shape, not only by the attribute a
//! hand-built one may lack: an `aria-modal="true"`, or a `class="overlay…"`
//! with a `role="dialog"` (or `alertdialog`) on it or right inside it (the
//! quick switcher has no `aria-modal`). A new one, one more in a listed
//! file, or a listed one gone fails the test: a new modal uses `Modal`, or
//! is a decision written here.

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
    (
        "switcher.rs",
        1,
        "quick switcher: a combobox like the palette, its list takes arrow keys, Tab is not its way round",
    ),
];

/// How far after an overlay's `class="overlay` its `role="dialog"` may
/// be (whitespace removed): the overlay's own tag and the next one.
const OVERLAY_REACH: usize = 400;

/// The dialogs over the page in `text` (see the module doc).
fn dialogs(text: &str) -> usize {
    let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let at = |needle: &str| -> Vec<usize> { flat.match_indices(needle).map(|(i, _)| i).collect() };
    let roles: Vec<usize> = at("role=\"dialog\"")
        .into_iter()
        .chain(at("role=\"alertdialog\""))
        .collect();
    let windows: Vec<(usize, usize)> = at("class=\"overlay")
        .into_iter()
        .map(|i| (i, i + OVERLAY_REACH))
        .filter(|(a, b)| roles.iter().any(|r| r >= a && r < b))
        .collect();
    let lone_modals = at("aria-modal=\"true\"")
        .into_iter()
        .filter(|m| !windows.iter().any(|(a, b)| m >= a && m < b))
        .count();
    windows.len() + lone_modals
}

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
        let n = dialogs(&std::fs::read_to_string(&path).unwrap());
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
                "{file}: {n} dialog(s) over the page built by hand; use lib_ui::Modal (focus stays in it) or list it in KEPT with why"
            )),
            Some(k) if k != n => problems.push(format!("{file}: {n} dialog(s), KEPT says {k}")),
            Some(_) => {}
        }
    }
    for file in kept.keys().filter(|f| !found.contains_key(*f)) {
        problems.push(format!(
            "{file}: listed in KEPT but has no dialog over the page now; drop it"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The shapes the scan finds, and the popover it leaves alone.
#[test]
fn a_dialog_over_the_page_is_found_by_its_shape() {
    let switcher = r#"<div class="overlay" on:click=move |_| close()>
            <div
                class="switcher"
                role="dialog"
                aria-label=t(T::QuickOpen)>"#;
    assert_eq!(dialogs(switcher), 1, "overlay + role=dialog, no aria-modal");
    let both =
        r#"<div class="overlay center"><div class="dialog" role="alertdialog" aria-modal="true">"#;
    assert_eq!(dialogs(both), 1, "one dialog, counted once");
    let own = r#"<div class="overlay center" role="dialog" aria-modal="true">"#;
    assert_eq!(dialogs(own), 1);
    assert_eq!(dialogs(r#"<div class="dialog" aria-modal="true">"#), 1);
    let popover = r#"<div class="menu popover" role="dialog" aria-label="Memory">"#;
    assert_eq!(dialogs(popover), 0, "a popover is no dialog over the page");
    assert_eq!(dialogs(r#"<div class="overlay"></div>"#), 0);
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
