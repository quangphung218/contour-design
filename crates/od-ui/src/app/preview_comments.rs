//! Comments on the preview (spec 1.2): pins over the frame for the shown
//! file, and the note box for an element picked in comment mode. Attached
//! comments ride along with the next message (`commentAttachments`).

use super::api;
use super::bridge;
use super::studio::StudioCtx;
use super::tasks::{Tasks, live};
use crate::comment_board::AnchorState;
use crate::i18n::{T, t};
use crate::preview_bridge::{self as pb, Located, Picked};
use leptos::prelude::*;
use od_contracts::{
    CommentRect, CommentResponse, CommentStatus, CreateCommentRequest, PreviewComment,
};
use std::collections::BTreeMap;

/// Pin label: the element's tag, else "element".
pub fn comment_label(c: &PreviewComment) -> String {
    if c.label.is_empty() {
        t(T::ElementFallback).to_owned()
    } else {
        c.label.clone()
    }
}

/// A comment being written on an element of a file. Drafts are kept by
/// file: another file shown leaves it, and it shows again with its file.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentDraft {
    pub picked: Picked,
    pub note: String,
    /// Images uploaded for it (`comments/…`).
    pub images: Vec<String>,
    /// Its element was not found when its file showed again: the note and
    /// images wait for an element picked again.
    pub lost: bool,
}

impl CommentDraft {
    /// Holds something the user would lose: a note or an image.
    #[must_use]
    pub fn unsent(&self) -> bool {
        !self.note.trim().is_empty() || !self.images.is_empty()
    }
}

/// Comment drafts by file.
pub type CommentDrafts = BTreeMap<String, CommentDraft>;

/// `od:locate` id of `file`'s draft on the element `selector`. It names the
/// selector: an answer for an element picked before is not this one's.
fn draft_locate_id(file: &str, selector: &str) -> String {
    format!("od-draft:{file}:{selector}")
}

/// `picked` in comment mode on `file`: a new draft, or the draft there,
/// with its note and images, on the element picked now.
pub fn pick_draft(drafts: &mut CommentDrafts, file: &str, picked: Picked) {
    match drafts.get_mut(file) {
        Some(d) => {
            d.picked = picked;
            d.lost = false;
        }
        None => {
            drafts.insert(
                file.to_owned(),
                CommentDraft {
                    picked,
                    note: String::new(),
                    images: Vec::new(),
                    lost: false,
                },
            );
        }
    }
}

/// Where `od:locate` found `file`'s draft element now, or that it is gone
/// or changed.
/// False when the answer has nothing for this draft.
fn locate_draft(drafts: &mut CommentDrafts, file: &str, items: &[Located]) -> bool {
    let Some(d) = drafts.get_mut(file) else {
        return false;
    };
    let id = draft_locate_id(file, &d.picked.selector);
    let Some(found) = items.iter().find(|l| l.id == id) else {
        return false;
    };
    // The selector may be a position (`p:nth-of-type(1)`): an element
    // there with other text is another one, or this one changed. Either
    // way the user picks it again, as a comment's anchor says "stale".
    match (
        crate::comment_board::anchor_state(&d.picked.text, found),
        found.rect,
    ) {
        (AnchorState::Anchored, Some(rect)) => {
            d.picked.rect = rect;
            d.lost = false;
        }
        _ => d.lost = true,
    }
    true
}

/// Of `images`, those no comment and no draft in `drafts` holds: files
/// nothing points to any more.
fn orphans(images: &[String], comments: &[PreviewComment], drafts: &CommentDrafts) -> Vec<String> {
    images
        .iter()
        .filter(|path| {
            !comments
                .iter()
                .any(|c| c.attachments.iter().any(|a| &a.path == *path))
                && !drafts.values().any(|d| d.images.contains(path))
        })
        .cloned()
        .collect()
}

/// Takes out of `drafts` those of files `keep` refuses, and returns them.
fn drop_gone(drafts: &mut CommentDrafts, keep: impl Fn(&str) -> bool) -> Vec<CommentDraft> {
    let gone: Vec<String> = drafts.keys().filter(|f| !keep(f)).cloned().collect();
    gone.iter().filter_map(|f| drafts.remove(f)).collect()
}

/// A file deleted or renamed takes its drafts along (comment, Manual
/// edit): nothing would show them again, and the leave guard would ask
/// about them for good.
pub fn forget_gone_drafts(ctx: StudioCtx) {
    Effect::new(move |_| {
        let present: std::collections::HashSet<String> = ctx
            .files
            .with(|files| files.iter().map(|f| f.path.clone()).collect());
        let keep = |p: &str| super::live_view::is_live(p) || present.contains(p);
        ctx.tools.manual.drop_gone(keep);
        let any_gone = ctx
            .comment_drafts
            .with_untracked(|d| d.keys().any(|f| !keep(f)));
        if !any_gone {
            return;
        }
        let mut gone = Vec::new();
        ctx.comment_drafts.update(|d| gone = drop_gone(d, keep));
        let images: Vec<String> = gone.into_iter().flat_map(|d| d.images).collect();
        ctx.drop_draft_images(&images);
    });
}

/// Files whose draft holds a note or an image not sent.
#[must_use]
pub fn unsent_files(drafts: &CommentDrafts) -> Vec<String> {
    drafts
        .iter()
        .filter(|(_, d)| d.unsent())
        .map(|(f, _)| f.clone())
        .collect()
}

impl StudioCtx {
    /// Files holding work that leaving would lose, sorted: a comment draft
    /// not sent, a Manual edit change not applied. Tracks what it reads.
    pub(super) fn unsent_files(self) -> Vec<String> {
        let mut files = self.comment_drafts.with(unsent_files);
        files.extend(self.tools.manual.unapplied_files(self.active().as_deref()));
        files.sort();
        files.dedup();
        files
    }

    /// An element picked in comment mode, for the file shown.
    pub(super) fn pick_for_comment(self, picked: Picked) {
        let Some(file) = self.active_untracked() else {
            return;
        };
        self.comment_drafts.update(|d| pick_draft(d, &file, picked));
    }

    /// The shown file's draft element answered `od:locate`.
    pub(super) fn on_draft_located(self, items: &[Located]) {
        let Some(file) = self.active_untracked() else {
            return;
        };
        let mut next = self.comment_drafts.get_untracked();
        if locate_draft(&mut next, &file, items)
            && self.comment_drafts.with_untracked(|d| *d != next)
        {
            self.comment_drafts.set(next);
        }
    }

    /// `file`'s draft is given up (Cancel): it goes, with the images
    /// uploaded for it.
    pub(super) fn cancel_comment_draft(self, file: &str) {
        let mut gone = None;
        self.comment_drafts.update(|d| gone = d.remove(file));
        if let Some(gone) = gone {
            self.drop_draft_images(&gone.images);
        }
    }

    /// Deletes those of `images` (uploaded for a draft) nothing holds now.
    pub(super) fn drop_draft_images(self, images: &[String]) {
        let paths = self.comment_drafts.with_untracked(|drafts| {
            self.comments
                .with_untracked(|comments| orphans(images, comments, drafts))
        });
        if paths.is_empty() {
            return;
        }
        let base = self.base();
        let work = async move {
            for path in paths {
                let url = format!("{base}/files/{}", api::file_path(&path));
                let _ = api::delete::<od_contracts::OkResponse>(&url).await;
            }
        };
        self.tasks.spawn_then(work, move |()| self.refresh_files());
    }

    /// An image of `file`'s draft removed (its x).
    pub(super) fn remove_draft_image(self, file: &str, path: &str) {
        self.comment_drafts.update(|d| {
            if let Some(d) = d.get_mut(file) {
                d.images.retain(|p| p != path);
            }
        });
        self.drop_draft_images(&[path.to_owned()]);
    }
}

/// Saves `file`'s draft as a comment, with `note`; `draft` and the
/// signals after it are the draft box's.
fn add_comment(
    ctx: StudioCtx,
    file: String,
    draft: Tasks,
    note: String,
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) {
    let (Some(d), Some(cid)) = (
        ctx.comment_drafts.with_untracked(|d| d.get(&file).cloned()),
        ctx.conversation_id.get_untracked(),
    ) else {
        return;
    };
    let picked = d.picked;
    let req = CreateCommentRequest {
        file_path: file.clone(),
        element_id: picked.od_id.clone().unwrap_or_default(),
        selector: picked.selector.clone(),
        label: picked.tag.clone(),
        text: picked.text.clone(),
        position: picked.rect,
        html_hint: String::new(),
        slide_index: ctx.deck.get_untracked().map(|d| d.active),
        note,
        status: Some(CommentStatus::Attached),
        attachments: d.images,
    };
    busy.set(true);
    // The comment is kept even if the draft box goes meanwhile; the box
    // closes or says why only while it is there.
    let path = format!("{}/conversations/{}/comments", ctx.base(), api::seg(&cid));
    let work = async move { api::post::<_, CommentResponse>(&path, &req).await };
    ctx.tasks.spawn_then(work, move |res| {
        let open = draft.alive();
        match res {
            Ok(r) => {
                ctx.comments.update(|c| c.push(r.comment));
                // Sent: nothing left to ask about, shown or not.
                ctx.comment_drafts.update(|d| _ = d.remove(&file));
            }
            Err(e) => {
                let why = ctx.failed_to(T::CommentFailed, &e);
                if open {
                    error.set(Some(why));
                } else {
                    ctx.notice.set(Some(why));
                }
            }
        }
        if open {
            busy.set(false);
        }
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
    // The shown file's draft element: located with the pins, to show its
    // box there again, or say it is gone, when the file shows again.
    let draft_at = Memo::new(move |_| {
        let file = ctx.active()?;
        let selector = ctx
            .comment_drafts
            .with(|d| d.get(&file).map(|d| d.picked.selector.clone()))?;
        Some((draft_locate_id(&file, &selector), selector))
    });
    // Ask the frame where these elements are now; it keeps answering on
    // scroll and resize, and a new document (`od:ready`) asks again.
    Effect::new(move |_| {
        ctx.frame_ready.track();
        let shown = pins();
        let draft = draft_at.get();
        bridge::post_main(&pb::locate(
            shown
                .iter()
                .map(|c| (c.id.as_str(), c.selector.as_str()))
                .chain(draft.iter().map(|(id, sel)| (id.as_str(), sel.as_str()))),
        ));
    });
    // The note box of the shown file's draft, while comment mode is on.
    // Keyed by file only: typing in it does not build it again.
    let draft_file = Memo::new(move |_| {
        let file = ctx.active()?;
        (ctx.comment_mode.get() && ctx.comment_drafts.with(|d| d.contains_key(&file)))
            .then_some(file)
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
            {move || draft_file.get().map(|file| view! { <DraftBox ctx=ctx file=file/> })}
        </div>
    }
}

#[component]
fn DraftBox(ctx: StudioCtx, file: String) -> impl IntoView {
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let tasks = Tasks::here();
    let view_height = super::bridge::main_frame_height();
    let file = StoredValue::new(file);
    // Its fields, read from the drafts: they outlive the box.
    let read = move |f: fn(&CommentDraft) -> String| {
        ctx.comment_drafts.with(|d| {
            file.with_value(|file| d.get(file).map(f))
                .unwrap_or_default()
        })
    };
    let note = move || read(|d| d.note.clone());
    let lost = move || {
        ctx.comment_drafts
            .with(|d| file.with_value(|file| d.get(file).is_some_and(|d| d.lost)))
    };
    let style = move || {
        ctx.comment_drafts.with(|d| {
            file.with_value(|file| {
                d.get(file)
                    .map(|d| draft_style(&d.picked.rect, view_height))
            })
            .unwrap_or_default()
        })
    };
    let label = move || {
        ctx.comment_drafts.with(|d| {
            file.with_value(|file| {
                d.get(file).map(|d| {
                    let p = &d.picked;
                    draft_label(&p.tag, p.label.as_deref().unwrap_or(&p.text))
                })
            })
            .unwrap_or_default()
        })
    };
    let set_note = move |text: String| {
        ctx.comment_drafts.update(|d| {
            if let Some(d) = file.with_value(|file| d.get_mut(file)) {
                d.note = text;
            }
        });
    };
    let cancel = move || file.with_value(|file| ctx.cancel_comment_draft(file));
    let submit = move || {
        let text = note().trim().to_owned();
        if !text.is_empty() && !busy.get_untracked() && !untrack(lost) {
            add_comment(ctx, file.get_value(), tasks, text, busy, error);
        }
    };
    let submit_key = submit;
    view! {
        <div class="comment-draft" style=style data-testid="comment-draft">
            <span class="muted small">{label}</span>
            {move || lost().then(|| view! {
                <p class="muted small" data-testid="comment-reselect">{t(T::CommentReselect)}</p>
            })}
            <textarea
                class="input"
                rows="2"
                data-testid="comment-note"
                placeholder=t(T::CommentPlaceholder)
                prop:value=note
                on:input=live(move |ev| set_note(event_target_value(&ev)))
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    // Cmd/Ctrl+Enter sends (Shift+Enter is a new line), and
                    // so does a plain Enter.
                    let primary = ev.meta_key() || ev.ctrl_key();
                    if ev.key() == "Enter" && (primary || !ev.shift_key()) && !ev.is_composing() {
                        ev.prevent_default();
                        submit_key();
                    } else if ev.key() == "Escape" {
                        cancel();
                    }
                }
            ></textarea>
            <super::comments_panel::DraftImages ctx=ctx file=file.get_value() error=error/>
            {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            <div class="row end tight">
                <button class="btn small ghost" data-testid="comment-cancel" on:click=move |_| cancel()>{t(T::Cancel)}</button>
                <button
                    class="btn small primary"
                    data-testid="comment-add"
                    disabled=move || busy.get() || lost() || note().trim().is_empty()
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
mod draft_tests {
    use super::*;

    fn picked(selector: &str) -> Picked {
        Picked {
            od_id: None,
            selector: selector.to_owned(),
            tag: "h1".to_owned(),
            text: "Title".to_owned(),
            label: None,
            rect: CommentRect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            },
        }
    }

    fn typed(drafts: &mut CommentDrafts, file: &str, note: &str, images: &[&str]) {
        let d = drafts.get_mut(file).expect("a draft");
        d.note = note.to_owned();
        d.images = images.iter().map(|&i| i.to_owned()).collect();
    }

    #[test]
    fn each_file_keeps_its_own_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"));
        typed(&mut drafts, "a.html", "Bigger", &["comments/ref.png"]);
        pick_draft(&mut drafts, "b.html", picked("p"));
        assert_eq!(
            drafts["a.html"].note, "Bigger",
            "b.html's pick leaves a.html's"
        );
        assert_eq!(
            unsent_files(&drafts),
            ["a.html"],
            "an element picked, nothing typed"
        );
    }

    #[test]
    fn an_element_picked_again_keeps_the_note_and_images() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"));
        typed(&mut drafts, "a.html", "Bigger", &["comments/ref.png"]);
        drafts.get_mut("a.html").unwrap().lost = true;
        pick_draft(&mut drafts, "a.html", picked("h2"));
        let d = &drafts["a.html"];
        assert_eq!(d.picked.selector, "h2");
        assert_eq!(
            (d.note.as_str(), d.images.len(), d.lost),
            ("Bigger", 1, false)
        );
    }

    #[test]
    fn the_draft_element_found_again_or_not() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"));
        let at = CommentRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let found = |id: String, rect| Located {
            id,
            rect,
            text: Some("New title".to_owned()),
        };
        // An answer for the element picked before is not this one's.
        let stale = [found(draft_locate_id("a.html", "h2"), None)];
        assert!(!locate_draft(&mut drafts, "a.html", &stale));
        assert!(!drafts["a.html"].lost);
        let here = [Located {
            text: Some("Title".to_owned()),
            ..found(draft_locate_id("a.html", "h1"), Some(at))
        }];
        assert!(locate_draft(&mut drafts, "a.html", &here));
        assert_eq!(drafts["a.html"].picked.rect, at, "pinned where it is now");
        assert!(!drafts["a.html"].lost);
        // Another element where the selector points now (by position).
        let other = [found(draft_locate_id("a.html", "h1"), Some(at))];
        assert!(locate_draft(&mut drafts, "a.html", &other));
        assert!(drafts["a.html"].lost, "other text: pick it again");
        pick_draft(&mut drafts, "a.html", picked("h1"));
        let gone = [found(draft_locate_id("a.html", "h1"), None)];
        assert!(locate_draft(&mut drafts, "a.html", &gone));
        assert!(drafts["a.html"].lost, "pick it again");
    }

    #[test]
    fn a_file_gone_takes_its_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"));
        pick_draft(&mut drafts, "b.html", picked("h1"));
        let gone = drop_gone(&mut drafts, |f| f == "b.html");
        assert_eq!(gone.len(), 1);
        assert_eq!(drafts.keys().collect::<Vec<_>>(), ["b.html"]);
    }

    #[test]
    fn only_images_nothing_holds_are_orphans() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "b.html", picked("p"));
        typed(&mut drafts, "b.html", "", &["comments/b.png"]);
        let comment: PreviewComment = serde_json::from_value(serde_json::json!({
            "id": "c1", "projectId": "p", "conversationId": "c", "filePath": "a.html",
            "elementId": "", "selector": "h1", "label": "h1", "text": "", "htmlHint": "",
            "position": {"x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0},
            "note": "n", "status": "open", "createdAt": 0, "updatedAt": 0,
            "attachments": [{"path": "comments/sent.png", "name": "sent.png"}],
        }))
        .unwrap();
        let images = [
            "comments/a.png".to_owned(),
            "comments/b.png".to_owned(),
            "comments/sent.png".to_owned(),
        ];
        assert_eq!(orphans(&images, &[comment], &drafts), ["comments/a.png"]);
    }
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
        assert_eq!(
            pin_style(&rect(100.0, 50.0)).as_deref(),
            Some("left:310px;top:90px")
        );
        // Partly shown: the pin stays on the frame's top edge.
        assert_eq!(
            pin_style(&rect(-40.0, 100.0)).as_deref(),
            Some("left:310px;top:0px")
        );
        assert_eq!(pin_style(&rect(-500.0, 100.0)), None);
    }

    #[test]
    fn the_note_box_stays_inside_the_frame() {
        assert_eq!(
            draft_style(&rect(100.0, 50.0), Some(800.0)),
            "left:20px;top:156px"
        );
        // Taller than the frame: pinned to its visible bottom.
        assert_eq!(
            draft_style(&rect(0.0, 2000.0), Some(800.0)),
            "left:20px;bottom:6px"
        );
        assert_eq!(
            draft_style(&rect(700.0, 50.0), Some(800.0)),
            "left:20px;bottom:6px"
        );
        // Frame height unknown: under the element, as before.
        assert_eq!(
            draft_style(&rect(0.0, 2000.0), None),
            "left:20px;top:2006px"
        );
    }

    #[test]
    fn the_note_box_label_cuts_at_a_word() {
        assert_eq!(draft_label("h1", "Hello tea"), "<h1> Hello tea");
        assert_eq!(
            draft_label(
                "section",
                "Mục 1 — Tiêu đề có dấu Đoạn văn 1. Link ngắn gọn"
            ),
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
