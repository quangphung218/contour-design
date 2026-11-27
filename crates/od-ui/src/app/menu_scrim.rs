//! The click-catcher behind an open drop-down menu. A click on it or Esc
//! closes the menu, as the original's menus do (`FileViewer.tsx`).

use leptos::prelude::*;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

/// Render it inside the menu's `<Show>`: Esc listens only while it is shown.
#[component]
pub fn MenuScrim(#[prop(into)] on_close: Callback<()>) -> impl IntoView {
    close_on_escape(on_close);
    // The menu follows the scrim: its first item takes focus.
    let scrim = NodeRef::<leptos::html::Div>::new();
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
pub fn close_on_escape(on_close: Callback<()>) {
    type KeyCallback = wasm_bindgen::closure::Closure<dyn FnMut(web_sys::KeyboardEvent)>;
    let key = KeyCallback::new(move |e: web_sys::KeyboardEvent| {
        if e.key() == "Escape" {
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
