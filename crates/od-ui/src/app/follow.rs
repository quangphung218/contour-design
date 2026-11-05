//! Browser side of [`crate::scroll_follow`]: the main chat and the side
//! chat keep new output in view only while the reader is at the end.

use super::icons::{Glyph, icon};
use crate::i18n::{T, t};
use crate::scroll_follow::{Follow, Metrics};
use leptos::html::Div;
use leptos::prelude::*;
use send_wrapper::SendWrapper;

/// A transcript's scroll box and its follow state.
#[derive(Clone, Copy)]
pub struct StickToBottom {
    list: NodeRef<Div>,
    state: RwSignal<Follow>,
}

impl StickToBottom {
    /// Also watches the list's DOM: parts of a turn render after the
    /// signals that [`Self::changed`] follows (turn files, next steps once
    /// loaded), and those keep the end in view too while following.
    pub fn new(list: NodeRef<Div>) -> Self {
        let this = Self {
            list,
            state: RwSignal::new(Follow::default()),
        };
        this.observe();
        this
    }

    fn observe(self) {
        use wasm_bindgen::JsCast;
        type Callback = wasm_bindgen::closure::Closure<dyn FnMut(js_sys::Array)>;
        let observer = StoredValue::new(None::<SendWrapper<(web_sys::MutationObserver, Callback)>>);
        Effect::new(move |_| {
            let Some(el) = self.list.get() else {
                return;
            };
            let target = el.clone();
            let on_change = Callback::new(move |_: js_sys::Array| {
                if self.sync() {
                    target.set_scroll_top(target.scroll_height());
                }
            });
            let Ok(obs) = web_sys::MutationObserver::new(on_change.as_ref().unchecked_ref()) else {
                return;
            };
            let init = web_sys::MutationObserverInit::new();
            init.set_child_list(true);
            init.set_subtree(true);
            init.set_character_data(true);
            let _ = obs.observe_with_options(&el, &init);
            observer.update_value(|o| {
                if let Some(old) = o.replace(SendWrapper::new((obs, on_change))) {
                    old.0.disconnect();
                }
            });
        });
        on_cleanup(move || {
            observer.with_value(|o| {
                if let Some(o) = o {
                    o.0.disconnect();
                }
            });
        });
    }

    /// For the list's `on:scroll`.
    pub fn on_scroll(self) {
        self.sync();
    }

    /// Reads where the list is now, then says whether it follows. A move
    /// up whose `scroll` event has not come yet (it comes on the next
    /// frame: a scrollbar drag, Home, PageUp) counts before we scroll to
    /// the end, or output landing in between would undo it.
    fn sync(self) -> bool {
        let Some(el) = self.list.get_untracked() else {
            return self.state.with_untracked(Follow::pinned);
        };
        let m = Metrics {
            scroll_top: f64::from(el.scroll_top()),
            client_height: f64::from(el.client_height()),
            scroll_height: f64::from(el.scroll_height()),
        };
        self.state.maybe_update(|f| {
            let before = *f;
            f.scrolled(m);
            // `last_top` moves on every event; only a visible change
            // (the button) needs to notify.
            before.show_jump() != f.show_jump()
        });
        self.state.with_untracked(Follow::pinned)
    }

    /// For the list's `on:wheel` (mouse wheel and trackpad). A pinch on a
    /// trackpad comes as a wheel with `ctrlKey` and scrolls nothing.
    pub fn on_wheel(self, ev: &web_sys::WheelEvent) {
        if ev.ctrl_key() {
            return;
        }
        let Some(el) = self.list.get_untracked() else {
            return;
        };
        let can_go_up = el.scroll_top() > 0;
        self.state.maybe_update(|f| {
            let before = f.show_jump();
            f.wheeled(ev.delta_y(), can_go_up);
            before != f.show_jump()
        });
    }

    /// After the transcript changed (`new_item`: a message was sent or a
    /// conversation opened): scroll to the end if following, and again on
    /// the next frame for content that renders after this (next steps).
    pub fn changed(self, new_item: bool) {
        self.sync();
        let mut follow = false;
        self.state.maybe_update(|f| {
            let before = f.show_jump();
            follow = f.changed(new_item);
            before != f.show_jump()
        });
        if follow {
            self.to_end();
        }
    }

    fn to_end(self) {
        let Some(el) = self.list.get_untracked() else {
            return;
        };
        el.set_scroll_top(el.scroll_height());
        request_animation_frame(move || {
            if self.sync() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    }

    fn jump(self) {
        self.state.update(Follow::jump);
        self.to_end();
    }
}

/// "Jump to latest", inside the list (sticks to its bottom edge), shown
/// while the reader is up the transcript and new output came in. Pressed
/// from the keyboard, it hides itself, so focus moves to `composer` (a
/// CSS selector) instead of falling back to the page.
#[component]
pub fn JumpToLatest(follow: StickToBottom, composer: &'static str) -> impl IntoView {
    let show = Memo::new(move |_| follow.state.with(Follow::show_jump));
    view! {
        <Show when=move || show.get()>
            <button
                class="btn small jump-latest"
                data-testid="jump-latest"
                on:click=move |ev: web_sys::MouseEvent| {
                    follow.jump();
                    // `detail` 0: Enter or Space, not a pointer.
                    if ev.detail() == 0 {
                        super::chat_view::focus_selector(composer);
                    }
                }
            >
                {icon(Glyph::ChevronDown)}
                {t(T::JumpToLatest)}
            </button>
        </Show>
    }
}
