//! Comments on the preview (spec 1.2): pins over the frame for the shown
//! file, and the note box for an element picked in comment mode. Attached
//! comments ride along with the next message (`commentAttachments`).

use super::api;
use super::bridge;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use crate::preview_bridge as pb;
use leptos::prelude::*;
use od_contracts::{CommentResponse, CommentStatus, CreateCommentRequest, PreviewComment};

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
                    .enumerate()
                    .map(|(i, c)| {
                        let id = c.id.clone();
                        let id_open = c.id.clone();
                        let attached = c.status == CommentStatus::Attached;
                        let at = rects.get(&c.id).copied().unwrap_or(c.position);
                        let style = format!(
                            "left:{:.0}px;top:{:.0}px",
                            at.x + at.width - 10.0,
                            (at.y - 10.0).max(0.0)
                        );
                        view! {
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
                                    {i + 1}
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
                        }
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
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let style = format!(
        "left:{:.0}px;top:{:.0}px",
        picked.rect.x.max(0.0),
        (picked.rect.y + picked.rect.height + 6.0).max(0.0)
    );
    let submit = move || {
        let text = note.get_untracked().trim().to_owned();
        if !text.is_empty() && !busy.get_untracked() {
            add_comment(ctx, text, busy, error);
        }
    };
    let submit_key = submit;
    view! {
        <div class="comment-draft" style=style data-testid="comment-draft">
            <span class="muted small">{format!("<{}> {}", picked.tag, picked.text.chars().take(40).collect::<String>())}</span>
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
