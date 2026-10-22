//! The browser app: routes, shared context and pages.

mod api;
mod chat_view;
mod desktop;
mod home;
mod onboarding;
mod projects;
mod settings_page;
mod sse;
mod studio;

use crate::events::{AgentInfo, AgentsResponse};
use crate::settings::{AppConfig, Theme};
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
        self.config.set(Some(saved));
        Ok(())
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
    };
    provide_context(shell);
    leptos::task::spawn_local(async move {
        // A daemon without settings yet still gets a usable UI.
        let config = api::get::<AppConfig>("/api/app-config")
            .await
            .unwrap_or_default();
        shell.config.set(Some(config));
    });
    shell.reload_agents();
    Effect::new(move |_| {
        if let Some(theme) = shell.config.with(|c| c.as_ref().map(|c| c.theme)) {
            apply_theme(theme);
        }
    });

    view! {
        <Router>
            <OnboardingGate/>
            <header class="topbar">
                <A href="/" attr:class="brand">
                    <span class="brand-mark" aria-hidden="true"></span>
                    "Open Design"
                </A>
                <nav class="topnav">
                    <A href="/" exact=true>"Home"</A>
                    <A href="/projects">"Projects"</A>
                    <A href="/settings">"Settings"</A>
                </nav>
            </header>
            <main class="main">
                <Routes fallback=|| view! { <p class="empty">"Page not found."</p> }>
                    <Route path=path!("/") view=home::Home/>
                    <Route path=path!("/projects") view=projects::Projects/>
                    <Route path=path!("/projects/:id") view=studio::Studio/>
                    <Route path=path!("/onboarding") view=onboarding::Onboarding/>
                    <Route path=path!("/settings") view=settings_page::SettingsPage/>
                </Routes>
            </main>
        </Router>
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
        if done == Some(false) && location.pathname.get() != "/onboarding" {
            navigate("/onboarding", Default::default());
        }
    });
}

/// Relative "x min ago" from epoch milliseconds.
pub(crate) fn ago(ms: i64) -> String {
    let now = js_sys::Date::now() as i64;
    let mins = (now - ms).max(0) / 60_000;
    match mins {
        0 => "just now".to_owned(),
        1..=59 => format!("{mins} min ago"),
        60..=1439 => format!("{} h ago", mins / 60),
        _ => format!("{} d ago", mins / 1440),
    }
}

/// Mounts the app on `<body>`.
pub fn mount() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
