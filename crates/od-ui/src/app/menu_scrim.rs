//! The click-catcher behind an open drop-down menu. A click on it or Esc
//! closes the menu, as the original's menus do (`FileViewer.tsx`).

use leptos::prelude::*;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

/// Render it inside the menu's `<Show>`: Esc listens only while it is shown.
#[component]
pub fn MenuScrim(#[prop(into)] on_close: Callback<()>) -> impl IntoView {
    // The menu follows the scrim: its first item takes focus.
    let scrim = NodeRef::<leptos::html::Div>::new();
    // The scrim's parent holds the menu (and often its button).
    close_on_escape(on_close, move || {
        scrim
            .get_untracked()
            .and_then(|s| web_sys::Element::from(s).parent_element())
    });
    super::focus::focus_while_open(scrim, |s| {
        s.next_element_sibling()?
            .query_selector(super::focus::FIRST_ITEM)
            .ok()
            .flatten()
    });
    view! { <div class="menu-scrim" node_ref=scrim on:click=move |_| on_close.run(())></div> }
}

/// While the calling component is mounted, Esc runs `on_close` and nothing
/// else: a capture-phase listener on the window stops the key there, so it
/// does not also leave comment mode, drop a tool or stop a running jury.
/// Only while the layer is on top: the focus is inside `scope` (the menu
/// and its button) or on nothing. A palette, dialog or leave question
/// opened over it takes the focus, and its Esc goes to it.
pub fn close_on_escape(
    on_close: Callback<()>,
    scope: impl Fn() -> Option<web_sys::Element> + 'static,
) {
    type KeyCallback = wasm_bindgen::closure::Closure<dyn FnMut(web_sys::KeyboardEvent)>;
    let key = KeyCallback::new(move |e: web_sys::KeyboardEvent| {
        if e.key() == "Escape" && on_top(scope()) {
            e.stop_immediate_propagation();
            on_close.run(());
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback_and_bool(
            "keydown",
            key.as_ref().unchecked_ref(),
            true,
        );
    }
    let key = StoredValue::new(SendWrapper::new(key));
    on_cleanup(move || {
        if let Some(w) = web_sys::window() {
            key.with_value(|k| {
                let _ = w.remove_event_listener_with_callback_and_bool(
                    "keydown",
                    k.as_ref().unchecked_ref(),
                    true,
                );
            });
        }
    });
}

/// Whether a layer whose elements are in `scope` is the top one: the focus
/// is inside it, or on nothing (the page body). A layer above takes it.
fn on_top(scope: Option<web_sys::Element>) -> bool {
    let doc = document();
    let Some(active) = doc.active_element() else {
        return true;
    };
    if doc.body().is_some_and(|b| *b == active) {
        return true;
    }
    scope.is_some_and(|s| s.contains(Some(&active)))
}
