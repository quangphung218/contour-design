//! DeepSeek Harness connection setup (track C3; the original's
//! `DeepSeekHarnessSetupDialog`): while DeepSeek Harness is not available,
//! its agent card opens this dialog. The original installs a bundled
//! profile component; this app does not ship it, so the dialog shows how
//! to install the `dsh` CLI and its `open-design` profile, links the
//! bundled guide, and detects again (`POST /api/agents/:id/detect`),
//! selecting the agent once it is available.

use super::icons::{Glyph, icon};
use super::lib_ui::{Modal, copy_text};
use super::{Shell, api};
use crate::events::AgentInfo;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;

/// The agent this dialog sets up.
pub const DSH_ID: &str = "deepseek-harness";

/// The bootstrap installers the original publishes for `dsh`, by shell.
const INSTALL_COMMANDS: [(&str, &str); 2] = [
    (
        "macOS / Linux",
        "curl -fsSL 'https://open-design.ai/install-dsh.sh?version=1' | sh",
    ),
    (
        "Windows PowerShell",
        "& ([scriptblock]::Create((irm 'https://open-design.ai/install-dsh.ps1?version=1')))",
    ),
];

/// Adds the profile package (built from the original's
/// `packages/dsh-runtime`, see the guide) and checks it.
const PROFILE_COMMAND: &str = "dsh plugin --profile open-design add <folder>/open-design-dsh-runtime-0.1.0.tgz\ndsh --profile open-design --probe";

/// The bundled guide's section on DeepSeek Harness.
const GUIDE_LINK: &str = "/docs#deepseek-harness";

/// Setup state while the dialog is open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupState {
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Detected {
    agent: AgentInfo,
}

/// Why a detected agent is still not available.
fn still_missing(agent: &AgentInfo) -> String {
    t(if agent.path.is_none() {
        T::DshSetupNoCli
    } else {
        T::DshSetupNoProfile
    })
    .to_owned()
}

#[component]
fn Command(label: &'static str, text: &'static str, testid: &'static str) -> impl IntoView {
    let copied = RwSignal::new(false);
    view! {
        <div class="dsh-command" data-testid=testid>
            <div class="dsh-command-head">
                <span class="muted small">{label}</span>
                <button class="btn ghost small" on:click=move |_| {
                    leptos::task::spawn_local(async move {
                        if copy_text(text).await.is_ok() {
                            copied.set(true);
                        }
                    });
                }>
                    {icon(Glyph::Clipboard)}
                    {move || t(if copied.get() { T::Copied } else { T::Copy })}
                </button>
            </div>
            <pre class="dsh-command-text"><code>{text}</code></pre>
        </div>
    }
}

#[component]
pub fn DshSetupDialog(
    /// `Some` while open.
    state: RwSignal<Option<SetupState>>,
    picked: RwSignal<Option<String>>,
    /// Where `dsh` was found, if it was.
    cli_path: Option<String>,
) -> impl IntoView {
    let shell = Shell::get();
    let cli_path = RwSignal::new(cli_path);
    let busy = move || state.with(|s| s.as_ref().is_some_and(|s| s.busy));
    let close = Callback::new(move |()| {
        if !busy() {
            state.set(None);
        }
    });
    let detect = move |_| {
        if busy() {
            return;
        }
        state.set(Some(SetupState {
            busy: true,
            error: None,
        }));
        leptos::task::spawn_local(async move {
            let path = format!("/api/agents/{DSH_ID}/detect");
            match api::post::<_, Detected>(&path, &json!({})).await {
                Ok(Detected { agent }) if agent.available => {
                    picked.set(Some(DSH_ID.to_owned()));
                    state.set(None);
                    // Last: the new list rebuilds the card that owns this
                    // dialog.
                    shell.agents.update(|list| {
                        if let Some(Ok(agents)) = list {
                            if let Some(a) = agents.iter_mut().find(|a| a.id == DSH_ID) {
                                *a = agent;
                            }
                        }
                    });
                }
                Ok(Detected { agent }) => {
                    cli_path.set(agent.path.clone());
                    state.set(Some(SetupState {
                        busy: false,
                        error: Some(still_missing(&agent)),
                    }));
                }
                Err(e) => state.set(Some(SetupState {
                    busy: false,
                    error: Some(e.message),
                })),
            }
        });
    };
    view! {
        <Modal
            title=t(T::DshSetupTitle).to_owned()
            subtitle=t(T::DshSetupDescription).to_owned()
            glyph=Glyph::Agent
            on_close=close
            testid="deepseek-harness-setup-dialog"
        >
            <div class="dialog-body dsh-setup">
                <section class="dsh-step" data-testid="deepseek-harness-step-cli">
                    <h3 class="dsh-step-title">{t(T::DshStepCli)}</h3>
                    {move || match cli_path.get() {
                        Some(p) => view! {
                            <p class="success-text small" data-testid="deepseek-harness-cli-found">
                                {icon(Glyph::CheckCircle)}{tf(T::DshCliFound, &[&p])}
                            </p>
                        }.into_any(),
                        None => INSTALL_COMMANDS.into_iter().map(|(label, cmd)| view! {
                            <Command label=label text=cmd testid="deepseek-harness-install-command"/>
                        }).collect_view().into_any(),
                    }}
                </section>
                <section class="dsh-step" data-testid="deepseek-harness-step-profile">
                    <h3 class="dsh-step-title">{t(T::DshStepProfile)}</h3>
                    <p class="muted small">{t(T::DshStepProfileHint)}</p>
                    <Command label="dsh" text=PROFILE_COMMAND testid="deepseek-harness-profile-command"/>
                </section>
                <p class="dsh-setup-note">
                    {t(T::DshSetupNote)}" "
                    <a href=GUIDE_LINK data-testid="deepseek-harness-guide" on:click=move |_| state.set(None)>
                        {icon(Glyph::Book)}{t(T::DshSetupGuide)}
                    </a>
                </p>
                {move || state.with(|s| s.as_ref().and_then(|s| s.error.clone())).map(|e| view! {
                    <p class="error-text" role="alert" data-testid="deepseek-harness-setup-error">{e}</p>
                })}
            </div>
            <div class="dialog-foot">
                <button class="btn" disabled=busy on:click=move |_| close.run(())>{t(T::Cancel)}</button>
                <button
                    class="btn primary"
                    data-testid="deepseek-harness-setup-detect"
                    disabled=busy
                    on:click=detect
                >
                    {move || if busy() {
                        view! { <span class="spinner on-accent" aria-hidden="true"></span> }.into_any()
                    } else {
                        icon(Glyph::Refresh).into_any()
                    }}
                    {move || t(if busy() { T::DshSetupDetecting } else { T::DshSetupDetect })}
                </button>
            </div>
        </Modal>
    }
}

/// Whether `agent`'s card should open the setup dialog instead of
/// showing it as missing: DeepSeek Harness, until it is available.
#[must_use]
pub fn needs_setup(agent: &AgentInfo) -> bool {
    agent.id == DSH_ID && !agent.available
}
