//! A small, scaled-down page that only loads once it scrolls into view:
//! project covers and template thumbnails. The frame runs no script
//! (`sandbox=""`) and cannot be focused or clicked.

use leptos::prelude::*;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{IntersectionObserver, IntersectionObserverEntry, IntersectionObserverInit};

type ViewCallback = Closure<dyn FnMut(js_sys::Array, IntersectionObserver)>;

/// Starts loading this far before the frame enters the viewport.
const MARGIN: &str = "200px";

#[component]
pub fn LazyFrame(src: String) -> impl IntoView {
    let host = NodeRef::<leptos::html::Span>::new();
    let visible = RwSignal::new(false);
    // The observer and its callback, kept until the frame unmounts.
    let observer = StoredValue::new(None::<SendWrapper<(IntersectionObserver, ViewCallback)>>);
    Effect::new(move |_| {
        let Some(el) = host.get() else {
            return;
        };
        let on_view =
            ViewCallback::new(move |entries: js_sys::Array, obs: IntersectionObserver| {
                let seen = entries.iter().any(|e| {
                    e.dyn_into::<IntersectionObserverEntry>()
                        .is_ok_and(|e| e.is_intersecting())
                });
                if seen {
                    obs.disconnect();
                    visible.set(true);
                }
            });
        let init = IntersectionObserverInit::new();
        init.set_root_margin(MARGIN);
        match IntersectionObserver::new_with_options(on_view.as_ref().unchecked_ref(), &init) {
            Ok(obs) => {
                obs.observe(&el);
                observer.set_value(Some(SendWrapper::new((obs, on_view))));
            }
            // No IntersectionObserver: load right away.
            Err(_) => visible.set(true),
        }
    });
    on_cleanup(move || {
        observer.with_value(|o| {
            if let Some(o) = o {
                o.0.disconnect();
            }
        });
    });
    view! {
        <span class="lazy-frame" node_ref=host>
            {move || visible.get().then(|| view! {
                <iframe
                    class="cover-frame"
                    sandbox=""
                    src=src.clone()
                    tabindex="-1"
                    title=""
                ></iframe>
            })}
        </span>
    }
}
