//! Focus for floating menus and panels: it moves in when one opens and goes
//! back to the control that opened it when it closes.

use leptos::html::ElementType;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

/// The first enabled item of a menu or popover.
pub const FIRST_ITEM: &str = "[role^=menuitem]:not(:disabled), button:not(:disabled), input:not([type=hidden]):not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])";

/// Call in the component that renders `node` while the menu or panel is
/// open. Once `node` mounts, focus moves to what `target` picks from it
/// (`None` focuses `node`, which then needs `tabindex="-1"`). When the
/// component unmounts and focus was left on nothing, it goes back to the
/// element that had it before.
pub fn focus_while_open<E>(
    node: NodeRef<E>,
    target: fn(&web_sys::Element) -> Option<web_sys::Element>,
) where
    E: ElementType + 'static,
    E::Output: JsCast + Clone + 'static,
{
    return_focus_on_cleanup();
    Effect::new(move |_| {
        let Some(el) = node.get() else {
            return;
        };
        let el: web_sys::Element = el.unchecked_into();
        after_render(move || {
            let to = target(&el).unwrap_or(el);
            if let Ok(to) = to.dyn_into::<web_sys::HtmlElement>() {
                let _ = to.focus();
            }
        });
    });
}

/// Focuses `node` each time it mounts: a field shown later (a sketch
/// label, a rename box). Use this, not the HTML `autofocus` attribute,
/// which a document honours once (QA-094): the second field built after
/// a page load would not get the focus.
pub fn focus_on_mount<E>(node: NodeRef<E>)
where
    E: ElementType + 'static,
    E::Output: JsCast + Clone + 'static,
{
    Effect::new(move |_| {
        if let Some(el) = node.get()
            && let Ok(el) = el
                .unchecked_into::<web_sys::Element>()
                .dyn_into::<web_sys::HtmlElement>()
        {
            let _ = el.focus();
        }
    });
}

/// Call in a component that takes the focus while it is shown: when it
/// unmounts and focus was left on nothing, it goes back to the element
/// that had it when the component mounted.
pub fn return_focus_on_cleanup() {
    let opener = SendWrapper::new(document().active_element());
    on_cleanup(move || {
        let opener = opener.take();
        after_render(move || {
            let doc = document();
            let lost = doc
                .active_element()
                .is_none_or(|a| doc.body().is_some_and(|b| *b == a));
            if let Some(o) = opener.filter(|o| lost && o.is_connected())
                && let Ok(o) = o.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = o.focus();
            }
        });
    });
}

/// Runs `f` once the DOM settled; touches only elements, so it also runs
/// after the component went (focus goes back on cleanup).
fn after_render(f: impl FnOnce() + 'static) {
    super::tasks::detached(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        f();
    });
}

/// Arrow keys (and Home, End) inside a tab list or radio group: focus the
/// next `[role=tab]` or `[role=radio]` of the group the handler sits on,
/// as in the WAI-ARIA tabs and radio group patterns. With `select` the
/// focused item is also picked (a click), so selection follows focus.
pub fn group_keys(ev: &web_sys::KeyboardEvent, select: bool) {
    let element =
        |t: Option<web_sys::EventTarget>| t.and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    let (Some(group), Some(from)) = (element(ev.current_target()), element(ev.target())) else {
        return;
    };
    let Ok(found) = group.query_selector_all("[role=tab], [role=radio]") else {
        return;
    };
    let items: Vec<web_sys::HtmlElement> = (0..found.length())
        .filter_map(|i| found.item(i)?.dyn_into().ok())
        .filter(|e: &web_sys::HtmlElement| !e.has_attribute("disabled"))
        .collect();
    let Some(at) = items.iter().position(|t| **t == from) else {
        return;
    };
    let Some(next) = next_index(&ev.key(), at, items.len()) else {
        return;
    };
    ev.prevent_default();
    let _ = items[next].focus();
    if select {
        items[next].click();
    }
}

/// Where an arrow, Home or End key moves from `at` among `len` items
/// (wrapping); `None` for any other key.
fn next_index(key: &str, at: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match key {
        "ArrowLeft" | "ArrowUp" => (at + len - 1) % len,
        "ArrowRight" | "ArrowDown" => (at + 1) % len,
        "Home" => 0,
        "End" => len - 1,
        _ => return None,
    })
}

/// Roving tabindex: only the picked item of a group is a Tab stop; the
/// arrows ([`group_keys`]) move between the others.
#[must_use]
pub fn roving(picked: bool) -> &'static str {
    if picked { "0" } else { "-1" }
}

#[cfg(test)]
mod tests {
    use super::next_index;

    /// Each `autofocus` attribute in `src`, as `file:line` (QA-094: a
    /// document applies it once; use [`super::focus_on_mount`]).
    fn autofocus_attrs(file: &str, src: &str) -> Vec<String> {
        let word = concat!("auto", "focus");
        src.lines()
            .enumerate()
            .filter(|(_, l)| !l.trim_start().starts_with("//"))
            .filter(|(_, l)| {
                l.match_indices(word).any(|(at, _)| {
                    let before = l[..at].chars().next_back();
                    let after = l[at + word.len()..].chars().next();
                    let part = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
                    !part(before) && !part(after)
                })
            })
            .map(|(i, _)| format!("{file}:{}", i + 1))
            .collect()
    }

    #[test]
    fn no_field_relies_on_the_autofocus_attribute() {
        fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        assert!(files.len() > 20, "the sources were found");
        let found: Vec<String> = files
            .iter()
            .flat_map(|p| {
                let src = std::fs::read_to_string(p).unwrap();
                autofocus_attrs(&p.strip_prefix(&root).unwrap().display().to_string(), &src)
            })
            .collect();
        assert!(
            found.is_empty(),
            "focus these with `focus_on_mount` (QA-094):\n{}",
            found.join("\n")
        );
    }

    #[test]
    fn an_autofocus_attribute_is_found() {
        let a = concat!("auto", "focus");
        let src = format!(
            "<input\n  {a}\n/>\n<input {a}=true/>\n// {a} in a comment\n<x initial_{a}=1/>"
        );
        assert_eq!(autofocus_attrs("a.rs", &src), ["a.rs:2", "a.rs:4"]);
    }

    #[test]
    fn arrows_wrap_and_home_end_jump() {
        assert_eq!(next_index("ArrowRight", 2, 3), Some(0));
        assert_eq!(next_index("ArrowLeft", 0, 3), Some(2));
        assert_eq!(next_index("ArrowDown", 0, 3), Some(1));
        assert_eq!(next_index("End", 0, 3), Some(2));
        assert_eq!(next_index("Home", 2, 3), Some(0));
        assert_eq!(next_index("Enter", 0, 3), None);
        assert_eq!(next_index("ArrowRight", 0, 0), None);
    }
}
