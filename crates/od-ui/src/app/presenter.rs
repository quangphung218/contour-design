//! Presenter view (Studio deck ▸ Present ▸ Presenter view): a window with
//! the current slide, the next one, the speaker notes and a timer, kept in
//! step with the Studio over a `BroadcastChannel` (either side moves both).
//! Route `/present/:id?file=…&slide=…`.

use super::api;
use super::bridge::{self, Source};
use super::icons::{Glyph, icon};
use crate::i18n::{T, t, tf};
use crate::preview_bridge::{self as pb, BridgeMsg, Slide};
use crate::speaker_notes;
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

/// Channel name shared by a Studio deck and its presenter window.
#[must_use]
pub fn channel_name(project: &str, file: &str) -> String {
    format!("od-deck:{project}:{file}")
}

/// A slide-sync channel: `on_slide` runs for each slide index the other
/// side announces. Closed when dropped.
pub struct DeckChannel {
    channel: web_sys::BroadcastChannel,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl DeckChannel {
    pub fn open(name: &str, mut on_slide: impl FnMut(u32) + 'static) -> Option<Self> {
        let channel = web_sys::BroadcastChannel::new(name).ok()?;
        let on_message =
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                if let Some(i) = js_sys::Reflect::get(&ev.data(), &"slide".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                {
                    on_slide(i as u32);
                }
            });
        channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Some(Self {
            channel,
            _on_message: on_message,
        })
    }

    pub fn announce(&self, slide: u32) {
        let msg = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&msg, &"slide".into(), &f64::from(slide).into());
        let _ = self.channel.post_message(&msg);
    }
}

impl Drop for DeckChannel {
    fn drop(&mut self) {
        self.channel.set_onmessage(None);
        self.channel.close();
    }
}

/// Opens the presenter window for `file` of `project` at `slide`.
pub fn open_window(project: &str, file: &str, slide: u32) {
    let url = format!(
        "/present/{}?file={}&slide={slide}",
        api::seg(project),
        api::seg(file)
    );
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target_and_features(
            &url,
            &format!("od-presenter-{project}"),
            "popup,width=1180,height=760",
        );
    }
}

/// `m:ss` of `secs`.
#[must_use]
pub fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[component]
pub fn Presenter() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let project = params.with_untracked(|p| p.get("id").unwrap_or_default());
    let file = query.with_untracked(|q| q.get("file").unwrap_or_default());
    let start_slide: u32 =
        query.with_untracked(|q| q.get("slide").and_then(|s| s.parse().ok()).unwrap_or(0));
    let active = RwSignal::new(start_slide);
    let count = RwSignal::new(0u32);
    let notes = RwSignal::new(Vec::<String>::new());
    let started = RwSignal::new(js_sys::Date::now());
    let now = RwSignal::new(js_sys::Date::now());
    let raw = format!(
        "/api/projects/{}/raw/{}",
        api::seg(&project),
        api::file_path(&file)
    );
    let src = format!("{raw}?bridge=1&v=presenter");

    // The shell (sidebar, title strip) is hidden in this window.
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-presenter", "");
    }
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        doc.set_title(&tf(
            T::PresenterTitle,
            &[file.rsplit('/').next().unwrap_or(&file)],
        ));
    }

    // A slide asked for by the Studio (or the start slide): its state is
    // not sent back, so the two windows never echo each other.
    let remote = StoredValue::new(Some(start_slide));
    let channel = StoredValue::new(
        DeckChannel::open(&channel_name(&project, &file), move |i| {
            if active.try_get_untracked().is_some_and(|a| a != i) {
                remote.set_value(Some(i));
                bridge::post_main(&Slide::Go(i).message());
            }
        })
        .map(SendWrapper::new),
    );
    let announce = move |i: u32, n: u32| {
        let (send, pending) = super::deck::should_announce(remote.get_value(), i, n);
        remote.set_value(pending);
        if !send {
            return;
        }
        channel.with_value(|c| {
            if let Some(c) = c {
                c.announce(i);
            }
        });
    };

    {
        let raw = raw.clone();
        leptos::task::spawn_local(async move {
            if let Ok(html) = api::get_text(&raw).await {
                let n = count.get_untracked().max(1) as usize;
                notes.set(speaker_notes::extract(&html, n));
                // Re-read with the slide count once the deck reports it.
                SOURCE.with(|s| *s.borrow_mut() = Some(html));
            }
        });
    }
    let ticker = leptos::prelude::set_interval_with_handle(
        move || now.set(js_sys::Date::now()),
        std::time::Duration::from_secs(1),
    )
    .ok();

    let messages = window_event_listener(leptos::ev::message, move |ev| {
        let Some((source, msg)) = bridge::decode(&ev) else {
            return;
        };
        match (source, msg) {
            (Source::Main, BridgeMsg::DeckReady { count: n }) => {
                count.set(n);
                SOURCE.with(|s| {
                    if let Some(html) = s.borrow().as_ref() {
                        notes.set(speaker_notes::extract(html, n as usize));
                    }
                });
                let i = active.get_untracked().min(n.saturating_sub(1));
                bridge::post_main(&Slide::Go(i).message());
                bridge::post_main(&pb::slide_state_request());
            }
            (
                Source::Main,
                BridgeMsg::SlideState {
                    active: i,
                    count: n,
                },
            ) => {
                count.set(n);
                if active.get_untracked() != i {
                    active.set(i);
                }
                announce(i, n);
                bridge::post_thumb(i + 1, &Slide::Go(i + 1).message());
            }
            (Source::Thumb(i), BridgeMsg::DeckReady { .. }) => {
                bridge::post_thumb(i, &Slide::Go(i).message());
            }
            _ => {}
        }
    });
    let reset = move || {
        started.set(js_sys::Date::now());
        bridge::post_main(&Slide::First.message());
    };
    let keys = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
            return;
        }
        let cmd = match ev.key().as_str() {
            "ArrowRight" | "PageDown" | " " => Some(Slide::Next),
            "ArrowLeft" | "PageUp" => Some(Slide::Prev),
            "Home" => Some(Slide::First),
            "End" => Some(Slide::Last),
            "r" | "R" => {
                reset();
                None
            }
            "Escape" => {
                if let Some(w) = web_sys::window() {
                    let _ = w.close();
                }
                None
            }
            _ => return,
        };
        ev.prevent_default();
        if let Some(cmd) = cmd {
            bridge::post_main(&cmd.message());
        }
    });
    on_cleanup(move || {
        messages.remove();
        keys.remove();
        if let Some(t) = ticker {
            t.clear();
        }
        channel.set_value(None);
    });

    let elapsed = move || clock(((now.get() - started.get()) / 1000.0).max(0.0) as u64);
    let note = move || {
        notes
            .with(|n| n.get(active.get() as usize).cloned())
            .filter(|n| !n.trim().is_empty())
    };
    let next_src = format!("{src}&thumb=next");
    view! {
        <div class="presenter" data-testid="presenter">
            <header class="presenter-head">
                <span class="pane-title">{file.clone()}</span>
                <span class="spacer"></span>
                <span class="presenter-timer mono" data-testid="presenter-timer" aria-label=t(T::Elapsed)>{elapsed}</span>
                <button class="btn small" data-testid="presenter-reset" on:click=move |_| reset()>{icon(Glyph::Reset)}{t(T::PresenterReset)}</button>
                <span class="deck-counter mono" data-testid="presenter-counter">
                    {move || tf(T::SlideOf, &[&(active.get() + 1).to_string(), &count.get().max(1).to_string()])}
                </span>
            </header>
            <div class="presenter-body">
                <div class="presenter-current">
                    <iframe class="preview-frame" data-testid="preview" title=t(T::CurrentSlide)
                        sandbox="allow-scripts allow-forms allow-popups allow-modals" src=src.clone()></iframe>
                </div>
                <aside class="presenter-side">
                    <span class="eyebrow">{t(T::NextSlide)}</span>
                    <div class="presenter-next">
                        {move || (active.get() + 1 < count.get()).then(|| {
                            let i = active.get_untracked() + 1;
                            view! {
                                <iframe data-thumb=i.to_string() src=format!("{next_src}{i}") sandbox="allow-scripts" tabindex="-1" aria-hidden="true"></iframe>
                            }
                        })}
                    </div>
                    <span class="eyebrow">{t(T::SpeakerNotes)}</span>
                    <div class="presenter-notes" data-testid="presenter-notes">
                        {move || match note() {
                            Some(n) => view! { <p class="notes-text">{n}</p> }.into_any(),
                            None => view! { <p class="muted">{t(T::NoSpeakerNotes)}</p> }.into_any(),
                        }}
                    </div>
                    <div class="presenter-nav">
                        <button class="btn" data-testid="presenter-prev" on:click=move |_| bridge::post_main(&Slide::Prev.message())>{icon(Glyph::ChevronLeft)}{t(T::PrevSlide)}</button>
                        <button class="btn primary" data-testid="presenter-next" on:click=move |_| bridge::post_main(&Slide::Next.message())>{t(T::NextSlide)}{icon(Glyph::ChevronRight)}</button>
                    </div>
                </aside>
            </div>
        </div>
    }
}

thread_local! {
    /// The deck source, kept to re-read notes once the slide count is known.
    static SOURCE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_channel() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(75), "1:15");
        assert_eq!(clock(3605), "60:05");
        assert_eq!(channel_name("p1", "deck.html"), "od-deck:p1:deck.html");
    }
}
