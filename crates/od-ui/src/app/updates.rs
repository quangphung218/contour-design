//! Check for Updates: the desktop Help menu (`od:menu` `check-updates`) and
//! Settings ▸ About open this dialog, which asks the daemon to look at
//! GitHub Releases (`POST /api/updates/check`). Nothing checks unasked.

use super::api;
use super::icons::{Glyph, icon};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::Value;

/// Window event of [`request_check`].
const CHECK_EVENT: &str = "od:check-updates";

/// What the dialog shows.
#[derive(Debug, Clone, PartialEq)]
enum Check {
    Checking,
    Update {
        version: String,
        url: String,
        current: String,
    },
    Current(String),
    Failed,
}

/// Opens the dialog and checks, from anywhere in the app.
pub fn request_check() {
    if let (Some(w), Ok(ev)) = (web_sys::window(), web_sys::Event::new(CHECK_EVENT)) {
        let _ = w.dispatch_event(&ev);
    }
}

async fn check() -> Check {
    let Ok(v) = api::post::<_, Value>("/api/updates/check", &serde_json::json!({})).await else {
        return Check::Failed;
    };
    let current = v["current"].as_str().unwrap_or_default().to_owned();
    match (v["update"]["version"].as_str(), v["update"]["url"].as_str()) {
        (Some(version), Some(url)) => Check::Update {
            version: version.to_owned(),
            url: url.to_owned(),
            current,
        },
        _ => Check::Current(current),
    }
}

/// Mounted once in the shell; hidden until a check is asked for.
#[component]
pub fn UpdateDialog() -> impl IntoView {
    let state = RwSignal::new(None::<Check>);
    let seq = StoredValue::new(0u32);
    let run = move || {
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        state.set(Some(Check::Checking));
        leptos::task::spawn_local(async move {
            let found = check().await;
            if seq.get_value() == mine && state.get_untracked().is_some() {
                state.set(Some(found));
            }
        });
    };
    let menu = window_event_listener(
        leptos::ev::Custom::<web_sys::CustomEvent>::new("od:menu"),
        move |ev| {
            if ev.detail().as_string().as_deref() == Some("check-updates") {
                run();
            }
        },
    );
    let asked = window_event_listener(
        leptos::ev::Custom::<web_sys::Event>::new(CHECK_EVENT),
        move |_| run(),
    );
    // On the next task, as the shortcuts dialog does: the click that closes
    // must still reach its other listeners.
    let close = move || {
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            state.set(None);
        });
    };
    // Escape closes it wherever the focus is (it opens from a menu).
    let keys = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && state.with_untracked(Option::is_some) {
            close();
        }
    });
    on_cleanup(move || {
        menu.remove();
        asked.remove();
        keys.remove();
    });
    let body = move || {
        state.get().map(|s| match s {
            Check::Checking => view! {
                <div class="dialog-head">
                    <span class="dialog-icon"><span class="spinner"></span></span>
                    <div class="grow"><h2 id="update-title">{t(T::CheckingForUpdates)}</h2></div>
                </div>
            }
            .into_any(),
            Check::Update { version, url, current } => view! {
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Download)}</span>
                    <div class="grow">
                        <h2 id="update-title" data-testid="update-available">{tf(T::UpdateAvailable, &[&version])}</h2>
                        <p>{tf(T::UpdateYouHave, &[&current])}</p>
                    </div>
                </div>
                <div class="dialog-body">
                    <a href=url.clone() target="_blank" rel="noopener" data-testid="update-notes">
                        {t(T::ReleaseNotes)}{icon(Glyph::ExternalLink)}
                    </a>
                </div>
                <div class="dialog-foot">
                    <button class="btn" data-testid="update-close" on:click=move |_| close()>{t(T::Close)}</button>
                    <a class="btn primary" href=url target="_blank" rel="noopener" data-testid="update-download">
                        {icon(Glyph::Download)}{t(T::OpenDownloadPage)}
                    </a>
                </div>
            }
            .into_any(),
            Check::Current(current) => view! {
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Check)}</span>
                    <div class="grow">
                        <h2 id="update-title" data-testid="update-current">{t(T::UpToDate)}</h2>
                        <p>{tf(T::UpToDateHint, &[&current])}</p>
                    </div>
                </div>
                <div class="dialog-foot">
                    <button class="btn primary" data-testid="update-close" on:click=move |_| close()>{t(T::Close)}</button>
                </div>
            }
            .into_any(),
            Check::Failed => view! {
                <div class="dialog-head">
                    <span class="dialog-icon danger">{icon(Glyph::Warning)}</span>
                    <div class="grow">
                        <h2 id="update-title">{t(T::CheckForUpdates)}</h2>
                        <p role="alert" data-testid="update-failed">{t(T::UpdateCheckFailed)}</p>
                    </div>
                </div>
                <div class="dialog-foot">
                    <button class="btn" data-testid="update-close" on:click=move |_| close()>{t(T::Close)}</button>
                    <button class="btn primary" data-testid="update-retry" on:click=move |_| run()>{icon(Glyph::Refresh)}{t(T::TryAgain)}</button>
                </div>
            }
            .into_any(),
        })
    };
    view! {
        <Show when=move || state.with(Option::is_some)>
            <div class="overlay center" on:click=move |_| close()>
                <div
                    class="dialog update-dialog"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="update-title"
                    aria-busy=move || (state.get() == Some(Check::Checking)).to_string()
                    data-testid="update-dialog"
                    tabindex="-1"
                    on:click=|ev| ev.stop_propagation()
                >
                    {body}
                </div>
            </div>
        </Show>
    }
}
