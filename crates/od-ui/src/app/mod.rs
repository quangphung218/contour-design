//! The browser app: routes, shared context and pages.

mod agent_w3;
mod api;
mod automations;
mod bridge;
mod chat_ops;
mod chat_view;
mod composer;
mod deck;
mod desktop;
mod ds_create;
mod export_menu;
mod home;
mod icons;
mod lazy_frame;
mod mcp_settings;
mod onboarding;
mod palette;
mod preview_comments;
mod preview_tools;
mod projects;
mod settings_page;
mod shortcuts;
mod sse;
mod studio;
mod switcher;
mod terminal;
mod versions;

use crate::events::{AgentInfo, AgentsResponse};
use crate::i18n::{self, Locale, T, t, tf};
use crate::settings::{AppConfig, Theme};
use icons::{Glyph, icon};
use leptos::prelude::*;
use leptos_router::components::{A, Route, Router, Routes};
use leptos_router::hooks::{use_location, use_navigate};
use leptos_router::path;

/// App-wide state shared through context.
#[derive(Clone, Copy)]
pub struct Shell {
    /// `None` until `/api/app-config` answered.
    pub config: RwSignal<Option<AppConfig>>,
    /// Detected agents; `Err` when detection is unavailable.
    pub agents: RwSignal<Option<Result<Vec<AgentInfo>, String>>>,
    /// UI language; the whole app re-renders when it changes.
    pub locale: RwSignal<Locale>,
    /// Command palette (Cmd/Ctrl+K) open.
    pub palette: RwSignal<bool>,
    /// Keyboard shortcuts help (`?`) open.
    pub shortcuts: RwSignal<bool>,
    /// The open Studio, whose commands the palette adds.
    pub(crate) studio: RwSignal<Option<studio::StudioCtx>>,
}

impl Shell {
    fn get() -> Self {
        expect_context::<Self>()
    }

    /// Saves settings and updates the shared copy with what the daemon kept.
    pub async fn save_config(self, next: AppConfig) -> Result<(), String> {
        let saved: AppConfig = api::put("/api/app-config", &next.to_patch())
            .await
            .map_err(|e| e.to_string())?;
        self.set_locale(locale_of(&saved));
        self.config.set(Some(saved));
        Ok(())
    }

    /// Switches the language; a no-op (no re-render) when unchanged.
    fn set_locale(self, locale: Locale) {
        if self.locale.get_untracked() != locale {
            self.locale.set(locale);
        }
    }

    pub fn reload_agents(self) {
        leptos::task::spawn_local(async move {
            let res = api::get::<AgentsResponse>("/api/agents")
                .await
                .map(|r| r.agents)
                .map_err(|e| e.to_string());
            self.agents.set(Some(res));
        });
    }

    /// The agent runs use: the configured one, else the first available.
    pub fn agent_id(self) -> Option<String> {
        let configured = self
            .config
            .with_untracked(|c| c.as_ref().and_then(|c| c.agent_id.clone()));
        configured.or_else(|| {
            self.agents.with_untracked(|a| match a {
                Some(Ok(list)) => list.iter().find(|a| a.available).map(|a| a.id.clone()),
                _ => None,
            })
        })
    }
}

/// The configured language, else the browser's.
fn locale_of(config: &AppConfig) -> Locale {
    config
        .locale
        .clone()
        .or_else(|| web_sys::window().and_then(|w| w.navigator().language()))
        .map(|tag| Locale::parse(&tag))
        .unwrap_or_default()
}

fn apply_theme(theme: Theme) {
    let root = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element());
    if let Some(root) = root {
        let _ = match theme {
            Theme::System => root.remove_attribute("data-theme"),
            t => root.set_attribute("data-theme", t.as_str()),
        };
    }
}

#[component]
pub fn App() -> impl IntoView {
    let shell = Shell {
        config: RwSignal::new(None),
        agents: RwSignal::new(None),
        locale: RwSignal::new(locale_of(&AppConfig::default())),
        palette: RwSignal::new(false),
        shortcuts: RwSignal::new(false),
        studio: RwSignal::new(None),
    };
    provide_context(shell);
    leptos::task::spawn_local(async move {
        // A daemon without settings yet still gets a usable UI.
        let config = api::get::<AppConfig>("/api/app-config")
            .await
            .unwrap_or_default();
        shell.set_locale(locale_of(&config));
        shell.config.set(Some(config));
    });
    shell.reload_agents();
    Effect::new(move |_| {
        if let Some(theme) = shell.config.with(|c| c.as_ref().map(|c| c.theme)) {
            apply_theme(theme);
        }
    });

    // Strings are read once per render: a language change rebuilds the app.
    // Wait for settings (a local call) so the first paint is already in the
    // user's language. A memo: saving settings must not rebuild the app.
    let ready = Memo::new(move |_| shell.config.with(Option::is_some));
    move || {
        if !ready.get() {
            return None;
        }
        let locale = shell.locale.get();
        i18n::set_locale(locale);
        if let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        {
            let _ = root.set_attribute("lang", locale.tag());
        }
        Some(view! { <Shelled/> })
    }
}

#[component]
fn Shelled() -> impl IntoView {
    let mac = is_mac_desktop();
    let shell = Shell::get();
    // App-wide keys: Cmd/Ctrl+K palette, `?` shortcuts help.
    let keys = window_event_listener(leptos::ev::keydown, move |ev| {
        let modifier = ev.ctrl_key() || ev.meta_key();
        if modifier && !ev.alt_key() && !ev.shift_key() && ev.key().eq_ignore_ascii_case("k") {
            ev.prevent_default();
            shell.shortcuts.set(false);
            shell.palette.update(|p| *p = !*p);
        } else if ev.key() == "?"
            && !modifier
            && !studio::is_editing(&ev)
            && !shell.palette.get_untracked()
        {
            ev.prevent_default();
            shell.shortcuts.update(|s| *s = !*s);
        } else if ev.key() == "Escape" && shell.shortcuts.get_untracked() {
            shell.shortcuts.set(false);
        }
    });
    on_cleanup(move || keys.remove());
    view! {
        <Router>
            <OnboardingGate/>
            <Show when=move || shell.palette.get()>
                <palette::CommandPalette/>
            </Show>
            <Show when=move || shell.shortcuts.get()>
                <shortcuts::ShortcutsDialog/>
            </Show>
            <div class="app">
                {mac.then(|| view! { <div class="titlebar" data-tauri-drag-region=""></div> })}
                <div class="app-body">
                    <Sidebar/>
                    <main class="main" id="main">
                        <Routes fallback=|| view! { <p class="empty page">{t(T::PageNotFound)}</p> }>
                            <Route path=path!("/") view=home::Home/>
                            <Route path=path!("/projects") view=projects::Projects/>
                            <Route path=path!("/projects/:id") view=studio::Studio/>
                            <Route path=path!("/onboarding") view=onboarding::Onboarding/>
                            <Route path=path!("/settings") view=settings_page::SettingsPage/>
                            <Route path=path!("/automations") view=automations::Automations/>
                        </Routes>
                    </main>
                </div>
            </div>
        </Router>
    }
}

/// `localStorage` keys of the sidebar state: the Studio has its own, since
/// it wants the room and starts collapsed.
const SIDEBAR_KEY: &str = "od.sidebar.collapsed";
const SIDEBAR_STUDIO_KEY: &str = "od.sidebar.studio-collapsed";

pub(super) fn stored_flag(key: &str) -> Option<bool> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item(key).ok()?.map(|v| v == "1")
}

pub(super) fn store_flag(key: &str, on: bool) {
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.set_item(key, if on { "1" } else { "0" });
    }
}

/// Left navigation: brand, pages, settings at the bottom. Collapses to an
/// icon rail; the choice is remembered per context (Studio or the rest).
#[component]
fn Sidebar() -> impl IntoView {
    let shell = Shell::get();
    let location = use_location();
    let in_studio = Memo::new(move |_| {
        location
            .pathname
            .with(|p| p.starts_with("/projects/") && p.len() > "/projects/".len())
    });
    let collapsed = RwSignal::new(stored_flag(SIDEBAR_KEY).unwrap_or(false));
    let studio_collapsed = RwSignal::new(stored_flag(SIDEBAR_STUDIO_KEY).unwrap_or(true));
    let rail = move || {
        if in_studio.get() {
            studio_collapsed.get()
        } else {
            collapsed.get()
        }
    };
    let toggle = move |_| {
        let (signal, key) = if in_studio.get_untracked() {
            (studio_collapsed, SIDEBAR_STUDIO_KEY)
        } else {
            (collapsed, SIDEBAR_KEY)
        };
        let next = !signal.get_untracked();
        signal.set(next);
        store_flag(key, next);
    };
    let tip = move |label: &'static str| move || rail().then_some(label);
    let link = move |href: &'static str, glyph: Glyph, label: T, exact: bool| {
        let label = t(label);
        view! {
            <A href=href exact=exact attr:class="nav-link" attr:data-tip=tip(label) attr:data-tip-pos="right">
                {icon(glyph)}
                <span class="nav-label">{label}</span>
            </A>
        }
    };
    view! {
        <aside class="sidebar" class:rail=rail>
            <div class="sidebar-top">
                <A href="/" attr:class="brand" attr:aria-label=APP_NAME>
                    {brand_mark("brand-mark")}
                    <span class="brand-name">{APP_NAME}</span>
                </A>
                <button
                    class="icon-btn sidebar-toggle"
                    aria-label=move || if rail() { t(T::ExpandSidebar) } else { t(T::CollapseSidebar) }
                    data-tip=move || if rail() { t(T::ExpandSidebar) } else { t(T::CollapseSidebar) }
                    data-tip-pos="right"
                    on:click=toggle
                >
                    {move || icon(if rail() { Glyph::SidebarUnfold } else { Glyph::SidebarFold })}
                </button>
            </div>
            <button
                class="palette-trigger"
                data-testid="open-palette"
                aria-label=t(T::CommandPalette)
                data-tip=tip(t(T::CommandPalette))
                data-tip-pos="right"
                on:click=move |_| shell.palette.set(true)
            >
                {icon(Glyph::Search)}
                <span class="nav-label">{t(T::PaletteTrigger)}</span>
                <kbd class="nav-label">{format!("{}K", shortcuts::modifier())}</kbd>
            </button>
            <nav class="sidenav" aria-label=t(T::MainNavigation)>
                {link("/", Glyph::Home, T::NavHome, true)}
                {link("/projects", Glyph::Projects, T::NavProjects, false)}
                {link("/automations", Glyph::Automations, T::NavAutomations, false)}
                <span class="spacer"></span>
                <button
                    class="nav-link"
                    data-testid="open-shortcuts"
                    data-tip=tip(t(T::KeyboardShortcuts))
                    data-tip-pos="right"
                    on:click=move |_| shell.shortcuts.set(true)
                >
                    {icon(Glyph::Keyboard)}
                    <span class="nav-label">{t(T::KeyboardShortcuts)}</span>
                </button>
                {link("/settings", Glyph::Settings, T::NavSettings, false)}
            </nav>
        </aside>
    }
}

/// Product name shown in the UI (docs/brand.md).
pub(crate) const APP_NAME: &str = "Nét";

/// The Nét mark (one stroke, `currentColor`) in a tile of class `class`.
pub(crate) fn brand_mark(class: &'static str) -> impl IntoView {
    view! { <span class=class aria-hidden="true" inner_html=include_str!("../../assets/net-mark.svg")></span> }
}

/// The Tauri window on macOS draws its traffic lights over the page
/// (overlay title bar), so the shell reserves a draggable strip for them.
fn is_mac_desktop() -> bool {
    let mac = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Mac OS"));
    let on = mac && desktop::is_desktop();
    if on {
        if let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        {
            let _ = root.set_attribute("data-shell", "mac");
        }
    }
    on
}

/// Sends first-time users to onboarding once settings are known.
#[component]
fn OnboardingGate() -> impl IntoView {
    let shell = Shell::get();
    let location = use_location();
    let navigate = use_navigate();
    Effect::new(move |_| {
        let done = shell
            .config
            .with(|c| c.as_ref().map(|c| c.onboarding_completed));
        if done == Some(false) && location.pathname.get() != "/onboarding" {
            navigate("/onboarding", Default::default());
        }
    });
}

/// Label of a project's kind.
pub(crate) fn kind_label(project: &od_contracts::Project) -> &'static str {
    use od_contracts::ProjectKind;
    match project.metadata.as_ref().and_then(|m| m.kind) {
        Some(ProjectKind::Prototype) => t(T::KindPrototype),
        Some(ProjectKind::Deck) => t(T::KindDeck),
        _ => t(T::KindProject),
    }
}

/// Relative "x min ago" from epoch milliseconds.
pub(crate) fn ago(ms: i64) -> String {
    let now = js_sys::Date::now() as i64;
    let mins = (now - ms).max(0) / 60_000;
    match mins {
        0 => t(T::JustNow).to_owned(),
        1..=59 => tf(T::MinAgo, &[&mins.to_string()]),
        60..=1439 => tf(T::HoursAgo, &[&(mins / 60).to_string()]),
        _ => tf(T::DaysAgo, &[&(mins / 1440).to_string()]),
    }
}

/// Mounts the app on `<body>`.
pub fn mount() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
