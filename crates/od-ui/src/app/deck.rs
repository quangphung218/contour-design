//! Deck viewer (spec 1.2): navigation bar, thumbnail rail, present mode and
//! speaker notes, all driven through the preview bridge.

use super::api;
use super::icons::{Glyph, icon};
use super::presenter::{DeckChannel, channel_name};
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use crate::preview_bridge::Slide;
use crate::speaker_notes;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

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
    // Built once per deck: only the counter and the present button follow
    // the slide and the mode (an open menu survives slide changes).
    let has_deck = Memo::new(move |_| ctx.deck.with(Option::is_some));
    move || {
        has_deck.get().then(|| view! {
            <div class="deck-bar" data-testid="deck-bar">
                {button(Slide::First, T::FirstSlide, Glyph::SkipFirst, "deck-first")}
                {button(Slide::Prev, T::PrevSlide, Glyph::ChevronLeft, "deck-prev")}
                <span class="deck-counter mono" data-testid="deck-counter">
                    {move || ctx.deck.get().map(|deck| tf(T::SlideOf, &[&(deck.active + 1).to_string(), &deck.count.to_string()]))}
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
                {move || if ctx.presenting.get() {
                    view! {
                        <button class="btn small" data-testid="deck-exit" on:click=move |_| exit_present(ctx)>
                            {icon(Glyph::Close)}
                            {t(T::ExitPresent)}
                        </button>
                    }
                        .into_any()
                } else {
                    view! { <PresentMenu ctx=ctx/> }.into_any()
                }}
            </div>
        })
    }
}

/// Whether slide `active` (of `count`) is news for the other window, and
/// what stays pending: nothing is sent while a slide the other window
/// asked for (`remote`) is on its way; reaching it (or it being out of
/// range) clears it, still without sending it back.
#[must_use]
pub(super) fn should_announce(remote: Option<u32>, active: u32, count: u32) -> (bool, Option<u32>) {
    match remote {
        Some(target) if target < count && target != active => (false, remote),
        Some(_) => (false, None),
        None => (true, None),
    }
}

/// Leaves present mode, and the browser's fullscreen with it.
fn exit_present(ctx: StudioCtx) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if doc.fullscreen_element().is_some() {
            doc.exit_fullscreen();
        }
    }
    ctx.presenting.set(false);
}

/// Starts present mode in the tab: keys go to the host, which drives the
/// deck, so the focused control lets go of them.
fn present_in_tab(ctx: StudioCtx) {
    ctx.set_comment_mode(false);
    ctx.presenting.set(true);
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
    {
        if let Ok(el) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = el.blur();
        }
    }
}

/// Present: in this tab, fullscreen, the deck alone in a new tab, or the
/// presenter view (notes, next slide, timer) in its own window.
#[component]
fn PresentMenu(ctx: StudioCtx) -> impl IntoView {
    let open = RwSignal::new(false);
    let pos = RwSignal::new(String::new());
    let fullscreen = move || {
        present_in_tab(ctx);
        let stage = web_sys::window().and_then(|w| w.document()).and_then(|d| {
            d.query_selector("[data-testid=preview-pane]")
                .ok()
                .flatten()
        });
        if let Some(el) = stage {
            let _ = el.request_fullscreen();
        }
    };
    let new_tab = move || {
        if let Some(path) = ctx.active_untracked() {
            super::files_panel::open_in_new_tab(ctx, &path);
        }
    };
    let presenter = move || {
        let Some(path) = ctx.active_untracked() else {
            return;
        };
        let slide = ctx.deck.get_untracked().map_or(0, |d| d.active);
        super::presenter::open_window(&ctx.project_id.get_value(), &path, slide);
        present_in_tab(ctx);
    };
    view! {
        <div class="menu-wrap split-btn">
            <button class="btn small primary" data-testid="deck-present" on:click=move |_| present_in_tab(ctx)>
                {icon(Glyph::Present)}
                {t(T::Present)}
            </button>
            <button class="btn small primary split-caret" aria-label=t(T::PresentOptions) aria-haspopup="menu"
                aria-expanded=move || open.get().to_string() data-testid="deck-present-menu"
                on:click=move |ev| { pos.set(super::files_panel::anchor_style(&ev, false)); open.update(|o| *o = !*o); }>
                {icon(Glyph::ChevronUp)}
            </button>
            <Show when=move || open.get()>
                <div class="menu-scrim" on:click=move |_| open.set(false)></div>
                <div class="menu" role="menu" data-testid="present-menu" style=move || pos.get()>
                    <button class="menu-item" role="menuitem" data-testid="present-tab" on:click=move |_| { open.set(false); present_in_tab(ctx); }>
                        {icon(Glyph::Present)}{t(T::PresentInTab)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="present-fullscreen" on:click=move |_| { open.set(false); fullscreen(); }>
                        {icon(Glyph::Fullscreen)}{t(T::PresentFullscreen)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="present-new-tab" on:click=move |_| { open.set(false); new_tab(); }>
                        {icon(Glyph::ExternalLink)}{t(T::PresentNewTab)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="present-presenter" on:click=move |_| { open.set(false); presenter(); }>
                        {icon(Glyph::Presenter)}{t(T::PresenterView)}
                    </button>
                </div>
            </Show>
        </div>
    }
}

/// Keeps the Studio deck and a presenter window on the same slide, and
/// ends present mode when the browser leaves fullscreen (Esc).
pub fn sync_deck(ctx: StudioCtx) {
    let channel = StoredValue::new(None::<SendWrapper<DeckChannel>>);
    // A slide asked for by the other window: its state is not sent back.
    let remote = StoredValue::new(None::<u32>);
    let key = Memo::new(move |_| {
        ctx.deck
            .with(Option::is_some)
            .then(|| ctx.active())
            .flatten()
    });
    Effect::new(move |_| {
        remote.set_value(None);
        channel.set_value(key.get().and_then(|path| {
            DeckChannel::open(
                &channel_name(&ctx.project_id.get_value(), &path),
                move |i| {
                    let shown = ctx.deck.try_get_untracked().flatten().map(|d| d.active);
                    if shown.is_some_and(|a| a != i) {
                        remote.set_value(Some(i));
                        ctx.slide(Slide::Go(i));
                    }
                },
            )
            .map(SendWrapper::new)
        }));
    });
    Effect::new(move |_| {
        let Some(d) = ctx.deck.get() else {
            return;
        };
        let (send, pending) = should_announce(remote.get_value(), d.active, d.count);
        remote.set_value(pending);
        if !send {
            return;
        }
        channel.with_value(|c| {
            if let Some(c) = c {
                c.announce(d.active);
            }
        });
    });
    // `fullscreenchange` fires on the document, not the window.
    let on_change = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
        let full = web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| d.fullscreen_element().is_some());
        if !full {
            ctx.presenting.set(false);
        }
    });
    let doc = web_sys::window().and_then(|w| w.document());
    if let Some(doc) = &doc {
        let _ = doc.add_event_listener_with_callback(
            "fullscreenchange",
            on_change.as_ref().unchecked_ref(),
        );
    }
    let on_change = SendWrapper::new(on_change);
    on_cleanup(move || {
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc.remove_event_listener_with_callback(
                "fullscreenchange",
                on_change.as_ref().unchecked_ref(),
            );
        }
        channel.set_value(None);
    });
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
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    // Another slide or file: leave the editor.
    Effect::new(move |_| {
        ctx.deck.with(|d| d.map(|d| d.active));
        ctx.active();
        editing.set(false);
    });
    let save = move || {
        let (Some(path), Some(deck)) = (ctx.active_untracked(), ctx.deck.get_untracked()) else {
            return;
        };
        let note = draft.get_untracked();
        saving.set(true);
        leptos::task::spawn_local(async move {
            let res = match api::get_text(&format!("{}/raw/{}", ctx.base(), api::file_path(&path)))
                .await
            {
                Ok(html) => {
                    let next = speaker_notes::set_note(
                        &html,
                        deck.active as usize,
                        &note,
                        deck.count as usize,
                    );
                    super::files_panel::replace_content(ctx, &path, &next, t(T::SpeakerNotes)).await
                }
                Err(e) => Err(e.to_string()),
            };
            saving.set(false);
            match res {
                Ok(_) => {
                    ctx.notes.update(|n| {
                        let i = deck.active as usize;
                        if n.len() <= i {
                            n.resize(i + 1, String::new());
                        }
                        n[i] = note.trim().to_owned();
                    });
                    editing.set(false);
                }
                Err(e) => ctx.notice.set(Some(tf(T::NotesSaveFailed, &[&e]))),
            }
        });
    };
    move || {
        let deck = ctx.deck.get()?;
        if !ctx.show_notes.get() {
            return None;
        }
        let note = ctx
            .notes
            .with(|n| n.get(deck.active as usize).cloned())
            .filter(|n| !n.trim().is_empty());
        let current = note.clone().unwrap_or_default();
        Some(view! {
            <aside class="notes-panel" data-testid="speaker-notes">
                <div class="notes-head">
                    <span class="notes-title">
                        {format!("{} · {}", t(T::SpeakerNotes), tf(T::SlideN, &[&(deck.active + 1).to_string()]))}
                    </span>
                    <span class="spacer"></span>
                    {move || (!editing.get() && !ctx.presenting.get()).then(|| {
                        let current = current.clone();
                        view! {
                            <button class="btn small ghost" data-testid="notes-edit" on:click=move |_| { draft.set(current.clone()); editing.set(true); }>
                                {icon(Glyph::Rename)}{t(T::EditNotes)}
                            </button>
                        }
                    })}
                </div>
                {move || if editing.get() {
                    view! {
                        <textarea class="input notes-input" rows="3" data-testid="notes-input"
                            placeholder=t(T::NotesPlaceholder)
                            prop:value=move || draft.get()
                            on:input=move |ev| draft.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if (ev.meta_key() || ev.ctrl_key()) && ev.key() == "Enter" {
                                    ev.prevent_default();
                                    save();
                                } else if ev.key() == "Escape" {
                                    ev.stop_propagation();
                                    editing.set(false);
                                }
                            }></textarea>
                        <div class="row end tight">
                            <button class="btn small ghost" on:click=move |_| editing.set(false)>{t(T::Cancel)}</button>
                            <button class="btn small primary" data-testid="notes-save" disabled=move || saving.get() on:click=move |_| save()>
                                {move || saving.get().then(|| view! { <span class="spinner on-accent"></span> })}
                                {t(T::SaveNotes)}
                            </button>
                        </div>
                    }.into_any()
                } else {
                    match note.clone() {
                        Some(n) => view! { <p class="notes-text" data-testid="notes-text">{n}</p> }.into_any(),
                        None => view! { <p class="muted">{t(T::NoSpeakerNotes)}</p> }.into_any(),
                    }
                }}
            </aside>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::should_announce;

    #[test]
    fn a_slide_asked_for_is_not_echoed() {
        // The user moved: tell the other window.
        assert_eq!(should_announce(None, 2, 3), (true, None));
        // On the way to slide 2 (the bridge reports 0 first): wait.
        assert_eq!(should_announce(Some(2), 0, 3), (false, Some(2)));
        // Arrived: nothing to send back, and the next move is news again.
        assert_eq!(should_announce(Some(2), 2, 3), (false, None));
        // A target the deck no longer has is dropped.
        assert_eq!(should_announce(Some(5), 0, 3), (false, None));
    }
}
