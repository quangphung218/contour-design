//! Help ▸ Documentation (track C2-chat-home): the user guide bundled in
//! the app (`crates/od-ui/docs/`), in the UI language. The original opens
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
        crate::markdown::to_html(match shell.locale.get() {
            Locale::Vi => GUIDE_VI,
            Locale::En => GUIDE_EN,
        })
    };
    view! {
        <section class="page docs-page" data-testid="docs-page">
            <header class="page-head">
                <div class="page-title">
                    <h1>{icon(Glyph::Book)}{t(T::Documentation)}</h1>
                </div>
                <button class="btn ghost small" data-testid="docs-report" on:click=move |_| super::quick_menu::request_report()>{icon(Glyph::Warning)}{t(T::ReportIssue)}</button>
            </header>
            <article class="docs-body md-preview card" inner_html=html></article>
        </section>
    }
}
