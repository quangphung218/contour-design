//! The browser app: routes, shared context and pages.

mod agent_w3;
mod api;
mod automations;
mod bridge;
mod byok_form;
mod capture;
mod chat_ops;
mod chat_view;
mod comments_panel;
mod composer;
mod deck;
mod desktop;
mod doc_view;
mod docs_page;
mod ds_create;
mod ds_page;
mod dsh_setup;
mod export_extra;
mod export_menu;
mod files_panel;
mod first_hint;
mod focus;
mod font_banner;
mod genui_view;
mod home;
mod home_tabs;
mod icons;
mod inbox;
mod integrations;
mod lazy_frame;
mod leave_guard;
mod lib_ui;
mod library_picker;
mod live_view;
mod manage;
mod manual_edit;
mod mcp_settings;
mod media_page;
mod media_rows;
mod memory_settings;
mod menu_scrim;
mod new_project;
mod notify;
mod onboarding;
mod onboarding_brand;
mod palette;
mod plugins_page;
mod presenter;
mod preview_comments;
mod preview_tools;
mod project_actions;
mod projects;
mod quick_menu;
mod recovery_tip;
mod settings_page;
mod shortcuts;
mod sketch;
mod sse;
mod studio;
mod switcher;
mod templates_page;
mod terminal;
mod updates;
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
    /// Whether leaving the page would lose work (`leave_guard`).
    unsaved: RwSignal<Option<Callback<(), bool>>>,
    /// Where the user asked to go while work would be lost: the leave
    /// question is open.
    leaving: RwSignal<Option<leave_guard::Leave>>,
    /// The page is about to unload on purpose: `beforeunload` stays quiet.
    released: StoredValue<bool>,
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

    /// Re-reads settings after a route that writes them on its own (project
    /// locations), so the next [`Shell::save_config`] does not send back
    /// the old values.
    pub async fn reload_config(self) {
        if let Ok(config) = api::get::<AppConfig>("/api/app-config").await {
            self.config.set(Some(config));
        }
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

    /// Onboarding ▸ Check again: the daemon reads the login shell's `PATH`
    /// again first, so an agent installed while the app runs is found.
    pub async fn rescan_agents(self) {
        let res = api::post::<_, AgentsResponse>("/api/agents/rescan", &serde_json::json!({}))
            .await
            .map(|r| r.agents)
            .map_err(|e| e.to_string());
        self.agents.set(Some(res));
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

/// Fetches `locales/<tag>.txt` (copied next to the bundle) and installs
/// it. A failed fetch installs an empty table: the app shows English
/// rather than retrying on every render.
async fn load_dict(locale: Locale) {
    let path = format!("/locales/{}.txt", locale.tag());
    let text = match api::get_text(&path).await {
        Ok(text) => text,
        Err(e) => {
            leptos::logging::warn!("{path}: {e}");
            String::new()
        }
    };
    let unknown = i18n::install_dict(locale, text);
    if !unknown.is_empty() {
        leptos::logging::warn!("{path}: unknown keys {unknown:?}");
    }
}

fn apply_theme(theme: Theme) {
    if desktop::is_desktop() {
        desktop::set_theme(theme.as_str());
    }
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
        unsaved: RwSignal::new(None),
        leaving: RwSignal::new(None),
        released: StoredValue::new(false),
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
    // A language outside en/vi first fetches its strings file. The first
    // paint waits for it (painting English, then rebuilding, would drop
    // the first screen's pending requests); a later switch keeps showing
    // the previous language until the file arrives.
    let ready = Memo::new(move |_| shell.config.with(Option::is_some));
    let dict_loaded = RwSignal::new(0u32);
    let painted = StoredValue::new(false);
    Effect::new(move |_| {
        let wanted = shell.locale.get();
        if !i18n::has_dict(wanted) {
            leptos::task::spawn_local(async move {
                load_dict(wanted).await;
                dict_loaded.update(|n| *n += 1);
            });
        }
    });
    move || {
        if !ready.get() {
            return None;
        }
        dict_loaded.track();
        let wanted = shell.locale.get();
        let locale = if i18n::has_dict(wanted) {
            wanted
        } else if painted.get_value() {
            i18n::locale()
        } else {
            return None;
        };
        painted.set_value(true);
        i18n::set_locale(locale);
        if let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        {
            let _ = root.set_attribute("lang", locale.tag());
            let _ = root.set_attribute("dir", if locale.is_rtl() { "rtl" } else { "ltr" });
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
        } else if modifier && !ev.alt_key() && !ev.shift_key() && ev.key() == "," {
            // Through the sidebar link, so the router handles it.
            ev.prevent_default();
            click_link("/settings");
        } else if modifier
            && !ev.alt_key()
            && !ev.shift_key()
            && ev.key().eq_ignore_ascii_case("b")
            && shell.studio.with_untracked(Option::is_none)
        {
            // In the Studio, Cmd/Ctrl+B shows or hides the files instead.
            ev.prevent_default();
            if let Some(w) = web_sys::window() {
                if let Ok(e) = web_sys::Event::new(TOGGLE_SIDEBAR_EVENT) {
                    let _ = w.dispatch_event(&e);
                }
            }
        }
    });
    // Desktop menu items the page carries out (Help, Settings).
    let menu = window_event_listener(
        leptos::ev::Custom::<web_sys::CustomEvent>::new("od:menu"),
        move |ev| match ev.detail().as_string().as_deref() {
            Some("shortcuts") => shell.shortcuts.set(true),
            Some("palette") => shell.palette.set(true),
            Some("settings") => click_link("/settings"),
            _ => {}
        },
    );
    on_cleanup(move || {
        keys.remove();
        menu.remove();
    });
    view! {
        <Router>
            <OnboardingGate/>
            <leave_guard::LeaveGuard/>
            <Show when=move || shell.palette.get()>
                <palette::CommandPalette/>
            </Show>
            <Show when=move || shell.shortcuts.get()>
                <shortcuts::ShortcutsDialog/>
            </Show>
            <updates::UpdateDialog/>
            <ViewTracker/>
            <ScrollReset/>
            <notify::RunWatcher/>
            <memory_settings::MemoryToast/>
            <div class="app">
                {mac.then(|| view! { <div class="titlebar" data-tauri-drag-region=""></div> })}
                <div class="app-body">
                    <Sidebar/>
                    <main class="main" id="main">
                        <Routes fallback=|| view! { <NotFound/> }>
                            <Route path=path!("/") view=home::Home/>
                            <Route path=path!("/projects") view=projects::Projects/>
                            <Route path=path!("/projects/:id") view=studio::Studio/>
                            <Route path=path!("/onboarding") view=onboarding::Onboarding/>
                            <Route path=path!("/settings") view=settings_page::SettingsPage/>
                            <Route path=path!("/automations") view=automations::Automations/>
                            <Route path=path!("/design-systems") view=ds_page::DesignSystemsPage/>
                            <Route path=path!("/plugins") view=plugins_page::PluginsPage/>
                            <Route path=path!("/media") view=media_page::MediaPage/>
                            <Route path=path!("/templates") view=templates_page::TemplatesPage/>
                            <Route path=path!("/integrations") view=integrations::Integrations/>
                            <Route path=path!("/inbox") view=inbox::Inbox/>
                            <Route path=path!("/present/:id") view=presenter::Presenter/>
                            <Route path=path!("/docs") view=docs_page::DocsPage/>
                        </Routes>
                    </main>
                </div>
            </div>
        </Router>
    }
}

/// Window event that toggles the sidebar (Cmd/Ctrl+B, desktop menu).
const TOGGLE_SIDEBAR_EVENT: &str = "od:toggle-sidebar";

/// Follows the sidebar link to `href` as if clicked.
pub(crate) fn click_link(href: &str) {
    let link = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| {
            d.query_selector(&format!(".sidebar a[href=\"{href}\"]"))
                .ok()
                .flatten()
        })
        .and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(e).ok());
    if let Some(a) = link {
        a.click();
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

/// An address no page answers: say so, and the way back.
#[component]
fn NotFound() -> impl IntoView {
    view! {
        <section class="page not-found" data-testid="not-found">
            <div class="empty-state">
                <span class="empty-icon">{icon(Glyph::Search)}</span>
                <h1>{t(T::PageNotFound)}</h1>
                <p>{t(T::PageNotFoundBody)}</p>
                <A href="/" attr:class="btn primary" attr:data-testid="go-home">{icon(Glyph::Home)}{t(T::GoToHome)}</A>
            </div>
        </section>
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
    let toggle = move |_: ()| {
        let (signal, key) = if in_studio.get_untracked() {
            (studio_collapsed, SIDEBAR_STUDIO_KEY)
        } else {
            (collapsed, SIDEBAR_KEY)
        };
        let next = !signal.get_untracked();
        signal.set(next);
        store_flag(key, next);
    };
    let toggle_key = window_event_listener(
        leptos::ev::Custom::<web_sys::Event>::new(TOGGLE_SIDEBAR_EVENT),
        move |_| toggle(()),
    );
    on_cleanup(move || toggle_key.remove());
    let tip = move |label: &'static str| move || rail().then_some(label);
    let link = move |href: &'static str, glyph: Glyph, label: T, exact: bool| {
        let label = t(label);
        view! {
            // The label is hidden on the rail: the link keeps its name.
            <A href=href exact=exact attr:class="nav-link" attr:aria-label=label attr:data-tip=tip(label) attr:data-tip-pos="right">
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
                    on:click=move |_| toggle(())
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
                {link("/templates", Glyph::Grid, T::NavTemplates, false)}
                {link("/design-systems", Glyph::DesignSystem, T::NavDesignSystems, false)}
                {link("/plugins", Glyph::Plugin, T::NavPlugins, false)}
                {link("/media", Glyph::Image, T::NavMedia, false)}
                {link("/automations", Glyph::Automations, T::NavAutomations, false)}
                {link("/integrations", Glyph::Mcp, T::NavIntegrations, false)}
                {link("/inbox", Glyph::Inbox, T::NavInbox, false)}
                <span class="spacer"></span>
                <quick_menu::QuickMenu rail=Signal::derive(rail)/>
                <button
                    class="nav-link"
                    data-testid="open-shortcuts"
                    aria-label=t(T::KeyboardShortcuts)
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
pub(crate) const APP_NAME: &str = "Contour";

/// The Contour mark (one stroke, `currentColor`) in a tile of class `class`.
pub(crate) fn brand_mark(class: &'static str) -> impl IntoView {
    view! { <span class=class aria-hidden="true" inner_html=include_str!("../../assets/contour-mark.svg")></span> }
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

/// Records each Studio visit for "Recently viewed".
#[component]
fn ViewTracker() -> impl IntoView {
    let location = use_location();
    Effect::new(move |_| {
        let path = location.pathname.get();
        let Some(id) = path
            .strip_prefix("/projects/")
            .map(|rest| rest.split('/').next().unwrap_or_default().to_owned())
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        leptos::task::spawn_local(async move {
            let _ = api::post::<_, serde_json::Value>(
                &format!("/api/projects/{}/viewed", api::seg(&id)),
                &serde_json::json!({}),
            )
            .await;
        });
    });
}

/// A new page opens at its top: `<main>` keeps its scroll position across
/// routes otherwise (scroll down onboarding, and Home opens scrolled past
/// the brief). A link to a section (`/settings#settings-about`) opens at
/// that section instead. Back and forward leave it alone.
#[component]
fn ScrollReset() -> impl IntoView {
    let location = use_location();
    let popped = StoredValue::new(false);
    let pop = window_event_listener(leptos::ev::popstate, move |_| popped.set_value(true));
    on_cleanup(move || pop.remove());
    Effect::new(move |prev: Option<(String, String)>| {
        let path = location.pathname.get();
        let hash = location.hash.get();
        let back_or_forward = std::mem::take(&mut *popped.write_value());
        let moved = prev.is_some_and(|(p, h)| p != path || h != hash);
        if moved && !back_or_forward {
            let anchor = hash.trim_start_matches('#').to_owned();
            if anchor.is_empty() {
                if let Some(main) = element_by_id("main") {
                    main.set_scroll_top(0);
                }
            } else {
                leptos::task::spawn_local(scroll_to_anchor(anchor));
            }
        }
        (path, hash)
    });
}

fn element_by_id(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
}

/// Scrolls `<main>` to the element `id` once the page has rendered, and
/// again while sections above it finish loading and push it down, unless
/// the user has scrolled since.
async fn scroll_to_anchor(id: String) {
    let mut placed = None;
    for wait in [0, 150, 400, 1000] {
        gloo_timers::future::TimeoutFuture::new(wait).await;
        let Some(main) = element_by_id("main") else {
            return;
        };
        if placed.is_some_and(|top| top != main.scroll_top()) {
            return;
        }
        let Some(target) = element_by_id(&id) else {
            continue;
        };
        target.scroll_into_view();
        placed = Some(main.scroll_top());
    }
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
        // The bundled guide stays reachable: onboarding's agent setup links it.
        let path = location.pathname.get();
        if done == Some(false) && path != "/onboarding" && path != "/docs" {
            navigate("/onboarding", Default::default());
        }
    });
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
