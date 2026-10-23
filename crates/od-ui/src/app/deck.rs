//! Deck viewer (spec 1.2): navigation bar, thumbnail rail, present mode and
//! speaker notes, all driven through the preview bridge.

use super::api;
use super::icons::{Glyph, icon};
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use crate::preview_bridge::Slide;
use crate::speaker_notes;
use leptos::prelude::*;

/// Reads the deck source and extracts its speaker notes.
pub fn load_notes(ctx: StudioCtx, count: u32) {
    let Some(path) = ctx.active_untracked() else {
        return;
    };
    leptos::task::spawn_local(async move {
        let url = format!("{}/raw/{}", ctx.base(), api::file_path(&path));
        if let Ok(html) = api::get_text(&url).await {
            if ctx.active_untracked().as_deref() == Some(path.as_str()) {
                ctx.notes.set(speaker_notes::extract(&html, count as usize));
            }
        }
    });
}

#[component]
pub fn DeckBar(ctx: StudioCtx) -> impl IntoView {
    let button = move |cmd: Slide, label: T, glyph: Glyph, testid: &'static str| {
        view! {
            <button
                class="icon-btn"
                aria-label=t(label)
                data-tip=t(label)
                data-tip-pos="top"
                data-testid=testid
                on:click=move |_| ctx.slide(cmd)
            >
                {icon(glyph)}
            </button>
        }
    };
    move || {
        let deck = ctx.deck.get()?;
        let presenting = ctx.presenting.get();
        Some(view! {
            <div class="deck-bar" data-testid="deck-bar">
                {button(Slide::First, T::FirstSlide, Glyph::SkipFirst, "deck-first")}
                {button(Slide::Prev, T::PrevSlide, Glyph::ChevronLeft, "deck-prev")}
                <span class="deck-counter mono" data-testid="deck-counter">
                    {tf(T::SlideOf, &[&(deck.active + 1).to_string(), &deck.count.to_string()])}
                </span>
                {button(Slide::Next, T::NextSlide, Glyph::ChevronRight, "deck-next")}
                {button(Slide::Last, T::LastSlide, Glyph::SkipLast, "deck-last")}
                <span class="spacer"></span>
                <button
                    class="btn small ghost"
                    class:active=move || ctx.show_notes.get()
                    aria-pressed=move || ctx.show_notes.get().to_string()
                    data-testid="deck-notes"
                    on:click=move |_| ctx.show_notes.update(|s| *s = !*s)
                >
                    {icon(Glyph::Notes)}
                    {t(T::SpeakerNotes)}
                </button>
                {if presenting {
                    view! {
                        <button class="btn small" data-testid="deck-exit" on:click=move |_| ctx.presenting.set(false)>
                            {icon(Glyph::Close)}
                            {t(T::ExitPresent)}
                        </button>
                    }
                        .into_any()
                } else {
                    view! {
                        <button
                            class="btn small primary"
                            data-testid="deck-present"
                            on:click=move |_| {
                                ctx.set_comment_mode(false);
                                ctx.presenting.set(true);
                                // Keys go to the host, which drives the deck.
                                if let Some(el) = web_sys::window()
                                    .and_then(|w| w.document())
                                    .and_then(|d| d.active_element())
                                {
                                    use wasm_bindgen::JsCast;
                                    if let Ok(el) = el.dyn_into::<web_sys::HtmlElement>() {
                                        let _ = el.blur();
                                    }
                                }
                            }
                        >
                            {icon(Glyph::Present)}
                            {t(T::Present)}
                        </button>
                    }
                        .into_any()
                }}
            </div>
        })
    }
}

/// One small live copy of the deck per slide; each copy is moved to its
/// slide once its bridge reports the deck (see `StudioCtx::on_bridge`).
#[component]
pub fn ThumbRail(ctx: StudioCtx, src: Memo<Option<String>>) -> impl IntoView {
    move || {
        let deck = ctx.deck.get()?;
        if deck.count < 2 || ctx.presenting.get() || ctx.viewing.with(Option::is_some) {
            return None;
        }
        let url = src.get_untracked()?;
        Some(view! {
            <nav class="thumb-rail" aria-label=t(T::Slides) data-testid="thumb-rail">
                {(0..deck.count)
                    .map(|i| {
                        let label = tf(T::SlideN, &[&(i + 1).to_string()]);
                        view! {
                            <button
                                class="thumb"
                                class:active=move || ctx.deck.with(|d| d.is_some_and(|d| d.active == i))
                                title=label.clone()
                                aria-label=label
                                data-testid="deck-thumb"
                                on:click=move |_| ctx.slide(Slide::Go(i))
                            >
                                <iframe
                                    data-thumb=i.to_string()
                                    src=format!("{url}&thumb={i}")
                                    sandbox="allow-scripts"
                                    tabindex="-1"
                                    aria-hidden="true"
                                ></iframe>
                                <span class="thumb-n">{i + 1}</span>
                            </button>
                        }
                    })
                    .collect_view()}
            </nav>
        })
    }
}

#[component]
pub fn NotesPanel(ctx: StudioCtx) -> impl IntoView {
    move || {
        let deck = ctx.deck.get()?;
        if !ctx.show_notes.get() {
            return None;
        }
        let note = ctx
            .notes
            .with(|n| n.get(deck.active as usize).cloned())
            .filter(|n| !n.trim().is_empty());
        Some(view! {
            <aside class="notes-panel" data-testid="speaker-notes">
                <span class="notes-title">
                    {format!("{} · {}", t(T::SpeakerNotes), tf(T::SlideN, &[&(deck.active + 1).to_string()]))}
                </span>
                {match note {
                    Some(n) => view! { <p class="notes-text">{n}</p> }.into_any(),
                    None => view! { <p class="muted">{t(T::NoSpeakerNotes)}</p> }.into_any(),
                }}
            </aside>
        })
    }
}
