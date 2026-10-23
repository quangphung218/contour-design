//! Project workspace recovery tip (upstream `ProjectWorkspaceRecoveryTip`):
//! while the project's event stream to the daemon is down, a small status
//! card in the bottom-left corner says the link is being restored, and the
//! Studio keeps showing what it has. Once the stream is back, files and
//! conversations are read again (events missed meanwhile are gone).

use super::studio::StudioCtx;
use crate::i18n::{T, t};
use leptos::prelude::*;

impl StudioCtx {
    /// The project stream opened (`up`) or dropped.
    pub(super) fn on_project_link(self, up: bool) {
        let was_down = self.link_down.get_untracked();
        if was_down == up {
            self.link_down.set(!up);
        }
        if up && was_down {
            self.refresh_files();
            self.reload_conversations();
            self.reload_preview();
        }
    }
}

#[component]
pub fn WorkspaceRecoveryTip(ctx: StudioCtx) -> impl IntoView {
    move || {
        ctx.link_down.get().then(|| {
            view! {
                <div class="workspace-recovery-tip" role="status" aria-live="polite" data-testid="workspace-recovery-tip">
                    <span class="spinner small" aria-hidden="true"></span>
                    <span>{t(T::WorkspaceRecovering)}</span>
                </div>
            }
        })
    }
}
