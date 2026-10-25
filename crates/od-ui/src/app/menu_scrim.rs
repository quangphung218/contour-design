//! The click-catcher behind an open drop-down menu. A click on it or Esc
//! closes the menu, as the original's menus do (`FileViewer.tsx`).

use leptos::ev;
use leptos::prelude::*;

/// Render it inside the menu's `<Show>`: Esc listens only while it is shown.
#[component]
pub fn MenuScrim(#[prop(into)] on_close: Callback<()>) -> impl IntoView {
    let key = window_event_listener(ev::keydown, move |e| {
        if e.key() == "Escape" {
            on_close.run(());
        }
    });
    on_cleanup(move || key.remove());
    view! { <div class="menu-scrim" on:click=move |_| on_close.run(())></div> }
}
