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
    let opener = SendWrapper::new(document().active_element());
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
    on_cleanup(move || {
        let opener = opener.take();
        after_render(move || {
            let doc = document();
            let lost = doc
                .active_element()
                .is_none_or(|a| doc.body().is_some_and(|b| *b == a));
            if let Some(o) = opener.filter(|o| lost && o.is_connected()) {
                if let Ok(o) = o.dyn_into::<web_sys::HtmlElement>() {
                    let _ = o.focus();
                }
            }
        });
    });
}

fn after_render(f: impl FnOnce() + 'static) {
    leptos::task::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        f();
    });
}
