//! DeepSeek Harness connection setup (track C3; the original's
//! `DeepSeekHarnessSetupDialog`): when `dsh` is installed but its
//! `open-design` profile component is not, its agent card opens this
//! dialog. Confirming installs the bundled component through the user's
//! `dsh`, rescans, and selects the agent once it is available.

use super::icons::Glyph;
use super::lib_ui::Modal;
use super::{Shell, api};
use crate::events::{AgentInfo, AgentsResponse};
use crate::i18n::{T, t};
use leptos::prelude::*;
use serde_json::{Value, json};

/// The agent whose component this dialog installs.
pub const DSH_ID: &str = "deepseek-harness";

/// Setup state while the dialog is open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupState {
    pub busy: bool,
    pub error: Option<String>,
}

#[component]
pub fn DshSetupDialog(
    /// `Some` while open.
    state: RwSignal<Option<SetupState>>,
    picked: RwSignal<Option<String>>,
) -> impl IntoView {
    let shell = Shell::get();
    let busy = move || state.with(|s| s.as_ref().is_some_and(|s| s.busy));
    let close = Callback::new(move |()| {
        if !busy() {
            state.set(None);
        }
    });
    let confirm = move |_| {
        if busy() {
            return;
        }
        state.set(Some(SetupState {
            busy: true,
            error: None,
        }));
        leptos::task::spawn_local(async move {
            let path = format!("/api/agents/{DSH_ID}/companion/install");
            let installed = match api::post::<_, Value>(&path, &json!({})).await {
                Ok(_) => api::get::<AgentsResponse>("/api/agents")
                    .await
                    .map_err(|e| e.message),
                Err(e) => Err(e.message),
            };
            match installed {
                Ok(r) if r.agents.iter().any(|a| a.id == DSH_ID && a.available) => {
                    picked.set(Some(DSH_ID.to_owned()));
                    state.set(None);
                    // Last: the new list rebuilds the card that owns this
                    // dialog.
                    shell.agents.set(Some(Ok(r.agents)));
                }
                Ok(_) => state.set(Some(SetupState {
                    busy: false,
                    error: Some(t(T::DshSetupRequired).to_owned()),
                })),
                Err(error) => state.set(Some(SetupState {
                    busy: false,
                    error: Some(error),
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
            <div class="dialog-body">
                <p class="dsh-setup-note">{t(T::DshSetupNote)}</p>
                {move || state.with(|s| s.as_ref().and_then(|s| s.error.clone())).map(|e| view! {
                    <p class="error-text" role="alert" data-testid="deepseek-harness-setup-error">{e}</p>
                })}
            </div>
            <div class="dialog-foot">
                <button class="btn" disabled=busy on:click=move |_| close.run(())>{t(T::Cancel)}</button>
                <button
                    class="btn primary"
                    data-testid="deepseek-harness-setup-confirm"
                    disabled=busy
                    on:click=confirm
                >
                    {move || busy().then(|| view! { <span class="spinner on-accent" aria-hidden="true"></span> })}
                    {move || t(if busy() { T::DshSetupInstalling } else { T::DshSetupConfirm })}
                </button>
            </div>
        </Modal>
    }
}

/// Whether `agent`'s card should open the setup dialog instead of
/// showing it as missing.
#[must_use]
pub fn needs_setup(agent: &AgentInfo) -> bool {
    agent.id == DSH_ID && !agent.available && agent.needs_setup && agent.path.is_some()
}
