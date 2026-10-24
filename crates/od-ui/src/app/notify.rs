//! Run notifications (Settings ▸ Notifications, the original's
//! `notifications`): a sound when a run succeeds or fails, and a desktop
//! notification while the window is in the background. [`RunWatcher`]
//! watches `GET /api/runs` app-wide; sounds are synthesized (no files).

use super::icons::{Glyph, icon};
use super::{Shell, api};
use crate::i18n::{T, t};
use leptos::prelude::*;
use od_contracts::{RunStatus, RunsResponse};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(3);
pub const SUCCESS_SOUNDS: [&str; 4] = ["ding", "chime", "two-tone-up", "pluck"];
pub const FAILURE_SOUNDS: [&str; 3] = ["buzz", "two-tone-down", "thud"];

/// The stored choices, with the original's defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub sound: bool,
    pub success_sound: String,
    pub failure_sound: String,
    pub desktop: bool,
}

impl Prefs {
    #[must_use]
    pub fn from_config(v: Option<&Value>) -> Self {
        let v = v.cloned().unwrap_or(Value::Null);
        let text = |k: &str, d: &str, allowed: &[&str]| {
            v[k].as_str()
                .filter(|s| allowed.contains(s))
                .unwrap_or(d)
                .to_owned()
        };
        Self {
            sound: v["soundEnabled"].as_bool().unwrap_or(true),
            success_sound: text("successSound", "ding", &SUCCESS_SOUNDS),
            failure_sound: text("failureSound", "buzz", &FAILURE_SOUNDS),
            desktop: v["desktopEnabled"].as_bool().unwrap_or(true),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "soundEnabled": self.sound,
            "successSound": self.success_sound,
            "failureSound": self.failure_sound,
            "desktopEnabled": self.desktop,
        })
    }
}

fn prefs(shell: Shell) -> Prefs {
    shell.config.with_untracked(|c| {
        Prefs::from_config(c.as_ref().and_then(|c| c.extra.get("notifications")))
    })
}

/// `(frequency Hz, start s, length s)` notes of a sound.
fn notes(sound: &str) -> &'static [(f32, f64, f64)] {
    match sound {
        "chime" => &[(880.0, 0.0, 0.5), (1320.0, 0.08, 0.6)],
        "two-tone-up" => &[(660.0, 0.0, 0.16), (990.0, 0.16, 0.22)],
        "pluck" => &[(740.0, 0.0, 0.18)],
        "buzz" => &[(140.0, 0.0, 0.32)],
        "two-tone-down" => &[(660.0, 0.0, 0.16), (440.0, 0.16, 0.24)],
        "thud" => &[(90.0, 0.0, 0.22)],
        _ => &[(1046.0, 0.0, 0.35)],
    }
}

/// Plays `sound` with the Web Audio API; silent when unavailable.
pub fn play(sound: &str) {
    let Ok(ctx) = web_sys::AudioContext::new() else {
        return;
    };
    let now = ctx.current_time();
    let wave = if matches!(sound, "buzz" | "thud") {
        web_sys::OscillatorType::Square
    } else {
        web_sys::OscillatorType::Sine
    };
    for &(freq, start, len) in notes(sound) {
        let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else {
            return;
        };
        osc.set_type(wave);
        osc.frequency().set_value(freq);
        let g = gain.gain();
        let _ = g.set_value_at_time(0.0001, now + start);
        let _ = g.exponential_ramp_to_value_at_time(0.18, now + start + 0.01);
        let _ = g.exponential_ramp_to_value_at_time(0.0001, now + start + len);
        let _ = osc.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&ctx.destination());
        let _ = osc.start_with_when(now + start);
        let _ = osc.stop_with_when(now + start + len + 0.05);
    }
}

fn in_background() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.hidden() || !d.has_focus().unwrap_or(true))
}

/// A desktop notification when permitted; clicking focuses the window.
pub fn desktop(title: &str, body: &str) {
    if web_sys::Notification::permission() != web_sys::NotificationPermission::Granted {
        return;
    }
    let opts = web_sys::NotificationOptions::new();
    opts.set_body(body);
    if let Ok(n) = web_sys::Notification::new_with_options(title, &opts) {
        let focus = wasm_bindgen::closure::Closure::<dyn Fn()>::new(|| {
            if let Some(w) = web_sys::window() {
                let _ = w.focus();
            }
        });
        n.set_onclick(Some(wasm_bindgen::JsCast::unchecked_ref(focus.as_ref())));
        focus.forget();
    }
}

/// Watches runs app-wide and announces the ones that end.
#[component]
pub fn RunWatcher() -> impl IntoView {
    let shell = Shell::get();
    let running = StoredValue::new(None::<BTreeSet<String>>);
    let tick = move || {
        leptos::task::spawn_local(async move {
            let Ok(r) = api::get::<RunsResponse>("/api/runs").await else {
                return;
            };
            let now: BTreeSet<String> = r
                .runs
                .iter()
                .filter(|r| r.status == RunStatus::Running)
                .map(|r| r.id.clone())
                .collect();
            // The app rebuilds on a language switch; a poll that answers
            // after that finds this watcher gone.
            let Some(before) = running.try_get_value() else {
                return;
            };
            running.try_set_value(Some(now.clone()));
            // First poll: only learn what is running.
            let Some(before) = before else { return };
            let p = prefs(shell);
            for run in r
                .runs
                .iter()
                .filter(|r| before.contains(&r.id) && !now.contains(&r.id))
            {
                let ok = match run.status {
                    RunStatus::Succeeded => true,
                    RunStatus::Failed => false,
                    // A canceled run was the user's own doing.
                    RunStatus::Canceled | RunStatus::Running => continue,
                };
                if p.sound {
                    play(if ok {
                        &p.success_sound
                    } else {
                        &p.failure_sound
                    });
                }
                if p.desktop && in_background() {
                    let (title, body) = if ok {
                        (t(T::NotifyDone), t(T::NotifyDoneBody))
                    } else {
                        (t(T::NotifyFailed), t(T::NotifyFailedBody))
                    };
                    desktop(title, body);
                }
            }
        });
    };
    tick();
    if let Ok(handle) = set_interval_with_handle(tick, POLL) {
        on_cleanup(move || handle.clear());
    }
}

/// The Settings section.
#[component]
pub fn NotificationSettings() -> impl IntoView {
    let shell = Shell::get();
    let current = RwSignal::new(prefs(shell));
    let permission = RwSignal::new(web_sys::Notification::permission());
    let state = RwSignal::new(None::<Result<(), String>>);
    let save = move |next: Prefs| {
        current.set(next.clone());
        let mut config = shell.config.get_untracked().unwrap_or_default();
        config
            .extra
            .insert("notifications".to_owned(), next.to_value());
        leptos::task::spawn_local(async move {
            state.set(Some(shell.save_config(config).await));
        });
    };
    let ask = move || {
        if let Ok(p) = web_sys::Notification::request_permission() {
            leptos::task::spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(p).await;
                permission.set(web_sys::Notification::permission());
            });
        }
    };
    let send_test = move |_| {
        let p = current.get_untracked();
        if p.sound {
            play(&p.success_sound);
        }
        if p.desktop {
            desktop(t(T::NotifyDone), t(T::NotifyTestBody));
        }
    };
    let sound_select = move |testid: &'static str,
                             label: &'static str,
                             list: &'static [&'static str],
                             get: fn(&Prefs) -> String,
                             put: fn(&mut Prefs, String)| {
        view! {
            <select class="small select-auto" data-testid=testid aria-label=label on:change=move |ev| {
                let mut next = current.get_untracked();
                put(&mut next, event_target_value(&ev));
                play(&get(&next));
                save(next);
            }>
                {list.iter().map(|s| {
                    let s = *s;
                    view! { <option value=s selected=move || current.with(|c| get(c) == s)>{s}</option> }
                }).collect_view()}
            </select>
        }
    };
    view! {
        <div class="card padded notification-settings" data-testid="notifications">
            <div class="row wrap">
                <label class="switch grow">
                    <input type="checkbox" data-testid="notify-sound" prop:checked=move || current.with(|c| c.sound)
                        on:change=move |ev| { let mut n = current.get_untracked(); n.sound = event_target_checked(&ev); save(n); }/>
                    <span>{icon(Glyph::Sound)}{t(T::CompletionSound)}</span>
                </label>
                <span class="muted small">{t(T::SuccessSound)}</span>
                {sound_select("notify-success-sound", t(T::SuccessSound), &SUCCESS_SOUNDS, |p| p.success_sound.clone(), |p, v| p.success_sound = v)}
                <span class="muted small">{t(T::FailureSound)}</span>
                {sound_select("notify-failure-sound", t(T::FailureSound), &FAILURE_SOUNDS, |p| p.failure_sound.clone(), |p, v| p.failure_sound = v)}
            </div>
            <label class="switch">
                <input type="checkbox" data-testid="notify-desktop" prop:checked=move || current.with(|c| c.desktop)
                    on:change=move |ev| {
                        let on = event_target_checked(&ev);
                        let mut n = current.get_untracked();
                        n.desktop = on;
                        save(n);
                        if on { ask(); }
                    }/>
                <span>{icon(Glyph::Bell)}{t(T::DesktopNotification)}</span>
            </label>
            <p class="muted small">{t(T::DesktopNotificationHint)}</p>
            {move || (permission.get() == web_sys::NotificationPermission::Denied).then(|| view! {
                <p class="warning-text small">{t(T::NotificationsBlocked)}</p>
            })}
            <div class="row">
                <button class="btn small" data-testid="notify-test" on:click=send_test>{icon(Glyph::Play)}{t(T::SendTest)}</button>
                {move || match state.get() {
                    Some(Ok(())) => view! { <span class="ok-text small">{t(T::Saved)}</span> }.into_any(),
                    Some(Err(e)) => view! { <span class="error-text small">{e}</span> }.into_any(),
                    None => ().into_any(),
                }}
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_unknown_sounds() {
        let p = Prefs::from_config(None);
        assert!(p.sound && p.desktop);
        assert_eq!(
            (p.success_sound.as_str(), p.failure_sound.as_str()),
            ("ding", "buzz")
        );
        let p = Prefs::from_config(Some(
            &json!({"soundEnabled": false, "successSound": "boom", "failureSound": "thud"}),
        ));
        assert!(!p.sound);
        assert_eq!(
            (p.success_sound.as_str(), p.failure_sound.as_str()),
            ("ding", "thud")
        );
        assert_eq!(Prefs::from_config(Some(&p.to_value())), p);
    }
}
