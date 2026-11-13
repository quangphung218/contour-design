//! File version history (spec 1.2): list the versions of the previewed HTML
//! file by source (AI, manual, restore), view one, restore it.

use super::file_state::ReadState;
use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, ViewedVersion, is_html_path};
use super::tasks::Tasks;
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
    let versions = RwSignal::new(ReadState::<Vec<FileVersion>>::default());
    let tasks = Tasks::here();
    // Each load's number: an answer that comes back after a newer load
    // started (another file picked, its content changed) is dropped.
    let seq = StoredValue::new(0u64);
    let reload = move || {
        seq.update_value(|n| *n += 1);
        let mine = seq.get_value();
        let Some(path) = ctx.active_untracked().filter(|p| is_html_path(p)) else {
            versions.update(|v| v.read(Ok(Vec::new())));
            return;
        };
        versions.update(ReadState::retry);
        tasks.spawn(async move {
            let res = api::get::<FileVersionsResponse>(&versions_url(ctx, &path))
                .await
                .map(|r| {
                    let mut list = r.versions;
                    list.sort_by_key(|a| std::cmp::Reverse(a.version));
                    list
                })
                .map_err(|e| ctx.failed(&e));
            if seq.try_get_value() == Some(mine) {
                versions.update(|v| v.read(res));
            }
        });
    };
    // Refresh when the file or its content changes; another file's list is
    // not kept.
    Effect::new(move |shown: Option<Option<String>>| {
        let active = ctx.active();
        ctx.preview_rev.track();
        if shown.is_some_and(|s| s != active) {
            versions.set(ReadState::default());
        }
        reload();
        active
    });
    on_cleanup(move || ctx.back_to_live());

    // Each View's number: only the last one shows.
    let viewed = StoredValue::new(0u64);
    let view_version = move |v: FileVersion| {
        let Some(path) = ctx.active_untracked() else {
            return;
        };
        viewed.update_value(|n| *n += 1);
        let mine = (seq.get_value(), viewed.get_value());
        // The panel's: closed, it shows no version any more.
        tasks.spawn(async move {
            let url = format!("{}/{}", versions_url(ctx, &path), api::seg(&v.id));
            let res = api::get::<VersionContentResponse>(&url).await;
            // Another file picked (or its content changed) meanwhile: the
            // panel stays, so this version would show over the new file and
            // Restore would post it against that file (QA-089). Another View
            // since: that one shows.
            if (seq.get_value(), viewed.get_value()) != mine {
                return;
            }
            match res {
                Ok(r) => ctx.view(ViewedVersion {
                    path,
                    id: v.id,
                    version: v.version,
                    content: r.content,
                }),
                Err(e) => ctx
                    .notice
                    .set(Some(ctx.failed_to(T::VersionOpenFailed, &e))),
            }
        });
    };
    // `path`: the file the version is of, never another file picked since.
    let restore = move |path: String, id: String| {
        // The Studio's: a restore asked for shows even if the panel closes.
        ctx.tasks.spawn(async move {
            let url = format!("{}/{}/restore", versions_url(ctx, &path), api::seg(&id));
            match api::post::<_, RestoreVersionResponse>(&url, &RestoreVersionRequest::default())
                .await
            {
                Ok(_) => {
                    ctx.back_to_live();
                    ctx.reload_preview();
                }
                Err(e) => ctx.notice.set(Some(ctx.failed_to(T::RestoreFailed, &e))),
            }
        });
    };

    let panel = NodeRef::<leptos::html::Aside>::new();
    super::focus::focus_while_open(panel, |_| None);
    view! {
        <aside class="versions" data-testid="versions" tabindex="-1" node_ref=panel>
            <div class="pane-head">
                <span class="pane-ico">{icon(Glyph::History)}</span>
                <span class="pane-title" title=move || tf(T::VersionsTitle, &[&ctx.active().unwrap_or_default()])>
                    {move || tf(T::VersionsTitle, &[&ctx.active().unwrap_or_default()])}
                </span>
                <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| ctx.history_open.set(false)>
                    {icon(Glyph::Close)}
                </button>
            </div>
            {move || ctx.viewed().map(|v| {
                let (path, id) = (v.path.clone(), v.id.clone());
                view! {
                    <div class="viewing">
                        <strong>{icon(Glyph::Eye)}{tf(T::ViewingVersion, &[&v.version.to_string()])}</strong>
                        <div class="row">
                            <button class="btn small" on:click=move |_| ctx.back_to_live()>{t(T::BackToLive)}</button>
                            <button class="btn small primary" on:click=move |_| restore(path.clone(), id.clone())>{icon(Glyph::Restore)}{t(T::Restore)}</button>
                        </div>
                    </div>
                }
            })}
            <ul class="version-list">
                {move || match versions.with(|v| (v.value().cloned(), v.error().map(str::to_owned))) {
                    (None, None) => view! { <li class="empty small">{t(T::Loading)}</li> }.into_any(),
                    (None, Some(e)) => view! {
                        <li class="error-text small" role="alert" data-testid="versions-read-error">
                            {e}
                            <button class="btn small" data-testid="versions-retry" on:click=move |_| reload()>
                                {icon(Glyph::Restore)}{t(T::NextRetry)}
                            </button>
                        </li>
                    }.into_any(),
                    (Some(list), _) if list.is_empty() => {
                        let msg = if ctx.active().is_some_and(|p| is_html_path(&p)) {
                            t(T::NoVersions)
                        } else {
                            t(T::VersionsOnlyHtml)
                        };
                        view! { <li class="empty small">{msg}</li> }.into_any()
                    }
                    (Some(list), _) => list
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
                                            on:click=move |_| if let Some(path) = ctx.active_untracked() { restore(path, id.clone()) }
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
