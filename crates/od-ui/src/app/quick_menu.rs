//! The Settings button in the sidebar corner (track C2-chat-home). The
//! original has an account avatar menu (cloud, N/A here) and, signed out, a
//! "Local mode" avatar; Contour has no account, so the corner is one
//! Settings button. Its menu: Open Settings (also Cmd/Ctrl+, and the
//! desktop Settings item, `od:menu` `settings`), theme, language, the main
//! Settings sections, shortcuts, and Help (Documentation, Report Issue),
//! which the desktop Help menu reaches too (`od:menu` `docs` /
//! `report-issue`).

use super::composer::close_on_press_outside;
use super::icons::{Glyph, icon};
use super::leave_guard::use_open_after_click;
use super::tasks::{Tasks, live};
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
        .map_err(|e| api::failure(T::ReportIssueFailed, &e))?;
    let text = v["text"].as_str().unwrap_or_default().to_owned();
    let copied = super::export_menu::copy_text(&text).await.is_ok();
    api::post::<_, Value>("/api/diagnostics/open-logs", &serde_json::json!({}))
        .await
        .map_err(|e| api::failure(T::ReportIssueFailed, &e))?;
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

/// Asks the quick menu to open Settings (Cmd/Ctrl+,): it sits inside the
/// router, so the page opens through the leave guard.
pub fn request_settings() {
    if let (Some(w), Ok(ev)) = (web_sys::window(), web_sys::Event::new(SETTINGS_EVENT)) {
        let _ = w.dispatch_event(&ev);
    }
}

/// Window event of [`request_settings`].
const SETTINGS_EVENT: &str = "od:open-settings";

const SECTIONS: [(&str, T, Glyph); 4] = [
    ("execution", T::SettingsModels, Glyph::Agent),
    ("memory", T::GlobalMemory, Glyph::Memory),
    ("media", T::MediaProviders, Glyph::FileImage),
    ("mcp", T::McpTitle, Glyph::Mcp),
];

#[component]
pub fn QuickMenu(#[prop(into)] rail: Signal<bool>) -> impl IntoView {
    let shell = Shell::get();
    let tasks = Tasks::here();
    let open_page = use_open_after_click();
    let open = RwSignal::new(false);
    let wrap = NodeRef::<Div>::new();
    // Esc is the popup's own (`QuickPop`): only while it is on top.
    close_on_press_outside(open, wrap);
    let notice = RwSignal::new(None::<Result<String, String>>);
    // The summary to copy by hand when the clipboard refused it.
    let manual = RwSignal::new(None::<String>);
    let seq = StoredValue::new(0u32);
    let show = move |r: Result<String, String>| {
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        notice.set(Some(r));
        tasks.spawn(async move {
            gloo_timers::future::TimeoutFuture::new(NOTICE_MS).await;
            if seq.get_value() == mine {
                notice.set(None);
            }
        });
    };
    // Several steps (summary, clipboard, log folder): all of them run.
    let report = move || {
        tasks.spawn_then(report_issue(), move |res| match res {
            Ok(r) if r.copied => show(Ok(r.log_dir)),
            Ok(r) => manual.set(Some(r.text)),
            Err(e) => show(Err(e)),
        });
    };
    let to_docs = open_page.clone();
    // Shut first: Cmd/Ctrl+, or a desktop menu item may come with the
    // popup open, and a leave question then sits over nothing else.
    let go_docs = Callback::new(move |()| {
        open.set(false);
        to_docs("/docs");
    });
    let to_settings = open_page.clone();
    let go_settings = Callback::new(move |()| {
        open.set(false);
        to_settings("/settings");
    });
    // The desktop Help and Settings items.
    let menu = window_event_listener(
        leptos::ev::Custom::<web_sys::CustomEvent>::new("od:menu"),
        move |ev| match ev.detail().as_string().as_deref() {
            Some("docs") => go_docs.run(()),
            Some("settings") => go_settings.run(()),
            Some("report-issue") => report(),
            _ => {}
        },
    );
    let asked = window_event_listener(
        leptos::ev::Custom::<web_sys::Event>::new(REPORT_EVENT),
        move |_| report(),
    );
    let settings_key = window_event_listener(
        leptos::ev::Custom::<web_sys::Event>::new(SETTINGS_EVENT),
        move |_| go_settings.run(()),
    );
    on_cleanup(move || {
        menu.remove();
        asked.remove();
        settings_key.remove();
    });
    let save = move |f: Box<dyn FnOnce(&mut crate::settings::AppConfig)>| {
        let mut next = shell.config.get_untracked().unwrap_or_default();
        f(&mut next);
        tasks.spawn_then(shell.save_config(next), move |res| {
            if let Err(e) = res {
                show(Err(e));
            }
        });
    };
    let theme = move || {
        shell
            .config
            .with(|c| c.as_ref().map(|c| c.theme).unwrap_or_default())
    };
    // A `Callback` is `Copy`: the menu's items are built again on each open.
    let go = Callback::new(move |href: String| {
        open.set(false);
        open_page(&href);
    });
    view! {
        <div class="menu-wrap quick-menu" node_ref=wrap>
            <button
                class="nav-link quick-trigger"
                data-testid="quick-menu"
                aria-haspopup="dialog"
                aria-expanded=move || open.get().to_string()
                // The rail hides the label: the button keeps its name.
                aria-label=t(T::NavSettings)
                data-tip=move || rail.get().then(|| t(T::NavSettings))
                data-tip-pos="right"
                on:click=move |ev| {
                    // WebKit does not focus a clicked button: focus it, so
                    // the closed menu gives the focus back to it.
                    if let Some(b) = ev.current_target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(t).ok()) {
                        let _ = b.focus();
                    }
                    open.update(|o| *o = !*o);
                }
            >
                {icon(Glyph::Settings)}
                <span class="nav-label">{t(T::NavSettings)}</span>
            </button>
            <Show when=move || open.get()>
                <QuickPop on_close=Callback::new(move |()| open.set(false)) scope=wrap>
                    <button class="menu-item" data-testid="quick-open-settings"
                        on:click=move |_| { open.set(false); go_settings.run(()); }>
                        {icon(Glyph::Settings)}{t(T::OpenSettings)}
                        // Keeps "Open Settings Ctrl+," apart in the read text.
                        {" "}
                        <kbd class="menu-hint">{format!("{},", super::shortcuts::modifier())}</kbd>
                    </button>
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
                        on:change=live(move |ev| {
                            let tag = event_target_value(&ev);
                            save(Box::new(move |c| c.locale = Some(tag)));
                        })>
                        {Locale::ALL.iter().copied().map(|l| view! {
                            <option value=l.tag() selected=move || shell.locale.get() == l>{l.native_name()}</option>
                        }).collect_view()}
                    </select>
                    <div class="menu-sep" role="presentation">{t(T::NavSettings)}</div>
                    {SECTIONS.into_iter().map(|(id, label, glyph)| {
                        view! {
                            <button class="menu-item" data-testid=format!("quick-section-{id}")
                                on:click=move |_| go.run(format!("/settings#settings-{id}"))>
                                {icon(glyph)}{t(label)}
                            </button>
                        }
                    }).collect_view()}
                    <button class="menu-item" data-testid="quick-shortcuts"
                        on:click=move |_| { open.set(false); shell.shortcuts.set(true); }>
                        {icon(Glyph::Keyboard)}{t(T::KeyboardShortcuts)}
                    </button>
                    <div class="menu-sep" role="presentation">{t(T::Help)}</div>
                    <button class="menu-item" data-testid="quick-docs"
                        on:click=move |_| { open.set(false); go_docs.run(()); }>
                        {icon(Glyph::Book)}{t(T::Documentation)}
                    </button>
                    <button class="menu-item" data-testid="quick-report"
                        on:click=move |_| { open.set(false); report(); }>
                        {icon(Glyph::Warning)}{t(T::ReportIssue)}
                    </button>
                </QuickPop>
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

/// The open menu: focus moves to its first item, the arrows go through its
/// items, Esc closes it and nothing under it (a running jury, comment
/// mode), and focus goes back to the Settings button when it closes.
/// A non-modal dialog, not `role=menu`: it holds a language `<select>` and
/// theme toggles, which a menu may not (axe `aria-required-children`).
#[component]
fn QuickPop(on_close: Callback<()>, scope: NodeRef<Div>, children: Children) -> impl IntoView {
    super::menu_scrim::close_on_escape(on_close, move || {
        scope.get_untracked().map(web_sys::Element::from)
    });
    let pop = NodeRef::<Div>::new();
    super::focus::focus_while_open(pop, |p| {
        p.query_selector(super::focus::FIRST_ITEM).ok().flatten()
    });
    view! {
        <div class="menu up quick-pop" role="dialog" data-testid="quick-menu-pop" aria-label=t(T::QuickSettings)
            node_ref=pop on:keydown=move |ev| super::focus::menu_keys(&ev, MENU_ITEMS)>
            {children()}
        </div>
    }
}

/// What the arrows go through: every control of the menu.
const MENU_ITEMS: &str = ".menu-item, .quick-theme .seg, .quick-language";
