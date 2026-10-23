//! The Comments tab (spec 1.2, upstream comment side list): the shown
//! file's comments as a numbered list beside the preview, with their
//! status, whether their anchor still holds (moved or lost since), the
//! images attached to them, and actions to send, resolve or remove. A
//! click outlines the comment's element in the preview.

use super::api;
use super::bridge;
use super::icons::{Glyph, icon};
use super::preview_tools::{Tool, panel_head};
use super::studio::StudioCtx;
use crate::comment_board::{self as cb, AnchorState};
use crate::i18n::{T, t, tf};
use crate::preview_bridge::{self as pb, Located};
use leptos::prelude::*;
use od_contracts::{CommentStatus, PreviewComment};

impl StudioCtx {
    /// The frame located the pinned comments: record their anchor state.
    pub(super) fn on_located(self, items: &[Located]) {
        let found = self.comments.with_untracked(|list| {
            items
                .iter()
                .filter_map(|l| {
                    let c = list.iter().find(|c| c.id == l.id)?;
                    Some((l.id.clone(), cb::anchor_state(&c.text, l)))
                })
                .collect()
        });
        self.anchors.set(found);
    }
}

fn status_label(status: CommentStatus) -> &'static str {
    match status {
        CommentStatus::Open => t(T::CommentStatusOpen),
        CommentStatus::Attached => t(T::CommentStatusAttached),
        CommentStatus::Applying => t(T::CommentStatusApplying),
        CommentStatus::Resolved => t(T::CommentStatusResolved),
    }
}

/// The toolbar button that opens the tab, with the count of open comments.
#[component]
pub fn CommentsToggle(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let open = move || {
        let file = ctx.active().unwrap_or_default();
        ctx.comments.with(|l| cb::board(l, &file, false).len())
    };
    view! {
        <button
            class="btn small ghost"
            class:active=move || tools.tool.get() == Some(Tool::Comments)
            data-testid="comments-tab"
            aria-pressed=move || (tools.tool.get() == Some(Tool::Comments)).to_string()
            on:click=move |_| ctx.toggle_tool(Tool::Comments)
        >
            {icon(Glyph::Board)}
            {t(T::CommentsTab)}
            {move || match open() {
                0 => None,
                n => Some(view! { <span class="count-badge" data-testid="comments-count">{n}</span> }),
            }}
        </button>
    }
}

#[component]
pub fn CommentsPanel(ctx: StudioCtx) -> impl IntoView {
    let resolved = RwSignal::new(false);
    let selected = RwSignal::new(None::<String>);
    // Leaving the tab takes the outline off the page.
    on_cleanup(|| bridge::post_main(&pb::mark(None, false)));
    let items = move || {
        let file = ctx.active().unwrap_or_default();
        let show_resolved = resolved.get();
        ctx.comments.with(|list| {
            cb::board(list, &file, show_resolved)
                .into_iter()
                .map(|c| (cb::number(list, c), c.clone()))
                .collect::<Vec<_>>()
        })
    };
    let filter = move |value: bool, label: T, testid: &'static str| {
        view! {
            <button
                class="seg"
                role="radio"
                data-testid=testid
                aria-checked=move || (resolved.get() == value).to_string()
                class:active=move || resolved.get() == value
                on:click=move |_| resolved.set(value)
            >
                {t(label)}
            </button>
        }
    };
    view! {
        {panel_head(ctx, t(T::CommentsTab).to_owned())}
        <div class="tool-body comments-board" data-testid="comments-panel">
            <div class="segmented small" role="radiogroup" aria-label=t(T::CommentsTab)>
                {filter(false, T::CommentsFilterOpen, "comments-filter-open")}
                {filter(true, T::CommentsFilterResolved, "comments-filter-resolved")}
            </div>
            {move || {
                let list = items();
                if list.is_empty() {
                    let text = if resolved.get() { t(T::CommentsNoneResolved) } else { t(T::CommentsNone) };
                    return view! {
                        <div class="empty-state compact" data-testid="comments-empty">
                            <span class="empty-icon">{icon(Glyph::Comment)}</span>
                            <p>{text}</p>
                        </div>
                    }
                    .into_any();
                }
                view! {
                    <ul class="comment-list">
                        {list
                            .into_iter()
                            .map(|(n, c)| view! { <CommentItem ctx=ctx number=n comment=c selected=selected/> })
                            .collect_view()}
                    </ul>
                }
                .into_any()
            }}
        </div>
    }
}

#[component]
fn CommentItem(
    ctx: StudioCtx,
    number: usize,
    comment: PreviewComment,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let id = comment.id.clone();
    let anchor_id = StoredValue::new(id.clone());
    let anchor = move || {
        ctx.anchors
            .with(|a| anchor_id.with_value(|id| a.get(id).copied()))
    };
    let is_selected = {
        let id = id.clone();
        move || selected.with(|s| s.as_deref() == Some(id.as_str()))
    };
    let pick = {
        let (id, selector) = (id.clone(), comment.selector.clone());
        move |_| {
            selected.set(Some(id.clone()));
            bridge::post_main(&pb::mark(Some(&selector), true));
        }
    };
    let status = comment.status;
    let (id_send, id_resolve, id_remove) = (id.clone(), id.clone(), id.clone());
    let images = comment.attachments.clone();
    let base = ctx.base();
    view! {
        <li
            class="comment-item"
            class:selected=is_selected
            data-testid="comment-item"
            data-status=status.as_str()
            data-anchor=move || anchor().map_or("unknown", AnchorState::as_str)
        >
            <button class="comment-item-main" on:click=pick>
                <span class="comment-item-head">
                    <span class="pin-number">{number}</span>
                    <strong class="mono small">{super::preview_comments::comment_label(&comment)}</strong>
                    <span class="badge" class:accent=status == CommentStatus::Attached>{status_label(status)}</span>
                    {move || match anchor() {
                        Some(AnchorState::Stale) => Some(view! {
                            <span class="badge warning" data-testid="anchor-stale">{icon(Glyph::Warning)}{t(T::AnchorStale)}</span>
                        }.into_any()),
                        Some(AnchorState::Lost) => Some(view! {
                            <span class="badge danger" data-testid="anchor-lost">{icon(Glyph::Warning)}{t(T::AnchorLost)}</span>
                        }.into_any()),
                        _ => None,
                    }}
                    <span class="spacer"></span>
                    <span class="subtle small">{super::ago(comment.created_at)}</span>
                </span>
                <span class="comment-item-note">{comment.note.clone()}</span>
            </button>
            {(!images.is_empty()).then(|| view! {
                <div class="comment-images">
                    {images
                        .into_iter()
                        .map(|img| {
                            let url = format!("{base}/raw/{}", api::file_path(&img.path));
                            let href = url.clone();
                            view! {
                                <a class="comment-image" href=href target="_blank" rel="noopener" title=img.name.clone() data-testid="comment-image">
                                    <img src=url alt=img.name.clone()/>
                                </a>
                            }
                        })
                        .collect_view()}
                </div>
            })}
            <div class="row wrap tight">
                {(status != CommentStatus::Resolved && status != CommentStatus::Applying).then(|| {
                    let attached = status == CommentStatus::Attached;
                    view! {
                        <button
                            class="btn small"
                            data-testid="comment-send-toggle"
                            on:click=move |_| ctx.set_comment_status(
                                &id_send,
                                if attached { CommentStatus::Open } else { CommentStatus::Attached },
                            )
                        >
                            {if attached { t(T::Detach) } else { t(T::AttachToNext) }}
                        </button>
                    }
                })}
                <button
                    class="btn small ghost"
                    data-testid="comment-resolve"
                    on:click=move |_| ctx.set_comment_status(
                        &id_resolve,
                        if status == CommentStatus::Resolved { CommentStatus::Open } else { CommentStatus::Resolved },
                    )
                >
                    {icon(if status == CommentStatus::Resolved { Glyph::Restore } else { Glyph::Check })}
                    {if status == CommentStatus::Resolved { t(T::CommentReopen) } else { t(T::CommentResolve) }}
                </button>
                <button class="btn small ghost" data-testid="comment-remove" on:click=move |_| ctx.remove_comment(&id_remove)>
                    {icon(Glyph::Delete)}{t(T::RemoveComment)}
                </button>
            </div>
        </li>
    }
}

/// The images picked for a comment draft: upload buttons and chips.
#[component]
pub fn DraftImages(
    ctx: StudioCtx,
    images: RwSignal<Vec<String>>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let input = NodeRef::<leptos::html::Input>::new();
    let busy = RwSignal::new(false);
    let picked = move |_| {
        let Some(el) = input.get() else { return };
        let files: Vec<web_sys::File> = el
            .files()
            .map(|l| (0..l.length()).filter_map(|i| l.get(i)).collect())
            .unwrap_or_default();
        el.set_value("");
        let room = od_contracts::MAX_COMMENT_IMAGES.saturating_sub(images.get_untracked().len());
        let files: Vec<_> = files.into_iter().take(room).collect();
        if files.is_empty() {
            return;
        }
        busy.set(true);
        leptos::task::spawn_local(async move {
            match super::files_panel::upload(ctx, files, COMMENT_IMAGES_DIR).await {
                Ok(paths) => {
                    images.update(|i| i.extend(paths));
                    ctx.refresh_files();
                }
                Err(e) => error.set(Some(tf(T::CommentImageFailed, &[&e]))),
            }
            busy.set(false);
        });
    };
    let base = ctx.base();
    view! {
        <div class="draft-images">
            {move || images.get().into_iter().map(|path| {
                let url = format!("{base}/raw/{}", api::file_path(&path));
                let gone = path.clone();
                view! {
                    <span class="draft-image" data-testid="draft-image">
                        <img src=url alt=""/>
                        <button class="chip-x" aria-label=t(T::Remove) on:click=move |_| images.update(|i| i.retain(|p| p != &gone))>
                            {icon(Glyph::Close)}
                        </button>
                    </span>
                }
            }).collect_view()}
            <input
                type="file"
                accept="image/*"
                multiple
                hidden
                node_ref=input
                data-testid="comment-image-input"
                on:change=picked
            />
            <button
                class="btn small ghost"
                data-testid="comment-attach-image"
                disabled=move || busy.get() || images.with(|i| i.len() >= od_contracts::MAX_COMMENT_IMAGES)
                on:click=move |_| { if let Some(el) = input.get() { el.click(); } }
            >
                {icon(Glyph::Image)}{t(T::CommentAttachImage)}
            </button>
        </div>
    }
}

/// Project folder of images attached to comments.
const COMMENT_IMAGES_DIR: &str = "comments";
