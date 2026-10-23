//! File version history (spec 1.2): list the versions of the previewed HTML
//! file by source (AI, manual, restore), view one, restore it.

use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, ViewedVersion, is_html_path};
use super::{ago, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::{
    FileVersion, FileVersionsResponse, RestoreVersionRequest, RestoreVersionResponse,
    VersionContentResponse, VersionSource,
};

fn source_label(s: VersionSource) -> &'static str {
    match s {
        VersionSource::Ai => t(T::SourceAi),
        VersionSource::Manual => t(T::SourceManual),
        VersionSource::Restore => t(T::SourceRestore),
        VersionSource::Sync => t(T::SourceSync),
    }
}

fn versions_url(ctx: StudioCtx, path: &str) -> String {
    format!("{}/files/{}/versions", ctx.base(), api::file_path(path))
}

#[component]
pub fn VersionsPanel(ctx: StudioCtx) -> impl IntoView {
    let versions = RwSignal::new(None::<Result<Vec<FileVersion>, String>>);
    let reload = move || {
        let Some(path) = ctx.active_untracked().filter(|p| is_html_path(p)) else {
            versions.set(Some(Ok(Vec::new())));
            return;
        };
        leptos::task::spawn_local(async move {
            let res = api::get::<FileVersionsResponse>(&versions_url(ctx, &path))
                .await
                .map(|r| {
                    let mut list = r.versions;
                    list.sort_by(|a, b| b.version.cmp(&a.version));
                    list
                })
                .map_err(|e| e.to_string());
            versions.set(Some(res));
        });
    };
    // Refresh when the file or its content changes.
    Effect::new(move |_| {
        ctx.active();
        ctx.preview_rev.get();
        reload();
    });
    on_cleanup(move || ctx.viewing.set(None));

    let view_version = move |v: FileVersion| {
        let Some(path) = ctx.active_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let url = format!("{}/{}", versions_url(ctx, &path), api::seg(&v.id));
            match api::get::<VersionContentResponse>(&url).await {
                Ok(r) => ctx.viewing.set(Some(ViewedVersion {
                    id: v.id,
                    version: v.version,
                    content: r.content,
                })),
                Err(e) => ctx.notice.set(Some(e.to_string())),
            }
        });
    };
    let restore = move |id: String| {
        let Some(path) = ctx.active_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let url = format!("{}/{}/restore", versions_url(ctx, &path), api::seg(&id));
            match api::post::<_, RestoreVersionResponse>(&url, &RestoreVersionRequest::default())
                .await
            {
                Ok(_) => {
                    ctx.viewing.set(None);
                    ctx.reload_preview();
                }
                Err(e) => ctx
                    .notice
                    .set(Some(tf(T::RestoreFailed, &[&e.to_string()]))),
            }
        });
    };

    view! {
        <aside class="versions" data-testid="versions">
            <div class="pane-head">
                <span class="pane-ico">{icon(Glyph::History)}</span>
                <span class="pane-title" title=move || tf(T::VersionsTitle, &[&ctx.active().unwrap_or_default()])>
                    {move || tf(T::VersionsTitle, &[&ctx.active().unwrap_or_default()])}
                </span>
                <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| ctx.history_open.set(false)>
                    {icon(Glyph::Close)}
                </button>
            </div>
            {move || ctx.viewing.get().map(|v| {
                let id = v.id.clone();
                view! {
                    <div class="viewing">
                        <strong>{icon(Glyph::Eye)}{tf(T::ViewingVersion, &[&v.version.to_string()])}</strong>
                        <div class="row">
                            <button class="btn small" on:click=move |_| ctx.viewing.set(None)>{t(T::BackToLive)}</button>
                            <button class="btn small primary" on:click=move |_| restore(id.clone())>{icon(Glyph::Restore)}{t(T::Restore)}</button>
                        </div>
                    </div>
                }
            })}
            <ul class="version-list">
                {move || match versions.get() {
                    None => view! { <li class="empty small">{t(T::Loading)}</li> }.into_any(),
                    Some(Err(e)) => view! { <li class="error-text small">{e}</li> }.into_any(),
                    Some(Ok(list)) if list.is_empty() => {
                        let msg = if ctx.active().is_some_and(|p| is_html_path(&p)) {
                            t(T::NoVersions)
                        } else {
                            t(T::VersionsOnlyHtml)
                        };
                        view! { <li class="empty small">{msg}</li> }.into_any()
                    }
                    Some(Ok(list)) => list
                        .into_iter()
                        .map(|v| {
                            let current = v.current;
                            let id = v.id.clone();
                            let shown = v.clone();
                            view! {
                                <li class="version" data-testid="version" data-version=v.version.to_string()>
                                    <div class="version-head">
                                        <strong class="version-n">{tf(T::VersionLabel, &[&v.version.to_string()])}</strong>
                                        <span class="badge outline">{source_label(v.source)}</span>
                                        {current.then(|| view! { <span class="badge accent">{t(T::Current)}</span> })}
                                        <span class="spacer"></span>
                                        <span class="subtle small">{ago(v.created_at)}</span>
                                    </div>
                                    {v.prompt.clone().map(|p| view! { <p class="version-prompt">{p}</p> })}
                                    <div class="row">
                                        <button class="btn small ghost" data-testid="version-view" on:click=move |_| view_version(shown.clone())>
                                            {icon(Glyph::Eye)}
                                            {t(T::View)}
                                        </button>
                                        <button
                                            class="btn small"
                                            data-testid="version-restore"
                                            disabled=current
                                            on:click=move |_| restore(id.clone())
                                        >
                                            {icon(Glyph::Restore)}
                                            {t(T::Restore)}
                                        </button>
                                    </div>
                                </li>
                            }
                        })
                        .collect_view()
                        .into_any(),
                }}
            </ul>
        </aside>
    }
}
