//! Check for Updates: the desktop Help menu (`od:menu` `check-updates`) and
//! Settings ▸ About open this dialog, which asks the daemon to look at
//! GitHub Releases (`POST /api/updates/check`). Nothing checks unasked.

use super::api;
use super::icons::{Glyph, icon};
use super::lib_ui::Modal;
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
    /// GitHub's limit for requests without an account is used up.
    RateLimited,
    Failed,
}

/// Opens the dialog and checks, from anywhere in the app.
pub fn request_check() {
    if let (Some(w), Ok(ev)) = (web_sys::window(), web_sys::Event::new(CHECK_EVENT)) {
        let _ = w.dispatch_event(&ev);
    }
}

async fn check() -> Check {
    let v = match api::post::<_, Value>("/api/updates/check", &serde_json::json!({})).await {
        Ok(v) => v,
        Err(e) if e.status == 429 => return Check::RateLimited,
        Err(_) => return Check::Failed,
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
    on_cleanup(move || {
        menu.remove();
        asked.remove();
    });
    view! {
        <Show when=move || state.with(Option::is_some)>
            <UpdateModal state=state retry=Callback::new(move |()| run())/>
        </Show>
    }
}

/// The open dialog. Each state is its own [`Modal`], so focus moves to that
/// state's main button; closing gives focus back to what had it before.
#[component]
fn UpdateModal(state: RwSignal<Option<Check>>, retry: Callback<()>) -> impl IntoView {
    super::focus::return_focus_on_cleanup();
    // On the next task, as the shortcuts dialog does: the click that closes
    // must still reach its other listeners.
    let close = Callback::new(move |()| {
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            state.set(None);
        });
    });
    move || {
        state.get().map(|s| match s {
            Check::Checking => view! {
                <Modal title=t(T::CheckForUpdates).to_owned() glyph=Glyph::Refresh on_close=close
                    testid="update-dialog" autofocus="[data-testid=update-close]">
                    <div class="dialog-body">
                        <p class="row" role="status" data-testid="update-checking"><span class="spinner"></span>{t(T::CheckingForUpdates)}</p>
                    </div>
                    <div class="dialog-foot">
                        <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                    </div>
                </Modal>
            }
            .into_any(),
            Check::Update { version, url, current } => view! {
                <Modal title=tf(T::UpdateAvailable, &[&version]) glyph=Glyph::Download on_close=close
                    testid="update-dialog" autofocus="[data-testid=update-view]">
                    <div class="dialog-body">
                        <p role="status" data-testid="update-available">{tf(T::UpdateYouHave, &[&current])}</p>
                    </div>
                    <div class="dialog-foot">
                        <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                        <a class="btn primary" href=url target="_blank" rel="noopener" data-testid="update-view">
                            {t(T::ViewRelease)}{icon(Glyph::ExternalLink)}
                        </a>
                    </div>
                </Modal>
            }
            .into_any(),
            Check::Current(current) => view! {
                <Modal title=t(T::UpToDate).to_owned() glyph=Glyph::Check on_close=close
                    testid="update-dialog" autofocus="[data-testid=update-close]">
                    <div class="dialog-body">
                        <p role="status" data-testid="update-current">{tf(T::UpToDateHint, &[&current])}</p>
                    </div>
                    <div class="dialog-foot">
                        <button class="btn primary" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                    </div>
                </Modal>
            }
            .into_any(),
            Check::RateLimited | Check::Failed => {
                let message = if s == Check::RateLimited { T::UpdateRateLimited } else { T::UpdateCheckFailed };
                view! {
                    <Modal title=t(T::CheckForUpdates).to_owned() glyph=Glyph::Warning on_close=close
                        testid="update-dialog" autofocus="[data-testid=update-retry]">
                        <div class="dialog-body">
                            <p role="alert" data-testid="update-failed">{t(message)}</p>
                        </div>
                        <div class="dialog-foot">
                            <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                            <button class="btn primary" data-testid="update-retry" on:click=move |_| retry.run(())>{icon(Glyph::Refresh)}{t(T::TryAgain)}</button>
                        </div>
                    </Modal>
                }
                .into_any()
            }
        })
    }
}
