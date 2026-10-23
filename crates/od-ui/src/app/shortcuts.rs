//! Keyboard shortcuts help (`?`, or from the command palette).

use super::Shell;
use super::icons::{Glyph, icon};
use crate::i18n::{T, t};
use leptos::prelude::*;

/// "⌘" on macOS, else "Ctrl+".
pub fn modifier() -> &'static str {
    let mac = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Mac OS"));
    if mac { "⌘" } else { "Ctrl+" }
}

/// Groups of (keys, what they do); `{m}` is the platform modifier.
const GROUPS: [(T, &[(&str, T)]); 4] = [
    (
        T::ShortcutsGeneral,
        &[
            ("{m}K", T::CommandPalette),
            ("?", T::KeyboardShortcuts),
            ("Esc", T::ShortcutEsc),
        ],
    ),
    (
        T::ShortcutsStudio,
        &[
            ("{m}P", T::QuickOpen),
            ("{m}B", T::ToggleFilesPanel),
            ("{m}W", T::CloseTab),
            ("{m}1…9", T::ShortcutPickTab),
            ("Ctrl+Tab", T::ShortcutCycleTabs),
            ("{m}= / {m}- / {m}0", T::ShortcutZoom),
        ],
    ),
    (
        T::ShortcutsChat,
        &[("Enter", T::Send), ("Shift+Enter", T::ShortcutNewLine)],
    ),
    (
        T::ShortcutsDeck,
        &[
            ("→ / Space", T::ShortcutNextSlide),
            ("←", T::ShortcutPrevSlide),
            ("Home / End", T::ShortcutFirstLast),
        ],
    ),
];

#[component]
pub fn ShortcutsDialog() -> impl IntoView {
    let shell = Shell::get();
    let m = modifier();
    // Closed on the next task: closing inside a click would drop the
    // listeners the same click still reaches on its way up (microtasks run
    // between listeners, so a spawned future is not late enough).
    let close = move || {
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            shell.shortcuts.set(false);
        });
    };
    view! {
        <div class="overlay center" on:click=move |_| close()>
            <div
                class="dialog shortcuts"
                role="dialog"
                aria-modal="true"
                aria-labelledby="shortcuts-title"
                data-testid="shortcuts"
                tabindex="-1"
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        close();
                    }
                }
            >
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Keyboard)}</span>
                    <h2 id="shortcuts-title">{t(T::KeyboardShortcuts)}</h2>
                    <span class="spacer"></span>
                    <button class="icon-btn" aria-label=t(T::Close) data-testid="shortcuts-close" on:click=move |_| close()>
                        {icon(Glyph::Close)}
                    </button>
                </div>
                <div class="dialog-body shortcut-groups">
                    {GROUPS
                        .iter()
                        .map(|(title, rows)| view! {
                            <section class="shortcut-group">
                                <h3 class="eyebrow">{t(*title)}</h3>
                                <dl>
                                    {rows
                                        .iter()
                                        .map(|(keys, label)| view! {
                                            <div class="shortcut-row">
                                                <dt>{t(*label)}</dt>
                                                <dd><kbd>{keys.replace("{m}", m)}</kbd></dd>
                                            </div>
                                        })
                                        .collect_view()}
                                </dl>
                            </section>
                        })
                        .collect_view()}
                </div>
            </div>
        </div>
    }
}
