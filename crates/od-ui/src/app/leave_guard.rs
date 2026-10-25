//! Leaving a page that holds work not saved yet (a manual edit not
//! applied): every in-app way out asks first, as the original's
//! `FileWorkspace` navigation guard does, and the browser asks before a
//! close or reload.
//!
//! The question is the app's own dialog, not `window.confirm`: in the
//! desktop app `tauri-plugin-dialog` makes `confirm` async, so its answer
//! never reaches a click handler (QA-011).

use super::Shell;
use super::icons::Glyph;
use super::lib_ui::Modal;
use crate::i18n::{T, t};
use leptos::ev;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

/// While the calling component lives, leaving asks when `unsaved` answers
/// true.
pub fn guard_leaving(unsaved: Callback<(), bool>) {
    let shell = Shell::get();
    shell.unsaved.set(Some(unsaved));
    on_cleanup(move || shell.unsaved.set(None));
}

/// `navigate(path)`, unless leaving would lose unsaved work: then the
/// question first, and the path once the user agrees.
pub fn use_leave() -> impl Fn(&str) + Clone + Send + Sync + 'static {
    let shell = Shell::get();
    let navigate = use_navigate();
    move |path: &str| {
        if shell.would_lose() {
            shell.leaving.set(Some(path.to_owned()));
        } else {
            navigate(path, Default::default());
        }
    }
}

impl Shell {
    fn would_lose(self) -> bool {
        self.unsaved
            .get_untracked()
            .is_some_and(|unsaved| unsaved.run(()))
    }
}

/// The guard on links and page unload, and the question. Inside the
/// router, once.
#[component]
pub fn LeaveGuard() -> impl IntoView {
    type ClickCallback = wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MouseEvent)>;
    let shell = Shell::get();
    let navigate = use_navigate();
    let unload = window_event_listener(ev::beforeunload, move |e| {
        if shell.would_lose() {
            e.prevent_default();
            e.set_return_value("");
        }
    });
    // Capture phase on the window: runs before the router's link handler.
    let on_click = ClickCallback::new(move |e: web_sys::MouseEvent| {
        if e.default_prevented() || e.button() != 0 || !shell.would_lose() {
            return;
        }
        let href = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("a[href]").ok().flatten())
            .filter(link_leaves_page)
            .and_then(|a| a.get_attribute("href"));
        if let Some(href) = href {
            e.prevent_default();
            e.stop_propagation();
            shell.leaving.set(Some(href));
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback_and_bool(
            "click",
            on_click.as_ref().unchecked_ref(),
            true,
        );
    }
    let on_click = StoredValue::new(SendWrapper::new(on_click));
    on_cleanup(move || {
        unload.remove();
        if let Some(w) = web_sys::window() {
            on_click.with_value(|c| {
                let _ = w.remove_event_listener_with_callback_and_bool(
                    "click",
                    c.as_ref().unchecked_ref(),
                    true,
                );
            });
        }
    });
    let stay = Callback::new(move |()| shell.leaving.set(None));
    let leave = Callback::new(move |()| {
        if let Some(path) = shell.leaving.get_untracked() {
            shell.leaving.set(None);
            navigate(&path, Default::default());
        }
    });
    view! {
        <Show when=move || shell.leaving.with(Option::is_some)>
            <Modal title=t(T::ManualLeaveTitle).to_owned() glyph=Glyph::EditText on_close=stay testid="leave-dialog">
                <div class="dialog-body"><p>{t(T::ManualLeaveConfirm)}</p></div>
                <div class="dialog-foot">
                    <button class="btn" data-testid="leave-stay" on:click=move |_| stay.run(())>{t(T::Cancel)}</button>
                    <button class="btn danger" data-testid="leave-go" on:click=move |_| leave.run(())>{t(T::ManualLeave)}</button>
                </div>
            </Modal>
        </Show>
    }
}

/// An in-app link to another page (not a new tab, not this same page).
fn link_leaves_page(a: &web_sys::Element) -> bool {
    let href = a.get_attribute("href").unwrap_or_default();
    if !href.starts_with('/') || a.get_attribute("target").is_some_and(|t| t == "_blank") {
        return false;
    }
    let here = web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .unwrap_or_default();
    href.split(['?', '#']).next() != Some(here.as_str())
}
