//! Quick settings in the sidebar corner (track C2-chat-home). The original
//! has an account avatar menu (cloud, N/A here) and, signed out, a "Local
//! mode" avatar; its settings popover (`EntrySettingsMenu`) carries
//! language and a way into Settings. This one: theme, language, the main
//! Settings sections, shortcuts, and Help (Documentation, Report Issue),
//! which the desktop Help menu reaches too (`od:menu` `docs` /
//! `report-issue`).

use super::composer::close_outside;
use super::icons::{Glyph, icon};
use super::leave_guard::use_leave;
use super::{Shell, api};
use crate::i18n::{Locale, T, t};
use crate::settings::Theme;
use leptos::html::Div;
use leptos::prelude::*;
use serde_json::Value;

/// How long the Report Issue result stays.
const NOTICE_MS: u32 = 6_000;

/// What Report Issue did.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub text: String,
    pub log_dir: String,
    /// The clipboard took the text; else the UI shows it to copy by hand
    /// (a webview may refuse clipboard writes outside a click).
    pub copied: bool,
}

/// Help ▸ Report Issue: the diagnostics text on the clipboard and the log
/// folder in the file manager. Nothing is sent anywhere.
pub async fn report_issue() -> Result<Report, String> {
    let v = api::get::<Value>("/api/diagnostics/summary")
        .await
        .map_err(|e| e.to_string())?;
    let text = v["text"].as_str().unwrap_or_default().to_owned();
    let copied = super::export_menu::copy_text(&text).await.is_ok();
    api::post::<_, Value>("/api/diagnostics/open-logs", &serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())?;
    Ok(Report {
        text,
        log_dir: v["logDir"].as_str().unwrap_or_default().to_owned(),
        copied,
    })
}

/// Asks the shell's quick menu to run Report Issue (same as the desktop
/// Help menu), from anywhere in the app.
pub fn request_report() {
    if let (Some(w), Ok(ev)) = (web_sys::window(), web_sys::Event::new(REPORT_EVENT)) {
        let _ = w.dispatch_event(&ev);
    }
}

/// Window event of [`request_report`].
const REPORT_EVENT: &str = "od:report-issue";

const SECTIONS: [(&str, T, Glyph); 4] = [
    ("execution", T::SettingsModels, Glyph::Agent),
    ("memory", T::GlobalMemory, Glyph::Memory),
    ("media", T::MediaProviders, Glyph::FileImage),
    ("mcp", T::McpTitle, Glyph::Mcp),
];

#[component]
pub fn QuickMenu(#[prop(into)] rail: Signal<bool>) -> impl IntoView {
    let shell = Shell::get();
    let navigate = use_leave();
    let open = RwSignal::new(false);
    let wrap = NodeRef::<Div>::new();
    close_outside(open, wrap);
    let notice = RwSignal::new(None::<Result<String, String>>);
    // The summary to copy by hand when the clipboard refused it.
    let manual = RwSignal::new(None::<String>);
    let seq = StoredValue::new(0u32);
    let show = move |r: Result<String, String>| {
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        notice.set(Some(r));
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(NOTICE_MS).await;
            if seq.get_value() == mine {
                notice.set(None);
            }
        });
    };
    let report = move || {
        leptos::task::spawn_local(async move {
            match report_issue().await {
                Ok(r) if r.copied => show(Ok(r.log_dir)),
                Ok(r) => manual.set(Some(r.text)),
                Err(e) => show(Err(e)),
            }
        });
    };
    let nav_docs = navigate.clone();
    let go_docs = Callback::new(move |()| nav_docs("/docs"));
    // The desktop Help menu.
    let go_docs_menu = move || go_docs.run(());
    let menu = window_event_listener(
        leptos::ev::Custom::<web_sys::CustomEvent>::new("od:menu"),
        move |ev| match ev.detail().as_string().as_deref() {
            Some("docs") => go_docs_menu(),
            Some("report-issue") => report(),
            _ => {}
        },
    );
    let asked = window_event_listener(
        leptos::ev::Custom::<web_sys::Event>::new(REPORT_EVENT),
        move |_| report(),
    );
    on_cleanup(move || {
        menu.remove();
        asked.remove();
    });
    let save = move |f: Box<dyn FnOnce(&mut crate::settings::AppConfig)>| {
        let mut next = shell.config.get_untracked().unwrap_or_default();
        f(&mut next);
        leptos::task::spawn_local(async move {
            if let Err(e) = shell.save_config(next).await {
                show(Err(e));
            }
        });
    };
    let theme = move || {
        shell
            .config
            .with(|c| c.as_ref().map(|c| c.theme).unwrap_or_default())
    };
    let go = move |href: String| {
        open.set(false);
        navigate(&href);
    };
    let go_section = go.clone();
    view! {
        <div class="menu-wrap quick-menu" node_ref=wrap>
            <button
                class="nav-link quick-trigger"
                data-testid="quick-menu"
                aria-haspopup="menu"
                aria-expanded=move || open.get().to_string()
                aria-label=move || if rail.get() { t(T::QuickSettings) } else { t(T::LocalMode) }
                data-tip=move || rail.get().then(|| t(T::QuickSettings))
                data-tip-pos="right"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <span class="quick-avatar" aria-hidden="true">{icon(Glyph::Terminal)}</span>
                <span class="nav-label">{t(T::LocalMode)}</span>
            </button>
            <Show when=move || open.get()>
                <div class="menu up quick-pop" role="menu" data-testid="quick-menu-pop" aria-label=t(T::QuickSettings)>
                    <div class="quick-head">
                        <span class="quick-avatar" aria-hidden="true">{icon(Glyph::Terminal)}</span>
                        <span class="grow">
                            <strong class="block">{t(T::LocalMode)}</strong>
                            <span class="muted small">{t(T::LocalModeHint)}</span>
                        </span>
                    </div>
                    <div class="menu-sep" role="presentation">{t(T::Appearance)}</div>
                    <div class="segmented small quick-theme" role="group" aria-label=t(T::Appearance)>
                        {Theme::ALL.into_iter().map(|th| {
                            let (glyph, label) = match th {
                                Theme::System => (Glyph::System, t(T::ThemeSystem)),
                                Theme::Light => (Glyph::Sun, t(T::ThemeLight)),
                                Theme::Dark => (Glyph::Moon, t(T::ThemeDark)),
                            };
                            view! {
                                <button class="seg" data-testid=format!("quick-theme-{}", th.as_str())
                                    aria-pressed=move || (theme() == th).to_string()
                                    class:active=move || theme() == th
                                    on:click=move |_| save(Box::new(move |c| c.theme = th))>
                                    {icon(glyph)}<span class="seg-label">{label}</span>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                    <div class="menu-sep" role="presentation">{t(T::Language)}</div>
                    <select class="small quick-language" data-testid="quick-language" aria-label=t(T::Language)
                        on:change=move |ev| {
                            let tag = event_target_value(&ev);
                            save(Box::new(move |c| c.locale = Some(tag)));
                        }>
                        {Locale::ALL.iter().copied().map(|l| view! {
                            <option value=l.tag() selected=move || shell.locale.get() == l>{l.native_name()}</option>
                        }).collect_view()}
                    </select>
                    <div class="menu-sep" role="presentation">{t(T::NavSettings)}</div>
                    {SECTIONS.into_iter().map(|(id, label, glyph)| {
                        let go = go_section.clone();
                        view! {
                            <button class="menu-item" role="menuitem" data-testid=format!("quick-section-{id}")
                                on:click=move |_| go(format!("/settings#settings-{id}"))>
                                {icon(glyph)}{t(label)}
                            </button>
                        }
                    }).collect_view()}
                    <button class="menu-item" role="menuitem" data-testid="quick-shortcuts"
                        on:click=move |_| { open.set(false); shell.shortcuts.set(true); }>
                        {icon(Glyph::Keyboard)}{t(T::KeyboardShortcuts)}
                    </button>
                    <div class="menu-sep" role="presentation">{t(T::Help)}</div>
                    <button class="menu-item" role="menuitem" data-testid="quick-docs"
                        on:click=move |_| { open.set(false); go_docs.run(()); }>
                        {icon(Glyph::Book)}{t(T::Documentation)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="quick-report"
                        on:click=move |_| { open.set(false); report(); }>
                        {icon(Glyph::Warning)}{t(T::ReportIssue)}
                    </button>
                </div>
            </Show>
            {move || manual.get().map(|text| view! {
                <super::lib_ui::Modal title=t(T::ReportIssue).to_owned() glyph=Glyph::Warning testid="report-manual"
                    on_close=Callback::new(move |()| manual.set(None))>
                    <div class="dialog-body">
                        <p class="muted small">{t(T::ReportManual)}</p>
                        <textarea class="input mono" rows="12" readonly=true data-testid="report-text"
                            on:focus=move |ev| {
                                let el = event_target::<web_sys::HtmlTextAreaElement>(&ev);
                                el.select();
                            }>{text}</textarea>
                    </div>
                </super::lib_ui::Modal>
            })}
            {move || notice.get().map(|n| {
                let (error, text) = match n {
                    Ok(dir) => (false, if dir.is_empty() { t(T::ReportCopied).to_owned() } else { crate::i18n::tf(T::ReportCopiedDir, &[&dir]) }),
                    Err(e) => (true, e),
                };
                view! {
                    <div class="toast-region">
                        <div class="toast" class:error=error role="status" data-testid="report-notice">
                            {icon(if error { Glyph::Warning } else { Glyph::CheckCircle })}
                            <span class="toast-text">{text}</span>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}
