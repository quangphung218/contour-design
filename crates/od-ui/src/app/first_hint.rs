//! One-time hint when the first artifact of a generation shows up in the
//! Studio (upstream `FirstArtifactHint`): a small card in the preview's top
//! corner pointing at the preview tools. It shows only for a project whose
//! first run started with nothing to preview, waits for the run to end and
//! the layout to settle, and the "seen" bit is spent when the user closes
//! it, not when it shows (a remount before that shows it again). Any click
//! outside the card closes it too, so it never sits over a menu the user
//! just opened or over the page they are working on. Sending the next
//! message closes it as well.

use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, is_previewable};
use super::{store_flag, stored_flag};
use crate::i18n::{T, t};
use leptos::ev;
use leptos::html::Div;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// `localStorage` key of the "seen" bit (once per browser, ever).
const SEEN_KEY: &str = "od:seen-first-artifact-hint";
/// Wait after the artifact appears, so the card lands on a settled layout.
const SETTLE_MS: u32 = 600;

/// Whether the card shows: not dismissed before, armed by a run that
/// started with nothing to preview, that run over, and an artifact there.
fn should_show(seen: bool, armed: bool, running: bool, has_artifact: bool) -> bool {
    !seen && armed && !running && has_artifact
}

#[component]
pub fn FirstArtifactHint(ctx: StudioCtx) -> impl IntoView {
    let seen = RwSignal::new(stored_flag(SEEN_KEY).unwrap_or(false));
    // Armed when a run starts while the project has nothing to preview
    // (a returning user opening a finished project never sees it).
    let armed = RwSignal::new(false);
    let settled = RwSignal::new(false);
    let has_artifact = move || ctx.files.with(|f| f.iter().any(is_previewable));
    Effect::new(move |_| {
        if ctx.live_run.with(Option::is_some) && !has_artifact() {
            armed.set(true);
        }
    });
    let ready = Memo::new(move |_| {
        should_show(
            seen.get(),
            armed.get(),
            ctx.live_run.with(Option::is_some),
            has_artifact(),
        )
    });
    Effect::new(move |_| {
        if ready.get() && !settled.get_untracked() {
            leptos::task::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(SETTLE_MS).await;
                let _ = settled.try_set(true);
            });
        }
    });
    let close = move || {
        store_flag(SEEN_KEY, true);
        seen.set(true);
    };
    let dismiss = move |_| close();
    // Seen and left open: the next message the user sends retires it, so
    // it does not sit over the preview turn after turn.
    Effect::new(move |_| {
        if ctx.live_run.with(Option::is_some) && settled.get_untracked() && !seen.get_untracked() {
            close();
        }
    });
    let card = NodeRef::<Div>::new();
    let down = window_event_listener(ev::mousedown, move |e| {
        if !(ready.get_untracked() && settled.get_untracked()) {
            return;
        }
        let inside = card.get_untracked().is_some_and(|c| {
            e.target()
                .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                .is_some_and(|n| c.contains(Some(&n)))
        });
        if !inside {
            close();
        }
    });
    on_cleanup(move || down.remove());
    move || {
        (ready.get() && settled.get()).then(|| {
            view! {
                <div class="first-artifact-hint" role="status" data-testid="first-artifact-hint" node_ref=card>
                    <span class="first-artifact-hint-ico">{icon(Glyph::Sparkle)}</span>
                    <div class="first-artifact-hint-body">
                        <strong>{t(T::FirstArtifactTitle)}</strong>
                        <span>{t(T::FirstArtifactBody)}</span>
                    </div>
                    <button
                        class="icon-btn small"
                        aria-label=t(T::FirstArtifactDismiss)
                        data-tip=t(T::FirstArtifactDismiss)
                        data-testid="first-artifact-hint-dismiss"
                        on:click=dismiss
                    >
                        {icon(Glyph::Close)}
                    </button>
                </div>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::should_show;

    #[test]
    fn shows_once_after_the_first_generation() {
        assert!(should_show(false, true, false, true));
        // Dismissed before, still running, nothing yet, or not armed.
        assert!(!should_show(true, true, false, true));
        assert!(!should_show(false, true, true, true));
        assert!(!should_show(false, true, false, false));
        assert!(!should_show(false, false, false, true));
    }
}
