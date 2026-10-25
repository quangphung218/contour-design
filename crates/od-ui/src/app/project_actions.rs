//! Project actions of the files panel: create a design system from this
//! project (the agent reads its pages and styles; a card says when the
//! system is ready and offers to use it here) and duplicate the project.

use super::api;
use super::files_panel::{FilesUi, anchor_style};
use super::icons::{Glyph, icon};
use super::menu_scrim::MenuScrim;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::{DuplicateProjectResponse, HandoffResponse, ProjectResponse};
use serde_json::{Value, json};

/// Poll interval of the generation job, ms.
const POLL_MS: u32 = 1500;
/// Pages and styles named in the brief at most.
const LISTED_FILES: usize = 20;

/// The design system made from this project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DsJob {
    Running(String),
    Ready { name: String, id: String },
    Failed(String),
}

/// What the agent is asked: describe the system the project already uses.
#[must_use]
pub fn brief(project: &str, dir: &str, files: &[String]) -> String {
    let mut out = format!(
        "Derive the design system of the existing project \"{project}\". Its files are in `{dir}`: read the pages and styles there and capture what they already do (colors, type, spacing, components, motion, voice). Do not invent a new direction.\n"
    );
    let listed: Vec<&String> = files
        .iter()
        .filter(|f| {
            let l = f.to_ascii_lowercase();
            l.ends_with(".html") || l.ends_with(".htm") || l.ends_with(".css")
        })
        .take(LISTED_FILES)
        .collect();
    if !listed.is_empty() {
        out.push_str("\nStart with:\n");
        for f in listed {
            out.push_str(&format!("- `{f}`\n"));
        }
    }
    out
}

fn start(ctx: StudioCtx, ui: FilesUi) {
    let name = ctx
        .project
        .with_untracked(|p| p.as_ref().map(|p| p.name.clone()))
        .unwrap_or_default();
    let files: Vec<String> = ctx
        .files
        .with_untracked(|f| f.iter().map(|f| f.path.clone()).collect());
    let agent = ctx.shell.agent_id();
    ui.ds_job.set(Some(DsJob::Running(name.clone())));
    leptos::task::spawn_local(async move {
        let fail = |e: String| ui.ds_job.set(Some(DsJob::Failed(e)));
        let dir = match api::get::<HandoffResponse>(&format!("{}/handoff", ctx.base())).await {
            Ok(h) => h.dir,
            Err(e) => return fail(e.to_string()),
        };
        let body = json!({ "name": name, "brief": brief(&name, &dir, &files), "agentId": agent });
        let job = match api::post::<_, Value>("/api/design-systems/generate", &body).await {
            Ok(v) => v["job"]["id"].as_str().unwrap_or_default().to_owned(),
            Err(e) => return fail(e.to_string()),
        };
        loop {
            gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
            let v = match api::get::<Value>(&format!("/api/design-systems/jobs/{}", api::seg(&job)))
                .await
            {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            match v["job"]["status"].as_str() {
                Some("succeeded") => {
                    ui.ds_job.set(Some(DsJob::Ready {
                        name: v["job"]["name"].as_str().unwrap_or_default().to_owned(),
                        id: v["job"]["designSystemId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    }));
                    return;
                }
                Some("failed") => {
                    return fail(v["job"]["error"].as_str().unwrap_or_default().to_owned());
                }
                _ => {}
            }
        }
    });
}

/// Asks first when an edit is not applied: the copy opens with a full load
/// (QA-020).
fn duplicate(ctx: StudioCtx) {
    super::leave_guard::leave_then(ctx.shell, move || duplicate_now(ctx));
}

fn duplicate_now(ctx: StudioCtx) {
    leptos::task::spawn_local(async move {
        match api::post::<_, DuplicateProjectResponse>(
            &format!("{}/duplicate", ctx.base()),
            &json!({}),
        )
        .await
        {
            Ok(r) => {
                // A full load: the Studio starts over on the copy.
                if let Some(w) = web_sys::window() {
                    let _ = w
                        .location()
                        .set_href(&format!("/projects/{}", api::seg(&r.project.id)));
                }
            }
            Err(e) => ctx.notice.set(Some(e.to_string())),
        }
    });
}

/// "…" in the files panel head.
#[component]
pub fn ProjectMenu(ctx: StudioCtx) -> impl IntoView {
    let ui = FilesUi::get();
    let open = RwSignal::new(false);
    let pos = RwSignal::new(String::new());
    let running = move || ui.ds_job.with(|j| matches!(j, Some(DsJob::Running(_))));
    view! {
        <div class="menu-wrap">
            <button class="icon-btn" aria-label=t(T::ProjectActions) data-tip=t(T::ProjectActions) data-tip-pos="left"
                aria-haspopup="menu" aria-expanded=move || open.get().to_string() data-testid="project-menu"
                on:click=move |ev| { pos.set(anchor_style(&ev, false)); open.update(|o| *o = !*o); }>
                {icon(Glyph::More)}
            </button>
            <Show when=move || open.get()>
                <MenuScrim on_close=move || open.set(false)/>
                <div class="menu" role="menu" data-testid="project-menu-list" style=move || pos.get()>
                    <button class="menu-item" role="menuitem" data-testid="project-refresh"
                        on:click=move |_| { open.set(false); ui.changed(ctx); }>
                        {icon(Glyph::Refresh)}{t(T::RefreshFiles)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="project-ds" disabled=running
                        on:click=move |_| { open.set(false); start(ctx, ui); }>
                        {icon(Glyph::DesignSystem)}{t(T::DsFromProject)}
                    </button>
                    <button class="menu-item" role="menuitem" data-testid="project-duplicate"
                        on:click=move |_| { open.set(false); duplicate(ctx); }>
                        {icon(Glyph::Duplicate)}{t(T::DuplicateProject)}
                    </button>
                </div>
            </Show>
        </div>
    }
}

/// Progress of the design system made from this project, then "ready":
/// use it for this project, or dismiss.
#[component]
pub fn BrandReady(ctx: StudioCtx) -> impl IntoView {
    let ui = FilesUi::get();
    let busy = RwSignal::new(false);
    let use_it = move |id: String| {
        busy.set(true);
        leptos::task::spawn_local(async move {
            match api::patch::<_, ProjectResponse>(&ctx.base(), &json!({ "designSystemId": id }))
                .await
            {
                Ok(r) => {
                    ctx.project.set(Some(r.project));
                    ui.ds_job.set(None);
                    ui.say(t(T::DsInUse).to_owned());
                }
                Err(e) => ctx.notice.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };
    move || {
        let job = ui.ds_job.get()?;
        let dismiss = move |_| ui.ds_job.set(None);
        Some(match job {
            DsJob::Running(name) => view! {
                <div class="brand-ready" role="status" data-testid="brand-progress">
                    <span class="spinner"></span>
                    <div class="grow">
                        <strong>{tf(T::DsFromProjectRunning, &[&name])}</strong>
                        <p class="subtle">{t(T::DsFromProjectHint)}</p>
                    </div>
                </div>
            }
            .into_any(),
            DsJob::Ready { name, id } => view! {
                <div class="brand-ready" role="status" data-testid="brand-ready">
                    <span class="dialog-icon">{icon(Glyph::DesignSystem)}</span>
                    <div class="grow">
                        <strong>{tf(T::DsReadyTitle, &[&name])}</strong>
                        <p class="subtle">{t(T::DsReadyHint)}</p>
                        <div class="row tight">
                            <button class="btn small primary" data-testid="brand-use" disabled=move || busy.get()
                                on:click=move |_| use_it(id.clone())>
                                {icon(Glyph::Check)}{t(T::DsUseHere)}
                            </button>
                            <button class="btn small ghost" data-testid="brand-dismiss" on:click=dismiss>{t(T::Dismiss)}</button>
                        </div>
                    </div>
                </div>
            }
            .into_any(),
            DsJob::Failed(e) => view! {
                <div class="brand-ready error" role="alert" data-testid="brand-failed">
                    <span class="toast-ico">{icon(Glyph::Warning)}</span>
                    <div class="grow">
                        <strong>{t(T::DsFromProjectFailed)}</strong>
                        <p class="subtle">{e}</p>
                    </div>
                    <button class="chip-x" aria-label=t(T::Close) on:click=dismiss>{icon(Glyph::Close)}</button>
                </div>
            }
            .into_any(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::brief;

    #[test]
    fn the_brief_points_at_pages_and_styles() {
        let files = ["index.html", "css/site.css", "logo.png", "about.htm"].map(str::to_owned);
        let b = brief("Tea", "/data/projects/p1", &files);
        assert!(
            b.contains("\"Tea\"") && b.contains("`/data/projects/p1`"),
            "{b}"
        );
        assert!(
            b.contains("- `index.html`\n- `css/site.css`\n- `about.htm`\n"),
            "{b}"
        );
        assert!(!b.contains("logo.png"));
    }
}
