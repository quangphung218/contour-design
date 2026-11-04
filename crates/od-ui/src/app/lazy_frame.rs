//! A small, scaled-down page that only loads once it scrolls into view:
//! project covers and template thumbnails. The frame runs no script
//! (`sandbox=""`) and cannot be focused or clicked. Its scripts are taken
//! out before it loads ([`still_doc`]): a sandbox that blocks them still
//! logs a console error for each one (UI-020).

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
    let doc = LocalResource::new(move || {
        let src = src.clone();
        async move {
            if !visible.get() {
                return None;
            }
            let html = super::api::get_text(&src).await.ok()?;
            let base = web_sys::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default();
            Some(still_doc(&html, &format!("{base}{src}")))
        }
    });
    view! {
        <span class="lazy-frame" node_ref=host>
            {move || doc.get().flatten().map(|doc| view! {
                <iframe
                    class="cover-frame"
                    sandbox=""
                    srcdoc=doc
                    tabindex="-1"
                    title=""
                ></iframe>
            })}
        </span>
    }
}

/// `html` without its `<script>` elements, with relative URLs resolved
/// against `base` (the page's own URL) unless it names a base itself.
#[must_use]
pub fn still_doc(html: &str, base: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find("<script") {
        let open = at + found;
        // `<scripts>` or `<script-x>` is another element.
        let next = lower.as_bytes().get(open + 7).copied();
        if !matches!(
            next,
            Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
        ) {
            out.push_str(&html[at..open + 7]);
            at = open + 7;
            continue;
        }
        out.push_str(&html[at..open]);
        at = match lower[open..].find("</script") {
            Some(close) => {
                let end = open + close;
                lower[end..].find('>').map_or(html.len(), |gt| end + gt + 1)
            }
            None => html.len(),
        };
    }
    out.push_str(&html[at..]);
    if out.to_ascii_lowercase().contains("<base") {
        return out;
    }
    let tag = format!(
        "<base href=\"{}\">",
        base.replace('&', "&amp;").replace('"', "&quot;")
    );
    let lower = out.to_ascii_lowercase();
    match lower
        .find("<head")
        .and_then(|h| lower[h..].find('>').map(|gt| h + gt + 1))
    {
        Some(at) => out.insert_str(at, &tag),
        None => out.insert_str(0, &tag),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::still_doc;

    const BASE: &str = "http://127.0.0.1:1/api/projects/p/raw/index.html";

    #[test]
    fn scripts_go_and_the_rest_stays() {
        let html = "<!doctype html><html><head><title>t</title><SCRIPT src=\"a.js\"></SCRIPT>\
                    <link rel=stylesheet href=s.css></head><body><h1>Hi</h1>\
                    <script type=module>if (a < b) go('</div>')</script><p>end</p></body></html>";
        let out = still_doc(html, BASE);
        assert!(!out.to_ascii_lowercase().contains("<script"), "{out}");
        assert!(!out.contains("go("), "{out}");
        assert!(
            out.contains("<h1>Hi</h1>") && out.contains("<p>end</p>"),
            "{out}"
        );
        assert!(
            out.contains(&format!("<head><base href=\"{BASE}\"><title>")),
            "{out}"
        );
    }

    #[test]
    fn look_alikes_and_unclosed_scripts() {
        let out = still_doc("<scripts>x</scripts><p>a</p><script>never closed", BASE);
        assert!(out.ends_with("<scripts>x</scripts><p>a</p>"), "{out}");
        assert!(out.starts_with("<base href="), "{out}");
    }

    #[test]
    fn an_own_base_is_kept() {
        let out = still_doc("<head><base href=\"/x/\"></head>", BASE);
        assert_eq!(out, "<head><base href=\"/x/\"></head>");
    }
}
