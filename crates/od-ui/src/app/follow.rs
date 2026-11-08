//! Browser side of [`crate::scroll_follow`]: the main chat and the side
//! chat keep new output in view only while the reader is at the end.

use super::icons::{Glyph, icon};
use crate::i18n::{T, t};
use crate::scroll_follow::{Follow, Metrics, key_scrolls_up};
use leptos::html::Div;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

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
        this.listen();
        this
    }

    /// The reader's own moves that `on:wheel` does not see, caught before
    /// the scroll they cause (its event comes a frame later, after output
    /// may have scrolled the list back to the end): keys, a finger on the
    /// list, the scrollbar grabbed. On the window, so the touch listeners
    /// stay passive and keys count with focus on the page (the browser
    /// then scrolls the box last pressed in).
    fn listen(self) {
        use leptos::ev;
        // Whether the last press landed in the list; the finger's last y.
        let pressed_in = StoredValue::new(false);
        let finger_y = StoredValue::new(None::<i32>);
        let down = window_event_listener(ev::pointerdown, move |e| {
            let inside = self.inside(e.target());
            pressed_in.set_value(inside);
            if inside && self.on_scrollbar(&e) {
                self.hold(true);
            }
        });
        // A finger lets go on `touchend`: its pointer is cancelled as soon
        // as the browser scrolls, the finger still down.
        let up = window_event_listener(ev::pointerup, move |e| {
            if e.pointer_type() != "touch" {
                self.hold(false);
            }
        });
        let cancel = window_event_listener(ev::pointercancel, move |e| {
            if e.pointer_type() != "touch" {
                self.hold(false);
            }
        });
        let key = window_event_listener(ev::keydown, move |e| {
            if !key_scrolls_up(&e.key(), e.shift_key()) || is_editable(e.target()) {
                return;
            }
            let on_page = e
                .target()
                .is_none_or(|t| t.dyn_ref::<web_sys::HtmlBodyElement>().is_some());
            if self.inside(e.target()) || (on_page && pressed_in.get_value()) {
                self.reader_up(e.target());
            }
        });
        let start = window_event_listener(ev::touchstart, move |e| {
            if self.inside(e.target()) {
                finger_y.set_value(e.touches().get(0).map(|t| t.client_y()));
                self.hold(true);
            }
        });
        let moved = window_event_listener(ev::touchmove, move |e| {
            let (Some(last), Some(now)) = (
                finger_y.get_value(),
                e.touches().get(0).map(|t| t.client_y()),
            ) else {
                return;
            };
            finger_y.set_value(Some(now));
            // The finger going down drags the content down: a move up.
            if now > last {
                self.reader_up(e.target());
            }
        });
        let end = window_event_listener(ev::touchend, move |e| {
            if e.touches().length() == 0 && finger_y.get_value().is_some() {
                finger_y.set_value(None);
                self.hold(false);
            }
        });
        let touch_cancel = window_event_listener(ev::touchcancel, move |_| {
            if finger_y.get_value().is_some() {
                finger_y.set_value(None);
                self.hold(false);
            }
        });
        on_cleanup(move || {
            for h in [down, up, cancel, key, start, moved, end, touch_cancel] {
                h.remove();
            }
        });
    }

    /// Whether `target` is the list or inside it.
    fn inside(self, target: Option<web_sys::EventTarget>) -> bool {
        let (Some(list), Some(node)) = (
            self.list.get_untracked(),
            target.and_then(|t| t.dyn_into::<web_sys::Node>().ok()),
        ) else {
            return false;
        };
        list.contains(Some(&node))
    }

    /// Whether a press is on the list's scrollbar: inside its box, outside
    /// the part its content shows in (either side, so right to left too).
    fn on_scrollbar(self, e: &web_sys::PointerEvent) -> bool {
        let Some(el) = self.list.get_untracked() else {
            return false;
        };
        if e.target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .as_ref()
            != Some(&el)
        {
            return false;
        }
        let r = el.get_bounding_client_rect();
        let x = f64::from(e.client_x());
        let left = r.left() + f64::from(el.client_left());
        let right = left + f64::from(el.client_width());
        x >= r.left() && x < r.right() && (x < left || x >= right)
    }

    fn hold(self, held: bool) {
        let before = self.state.with_untracked(Follow::follows);
        self.state.update_untracked(|f| f.hold(held));
        // Let go at the end: catch up with output that came meanwhile.
        // `sync` first: a drag whose `scroll` event has not come yet is a
        // move up, not the end.
        if !before && self.sync() {
            self.to_end();
        }
    }

    /// A move up seen before its scroll, from a press on `target`. One a
    /// box inside the list takes (a run's log) leaves the list where it is.
    fn reader_up(self, target: Option<web_sys::EventTarget>) {
        let Some(el) = self.list.get_untracked() else {
            return;
        };
        if inner_box_goes_up(target, &el) {
            return;
        }
        let can_go_up = el.scroll_top() > 0;
        self.state.maybe_update(|f| {
            let before = f.show_jump();
            f.reader_up(can_go_up);
            before != f.show_jump()
        });
    }

    fn observe(self) {
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

    fn metrics(self) -> Option<Metrics> {
        let el = self.list.get_untracked()?;
        Some(Metrics {
            scroll_top: f64::from(el.scroll_top()),
            client_height: f64::from(el.client_height()),
            scroll_height: f64::from(el.scroll_height()),
        })
    }

    /// For the list's `on:scroll`.
    pub fn on_scroll(self) {
        let Some(m) = self.metrics() else {
            return;
        };
        self.state.maybe_update(|f| {
            let before = *f;
            f.scrolled(m);
            // `last_top` moves on every event; only a visible change
            // (the button) needs to notify.
            before.show_jump() != f.show_jump()
        });
    }

    /// Whether the list follows, counting a move up whose `scroll` event
    /// has not come yet ([`Follow::noticed`]): read before every scroll
    /// to the end.
    fn sync(self) -> bool {
        if let Some(m) = self.metrics() {
            self.state.maybe_update(|f| {
                let before = f.pinned();
                f.noticed(m);
                before != f.pinned()
            });
        }
        self.state.with_untracked(Follow::follows)
    }

    /// For the list's `on:wheel` (mouse wheel and trackpad). A pinch on a
    /// trackpad comes as a wheel with `ctrlKey` and scrolls nothing.
    pub fn on_wheel(self, ev: &web_sys::WheelEvent) {
        if ev.ctrl_key() || ev.delta_y() >= 0.0 {
            return;
        }
        self.reader_up(ev.target());
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
        // The frame can come after the chat pane went (the studio left
        // through the router): its list and state are disposed then.
        request_animation_frame(move || {
            if self.state.try_with_untracked(|_| ()).is_some() && self.sync() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    }

    fn jump(self) {
        self.state.update(Follow::jump);
        self.to_end();
    }
}

/// Whether a box between `target` and `list` scrolls up before the list
/// does: the browser moves the innermost box that can go that way.
fn inner_box_goes_up(target: Option<web_sys::EventTarget>, list: &web_sys::HtmlDivElement) -> bool {
    let mut node = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    while let Some(el) = node {
        if el.is_same_node(Some(list)) {
            return false;
        }
        if el.scroll_top() > 0 && el.scroll_height() > el.client_height() {
            return true;
        }
        node = el.parent_element();
    }
    false
}

/// Whether keys pressed in `target` type rather than scroll.
fn is_editable(target: Option<web_sys::EventTarget>) -> bool {
    target
        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
        .is_some_and(|el| {
            el.is_content_editable()
                || matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        })
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
