//! A handler that closes what it sits in (a dialog's Cancel, a menu's
//! item, the leave question's Stay) removes that tree while the same event
//! still bubbles through it. Leptos hands a bubbling event from node to
//! parent node, and a listener of the removed tree is already dropped:
//! wasm throws `closure invoked recursively or after being dropped`
//! (QA-010, PRJ-21, the folder and instructions dialogs). Only an element
//! around the target that listens for the same event is in the way, so
//! no element listens for an event that one inside it listens for too:
//! the overlay's click lands on a backdrop beside the dialog, as in
//! `lib_ui::Modal`.
//!
//! This test reads every `view!` under `src` and finds each element with
//! an `on:<event>` inside another with the same `on:<event>`. A component
//! that puts its `children` inside a listening element (`Modal` around
//! its children, for `keydown`) counts as that element where it is used.
//! A component is found by its name however it is written:
//! `<super::lib_ui::Modal>`, or imported `as` another name; its children
//! under any name it binds them to (`StoredValue::new(children)`). A pair
//! that may stay is one whose inner handler never removes the outer
//! element; it is listed in [`KEPT`] with why.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// `(file under src, fn the outer element is in, event, pairs, why)`.
type Kept = (
    &'static str,
    &'static str,
    &'static str,
    usize,
    &'static str,
);

const KEPT: &[Kept] = &[
    (
        "app/chat_ops.rs",
        "FolderDialogView",
        "keydown",
        1,
        "the path field's Enter only starts the link; the dialog closes once it answered",
    ),
    (
        "app/export_extra.rs",
        "SaveTemplateDialog",
        "keydown",
        1,
        "the name field's Enter only starts the save; the dialog closes on the next task",
    ),
    (
        "app/files_panel.rs",
        "FileDialog",
        "keydown",
        1,
        "the name field's Enter only starts the create; the dialog closes on the next task",
    ),
    (
        "app/new_project.rs",
        "NewProjectPanel",
        "keydown",
        1,
        "the tab list's arrows move the focus between tabs; nothing closes",
    ),
    (
        "app/ds_create.rs",
        "CreateFlow",
        "keydown",
        1,
        "the source chips' arrows move the focus inside the `Modal`; nothing closes",
    ),
    (
        "app/library_picker.rs",
        "LibraryPicker",
        "keydown",
        1,
        "the kind tabs' arrows move the focus inside the `Modal`; nothing closes",
    ),
    (
        "app/ds_page.rs",
        "DsDetail",
        "keydown",
        2,
        "the kit tabs' arrows move the focus; the large preview `Modal` stops Escape \
         before it closes, so the key never reaches the detail it went from",
    ),
    (
        "app/media_page.rs",
        "AssetPreview",
        "keydown",
        1,
        "the `Modal` stops Escape before it closes; the arrows it lets through reach this \
         element, the last listener of the preview, which switches the asset",
    ),
    (
        "app/studio.rs",
        "TabsBar",
        "keydown",
        2,
        "Delete on a tab closes that tab only; the tab list around it stays",
    ),
    (
        "app/files_panel.rs",
        "FilesPanel",
        "dragover",
        2,
        "a drag over the tree only marks a folder; the files panel around it stays",
    ),
    (
        "app/files_panel.rs",
        "FilesPanel",
        "drop",
        2,
        "a drop on the tree moves files inside the panel; the files panel around it stays",
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

/// The name of a `fn` declared at the start of `line`, if any.
fn declared_fn(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    // `#[component] pub fn Modal(`: attributes before it on the same line.
    while rest.starts_with("#[") {
        rest = rest[rest.find(']')? + 1..].trim_start();
    }
    let at = rest.find("fn ")?;
    let head = &rest[..at];
    if !head.split_whitespace().all(|w| {
        matches!(w, "pub" | "async" | "const" | "unsafe" | "extern") || w.starts_with("pub(")
    }) {
        return None;
    }
    let name = &rest[at + 3..];
    let len = name
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(name.len());
    (len > 0).then(|| &name[..len])
}

/// Elements that never hold children.
const VOID: &[&str] = &[
    "area", "br", "circle", "col", "embed", "hr", "img", "input", "line", "link", "meta", "path",
    "polygon", "polyline", "rect", "source", "stop", "track", "use", "wbr",
];

/// The end of the string, char or comment starting at `i`, if one does.
fn skip_literal(s: &[u8], i: usize) -> Option<usize> {
    let rest = &s[i..];
    if rest.starts_with(b"//") {
        return Some(
            rest.iter()
                .position(|&b| b == b'\n')
                .map_or(s.len(), |n| i + n),
        );
    }
    if rest.starts_with(b"/*") {
        return Some(
            rest.windows(2)
                .position(|w| w == b"*/")
                .map_or(s.len(), |n| i + n + 2),
        );
    }
    let raw = rest.starts_with(b"r\"") || rest.starts_with(b"r#");
    if raw && (i == 0 || !is_ident(s[i - 1])) {
        let hashes = rest[1..].iter().take_while(|&&b| b == b'#').count();
        if rest.get(1 + hashes) == Some(&b'"') {
            let close: Vec<u8> = std::iter::once(b'"')
                .chain(std::iter::repeat_n(b'#', hashes))
                .collect();
            let body = i + 2 + hashes;
            return Some(
                s[body..]
                    .windows(close.len())
                    .position(|w| w == close)
                    .map_or(s.len(), |n| body + n + close.len()),
            );
        }
    }
    match rest.first()? {
        b'"' => {
            let mut j = i + 1;
            while j < s.len() && s[j] != b'"' {
                j += if s[j] == b'\\' { 2 } else { 1 };
            }
            Some(j + 1)
        }
        // A char literal (`'"'`, `'\''`), not a lifetime (`'static`).
        b'\'' if rest.get(1) == Some(&b'\\') => {
            Some(i + 2 + rest[2..].iter().position(|&b| b == b'\'')? + 1)
        }
        b'\'' if rest.get(2) == Some(&b'\'') => Some(i + 3),
        _ => None,
    }
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// An element or component tag starting at `s[i] == b'<'`.
struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    /// The `on:<event>` it listens for (not `:undelegated`).
    events: BTreeSet<String>,
    end: usize,
}

fn tag_at(s: &[u8], i: usize) -> Option<Tag> {
    if i > 0 && (is_ident(s[i - 1]) || s[i - 1] == b':' || s[i - 1] == b'<') {
        return None; // `Vec<u8>`, `Option<String>`, `<<`
    }
    let closing = s.get(i + 1) == Some(&b'/');
    let start = i + 1 + usize::from(closing);
    if !s.get(start)?.is_ascii_alphabetic() {
        return None; // `a < b`, `<=`
    }
    // `<super::lib_ui::Modal>` is the component `Modal`.
    let len = s[start..]
        .iter()
        .position(|&b| !(is_ident(b) || b == b'-' || b == b':'))
        .unwrap_or(s.len() - start);
    let full = String::from_utf8_lossy(&s[start..start + len]);
    let name = full.rsplit("::").next().unwrap_or_default().to_owned();
    if name.is_empty() {
        return None;
    }
    let mut events = BTreeSet::new();
    let mut depth = 0usize;
    let mut j = start + len;
    while j < s.len() {
        if let Some(end) = skip_literal(s, j) {
            j = end;
            continue;
        }
        match s[j] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            // As `view!` reads it: a `>` outside a block ends the tag.
            b'>' if depth == 0 => {
                let self_closing = s[..j].trim_ascii_end().ends_with(b"/");
                return Some(Tag {
                    name,
                    closing,
                    self_closing,
                    events,
                    end: j + 1,
                });
            }
            b'o' if depth == 0 && s[j..].starts_with(b"on:") && s[j - 1].is_ascii_whitespace() => {
                let ev = &s[j + 3..];
                let n = ev.iter().position(|&b| !is_ident(b)).unwrap_or(ev.len());
                if !ev[n..].starts_with(b":undelegated") {
                    events.insert(String::from_utf8_lossy(&ev[..n]).into_owned());
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// One element listening for `event` inside another that does too.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Nested {
    /// The fn the outer element is written in.
    outer_fn: String,
    event: String,
    outer_line: usize,
    inner_line: usize,
}

/// `src` with its comments and string literals blanked out (each byte a
/// space, newlines kept), as the main scan skips them ([`skip_literal`]).
fn code_only(src: &str) -> String {
    let s = src.as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if let Some(end) = skip_literal(s, i) {
            let end = end.min(s.len());
            out.extend(
                s[i..end]
                    .iter()
                    .map(|&b| if b == b'\n' { b'\n' } else { b' ' }),
            );
            i = end;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// `alias → name` of the components a file imports under another name
/// (`use super::lib_ui::{Modal as Dialog};`). Only code counts (a comment
/// "we use Modal as a base" renames nothing), and only an alias that is
/// itself a component name (capitalised: `view!` takes a lowercase tag
/// for an element).
fn use_aliases(src: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let code = code_only(src);
    let mut rest = code.as_str();
    while let Some(at) = rest.find("use ") {
        let fresh = at == 0 || rest[..at].ends_with(|c: char| c.is_whitespace() || c == ';');
        let end = rest[at..].find(';').map_or(rest.len(), |e| at + e);
        if fresh {
            let words: Vec<&str> = rest[at..end]
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty())
                .collect();
            for w in words.windows(3) {
                let capital = |w: &str| w.starts_with(|c: char| c.is_ascii_uppercase());
                if w[1] == "as" && capital(w[0]) && capital(w[2]) {
                    out.insert(w[2].to_owned(), w[0].to_owned());
                }
            }
        }
        rest = &rest[end.max(at + 4)..];
    }
    out
}

/// `NAME` when the word at `i` is the value of `let NAME =` (on its line,
/// or on the line before when rustfmt moved the value down).
fn let_name(src: &str, i: usize) -> Option<String> {
    let line_start = src[..i].rfind('\n').map_or(0, |n| n + 1);
    let mut head = src[line_start..i].trim().to_owned();
    if !head.starts_with("let ") && line_start > 0 {
        let prev_start = src[..line_start - 1].rfind('\n').map_or(0, |n| n + 1);
        let prev = src[prev_start..line_start].trim();
        if prev.starts_with("let ") && prev.ends_with('=') {
            head = format!("{prev} {head}");
        }
    }
    let rest = head.strip_prefix("let ")?.trim_start();
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let len = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    rest[len..]
        .trim_start()
        .starts_with('=')
        .then(|| rest[..len].to_owned())
}

/// What the scan knows of each component (by fn name): the events around
/// its `children`, and the events any element in it listens for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Components {
    wraps: BTreeMap<String, BTreeSet<String>>,
    listens: BTreeMap<String, BTreeSet<String>>,
}

/// The listening elements of one file: each pair of nested ones, and what
/// its components wrap and listen for. `known` is what earlier rounds found
/// of every component.
fn scan_source(src: &str, known: &Components) -> (Vec<Nested>, Components) {
    let s = src.as_bytes();
    let newlines: Vec<usize> = (0..s.len()).filter(|&k| s[k] == b'\n').collect();
    let line_at = |i: usize| newlines.partition_point(|&k| k < i) + 1;
    // `(where a line starts, the fn declared on it)`, in order.
    let mut start = 0;
    let mut fns = Vec::new();
    for line in src.split_inclusive('\n') {
        if let Some(name) = declared_fn(line) {
            fns.push((start, name));
        }
        start += line.len();
    }
    let fn_at = |i: usize| {
        let before = fns.partition_point(|(at, _)| *at <= i);
        before
            .checked_sub(1)
            .map_or("(top)", |k| fns[k].1)
            .to_owned()
    };
    // `(name, line, fn, events)` of the open elements, outermost first.
    let mut open: Vec<(String, usize, String, BTreeSet<String>)> = Vec::new();
    let mut nested = Vec::new();
    let mut found = Components::default();
    let renamed = use_aliases(src);
    let real = |name: String| renamed.get(&name).cloned().unwrap_or(name);
    // Per fn, the names its children go by: `children`, and what it is
    // bound to (`let kept = StoredValue::new(children);`).
    let mut kids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut i = 0;
    while i < s.len() {
        if let Some(end) = skip_literal(s, i) {
            i = end;
            continue;
        }
        if is_ident(s[i]) && (i == 0 || !(is_ident(s[i - 1]) || s[i - 1] == b'.')) {
            let len = s[i..]
                .iter()
                .position(|&b| !is_ident(b))
                .unwrap_or(s.len() - i);
            let word = &src[i..i + len];
            let f = fn_at(i);
            let names = kids.entry(f.clone()).or_default();
            if word == "children" || names.contains(word) {
                if let Some(bound) = let_name(src, i) {
                    names.insert(bound);
                }
                let events: BTreeSet<String> = open.iter().flat_map(|o| o.3.clone()).collect();
                if !events.is_empty() {
                    found.wraps.entry(f).or_default().extend(events);
                }
            }
            i += len;
            continue;
        }
        if s[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(tag) = tag_at(s, i) else {
            i += 1;
            continue;
        };
        let tag = Tag {
            name: real(tag.name),
            ..tag
        };
        let component = tag.name.starts_with(|c: char| c.is_ascii_uppercase());
        if tag.closing {
            if let Some(at) = open.iter().rposition(|o| o.0 == tag.name) {
                open.truncate(at);
            }
        } else {
            // A component listens for what its own elements do, and its
            // children sit inside what it wraps them in.
            let mut listens = tag.events.clone();
            let mut events = tag.events.clone();
            if component {
                listens.extend(known.listens.get(&tag.name).cloned().unwrap_or_default());
                events = known.wraps.get(&tag.name).cloned().unwrap_or_default();
            }
            if !listens.is_empty() {
                found
                    .listens
                    .entry(fn_at(i))
                    .or_default()
                    .extend(listens.clone());
            }
            let line = line_at(i);
            for event in &listens {
                if let Some(outer) = open.iter().rev().find(|o| o.3.contains(event)) {
                    nested.push(Nested {
                        outer_fn: outer.2.clone(),
                        event: event.clone(),
                        outer_line: outer.1,
                        inner_line: line,
                    });
                }
            }
            if !tag.self_closing && !VOID.contains(&tag.name.as_str()) {
                open.push((tag.name, line, fn_at(i), events));
            }
        }
        i = tag.end;
    }
    (nested, found)
}

/// `(file, nested pairs)` for every file under `src`.
fn scan(src: &Path) -> (Vec<(String, Vec<Nested>)>, Components) {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    files.sort();
    let sources: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(p).unwrap())
        })
        .collect();
    // What components wrap and listen for, through the components they
    // use too: until a round finds nothing new.
    let mut known = Components::default();
    loop {
        let mut next = Components::default();
        for (_, text) in &sources {
            let found = scan_source(text, &known).1;
            for (name, events) in found.wraps {
                next.wraps.entry(name).or_default().extend(events);
            }
            for (name, events) in found.listens {
                next.listens.entry(name).or_default().extend(events);
            }
        }
        if next == known {
            break;
        }
        known = next;
    }
    let pairs = sources
        .iter()
        .map(|(rel, text)| (rel.clone(), scan_source(text, &known).0))
        .collect();
    (pairs, known)
}

#[test]
fn no_listener_sits_inside_one_for_the_same_event() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found: BTreeMap<(String, String, String), Vec<Nested>> = BTreeMap::new();
    for (file, pairs) in scan(&src).0 {
        for n in pairs {
            found
                .entry((file.clone(), n.outer_fn.clone(), n.event.clone()))
                .or_default()
                .push(n);
        }
    }
    let kept: BTreeMap<_, _> = KEPT
        .iter()
        .map(|(file, f, ev, n, _)| (((*file).to_owned(), (*f).to_owned(), (*ev).to_owned()), *n))
        .collect();
    let mut wrong = Vec::new();
    for (site, pairs) in &found {
        let lines: Vec<String> = pairs
            .iter()
            .map(|p| format!("line {} inside line {}", p.inner_line, p.outer_line))
            .collect();
        match kept.get(site) {
            Some(n) if *n == pairs.len() => {}
            Some(n) => wrong.push(format!(
                "{site:?}: {} pairs ({}), KEPT lists {n}",
                pairs.len(),
                lines.join(", ")
            )),
            None => wrong.push(format!(
                "{site:?}: on:{} inside on:{} ({}): a handler that closes the outer \
                 element drops its listener while the event still bubbles to it; put the \
                 outer listener beside the inner one (a backdrop, as in `Modal`)",
                site.2,
                site.2,
                lines.join(", ")
            )),
        }
    }
    for site in kept.keys() {
        if !found.contains_key(site) {
            wrong.push(format!("{site:?}: in KEPT but gone; drop the entry"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_kept_pair_says_why() {
    for (file, f, ev, n, why) in KEPT {
        assert!(
            *n > 0 && why.len() > 20,
            "{file} {f} {ev}: say why it may stay"
        );
    }
}

fn pairs(src: &str) -> Vec<(usize, usize)> {
    scan_source(src, &Components::default())
        .0
        .iter()
        .map(|n| (n.outer_line, n.inner_line))
        .collect()
}

#[test]
fn the_scan_sees_a_listener_inside_one_for_the_same_event() {
    // The dialog that keeps clicks from its overlay, around a Cancel.
    let overlay = "fn D() { view! {\n<div class=\"overlay\" on:click=move |_| close()>\n\
                   <div class=\"dialog\" on:click=|ev| ev.stop_propagation()>\n\
                   <button on:click=move |_| open.set(false)>{t(T::Cancel)}</button>\n\
                   </div></div> } }";
    assert_eq!(pairs(overlay), vec![(2, 3), (3, 4)]);
    // A backdrop beside the dialog: nothing around the button listens.
    let modal = "fn D() { view! {\n<div class=\"overlay\">\n\
                 <div class=\"modal-backdrop\" on:click=move |_| close()></div>\n\
                 <div class=\"dialog\" on:keydown=keys>\n\
                 <button on:click=move |_| open.set(false)>{t(T::Cancel)}</button>\n\
                 </div></div> } }";
    assert_eq!(pairs(modal), vec![]);
    // Another event, a self-closing listener, a closed one: not around it.
    assert_eq!(
        pairs("<div on:keydown=k><button on:click=c></button></div>"),
        vec![]
    );
    assert_eq!(
        pairs("<div on:click=a/><button on:click=b></button>"),
        vec![]
    );
    assert_eq!(
        pairs("<input on:click=a><button on:click=b></button>"),
        vec![]
    );
    // `=>` and `>` inside a block, and a `<div` in a string are not tags.
    let arms = "<div on:click=move |_| match k { A => x(), _ => {} } on:keydown=|e| { if n > 1 {} }>\n\
                {\"<div on:click=s>\"}<button on:click=b></button></div>";
    assert_eq!(pairs(arms), vec![(1, 2)]);
    let block =
        "<div on:keydown=|e| { if n > 1 { k(e) } } on:click=a><button on:click=b></button></div>";
    assert_eq!(pairs(block), vec![(1, 1)]);
    assert_eq!(
        pairs("let v: Vec<u8> = f::<a>(); <div on:click=a></div><a on:click=b></a>"),
        vec![]
    );
    // A listener in a closure that renders inside the element counts.
    assert_eq!(
        pairs("<div on:click=a>{move || show.then(|| view! { <a on:click=b></a> })}</div>"),
        vec![(1, 1)]
    );
    // Only the delegated listener: `:undelegated` is removed with its node.
    assert_eq!(
        pairs("<div on:click:undelegated=a><button on:click=b></button></div>"),
        vec![]
    );
}

#[test]
fn a_component_around_its_children_counts_as_its_listener() {
    let wraps = "fn Box(children: Children) -> impl IntoView {\n\
                 view! { <div class=\"x\" on:keydown=keys>{children()}</div> } }";
    let (_, known) = scan_source(wraps, &Components::default());
    assert_eq!(known.wraps.get("Box").map(BTreeSet::len), Some(1));
    let used = "fn Page() { view! {\n<Box>\n<input on:keydown=enter/>\n</Box>\n\
                <Other><input on:keydown=enter/></Other> } }";
    let (nested, _) = scan_source(used, &known);
    assert_eq!(
        nested.iter().map(|n| n.inner_line).collect::<Vec<_>>(),
        vec![3]
    );
    // `Modal` wraps its children in the dialog's keydown, not a click.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/lib_ui.rs");
    let (_, lib) = scan_source(
        &std::fs::read_to_string(src).unwrap(),
        &Components::default(),
    );
    assert_eq!(
        lib.wraps
            .get("Modal")
            .map(|e| e.iter().cloned().collect::<Vec<_>>()),
        Some(vec!["keydown".to_owned()])
    );
}

#[test]
fn a_component_with_a_listener_counts_inside_a_listening_element() {
    let menu = "fn Menu() -> impl IntoView {\n\
                view! { <ul><li on:click=move |_| open.set(false)>{x}</li></ul> } }";
    let (_, known) = scan_source(menu, &Components::default());
    let card = "fn Card() { view! {\n<article on:click=open>\n<Menu/>\n</article>\n<Menu/> } }";
    let (nested, _) = scan_source(card, &known);
    assert_eq!(
        nested
            .iter()
            .map(|n| (n.outer_line, n.inner_line))
            .collect::<Vec<_>>(),
        vec![(2, 3)]
    );
    // The same component by its path, or imported under another name.
    let by_path =
        "fn Card() { view! {\n<article on:click=open>\n<super::menu::Menu/>\n</article> } }";
    assert_eq!(scan_source(by_path, &known).0.len(), 1);
    let open_close = "fn Card() { view! {\n<article on:click=open>\n<super::menu::Menu>\n\
                      </super::menu::Menu>\n<button on:click=b></button>\n</article> } }";
    assert_eq!(scan_source(open_close, &known).0.len(), 2);
    let aliased = "use super::menu::{Menu as Options, Other};\n\
                   fn Card() { view! {\n<article on:click=open>\n<Options/>\n</article> } }";
    assert_eq!(scan_source(aliased, &known).0.len(), 1);
    // "use … as" in a comment or a string renames nothing: `<a>` stays an
    // element listening for clicks, and the button inside it is a pair.
    let modal = "fn Modal() -> impl IntoView { view! { <div on:click=close></div> } }";
    let (_, with_modal) = scan_source(modal, &known);
    for words in [
        "// We use Modal as a base for the menu.\n",
        "/* We use Modal as a base; */\n",
        "const NOTE: &str = \"use Modal as a base;\";\n",
        // Code, but a lowercase alias is never a component.
        "use super::lib_ui::Modal as a;\n",
    ] {
        let src = format!(
            "{words}fn Menu() {{ view! {{\n<a on:click=go>\n<button on:click=close></button>\n</a> }} }}"
        );
        assert_eq!(scan_source(&src, &with_modal).0.len(), 1, "{words}");
    }
    // A capitalised "as" in a comment or a string renames nothing either:
    // `<Item/>` (no listener) is not taken for `Menu` (listens for clicks).
    for words in [
        "// Shown as we use Menu as Item;\n",
        "const NOTE: &str = \"use Menu as Item;\";\n",
    ] {
        let src = format!(
            "{words}fn Card() {{ view! {{\n<article on:click=open>\n<Item/>\n</article> }} }}"
        );
        assert_eq!(scan_source(&src, &known).0.len(), 0, "{words}");
    }
}

#[test]
fn a_component_keeps_its_children_under_any_name() {
    let kinds = [
        "#[component] pub fn Box(children: Children) -> impl IntoView {\n\
         view! { <div on:click=c>{children()}</div> } }",
        "fn Box(children: ChildrenFn) -> impl IntoView {\n\
         let kept = StoredValue::new(children);\n\
         view! { <div on:click=c>{kept.with_value(|k| k())}</div> } }",
        "fn Box(children: ChildrenFn) -> impl IntoView {\n\
         let kept =\n    StoredValue::new(Some(children));\n\
         view! { <div on:click=c>{move || kept.with_value(|k| k.as_ref().map(|k| k()))}</div> } }",
        "fn Box(children: Option<ChildrenFn>) -> impl IntoView {\n\
         view! { <div on:click=c>{children.map(|c| c())}</div> } }",
    ];
    for src in kinds {
        let (_, known) = scan_source(src, &Components::default());
        assert!(known.wraps.contains_key("Box"), "{src}");
    }
    // `children` outside the element does not count.
    let outside = "fn Box(children: Children) -> impl IntoView {\n\
                   let c = children();\n view! { <div on:click=c></div> } }";
    assert!(
        !scan_source(outside, &Components::default())
            .1
            .wraps
            .contains_key("Box")
    );
}

#[test]
fn the_scan_knows_what_the_repos_components_listen_for() {
    // The components the app uses by their path (`<quick_menu::QuickMenu>`,
    // `<memory_settings::MemoryToast/>`, `<super::lib_ui::Modal>`) are
    // known from their own definitions: what each listens for and what it
    // wraps its children in. This checks the definitions only; that a use
    // by path counts where it is used is held by the `by_path` and
    // `open_close` cases of
    // `a_component_with_a_listener_counts_inside_a_listening_element`.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (_, known) = scan(&src);
    for name in [
        "QuickMenu",
        "MemoryToast",
        "PlusMenu",
        "ConversationMenu",
        "Modal",
    ] {
        assert!(
            known.listens.get(name).is_some_and(|e| e.contains("click")),
            "{name} listens for clicks"
        );
    }
    assert!(
        known.wraps.contains_key("Modal"),
        "Modal wraps its children"
    );
}
