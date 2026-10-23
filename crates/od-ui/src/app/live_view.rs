//! Live artifacts in the Studio: listed at the top of the files panel and
//! opened as tabs (`live:<id>`) with Preview, Code (the template), Data
//! (its JSON) and the refresh history, plus Refresh for artifacts whose
//! source allows it. Over `/api/live-artifacts/*`.

use super::api;
use super::files_panel::FilesUi;
use super::icons::{Glyph, icon};
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde_json::{Value, json};

/// Tab paths of live artifacts start with this.
pub const PREFIX: &str = "live:";

#[must_use]
pub fn is_live(path: &str) -> bool {
    path.starts_with(PREFIX)
}

/// Tab label: the artifact's title (its id until the list is in).
pub fn tab_label(ui: FilesUi, path: &str) -> String {
    let id = path.strip_prefix(PREFIX).unwrap_or(path);
    ui.live
        .with_untracked(|list| {
            list.iter()
                .find(|a| a["id"] == id)
                .and_then(|a| a["title"].as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| id.to_owned())
}

fn query(ctx: StudioCtx) -> String {
    format!("projectId={}", api::seg(&ctx.project_id.get_value()))
}

/// Re-reads the project's live artifacts.
pub fn refresh_list(ctx: StudioCtx, ui: FilesUi) {
    leptos::task::spawn_local(async move {
        if let Ok(v) = api::get::<Value>(&format!("/api/live-artifacts?{}", query(ctx))).await {
            let list = v["artifacts"].as_array().cloned().unwrap_or_default();
            ui.live.set(list);
        }
    });
}

fn status_label(status: &str) -> &'static str {
    t(match status {
        "running" => T::LiveRefreshing,
        "succeeded" => T::LiveUpToDate,
        "failed" => T::LiveRefreshFailed,
        "never" => T::LiveNotRefreshable,
        _ => T::LiveReady,
    })
}

/// The "Live artifacts" group above the file tree.
#[component]
pub fn LiveList(ctx: StudioCtx) -> impl IntoView {
    let ui = FilesUi::get();
    move || {
        let list = ui.live.get();
        (!list.is_empty()).then(|| {
            view! {
                <div class="live-list" data-testid="live-list">
                    <div class="files-label eyebrow">{t(T::LiveArtifacts)}</div>
                    {list.into_iter().map(|a| {
                        let id = a["id"].as_str().unwrap_or_default().to_owned();
                        let path = format!("{PREFIX}{id}");
                        let active = path.clone();
                        let title = a["title"].as_str().unwrap_or_default().to_owned();
                        view! {
                            <button class="file" data-testid="live-row"
                                class:active=move || ctx.active().as_deref() == Some(active.as_str())
                                on:click=move |_| ctx.open_file(&path)>
                                <span class="file-caret" aria-hidden="true"></span>
                                <span class="file-ico kind-html">{icon(Glyph::Sparkle)}</span>
                                <span class="file-name">{title}</span>
                            </button>
                        }
                    }).collect_view()}
                </div>
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Preview,
    Code,
    Data,
    History,
}

impl Pane {
    const ALL: [Self; 4] = [Self::Preview, Self::Code, Self::Data, Self::History];

    fn label(self) -> &'static str {
        t(match self {
            Self::Preview => T::Preview,
            Self::Code => T::Code,
            Self::Data => T::LiveData,
            Self::History => T::LiveHistory,
        })
    }

    fn key(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Code => "code",
            Self::Data => "data",
            Self::History => "history",
        }
    }
}

/// One live artifact, shown in place of the preview.
#[component]
pub fn LiveView(ctx: StudioCtx, path: String) -> impl IntoView {
    let ui = FilesUi::get();
    let id = path.strip_prefix(PREFIX).unwrap_or(&path).to_owned();
    let base = format!("/api/live-artifacts/{}", api::seg(&id));
    let pane = RwSignal::new(Pane::Preview);
    let rev = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(None::<Result<String, String>>);
    let detail = {
        let base = base.clone();
        LocalResource::new(move || {
            let url = format!("{base}?{}", query(ctx));
            rev.track();
            async move { api::get::<Value>(&url).await.map_err(|e| e.to_string()) }
        })
    };
    let history = {
        let base = base.clone();
        LocalResource::new(move || {
            let url = format!("{base}/refreshes?{}", query(ctx));
            rev.track();
            async move { api::get::<Value>(&url).await.map_err(|e| e.to_string()) }
        })
    };
    let template = {
        let raw = format!("{base}/template?{}", query(ctx));
        LocalResource::new(move || {
            let raw = raw.clone();
            rev.track();
            async move { api::get_text(&raw).await.map_err(|e| e.to_string()) }
        })
    };
    let artifact = move || {
        detail
            .get()
            .and_then(Result::ok)
            .map(|v| v["artifact"].clone())
    };
    let refreshable = move || {
        let id = id.clone();
        ui.live.with(|l| {
            l.iter()
                .any(|a| a["id"] == id.as_str() && a["refreshable"] == true)
        })
    };
    let refresh = {
        let base = base.clone();
        move |_| {
            busy.set(true);
            message.set(None);
            let url = format!("{base}/refresh?{}", query(ctx));
            leptos::task::spawn_local(async move {
                let res = api::post::<_, Value>(&url, &json!({})).await;
                busy.set(false);
                match res {
                    Ok(_) => message.set(Some(Ok(t(T::LiveRefreshed).to_owned()))),
                    Err(e) => message.set(Some(Err(tf(T::LiveRefreshError, &[&e.to_string()])))),
                }
                rev.update(|r| *r = r.wrapping_add(1));
                refresh_list(ctx, ui);
            });
        }
    };
    let preview_src = format!("{base}/preview?{}", query(ctx));
    view! {
        <div class="md-view live-view" data-testid="live-view">
            <div class="md-bar">
                <div class="segmented small" role="radiogroup" aria-label=t(T::LiveArtifacts)>
                    {Pane::ALL.into_iter().map(|p| view! {
                        <button class="seg" role="radio" data-testid=format!("live-tab-{}", p.key())
                            aria-checked=move || (pane.get() == p).to_string() class:active=move || pane.get() == p
                            on:click=move |_| pane.set(p)>{p.label()}</button>
                    }).collect_view()}
                </div>
                <span class="spacer"></span>
                <span class="subtle" data-testid="live-status">
                    {move || artifact().map(|a| status_label(a["refreshStatus"].as_str().unwrap_or_default()))}
                </span>
                <button class="btn small" data-testid="live-refresh" disabled=move || busy.get() || !refreshable()
                    on:click=refresh>
                    {move || if busy.get() { view! { <span class="spinner"></span> }.into_any() } else { icon(Glyph::Refresh).into_any() }}
                    {t(T::Refresh)}
                </button>
            </div>
            {move || message.get().map(|m| match m {
                Ok(text) => view! { <p class="live-message" role="status" data-testid="live-message">{icon(Glyph::CheckCircle)}{text}</p> }.into_any(),
                Err(text) => view! { <p class="live-message error-text" role="alert" data-testid="live-message">{icon(Glyph::Warning)}{text}</p> }.into_any(),
            })}
            <div class="live-body">
                {move || match pane.get() {
                    Pane::Preview => {
                        let src = format!("{preview_src}&v={}", rev.get());
                        view! { <iframe class="doc-frame" data-testid="live-preview" title=t(T::Preview) sandbox="" src=src></iframe> }.into_any()
                    }
                    Pane::Code => match template.get() {
                        Some(Ok(text)) => view! { <pre class="code-src mono live-pre" data-testid="live-code">{text}</pre> }.into_any(),
                        Some(Err(e)) => view! { <p class="error-text">{e}</p> }.into_any(),
                        None => view! { <div class="skeleton code-skeleton"></div> }.into_any(),
                    },
                    Pane::Data => {
                        let data = artifact()
                            .map(|a| serde_json::to_string_pretty(&a["document"]["dataJson"]).unwrap_or_default())
                            .unwrap_or_default();
                        view! { <pre class="code-src mono live-pre" data-testid="live-data">{data}</pre> }.into_any()
                    }
                    Pane::History => {
                        let list = history.get().and_then(Result::ok).and_then(|v| v["refreshes"].as_array().cloned()).unwrap_or_default();
                        if list.is_empty() {
                            view! { <p class="muted live-empty">{t(T::LiveNoHistory)}</p> }.into_any()
                        } else {
                            view! {
                                <ol class="live-history" data-testid="live-history">
                                    {list.into_iter().rev().map(|r| {
                                        let status = r["status"].as_str().unwrap_or_default().to_owned();
                                        let (ok, failed) = (status == "succeeded", status == "failed");
                                        let when = r["finishedAt"].as_str().or_else(|| r["startedAt"].as_str()).unwrap_or_default().to_owned();
                                        let error = r["error"]["message"].as_str().or_else(|| r["error"].as_str()).map(str::to_owned);
                                        view! {
                                            <li>
                                                <span class="badge" class:success=ok class:danger=failed>{status}</span>
                                                <span class="mono subtle">{when}</span>
                                                {error.map(|e| view! { <span class="error-text">{e}</span> })}
                                            </li>
                                        }
                                    }).collect_view()}
                                </ol>
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>
    }
}
