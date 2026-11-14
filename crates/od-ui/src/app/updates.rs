//! Check for Updates: the desktop Help menu (`od:menu` `check-updates`) and
//! Settings ▸ About open this dialog, which asks the daemon to look at
//! GitHub Releases (`POST /api/updates/check`). Nothing checks unasked.
//!
//! In the desktop app a newer release offers Update now: the shell
//! downloads it, checks its signature, installs it (`desktop::install_update`,
//! progress as `od:update-progress` events) and the dialog offers Restart.
//! Nothing downloads unasked either. A copy that cannot replace itself (a
//! `.deb`, a dev build) and the browser keep View release.

use super::api;
use super::desktop;
use super::icons::{Glyph, icon};
use super::lib_ui::Modal;
use super::tasks::Tasks;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::Value;

/// Window event of [`request_check`].
const CHECK_EVENT: &str = "od:check-updates";
/// Window event the shell sends while Update now runs.
const PROGRESS_EVENT: &str = "od:update-progress";

/// Whether this copy can install the update itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Install {
    /// Update now.
    Ready,
    /// A dev build: update from source.
    DevBuild,
    /// A package manager's copy (`.deb`): the release page.
    Package,
    /// The browser (`od` daemon): the release page, no hint.
    Browser,
}

impl Install {
    fn from_shell(support: Option<&str>) -> Self {
        match support {
            Some("ready") => Self::Ready,
            Some("devBuild") => Self::DevBuild,
            Some(_) => Self::Package,
            None => Self::Browser,
        }
    }

    fn hint(self) -> Option<T> {
        match self {
            Self::DevBuild => Some(T::UpdateDevBuild),
            Self::Package => Some(T::UpdatePackage),
            Self::Ready | Self::Browser => None,
        }
    }
}

/// Why Update now stopped (the shell's failure `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stopped {
    Network,
    Signature,
    NoBuild,
    Other,
}

impl Stopped {
    fn from_kind(kind: &str) -> Self {
        match kind {
            "network" => Self::Network,
            "signature" => Self::Signature,
            "noBuild" | "unsupported" => Self::NoBuild,
            _ => Self::Other,
        }
    }

    fn message(self) -> T {
        match self {
            Self::Network => T::UpdateNetworkFailed,
            Self::Signature => T::UpdateSignatureFailed,
            Self::NoBuild => T::UpdateNoBuild,
            Self::Other => T::UpdateInstallFailed,
        }
    }

    /// Trying again can help only when the network was the trouble.
    fn retry(self) -> bool {
        self == Self::Network
    }
}

/// What the dialog shows.
#[derive(Debug, Clone, PartialEq)]
enum Check {
    Checking,
    Update {
        version: String,
        url: String,
        current: String,
        install: Install,
    },
    /// Update now is downloading or installing; [`Phase`] says which.
    Installing {
        version: String,
    },
    /// Installed: Restart opens it.
    Installed {
        version: String,
    },
    InstallFailed {
        version: String,
        url: String,
        why: Stopped,
    },
    Current(String),
    /// GitHub's limit for requests without an account is used up.
    RateLimited,
    Failed,
}

/// Where Update now is, from the shell's progress events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Starting,
    Percent(u8),
    /// The size is unknown: bytes so far.
    Bytes(u64),
    Installing,
}

impl Phase {
    fn from_detail(detail: &Value) -> Option<Self> {
        match detail["phase"].as_str()? {
            "installing" => Some(Self::Installing),
            "downloading" => {
                let done = detail["downloaded"].as_u64()?;
                Some(match detail["total"].as_u64().filter(|t| *t > 0) {
                    Some(total) => {
                        Self::Percent(u8::try_from(done.min(total) * 100 / total).unwrap_or(100))
                    }
                    None => Self::Bytes(done),
                })
            }
            _ => None,
        }
    }
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
            install: Install::from_shell(desktop::update_support().await.as_deref()),
        },
        _ => Check::Current(current),
    }
}

/// Mounted once in the shell; hidden until a check is asked for.
#[component]
pub fn UpdateDialog() -> impl IntoView {
    let state = RwSignal::new(None::<Check>);
    let phase = RwSignal::new(Phase::Starting);
    let tasks = Tasks::here();
    let seq = StoredValue::new(0u32);
    let busy = move || matches!(state.get_untracked(), Some(Check::Installing { .. }));
    let run = move || {
        // A check never replaces a running update.
        if busy() {
            return;
        }
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        state.set(Some(Check::Checking));
        tasks.spawn(async move {
            let found = check().await;
            if seq.get_value() == mine && state.get_untracked().is_some() {
                state.set(Some(found));
            }
        });
    };
    let install = Callback::new(move |(version, url): (String, String)| {
        if busy() {
            return;
        }
        seq.update_value(|n| *n += 1);
        phase.set(Phase::Starting);
        state.set(Some(Check::Installing {
            version: version.clone(),
        }));
        let to = version.clone();
        let work = async move { desktop::install_update(&to).await };
        tasks.spawn_then(work, move |res| {
            let next = match res {
                Ok(()) => Some(Check::Installed { version }),
                Err(kind) if kind == "cancelled" => None,
                Err(kind) => Some(Check::InstallFailed {
                    version,
                    url,
                    why: Stopped::from_kind(&kind),
                }),
            };
            state.set(next);
        });
    });
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
    let progress = window_event_listener(
        leptos::ev::Custom::<web_sys::CustomEvent>::new(PROGRESS_EVENT),
        move |ev| {
            let detail: Option<Value> = js_sys::JSON::stringify(&ev.detail())
                .ok()
                .and_then(|s| s.as_string())
                .and_then(|s| serde_json::from_str(&s).ok());
            if let Some(p) = detail.as_ref().and_then(Phase::from_detail) {
                phase.set(p);
            }
        },
    );
    on_cleanup(move || {
        menu.remove();
        asked.remove();
        progress.remove();
    });
    view! {
        <Show when=move || state.with(Option::is_some)>
            <UpdateModal state=state phase=phase retry=Callback::new(move |()| run()) install=install tasks=tasks/>
        </Show>
    }
}

/// The download's progress line and bar.
fn progress_view(phase: RwSignal<Phase>) -> impl IntoView {
    let text = move || match phase.get() {
        Phase::Starting | Phase::Bytes(_) => t(T::UpdateDownloading).to_owned(),
        Phase::Percent(p) => tf(T::UpdateDownloadingPercent, &[&p.to_string()]),
        Phase::Installing => t(T::UpdateInstalling).to_owned(),
    };
    // No value while the size is unknown or installing: an indeterminate bar.
    let bar = move || {
        match phase.get() {
        Phase::Percent(p) => view! {
            <progress class="update-progress" max="100" value=p.to_string() aria-hidden="true" data-testid="update-progress-bar"></progress>
        }
        .into_any(),
        _ => view! {
            <progress class="update-progress" aria-hidden="true" data-testid="update-progress-bar"></progress>
        }
        .into_any(),
    }
    };
    view! {
        <p role="status" data-testid="update-progress">{text}</p>
        {bar}
    }
}

/// The release page link: the main button where Update now is not offered.
fn view_release(url: String, primary: bool) -> impl IntoView {
    let class = if primary { "btn primary" } else { "btn" };
    view! {
        <a class=class href=url target="_blank" rel="noopener" data-testid="update-view">
            {t(T::ViewRelease)}{icon(Glyph::ExternalLink)}
        </a>
    }
}

/// The open dialog. Each state is its own [`Modal`], so focus moves to that
/// state's main button; closing gives focus back to what had it before.
#[component]
fn UpdateModal(
    state: RwSignal<Option<Check>>,
    phase: RwSignal<Phase>,
    retry: Callback<()>,
    install: Callback<(String, String)>,
    /// The dialog's tasks: closing touches only its `state`.
    tasks: Tasks,
) -> impl IntoView {
    super::focus::return_focus_on_cleanup();
    // On the next task, as the shortcuts dialog does: the click that closes
    // must still reach its other listeners.
    let close = Callback::new(move |()| {
        tasks.spawn(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            state.set(None);
        });
    });
    // Restart into the update: the leave guard asks first if work would
    // be lost, with this dialog gone (closed as `close` does). Cancel keeps
    // the page; the update waits for the next start.
    let shell = super::Shell::get();
    let restart = move || {
        tasks.spawn(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            state.set(None);
            super::leave_guard::leave_then(shell, |release| {
                release.now();
                desktop::restart_app();
            });
        });
    };
    // While installing the files are being replaced: Cancel only before that.
    let cancel = Callback::new(move |()| {
        if phase.get_untracked() != Phase::Installing {
            desktop::cancel_update();
        }
    });
    move || {
        state.get().map(|s| match s {
            Check::Checking => view! {
                <Modal title=t(T::CheckForUpdates).to_owned() glyph=Glyph::Refresh on_close=close
                    testid="update-dialog" initial_focus="[data-testid=update-close]">
                    <div class="dialog-body">
                        <p class="row" role="status" data-testid="update-checking"><span class="spinner"></span>{t(T::CheckingForUpdates)}</p>
                    </div>
                    <div class="dialog-foot">
                        <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                    </div>
                </Modal>
            }
            .into_any(),
            Check::Update { version, url, current, install: Install::Ready } => {
                let target = (version.clone(), url.clone());
                view! {
                    <Modal title=tf(T::UpdateAvailable, &[&version]) glyph=Glyph::Download on_close=close
                        testid="update-dialog" initial_focus="[data-testid=update-now]">
                        <div class="dialog-body">
                            <p role="status" data-testid="update-available">{tf(T::UpdateYouHave, &[&current])}</p>
                        </div>
                        <div class="dialog-foot">
                            <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                            {view_release(url, false)}
                            <button class="btn primary" data-testid="update-now"
                                on:click=move |_| install.run(target.clone())>{icon(Glyph::Download)}{t(T::UpdateNow)}</button>
                        </div>
                    </Modal>
                }
                .into_any()
            }
            Check::Update { version, url, current, install: how } => view! {
                <Modal title=tf(T::UpdateAvailable, &[&version]) glyph=Glyph::Download on_close=close
                    testid="update-dialog" initial_focus="[data-testid=update-view]">
                    <div class="dialog-body">
                        <p role="status" data-testid="update-available">{tf(T::UpdateYouHave, &[&current])}</p>
                        {how.hint().map(|h| view! { <p class="muted small" data-testid="update-hint">{t(h)}</p> })}
                    </div>
                    <div class="dialog-foot">
                        <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                        {view_release(url, true)}
                    </div>
                </Modal>
            }
            .into_any(),
            Check::Installing { version } => view! {
                <Modal title=tf(T::UpdatingTo, &[&version]) glyph=Glyph::Download on_close=cancel
                    testid="update-dialog" initial_focus="[data-testid=update-cancel]">
                    <div class="dialog-body">{progress_view(phase)}</div>
                    <div class="dialog-foot">
                        <button class="btn" data-testid="update-cancel"
                            disabled=move || phase.get() == Phase::Installing
                            on:click=move |_| cancel.run(())>{t(T::Cancel)}</button>
                    </div>
                </Modal>
            }
            .into_any(),
            Check::Installed { version } => view! {
                <Modal title=tf(T::UpdateInstalled, &[&version]) glyph=Glyph::Check on_close=close
                    testid="update-dialog" initial_focus="[data-testid=update-restart]">
                    <div class="dialog-body">
                        <p role="status" data-testid="update-installed">{t(T::UpdateRestartHint)}</p>
                    </div>
                    <div class="dialog-foot">
                        <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Later)}</button>
                        <button class="btn primary" data-testid="update-restart"
                            on:click=move |_| restart()>{icon(Glyph::Refresh)}{t(T::RestartApp)}</button>
                    </div>
                </Modal>
            }
            .into_any(),
            Check::InstallFailed { version, url, why } => {
                let target = (version, url.clone());
                let focus = if why.retry() { "[data-testid=update-now]" } else { "[data-testid=update-view]" };
                view! {
                    <Modal title=t(T::UpdateNotInstalled).to_owned() glyph=Glyph::Warning on_close=close
                        testid="update-dialog" initial_focus=focus>
                        <div class="dialog-body">
                            <p role="alert" data-testid="update-install-failed" data-kind=format!("{why:?}")>{t(why.message())}</p>
                        </div>
                        <div class="dialog-foot">
                            <button class="btn" data-testid="update-close" on:click=move |_| close.run(())>{t(T::Close)}</button>
                            {view_release(url, !why.retry())}
                            {why.retry().then(|| view! {
                                <button class="btn primary" data-testid="update-now"
                                    on:click=move |_| install.run(target.clone())>{icon(Glyph::Refresh)}{t(T::TryAgain)}</button>
                            })}
                        </div>
                    </Modal>
                }
                .into_any()
            }
            Check::Current(current) => view! {
                <Modal title=t(T::UpToDate).to_owned() glyph=Glyph::Check on_close=close
                    testid="update-dialog" initial_focus="[data-testid=update-close]">
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
                        testid="update-dialog" initial_focus="[data-testid=update-retry]">
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_says_whether_update_now_is_offered() {
        assert_eq!(Install::from_shell(Some("ready")), Install::Ready);
        assert_eq!(Install::from_shell(Some("devBuild")), Install::DevBuild);
        assert_eq!(Install::from_shell(Some("package")), Install::Package);
        assert_eq!(Install::from_shell(Some("anything else")), Install::Package);
        assert_eq!(Install::from_shell(None), Install::Browser);
        assert_eq!(Install::Browser.hint(), None);
        assert_eq!(Install::Package.hint(), Some(T::UpdatePackage));
    }

    #[test]
    fn only_a_network_failure_offers_try_again() {
        assert_eq!(
            Stopped::from_kind("signature").message(),
            T::UpdateSignatureFailed
        );
        assert!(!Stopped::from_kind("signature").retry());
        assert!(Stopped::from_kind("network").retry());
        assert_eq!(Stopped::from_kind("noBuild"), Stopped::NoBuild);
        assert_eq!(Stopped::from_kind("busy"), Stopped::Other);
    }

    #[test]
    fn progress_events_become_phases() {
        let p = |v: Value| Phase::from_detail(&v);
        assert_eq!(
            p(serde_json::json!({"phase": "downloading", "downloaded": 50, "total": 200})),
            Some(Phase::Percent(25))
        );
        assert_eq!(
            p(serde_json::json!({"phase": "downloading", "downloaded": 300, "total": 200})),
            Some(Phase::Percent(100))
        );
        assert_eq!(
            p(serde_json::json!({"phase": "downloading", "downloaded": 7, "total": null})),
            Some(Phase::Bytes(7))
        );
        assert_eq!(
            p(serde_json::json!({"phase": "downloading", "downloaded": 7, "total": 0})),
            Some(Phase::Bytes(7))
        );
        assert_eq!(
            p(serde_json::json!({"phase": "installing"})),
            Some(Phase::Installing)
        );
        assert_eq!(p(serde_json::json!({"phase": "other"})), None);
        assert_eq!(p(Value::Null), None);
    }
}
