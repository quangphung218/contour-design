//! First run (spec 1.5, no cloud step), as the original's runtime setup:
//! a local coding agent (picked, then tested once with a smoke prompt) or
//! an API key (BYOK form with its own test). Continue waits until the
//! chosen runtime answered; Skip is always there.

use super::Shell;
use super::api;
use super::byok_form::{ByokForm, TestResultLine, TestState};
use super::icons::{Glyph, icon};
use crate::events::AgentInfo;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use od_contracts::BrowserResponse;
use serde_json::{Value, json};

#[component]
pub fn Onboarding() -> impl IntoView {
    let shell = Shell::get();
    let navigate = use_navigate();
    let picked = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let use_key = RwSignal::new(false);
    let test = RwSignal::new(None::<TestState>);
    let byok_ready = RwSignal::new(false);
    // Track C2: step 2, a design system extracted from the brand's site.
    let brand = RwSignal::new(super::onboarding_brand::BrandState::Idle);
    let extracting = move || matches!(brand.get(), super::onboarding_brand::BrandState::Running(_));
    let checking = RwSignal::new(false);
    // PDF, PPTX and image export drive Chrome: say so now, not at export.
    let browser = LocalResource::new(|| api::get::<BrowserResponse>("/api/browser"));
    let no_browser = move || browser.get().and_then(Result::ok).is_some_and(|b| !b.found);
    let no_agent =
        move || matches!(shell.agents.get(), Some(Ok(list)) if !list.iter().any(installed));
    // Check again found nothing: say so, a press that changes nothing reads
    // as a broken button.
    let still_none = RwSignal::new(false);
    let windows = on_windows();
    // Preselect the first available agent once detection answers.
    Effect::new(move |_| {
        if picked.get_untracked().is_none() {
            if let Some(Ok(list)) = shell.agents.get() {
                picked.set(
                    list.iter()
                        .find(|a| a.available && a.id != "byok")
                        .map(|a| a.id.clone()),
                );
            }
        }
    });
    // Test the picked agent once per pick (the original auto-tests); only
    // the latest test's answer is shown.
    let seq = StoredValue::new(0u32);
    let run_test = move |agent: String| {
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        test.set(Some(TestState::Running));
        leptos::task::spawn_local(async move {
            let body = json!({ "mode": "agent", "agentId": agent });
            let res = match api::post::<_, Value>("/api/test/connection", &body).await {
                Ok(v) => TestState::Done(v),
                Err(e) => TestState::Failed(e.to_string()),
            };
            if seq.get_value() == mine {
                test.set(Some(res));
            }
        });
    };
    Effect::new(move |_| {
        let Some(agent) = picked.get() else {
            test.set(None);
            return;
        };
        run_test(agent);
    });
    // Rescan, then test the picked agent again: after signing in, the same
    // agent now answers. With none picked, the preselect above picks and
    // tests a newly found one.
    let check_again =
        Callback::new(move |()| {
            if checking.get_untracked() {
                return;
            }
            checking.set(true);
            let had = picked.get_untracked();
            leptos::task::spawn_local(async move {
                shell.rescan_agents().await;
                checking.set(false);
                still_none.set(shell.agents.with_untracked(
                    |a| matches!(a, Some(Ok(list)) if !list.iter().any(installed)),
                ));
                if let Some(agent) = had.filter(|a| picked.get_untracked().as_ref() == Some(a)) {
                    run_test(agent);
                }
            });
        });
    // Claude Code or Codex failed its test for want of a sign-in (the
    // daemon's `category`): say how to sign in.
    let sign_in_again = move || {
        let auth = matches!(test.get(), Some(TestState::Done(v)) if v["ok"] != true && v["category"] == "auth");
        let agent = picked.get()?;
        let featured = FEATURED.iter().find(|f| f.id == agent)?;
        (auth && !windows)
            .then(|| view! { <SignInLine command=featured.sign_in testid="onboarding-signin"/> })
    };
    let agent_ready = move || matches!(test.get(), Some(TestState::Done(v)) if v["ok"] == true);
    let test_failed = move || match test.get() {
        Some(TestState::Done(v)) => v["ok"] != true,
        Some(TestState::Failed(_)) => true,
        _ => false,
    };
    // An installed agent failed: say why the install cards appeared under it.
    let failed_lead = move || {
        if !test_failed() || no_agent() {
            return None;
        }
        let id = picked.get()?;
        let name = shell.agents.with(|a| match a {
            Some(Ok(list)) => list.iter().find(|a| a.id == id).map(|a| a.name.clone()),
            _ => None,
        })?;
        Some(view! {
            <p class="agent-test-lead small" data-testid="onboarding-test-lead">{tf(T::AgentTestFailedLead, &[&name])}</p>
        })
    };
    // No agent works yet: none installed, or the picked one failed its
    // test. The ways forward (install cards, API key) stay in view.
    let need_help = Signal::derive(move || no_agent() || test_failed());
    let ready = move || {
        if use_key.get() {
            byok_ready.get()
        } else {
            agent_ready()
        }
    };

    let finish = move |agent: Option<String>| {
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let mut next = shell.config.get_untracked().unwrap_or_default();
            if agent.is_some() {
                next.agent_id = agent;
            }
            // The extracted system becomes the default for new projects.
            if let super::onboarding_brand::BrandState::Done(id, _) = brand.get_untracked() {
                next.extra.insert(
                    crate::settings::DEFAULT_DESIGN_SYSTEM_KEY.to_owned(),
                    serde_json::json!(id),
                );
            }
            next.onboarding_completed = true;
            match shell.save_config(next).await {
                Ok(()) => navigate("/", Default::default()),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let finish_skip = finish.clone();
    let on_byok_saved = Callback::new(move |()| byok_ready.set(true));

    view! {
        <section class="page onboarding">
            <div class="onboarding-card card">
                <header class="onboarding-head">
                    {super::brand_mark("brand-mark large")}
                    <p class="onboarding-brand"><strong>{super::APP_NAME}</strong><span class="muted">{t(T::Tagline)}</span></p>
                    <h1>{t(T::OnboardingTitle)}</h1>
                    <p class="muted">{t(T::OnboardingIntro)}</p>
                </header>
                <div class="segmented" role="tablist" aria-label=t(T::ExecutionMode)>
                    <button class="seg" role="tab" data-testid="onboarding-local"
                        aria-selected=move || (!use_key.get()).to_string() class:active=move || !use_key.get()
                        on:click=move |_| use_key.set(false)>{icon(Glyph::Terminal)}{t(T::LocalAgent)}</button>
                    <button class="seg" role="tab" data-testid="onboarding-byok"
                        aria-selected=move || use_key.get().to_string() class:active=move || use_key.get()
                        on:click=move |_| use_key.set(true)>{icon(Glyph::Key)}{t(T::UseApiKey)}</button>
                </div>
                <Show
                    when=move || !use_key.get()
                    fallback=move || view! {
                        <p class="byok-limits" data-testid="byok-limits">{icon(Glyph::Info)}{t(T::ByokLimits)}</p>
                        <p class="muted">{t(T::ByokIntro)}</p>
                        <ByokForm on_saved=on_byok_saved/>
                    }
                >
                    {move || (!no_agent()).then(|| view! { <p class="muted">{t(T::LocalAgentIntro)}</p> })}
                    {move || no_agent().then(|| view! {
                        <p class="agents-none" data-testid="agents-none">
                            {t(if windows { T::AgentsNoneHintWindows } else { T::AgentsNoneHint })}
                        </p>
                    })}
                    // The test result sits under the installed agents, above
                    // the install cards it explains.
                    <AgentPicker picked=picked need_help=need_help
                        rescan=(!windows).then_some(Rescan { checking, on_click: check_again, still_none })
                        status=ViewFn::from(move || view! {
                            <div class="row">
                                <TestResultLine state=test testid="onboarding-test"/>
                            </div>
                            {failed_lead}
                            {sign_in_again}
                        })/>
                    {move || need_help.get().then(|| view! {
                        <p class="byok-alt muted small" data-testid="byok-alt">
                            <button class="link-btn" on:click=move |_| use_key.set(true)>{t(T::UseApiKeyInstead)}</button>
                            <span>{t(T::ByokLimits)}</span>
                        </p>
                    })}
                </Show>
                // Extracting a brand runs the agent: only the picked one, once
                // it passed its test.
                <Show when=agent_ready>
                    <super::onboarding_brand::BrandStep state=brand agent=picked/>
                </Show>
                {move || no_browser().then(|| view! {
                    <p class="onboarding-note" data-testid="onboarding-chrome">
                        {icon(Glyph::Warning)}
                        <span>{t(T::ChromeNeeded)}</span>
                        <a href=CHROME_URL target="_blank" rel="noopener">{t(T::GetChrome)}{icon(Glyph::ExternalLink)}</a>
                    </p>
                })}
                {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                <div class="onboarding-foot">
                    // With no agent the button sits under the install cards.
                    <Show when=move || !use_key.get() && !no_agent() && !windows>
                        <CheckAgainButton checking=checking on_click=check_again testid="onboarding-check-again"/>
                    </Show>
                    <span class="spacer"></span>
                    <button class="btn ghost" on:click=move |_| finish_skip(None)>{t(T::SkipForNow)}</button>
                    <span data-tip=move || {
                        if extracting() {
                            Some(t(T::BrandStepWait))
                        } else {
                            (!ready()).then(|| t(if use_key.get() { T::GateByok } else { T::GateAgent }))
                        }
                    }>
                        <button
                            class="btn primary"
                            data-testid="onboarding-continue"
                            disabled=move || !ready() || extracting()
                            on:click=move |_| finish(if use_key.get_untracked() { Some("byok".to_owned()) } else { picked.get_untracked() })
                        >
                            {t(T::Continue)}
                            {icon(Glyph::ArrowRight)}
                        </button>
                    </span>
                </div>
            </div>
        </section>
    }
}

/// Where to get Chrome when export has no browser to drive.
const CHROME_URL: &str = "https://www.google.com/chrome/";

/// On this machine: ready, or installed and waiting for setup (DeepSeek
/// Harness without its profile). BYOK is not an agent here.
fn installed(a: &AgentInfo) -> bool {
    (a.available || a.needs_setup) && a.id != "byok"
}

/// An agent a new user is pointed to first.
struct Featured {
    id: &'static str,
    /// Install command for macOS and Linux. Windows gets the setup page
    /// only: detection there cannot find what it installs yet.
    unix: &'static str,
    /// Official setup page.
    docs: &'static str,
    /// What to run once installed to sign in.
    sign_in: &'static str,
    /// The account it needs, from the setup page.
    account: T,
}

const FEATURED: &[Featured] = &[
    Featured {
        id: "claude",
        unix: "curl -fsSL https://claude.ai/install.sh | bash",
        docs: "https://code.claude.com/docs/en/setup",
        sign_in: "claude",
        account: T::ClaudeAccount,
    },
    Featured {
        id: "codex",
        unix: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        docs: "https://developers.openai.com/codex/cli",
        sign_in: "codex login",
        account: T::CodexAccount,
    },
];

/// The page runs on Windows (the daemon is on the same machine).
fn on_windows() -> bool {
    web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Windows"))
}

/// Detected agents: installed ones as cards to pick. With none installed,
/// Claude Code and Codex come first with how to install them; every other
/// agent waits in a folded list. Shared with Settings. BYOK has its own form.
/// `need_help`: show the install cards even next to installed agents (none
/// of them works yet). `rescan`: a Check again button right under the
/// install cards (before the folded list), where onboarding's hint points.
/// `status`: shown right under the installed agents (onboarding's test).
#[component]
pub fn AgentPicker(
    picked: RwSignal<Option<String>>,
    #[prop(optional, into)] need_help: Option<Signal<bool>>,
    #[prop(default = None)] rescan: Option<Rescan>,
    #[prop(optional, into)] status: Option<ViewFn>,
) -> impl IntoView {
    let shell = Shell::get();
    move || {
        match shell.agents.get() {
        None => view! {
            <div class="agent-list" role="radiogroup">
                {(0..3).map(|_| view! { <div class="skeleton agent-skeleton"></div> }).collect_view()}
            </div>
        }
            .into_any(),
        Some(Err(e)) => {
            view! { <p class="error-text">{tf(T::AgentDetectionUnavailable, &[&e])}</p> }.into_any()
        }
        Some(Ok(list)) if list.is_empty() => view! {
            <div class="empty-state compact">
                <span class="empty-icon">{icon(Glyph::Agent)}</span>
                <p>{t(T::NoAgentsFound)}</p>
            </div>
        }
            .into_any(),
        Some(Ok(list)) => {
            // Installed, or installed and waiting for setup, stay in view;
            // the rest is folded away.
            let (ready, mut missing): (Vec<_>, Vec<_>) = list
                .into_iter()
                .filter(|a| a.id != "byok")
                .partition(installed);
            // Featured while nothing is installed, or nothing installed
            // works: then they are the way forward, otherwise noise next to
            // a working agent. One that is installed sits with the others.
            let none_installed = ready.is_empty();
            let featured: Vec<_> = if none_installed || need_help.is_some_and(|h| h.get()) {
                FEATURED
                    .iter()
                    .filter_map(|f| {
                        let at = missing.iter().position(|a| a.id == f.id)?;
                        Some((missing.remove(at), f))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            view! {
                {(!ready.is_empty()).then(|| view! {
                    <div class="agent-list" role="radiogroup">
                        {ready.into_iter().map(|a| view! { <AgentCard agent=a picked=picked/> }).collect_view()}
                    </div>
                })}
                {status.clone().map(|s| s.run())}
                {(!featured.is_empty()).then(|| view! {
                    <div class="agent-install-list">
                        {featured.into_iter().map(|(a, f)| view! {
                            <InstallCard agent=a featured=f/>
                        }).collect_view()}
                    </div>
                    // With an agent installed, Check again is in the footer.
                    {rescan.filter(|_| none_installed).map(|r| view! {
                        <div class="row check-again-row">
                            <CheckAgainButton checking=r.checking on_click=r.on_click testid="agents-check-again"/>
                        </div>
                        {move || r.still_none.get().then(|| view! {
                            <StillNoneLine/>
                        })}
                    })}
                })}
                {(!missing.is_empty()).then(|| {
                    let count = missing.len().to_string();
                    view! {
                        <details class="agent-missing" data-testid="agents-other">
                            <summary>{icon(Glyph::ChevronRight)}{tf(T::OtherAgentsCount, &[&count])}</summary>
                            <div class="agent-list compact">
                                {missing.into_iter().map(|a| view! { <AgentCard agent=a picked=picked/> }).collect_view()}
                            </div>
                        </details>
                    }
                })}
            }
                .into_any()
        }
    }
    }
}

/// Detects the agents again (`POST /api/agents/rescan` through
/// `on_click`); busy while `checking`. Onboarding and Settings ▸ Agent.
#[component]
pub fn CheckAgainButton(
    checking: RwSignal<bool>,
    on_click: Callback<()>,
    testid: &'static str,
) -> impl IntoView {
    view! {
        <button class="btn ghost" data-testid=testid
            disabled=move || checking.get()
            on:click=move |_| on_click.run(())>
            {move || if checking.get() {
                view! { <span class="spinner"></span> }.into_any()
            } else {
                icon(Glyph::Refresh).into_any()
            }}
            {move || t(if checking.get() { T::CheckingAgents } else { T::CheckAgain })}
        </button>
    }
}

/// Onboarding's Check again under the install cards.
#[derive(Clone, Copy)]
pub struct Rescan {
    pub checking: RwSignal<bool>,
    pub on_click: Callback<()>,
    /// The last Check again found no agent.
    pub still_none: RwSignal<bool>,
}

/// "Still no agent found. Open a new Terminal window and run
/// `claude --version` or `codex --version`."
#[component]
fn StillNoneLine() -> impl IntoView {
    let text = t(T::StillNoAgent);
    let (before, rest) = text.split_once("{0}").unwrap_or((text, ""));
    let (between, after) = rest.split_once("{1}").unwrap_or((rest, ""));
    view! {
        <p class="agents-still-none small" role="status" data-testid="agents-still-none">
            {before}<code>"claude --version"</code>{between}<code>"codex --version"</code>{after}
        </p>
    }
}

/// "Then open a new Terminal window and run `claude` to sign in."
#[component]
fn SignInLine(command: &'static str, testid: &'static str) -> impl IntoView {
    let text = t(T::SignInAfterInstall);
    let (before, after) = text.split_once("{0}").unwrap_or((text, ""));
    view! {
        <p class="install-signin muted small" data-testid=testid>
            {before}<code>{command}</code>{after}
        </p>
    }
}

/// A recommended agent that is not installed: the command to paste into
/// Terminal (or PowerShell) with a Copy button, the official setup page,
/// and how to sign in once installed.
#[component]
fn InstallCard(agent: AgentInfo, featured: &'static Featured) -> impl IntoView {
    let windows = on_windows();
    let command = (!windows).then_some(featured.unix);
    let copied = RwSignal::new(false);
    let copy = move |_| {
        let Some(command) = command else {
            return;
        };
        leptos::task::spawn_local(async move {
            if super::lib_ui::copy_text(command).await.is_ok() {
                copied.set(true);
                gloo_timers::future::TimeoutFuture::new(2_000).await;
                copied.set(false);
            }
        });
    };
    view! {
        <div class="agent-card agent-install" data-agent=agent.id.clone() data-testid="agent-install">
            <span class="agent-icon">{icon(Glyph::Download)}</span>
            <span class="agent-text">
                <span class="agent-name">{agent.name.clone()}</span>
                <span class="agent-detail">{t(T::NotInstalled)}</span>
            </span>
            <a class="btn ghost small" href=featured.docs target="_blank" rel="noopener" data-testid="install-docs">
                {t(T::InstallGuide)}{icon(Glyph::ExternalLink)}
            </a>
            {command.map(|command| view! {
                <div class="install-cmd">
                    // Scrolls sideways when narrow, so it takes keyboard focus.
                    <code data-testid="install-command" tabindex="0">{command}</code>
                    <button class="btn small" data-testid="install-copy" aria-label=t(T::CopyCommand) on:click=copy>
                        {move || if copied.get() { icon(Glyph::Check) } else { icon(Glyph::Clipboard) }}
                        {move || t(if copied.get() { T::Copied } else { T::Copy })}
                    </button>
                </div>
            })}
            {(!windows).then(|| view! { <SignInLine command=featured.sign_in testid="install-signin"/> })}
            <p class="install-account muted small" data-testid="install-account">{t(featured.account)}</p>
        </div>
    }
}

#[component]
pub fn AgentCard(agent: AgentInfo, picked: RwSignal<Option<String>>) -> impl IntoView {
    let id = agent.id.clone();
    let id_check = id.clone();
    // DeepSeek Harness not available yet: the card opens the setup dialog
    // instead of being a dead entry.
    let setup = super::dsh_setup::needs_setup(&agent);
    let cli_path = agent.path.clone();
    let setup_state = RwSignal::new(None::<super::dsh_setup::SetupState>);
    // Only opening and closing rebuild the dialog, not its busy/error state.
    let setup_open = Memo::new(move |_| setup_state.with(Option::is_some));
    let detail = if agent.available {
        agent.version.clone().map_or_else(
            || t(T::Installed).to_owned(),
            |v| tf(T::InstalledVersion, &[&v]),
        )
    } else if agent.needs_setup {
        t(T::DshSetupRequired).to_owned()
    } else if agent.path.is_none() {
        t(T::NotInstalled).to_owned()
    } else {
        agent
            .error
            .clone()
            .unwrap_or_else(|| t(T::NotInstalled).to_owned())
    };
    let available = agent.available;
    let detail_title = detail.clone();
    // Not installed: a plain row, not a button that cannot be pressed.
    if !available && !setup {
        return view! {
            <div class="agent-card compact missing" data-agent=agent.id.clone()>
                <span class="agent-icon">{icon(Glyph::Download)}</span>
                <span class="agent-text">
                    <span class="agent-name">{agent.name.clone()}</span>
                    <span class="agent-detail" title=detail_title>{detail}</span>
                </span>
            </div>
        }
        .into_any();
    }
    let selected = move || picked.get().as_deref() == Some(id_check.as_str());
    view! {
        <button
            class="agent-card"
            class:compact=!available
            class:needs-setup=setup
            class:active=selected.clone()
            role="radio"
            aria-checked=move || selected().to_string()
            data-agent=agent.id.clone()
            data-needs-setup=setup.then_some("true")
            on:click=move |_| {
                if setup {
                    setup_state.set(Some(super::dsh_setup::SetupState::default()));
                } else {
                    picked.set(Some(id.clone()));
                }
            }
        >
            <span class="agent-icon">{icon(if available { Glyph::Agent } else { Glyph::Settings })}</span>
            <span class="agent-text">
                <span class="agent-name">{agent.name.clone()}</span>
                <span class="agent-detail" title=detail_title>{detail}</span>
            </span>
            {available.then(|| view! {
                <span class="agent-check" aria-hidden="true">{icon(Glyph::CheckCircle)}</span>
            })}
        </button>
        {move || setup_open.get().then(|| view! {
            <super::dsh_setup::DshSetupDialog state=setup_state picked=picked cli_path=cli_path.clone()/>
        })}
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, available: bool, needs_setup: bool) -> AgentInfo {
        AgentInfo {
            id: id.to_owned(),
            name: id.to_owned(),
            bin: id.to_owned(),
            available,
            path: (available || needs_setup).then(|| format!("/bin/{id}")),
            version: None,
            capabilities: Vec::new(),
            supports_resume: false,
            supports_steer: false,
            error: None,
            needs_setup,
        }
    }

    #[test]
    fn an_agent_waiting_for_setup_counts_as_installed() {
        assert!(installed(&agent("claude", true, false)));
        assert!(installed(&agent("deepseek-harness", false, true)));
        assert!(!installed(&agent("codex", false, false)));
        assert!(!installed(&agent("byok", true, false)));
    }
}
