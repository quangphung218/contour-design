//! Comments on the preview (spec 1.2): pins over the frame for the shown
//! file, and the note box for an element picked in comment mode. Attached
//! comments ride along with the next message (`commentAttachments`).

use super::api;
use super::bridge;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use crate::preview_bridge as pb;
use leptos::prelude::*;
use od_contracts::{
    CommentRect, CommentResponse, CommentStatus, CreateCommentRequest, PreviewComment,
};

/// Pin label: the element's tag, else "element".
pub fn comment_label(c: &PreviewComment) -> String {
    if c.label.is_empty() {
        t(T::ElementFallback).to_owned()
    } else {
        c.label.clone()
    }
}

fn add_comment(
    ctx: StudioCtx,
    note: String,
    images: RwSignal<Vec<String>>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) {
    let (Some(picked), Some(file), Some(cid)) = (
        ctx.comment_draft.get_untracked(),
        ctx.active_untracked(),
        ctx.conversation_id.get_untracked(),
    ) else {
        return;
    };
    let req = CreateCommentRequest {
        file_path: file,
        element_id: picked.od_id.clone().unwrap_or_default(),
        selector: picked.selector.clone(),
        label: picked.tag.clone(),
        text: picked.text.clone(),
        position: picked.rect,
        html_hint: String::new(),
        slide_index: ctx.deck.get_untracked().map(|d| d.active),
        note,
        status: Some(CommentStatus::Attached),
        attachments: images.get_untracked(),
    };
    busy.set(true);
    leptos::task::spawn_local(async move {
        let path = format!("{}/conversations/{}/comments", ctx.base(), api::seg(&cid));
        match api::post::<_, CommentResponse>(&path, &req).await {
            Ok(r) => {
                ctx.comments.update(|c| c.push(r.comment));
                ctx.comment_draft.set(None);
            }
            Err(e) => error.set(Some(tf(T::CommentFailed, &[&e.to_string()]))),
        }
        busy.set(false);
    });
}

#[component]
pub fn CommentLayer(ctx: StudioCtx) -> impl IntoView {
    let open_pin = RwSignal::new(None::<String>);
    let pins = move || {
        let file = ctx.active();
        let slide = ctx.deck.get().map(|d| d.active);
        ctx.comments.with(|list| {
            list.iter()
                .filter(|c| Some(&c.file_path) == file.as_ref())
                .filter(|c| matches!(c.status, CommentStatus::Open | CommentStatus::Attached))
                .filter(|c| slide.is_none() || c.slide_index.is_none() || c.slide_index == slide)
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    // Ask the frame where these elements are now; it keeps answering on
    // scroll and resize, and a new document (`od:ready`) asks again.
    Effect::new(move |_| {
        ctx.frame_ready.track();
        let shown = pins();
        bridge::post_main(&pb::locate(
            shown.iter().map(|c| (c.id.as_str(), c.selector.as_str())),
        ));
    });
    view! {
        <div class="comment-layer" aria-live="polite">
            {move || {
                let rects = ctx.pin_rects.get();
                pins()
                    .into_iter()
                    .filter_map(|c| {
                        let number = ctx.comments.with_untracked(|l| crate::comment_board::number(l, &c));
                        let id = c.id.clone();
                        let id_open = c.id.clone();
                        let attached = c.status == CommentStatus::Attached;
                        let at = rects.get(&c.id).copied().unwrap_or(c.position);
                        let style = pin_style(&at)?;
                        Some(view! {
                            <div class="pin-wrap" style=style>
                                <button
                                    class="pin"
                                    class:attached=attached
                                    data-testid="comment-pin"
                                    title=c.note.clone()
                                    on:click=move |_| open_pin.update(|o| {
                                        *o = if o.as_deref() == Some(id_open.as_str()) { None } else { Some(id_open.clone()) };
                                    })
                                >
                                    {number}
                                </button>
                                {move || (open_pin.get().as_deref() == Some(id.as_str())).then(|| {
                                    let id_status = id.clone();
                                    let id_remove = id.clone();
                                    view! {
                                        <div class="pin-card">
                                            <strong>{comment_label(&c)}</strong>
                                            <p>{c.note.clone()}</p>
                                            <div class="row">
                                                <button
                                                    class="btn small"
                                                    on:click=move |_| ctx.set_comment_status(
                                                        &id_status,
                                                        if attached { CommentStatus::Open } else { CommentStatus::Attached },
                                                    )
                                                >
                                                    {if attached { t(T::Detach) } else { t(T::AttachToNext) }}
                                                </button>
                                                <button class="btn small ghost" on:click=move |_| ctx.remove_comment(&id_remove)>
                                                    {t(T::RemoveComment)}
                                                </button>
                                            </div>
                                        </div>
                                    }
                                })}
                            </div>
                        })
                    })
                    .collect_view()
            }}
            {move || ctx.comment_draft.get().map(|picked| view! { <DraftBox ctx=ctx picked=picked/> })}
        </div>
    }
}

#[component]
fn DraftBox(ctx: StudioCtx, picked: crate::preview_bridge::Picked) -> impl IntoView {
    let note = RwSignal::new(String::new());
    let images = RwSignal::new(Vec::<String>::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let view_height = super::bridge::main_frame_height();
    let style = draft_style(&picked.rect, view_height);
    let label = draft_label(&picked.tag, picked.label.as_deref().unwrap_or(&picked.text));
    let submit = move || {
        let text = note.get_untracked().trim().to_owned();
        if !text.is_empty() && !busy.get_untracked() {
            add_comment(ctx, text, images, busy, error);
        }
    };
    let submit_key = submit;
    view! {
        <div class="comment-draft" style=style data-testid="comment-draft">
            <span class="muted small">{label}</span>
            <textarea
                class="input"
                rows="2"
                data-testid="comment-note"
                placeholder=t(T::CommentPlaceholder)
                prop:value=move || note.get()
                on:input=move |ev| note.set(event_target_value(&ev))
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    // Cmd/Ctrl+Enter sends (Shift+Enter is a new line), and
                    // so does a plain Enter.
                    let primary = ev.meta_key() || ev.ctrl_key();
                    if ev.key() == "Enter" && (primary || !ev.shift_key()) && !ev.is_composing() {
                        ev.prevent_default();
                        submit_key();
                    } else if ev.key() == "Escape" {
                        ctx.comment_draft.set(None);
                    }
                }
            ></textarea>
            <super::comments_panel::DraftImages ctx=ctx images=images error=error/>
            {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            <div class="row end tight">
                <button class="btn small ghost" on:click=move |_| ctx.comment_draft.set(None)>{t(T::Cancel)}</button>
                <button
                    class="btn small primary"
                    data-testid="comment-add"
                    disabled=move || busy.get() || note.with(|n| n.trim().is_empty())
                    on:click=move |_| submit()
                >
                    {t(T::AddComment)}
                </button>
            </div>
        </div>
    }
}

/// Where a pin sits over the frame: at the element's top right, or nowhere
/// once the element is scrolled above the frame, rather than parked on the
/// top edge over whatever shows there (QA-059).
fn pin_style(at: &CommentRect) -> Option<String> {
    (at.y + at.height > 0.0).then(|| {
        format!(
            "left:{:.0}px;top:{:.0}px",
            at.x + at.width - 10.0,
            (at.y - 10.0).max(0.0)
        )
    })
}

/// Height the note box needs, about (label, two rows, buttons).
const DRAFT_HEIGHT: f64 = 180.0;

/// Where the note box opens: under the element, or pinned to the frame's
/// visible bottom when that would put it below the frame (QA-057: a tall
/// element hid "Add comment" under the preview's edge). `view` is the
/// frame's height when known.
fn draft_style(at: &CommentRect, view: Option<f64>) -> String {
    let left = at.x.max(0.0);
    let below = (at.y + at.height + 6.0).max(0.0);
    if view.is_some_and(|h| below + DRAFT_HEIGHT > h) {
        format!("left:{left:.0}px;bottom:6px")
    } else {
        format!("left:{left:.0}px;top:{below:.0}px")
    }
}

/// The note box's label: the tag and the start of the element's text, cut
/// at a word (QA-060).
fn draft_label(tag: &str, text: &str) -> String {
    const MAX: usize = 40;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= MAX {
        return format!("<{tag}> {text}");
    }
    let head: String = text.chars().take(MAX + 1).collect();
    let cut = match head.rfind(' ') {
        Some(i) if i > 0 => head[..i].to_owned(),
        _ => head.chars().take(MAX).collect(),
    };
    format!("<{tag}> {}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(y: f64, height: f64) -> CommentRect {
        CommentRect {
            x: 20.0,
            y,
            width: 300.0,
            height,
        }
    }

    #[test]
    fn a_pin_scrolled_above_the_frame_hides() {
        assert_eq!(pin_style(&rect(100.0, 50.0)).as_deref(), Some("left:310px;top:90px"));
        // Partly shown: the pin stays on the frame's top edge.
        assert_eq!(pin_style(&rect(-40.0, 100.0)).as_deref(), Some("left:310px;top:0px"));
        assert_eq!(pin_style(&rect(-500.0, 100.0)), None);
    }

    #[test]
    fn the_note_box_stays_inside_the_frame() {
        assert_eq!(draft_style(&rect(100.0, 50.0), Some(800.0)), "left:20px;top:156px");
        // Taller than the frame: pinned to its visible bottom.
        assert_eq!(draft_style(&rect(0.0, 2000.0), Some(800.0)), "left:20px;bottom:6px");
        assert_eq!(draft_style(&rect(700.0, 50.0), Some(800.0)), "left:20px;bottom:6px");
        // Frame height unknown: under the element, as before.
        assert_eq!(draft_style(&rect(0.0, 2000.0), None), "left:20px;top:2006px");
    }

    #[test]
    fn the_note_box_label_cuts_at_a_word() {
        assert_eq!(draft_label("h1", "Hello tea"), "<h1> Hello tea");
        assert_eq!(
            draft_label("section", "Mục 1 — Tiêu đề có dấu Đoạn văn 1. Link ngắn gọn"),
            "<section> Mục 1 — Tiêu đề có dấu Đoạn văn 1. Link…"
        );
        // One long word: cut where it must.
        assert_eq!(
            draft_label("p", &"x".repeat(50)),
            format!("<p> {}…", "x".repeat(40))
        );
        assert_eq!(draft_label("div", ""), "<div> ");
    }
}
