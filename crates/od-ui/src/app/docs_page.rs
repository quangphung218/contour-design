//! Help ▸ Documentation (track C2-chat-home): the user guide bundled in
//! the app (`crates/od-ui/docs/`), in the UI language (English for the
//! languages it has not been written in). The original opens
//! its README on GitHub; this app stays off the network.

use super::icons::{Glyph, icon};
use crate::i18n::{Locale, T, t};
use leptos::prelude::*;

const GUIDE_EN: &str = include_str!("../../docs/guide.en.md");
const GUIDE_VI: &str = include_str!("../../docs/guide.vi.md");

#[component]
pub fn DocsPage() -> impl IntoView {
    let shell = super::Shell::get();
    let html = move || {
        crate::markdown::guide_html(match shell.locale.get() {
            Locale::Vi => GUIDE_VI,
            // The guide is written in English and Vietnamese only.
            _ => GUIDE_EN,
        })
    };
    // A link to a section (`/docs#deepseek-harness`) opens at it.
    let body = NodeRef::<leptos::html::Article>::new();
    Effect::new(move |_| {
        let Some(el) = body.get() else { return };
        let hash = web_sys::window()
            .and_then(|w| w.location().hash().ok())
            .unwrap_or_default();
        if let Some(target) = hash
            .strip_prefix('#')
            .filter(|id| !id.is_empty())
            .and_then(|id| el.query_selector(&format!("[id=\"{id}\"]")).ok().flatten())
        {
            target.scroll_into_view();
        }
    });
    view! {
        <section class="page docs-page" data-testid="docs-page">
            <header class="page-head">
                <div class="page-title">
                    <h1>{icon(Glyph::Book)}{t(T::Documentation)}</h1>
                </div>
                <button class="btn ghost small" data-testid="docs-report" on:click=move |_| super::quick_menu::request_report()>{icon(Glyph::Warning)}{t(T::ReportIssue)}</button>
            </header>
            <article class="docs-body md-preview card" node_ref=body inner_html=html></article>
        </section>
    }
}
