//! Leaving a page that holds work not saved yet (a comment not sent, a
//! manual edit not applied, in any file): every in-app way out asks first,
//! naming the files, as the original's
//! `FileWorkspace` navigation guard does, the browser's Back included, and
//! the browser asks before a close or reload. In the desktop app, closing
//! the window or quitting asks too: the shell knows whether work would be
//! lost (`set_unsaved`) and sends `od:menu` `close` or `quit`.
//!
//! The question is the app's own dialog, not `window.confirm`: in the
//! desktop app `tauri-plugin-dialog` makes `confirm` async, so its answer
//! never reaches a click handler (QA-011).

use super::Shell;
use super::desktop;
use super::icons::Glyph;
use super::lib_ui::Modal;
use crate::i18n::{T, t};
use leptos::ev;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

/// While the calling component lives, leaving asks when `unsaved` names
/// files (the files with work not saved). `unsaved` should track the
/// signals its answer depends on.
pub fn guard_leaving(unsaved: Callback<(), Vec<String>>) {
    let shell = Shell::get();
    shell.unsaved.set(Some(unsaved));
    on_cleanup(move || shell.unsaved.set(None));
}

/// A way out the leave question waits on.
#[derive(Clone)]
pub(super) enum Leave {
    /// An in-app path.
    To(String),
    /// Something that leaves by itself (a full page load).
    Run(std::sync::Arc<dyn Fn(Release) + Send + Sync>),
    /// The browser's Back (or Forward): the entry it left is pushed back
    /// while the question is open, and the target is this many entries
    /// back.
    Back(i32),
    /// The desktop app's window closing or the app quitting: the shell
    /// holds it until the answer (`close_answer`).
    App,
}

/// The desktop shell's `od:menu` `close` or `quit`: the shell holds the
/// close until the page answers, `go` at once when nothing would be lost,
/// else after the question (`asking` first, so the shell waits).
fn app_closing(shell: Shell) {
    if shell.would_lose() {
        desktop::close_answer("asking");
        shell.leaving.set(Some(Leave::App));
    } else {
        desktop::close_answer("go");
    }
}

/// `navigate(path)`, unless leaving would lose unsaved work: then the
/// question first, and the path once the user agrees.
pub fn use_leave() -> impl Fn(&str) + Clone + Send + Sync + 'static {
    let shell = Shell::get();
    let navigate = use_navigate();
    move |path: &str| {
        if shell.would_lose() {
            shell.leaving.set(Some(Leave::To(path.to_owned())));
        } else {
            navigate(path, Default::default());
        }
    }
}

/// Handed to a [`leave_then`] action: [`Release::now`] right before the
/// page unloads keeps the browser from asking a second time.
#[derive(Clone, Copy)]
pub struct Release(Shell);

impl Release {
    /// Only right before the unload: if the action fails first, the page
    /// stays and must still ask on a close.
    pub fn now(self) {
        self.0.released.set_value(true);
    }
}

/// Runs `leave` (a way out the router does not see, such as a full page
/// load), unless that would lose unsaved work: then the question first.
/// Takes the shell: a menu item calls it after its menu, and the owner
/// holding the context, are gone.
pub fn leave_then(shell: Shell, leave: impl Fn(Release) + Send + Sync + 'static) {
    if shell.would_lose() {
        shell
            .leaving
            .set(Some(Leave::Run(std::sync::Arc::new(leave))));
    } else {
        leave(Release(shell));
    }
}

impl Shell {
    /// The files whose work leaving would lose.
    fn losing(self) -> Vec<String> {
        self.unsaved
            .get_untracked()
            .map(|unsaved| untrack(|| unsaved.run(())))
            .unwrap_or_default()
    }

    fn would_lose(self) -> bool {
        !self.losing().is_empty()
    }
}

/// The guard on links and page unload, and the question. Inside the
/// router, once.
#[component]
pub fn LeaveGuard() -> impl IntoView {
    type ClickCallback = wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MouseEvent)>;
    type PopCallback = wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>;
    let shell = Shell::get();
    let navigate = use_navigate();
    let location = leptos_router::hooks::use_location();
    // Set by Leave on a Back: the next `popstate` goes to the router.
    let let_pop = StoredValue::new(false);
    // Leave was chosen for a way out that unloads the page: no second ask.
    let released = shell.released;
    let unload = window_event_listener(ev::beforeunload, move |e| {
        if !released.get_value() && shell.would_lose() {
            e.prevent_default();
            e.set_return_value("");
        }
    });
    // Capture phase on the window: runs before the router's link handler.
    let on_click = ClickCallback::new(move |e: web_sys::MouseEvent| {
        // A modified click opens a new tab or window: this one stays (QA-021).
        let modified = e.ctrl_key() || e.meta_key() || e.shift_key() || e.alt_key();
        if e.default_prevented() || e.button() != 0 || modified || !shell.would_lose() {
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
            shell.leaving.set(Some(Leave::To(href)));
        }
    });
    // Back has already moved the URL when `popstate` fires, and a Back to
    // another document fires none. So while work would be lost the page
    // holds a second entry of its own URL: Back lands on the first one, in
    // this document. The guard then keeps the router from following, puts
    // the entry back and asks (QA-022). Capture phase: runs before the
    // router's own listener on the window.
    // The router's `search` has no `?` (its `hash` keeps the `#`): a page
    // with a query pushed `/projects/p1preview=stopped`.
    let here = move || {
        let search = location.search.get();
        let mark = if search.is_empty() { "" } else { "?" };
        format!(
            "{}{mark}{search}{}",
            location.pathname.get(),
            location.hash.get()
        )
    };
    let armed = StoredValue::new(None::<String>);
    // What the desktop shell was last told (`set_unsaved`).
    let told = StoredValue::new(None::<bool>);
    let desktop = desktop::is_desktop();
    Effect::new(move |_| {
        let url = here();
        let lose = shell
            .unsaved
            .get()
            .is_some_and(|unsaved| !unsaved.run(()).is_empty());
        if desktop && told.get_value() != Some(lose) {
            told.set_value(Some(lose));
            desktop::set_unsaved(lose);
        }
        if lose && armed.with_value(|a| a.as_deref() != Some(url.as_str())) {
            push_entry(&url);
            armed.set_value(Some(url));
        }
    });
    let on_pop = PopCallback::new(move |e: web_sys::Event| {
        if let_pop.get_value() {
            let_pop.set_value(false);
            released.set_value(false);
            return;
        }
        if !shell.would_lose() {
            return;
        }
        e.stop_immediate_propagation();
        let url = untrack(here);
        let now = web_sys::window()
            .map(|w| w.location())
            .map(|l| {
                let part = |r: Result<String, _>| r.unwrap_or_default();
                format!(
                    "{}{}{}",
                    part(l.pathname()),
                    part(l.search()),
                    part(l.hash())
                )
            })
            .unwrap_or_default();
        push_entry(&url);
        // Landed on the page's own first entry: the target is one further.
        shell
            .leaving
            .set(Some(Leave::Back(if now == url { 2 } else { 1 })));
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback_and_bool(
            "click",
            on_click.as_ref().unchecked_ref(),
            true,
        );
        let _ = w.add_event_listener_with_callback_and_bool(
            "popstate",
            on_pop.as_ref().unchecked_ref(),
            true,
        );
    }
    // The desktop window closing or the app quitting (`close_guard` in
    // the shell).
    let menu = window_event_listener(
        ev::Custom::<web_sys::CustomEvent>::new("od:menu"),
        move |e| {
            if matches!(e.detail().as_string().as_deref(), Some("close" | "quit")) {
                app_closing(shell);
            }
        },
    );
    let listeners = StoredValue::new(SendWrapper::new((on_click, on_pop)));
    on_cleanup(move || {
        unload.remove();
        menu.remove();
        if let Some(w) = web_sys::window() {
            listeners.with_value(|l| {
                let (click, pop) = &**l;
                let _ = w.remove_event_listener_with_callback_and_bool(
                    "click",
                    click.as_ref().unchecked_ref(),
                    true,
                );
                let _ = w.remove_event_listener_with_callback_and_bool(
                    "popstate",
                    pop.as_ref().unchecked_ref(),
                    true,
                );
            });
        }
    });
    let stay = Callback::new(move |()| {
        if matches!(shell.leaving.get_untracked(), Some(Leave::App)) {
            desktop::close_answer("stay");
        }
        shell.leaving.set(None);
    });
    let leave = Callback::new(move |()| {
        let Some(way) = shell.leaving.get_untracked() else {
            return;
        };
        shell.leaving.set(None);
        match way {
            Leave::To(path) => navigate(&path, Default::default()),
            Leave::Run(leave) => leave(Release(shell)),
            Leave::Back(steps) => {
                if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
                    let_pop.set_value(true);
                    released.set_value(true);
                    let _ = history.go_with_delta(-steps);
                }
            }
            Leave::App => {
                // The window goes with the page: no second ask.
                released.set_value(true);
                desktop::close_answer("go");
            }
        }
    });
    view! {
        <Show when=move || shell.leaving.with(Option::is_some)>
            <Modal title=t(T::ManualLeaveTitle).to_owned() glyph=Glyph::EditText on_close=stay testid="leave-dialog"
                initial_focus="[data-testid=leave-stay]">
                <div class="dialog-body">
                    <p>{t(T::LeaveDraftsConfirm)}</p>
                    <ul class="leave-files" data-testid="leave-files">
                        {shell.losing().into_iter().map(|f| view! { <li class="mono small">{f}</li> }).collect_view()}
                    </ul>
                </div>
                <div class="dialog-foot">
                    <button class="btn" data-testid="leave-stay" on:click=move |_| stay.run(())>{t(T::Cancel)}</button>
                    <button class="btn danger" data-testid="leave-go" on:click=move |_| leave.run(())>{t(T::ManualLeave)}</button>
                </div>
            </Modal>
        </Show>
    }
}

/// A history entry for `url` (this page) after the current one.
fn push_entry(url: &str) {
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(url));
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
