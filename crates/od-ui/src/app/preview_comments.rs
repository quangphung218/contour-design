//! Comments on the preview (spec 1.2): pins over the frame for the shown
//! file, and the note box for an element picked in comment mode. Attached
//! comments ride along with the next message (`commentAttachments`).

use super::api;
use super::bridge;
use super::studio::StudioCtx;
use super::tasks::{Tasks, live};
use crate::comment_board::AnchorState;
use crate::i18n::{T, t, tf};
use crate::preview_bridge::{self as pb, Located, Picked, Slide};
use leptos::prelude::*;
use od_contracts::{
    CommentRect, CommentResponse, CommentStatus, CreateCommentRequest, PreviewComment,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// Tells it from any other draft, whatever file it is on: what finds
    /// it again after an `await` (a send, an upload) or from its box finds
    /// it by this, not by a file a rename may have changed meanwhile.
    pub id: u64,
    pub picked: Picked,
    pub note: String,
    /// Images uploaded for it (`comments/…`).
    pub images: Vec<String>,
    /// Its element was not found when its file showed again: the note and
    /// images wait for an element picked again.
    pub lost: bool,
    /// The deck slide its element was picked on: sent with it, and the box
    /// shows only over that slide.
    pub slide: Option<u32>,
    /// Being saved: not sent again, not cancelled (its images are the
    /// comment's), whether its box went meanwhile or not.
    pub sending: bool,
    /// Images being uploaded for it, by upload: not sent until they land.
    pub uploads: Vec<u32>,
    /// An element picked on another slide while this draft waits on its
    /// own: it does not take the note over unseen; the user goes back to
    /// the draft or gives it up.
    pub waiting: Option<Waiting>,
}

/// An element picked on `slide` while the draft hides on another one.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub picked: Picked,
    pub slide: Option<u32>,
}

impl CommentDraft {
    /// Holds something the user would lose: a note, an image, or one on
    /// its way (leaving would drop it, and its file would stay unseen).
    #[must_use]
    pub fn unsent(&self) -> bool {
        !self.note.trim().is_empty() || !self.images.is_empty() || !self.uploads.is_empty()
    }
}

/// Comment drafts by file.
pub type CommentDrafts = BTreeMap<String, CommentDraft>;

/// The images of upload `token` landed (`landed`, none if it failed): the
/// draft waiting for it takes them, whatever its file is called now (a
/// rename meanwhile). False when no draft waits for it any more (cancelled
/// or given up meanwhile): nothing holds them.
pub fn land_upload(drafts: &mut CommentDrafts, token: u32, landed: &[String]) -> bool {
    let Some(d) = drafts.values_mut().find(|d| d.uploads.contains(&token)) else {
        return false;
    };
    d.uploads.retain(|t| *t != token);
    d.images.extend_from_slice(landed);
    true
}

/// The next draft's [`CommentDraft::id`].
static NEXT_DRAFT: AtomicU64 = AtomicU64::new(1);

/// The file of the draft `id`, and the draft.
pub fn draft_by_id(drafts: &CommentDrafts, id: u64) -> Option<(&String, &CommentDraft)> {
    drafts.iter().find(|(_, d)| d.id == id)
}

fn draft_by_id_mut(drafts: &mut CommentDrafts, id: u64) -> Option<&mut CommentDraft> {
    drafts.values_mut().find(|d| d.id == id)
}

/// The drafts of files renamed or moved (`moves`) go with them, as they
/// are: note, images, element, a send on its way.
pub fn rename_drafts(drafts: &mut CommentDrafts, moves: &[super::drafts::Move]) {
    let renamed: Vec<(String, String)> = drafts
        .keys()
        .filter_map(|f| Some((f.clone(), super::drafts::moved(moves, f)?)))
        .collect();
    let mut taken: Vec<(String, CommentDraft)> = renamed
        .into_iter()
        .filter_map(|(from, to)| Some((to, drafts.remove(&from)?)))
        .collect();
    for (to, d) in taken.drain(..) {
        drafts.insert(to, d);
    }
}

/// `od:locate` id of `file`'s draft on the element `selector`. It names the
/// selector: an answer for an element picked before is not this one's.
fn draft_locate_id(file: &str, selector: &str) -> String {
    format!("od-draft:{file}:{selector}")
}

/// `picked` in comment mode on `file`, on deck slide `slide`: a new draft,
/// or the draft there, with its note and images, on the element picked
/// now. A draft being sent stays as it is. A draft hidden on another slide
/// with something in it (or being sent) keeps its element: the pick waits
/// for the user to go back to it or give it up.
///
/// `count` is the deck's slide count, if known: a draft on a slide past it
/// (the deck lost that slide) is here, wherever the pick is.
pub fn pick_draft(
    drafts: &mut CommentDrafts,
    file: &str,
    picked: Picked,
    slide: Option<u32>,
    count: Option<u32>,
) {
    match drafts.get_mut(file) {
        Some(d)
            if draft_slide_shown(d.slide, slide, count).is_none() && (d.sending || d.unsent()) =>
        {
            d.waiting = Some(Waiting { picked, slide });
        }
        Some(d) if d.sending => {}
        Some(d) => {
            d.picked = picked;
            d.lost = false;
            d.slide = slide;
            d.waiting = None;
        }
        None => {
            drafts.insert(file.to_owned(), new_draft(picked, slide));
        }
    }
}

fn new_draft(picked: Picked, slide: Option<u32>) -> CommentDraft {
    CommentDraft {
        id: NEXT_DRAFT.fetch_add(1, Ordering::Relaxed),
        picked,
        note: String::new(),
        images: Vec::new(),
        lost: false,
        slide,
        sending: false,
        uploads: Vec::new(),
        waiting: None,
    }
}

/// `file`'s draft given up for the element waiting on another slide: a new
/// draft there. Returns the one given up (its images go), not while it is
/// being sent.
fn take_waiting(drafts: &mut CommentDrafts, file: &str) -> Option<CommentDraft> {
    let d = drafts.get(file)?;
    if d.sending {
        return None;
    }
    let w = d.waiting.clone()?;
    drafts.insert(file.to_owned(), new_draft(w.picked, w.slide))
}

/// The draft `id` saved with the note `note` and the images `sent`: it
/// goes, unless something came to it meanwhile: images (or ones still on
/// the way), or words typed while it was sent. Those stay in it, for the
/// next comment, rather than be lost unseen. The note stays whole once
/// typed into: the user sees what was sent and takes it from there. Found
/// by id: its file may have been renamed while it was sent.
fn sent_draft(drafts: &mut CommentDrafts, id: u64, note: &str, sent: &[String]) {
    let Some(file) = draft_by_id(drafts, id).map(|(f, _)| f.clone()) else {
        return;
    };
    let Some(d) = drafts.get_mut(&file) else {
        return;
    };
    d.images.retain(|p| !sent.contains(p));
    let typed = d.note.trim() != note;
    if !typed && d.images.is_empty() && d.uploads.is_empty() {
        drafts.remove(&file);
    } else {
        if !typed {
            d.note.clear();
        }
        d.sending = false;
    }
}

/// Whether something placed on slide `on` shows over slide `shown`: as
/// the pins, anything when the file is no deck (yet) or it has no slide.
fn on_slide(on: Option<u32>, shown: Option<u32>) -> bool {
    shown.is_none() || on.is_none() || on == shown
}

/// Whether a draft picked on slide `on` shows over slide `shown` of a deck
/// of `count` slides (if known), and if so whether its slide is gone:
/// `Some(false)` over its own slide (as the pins), `Some(true)` over any
/// slide once the deck has no slide `on` any more (the agent rewrote it
/// shorter): the note shows, to pick an element again or discard it,
/// rather than wait unseen on a slide that is not there. `None`: it waits
/// on its own slide. Its slide is kept: a deck that grows back (the agent
/// still writing it) shows it there again.
pub fn draft_slide_shown(on: Option<u32>, shown: Option<u32>, count: Option<u32>) -> Option<bool> {
    if on_slide(on, shown) {
        return Some(false);
    }
    match (on, count) {
        (Some(on), Some(count)) if on >= count => Some(true),
        _ => None,
    }
}

/// Takes `file`'s draft out (Cancel), unless it is being sent: its images
/// are that comment's now.
fn cancel_draft(drafts: &mut CommentDrafts, file: &str) -> Option<CommentDraft> {
    if drafts.get(file)?.sending {
        return None;
    }
    drafts.remove(file)
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

/// Takes out of `drafts` those of files `keep` refuses, and returns them
/// with their files.
fn drop_gone(
    drafts: &mut CommentDrafts,
    keep: impl Fn(&str) -> bool,
) -> Vec<(String, CommentDraft)> {
    let gone: Vec<String> = drafts.keys().filter(|f| !keep(f)).cloned().collect();
    gone.into_iter()
        .filter_map(|f| drafts.remove(&f).map(|d| (f, d)))
        .collect()
}

/// A file gone (deleted, or renamed or moved outside the app: the agent,
/// Finder) takes its drafts along (comment, Manual edit): nothing would
/// show them again, and the leave guard would ask about them for good. The
/// Studio says which files lost work. A file being renamed or moved in the
/// app is not gone: its drafts follow it once the server answers.
pub fn forget_gone_drafts(ctx: StudioCtx) {
    Effect::new(move |_| {
        let present: std::collections::HashSet<String> = ctx
            .files
            .with(|files| files.iter().map(|f| f.path.clone()).collect());
        let moving = ctx.moving.get_value();
        let keep = |p: &str| {
            super::live_view::is_live(p)
                || present.contains(p)
                || super::drafts::is_moving(&moving, p)
        };
        let mut lost = ctx.tools.manual.drop_gone(keep);
        let say = move |mut lost: Vec<String>| {
            if lost.is_empty() {
                return;
            }
            lost.sort();
            lost.dedup();
            ctx.notice
                .set(Some(tf(T::DraftsDroppedFileGone, &[&lost.join(", ")])));
        };
        // A draft being sent stays: its answer takes it out.
        let keep_draft =
            |p: &str, drafts: &CommentDrafts| keep(p) || drafts.get(p).is_some_and(|d| d.sending);
        let any_gone = ctx
            .comment_drafts
            .with_untracked(|d| d.keys().any(|f| !keep_draft(f, d)));
        if !any_gone {
            say(lost);
            return;
        }
        let mut gone = Vec::new();
        ctx.comment_drafts.update(|d| {
            let sending: Vec<String> = d
                .iter()
                .filter(|(_, x)| x.sending)
                .map(|(f, _)| f.clone())
                .collect();
            gone = drop_gone(d, |f| keep(f) || sending.iter().any(|s| s == f));
        });
        lost.extend(
            gone.iter()
                .filter(|(_, d)| d.unsent())
                .map(|(f, _)| f.clone()),
        );
        let images: Vec<String> = gone.into_iter().flat_map(|(_, d)| d.images).collect();
        ctx.drop_draft_images(&images);
        say(lost);
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
        let deck = self.deck.get_untracked();
        let (slide, count) = (deck.map(|d| d.active), deck.map(|d| d.count));
        self.comment_drafts
            .update(|d| pick_draft(d, &file, picked, slide, count));
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
    /// uploaded for it. Not while it is being sent.
    pub(super) fn cancel_comment_draft(self, file: &str) {
        if !self
            .comment_drafts
            .with_untracked(|d| d.get(file).is_some_and(|d| !d.sending))
        {
            return;
        }
        let mut gone = None;
        self.comment_drafts.update(|d| gone = cancel_draft(d, file));
        if let Some(gone) = gone {
            self.drop_draft_images(&gone.images);
        }
    }

    /// Back to the slide `file`'s draft waits on, from an element picked
    /// on another one: the pick is let go.
    fn go_to_draft(self, file: &str) {
        let mut slide = None;
        self.comment_drafts.update(|d| {
            if let Some(d) = d.get_mut(file) {
                d.waiting = None;
                slide = d.slide;
            }
        });
        if let Some(i) = slide {
            self.slide(Slide::Go(i));
        }
    }

    /// `file`'s draft on another slide given up for the element picked
    /// here: it goes, with its images; a new draft starts on that element.
    fn give_up_for_waiting(self, file: &str) {
        let mut gone = None;
        self.comment_drafts.update(|d| gone = take_waiting(d, file));
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

    /// An image of `file`'s draft removed (its x); not while it is sent.
    pub(super) fn remove_draft_image(self, file: &str, path: &str) {
        if self
            .comment_drafts
            .with_untracked(|d| d.get(file).is_none_or(|d| d.sending))
        {
            return;
        }
        self.comment_drafts.update(|d| {
            if let Some(d) = d.get_mut(file) {
                d.images.retain(|p| p != path);
            }
        });
        self.drop_draft_images(&[path.to_owned()]);
    }
}

impl StudioCtx {
    /// Whether a draft picked on slide `on` of the shown file shows now,
    /// and if so whether its slide is gone ([`draft_slide_shown`]). Tracks
    /// the deck.
    fn draft_here(self, on: Option<u32>) -> Option<bool> {
        let count = self.deck.with(|d| d.map(|d| d.count));
        draft_slide_shown(on, self.shown_slide(), count)
    }
}

/// Saves the draft `id` as a comment, with `note`, unless it is being
/// sent already. `draft` and `error` are the draft box's.
fn add_comment(
    ctx: StudioCtx,
    id: u64,
    draft: Tasks,
    note: String,
    error: RwSignal<Option<String>>,
) {
    let (Some((file, d)), Some(cid)) = (
        ctx.comment_drafts
            .with_untracked(|d| draft_by_id(d, id).map(|(f, d)| (f.clone(), d.clone()))),
        ctx.conversation_id.get_untracked(),
    ) else {
        return;
    };
    let slide_gone = untrack(|| ctx.draft_here(d.slide)) == Some(true);
    if d.sending || d.lost || slide_gone || !d.uploads.is_empty() {
        return;
    }
    let picked = d.picked;
    let req = CreateCommentRequest {
        file_path: file.clone(),
        element_id: picked.od_id.clone().unwrap_or_default(),
        selector: picked.selector.clone(),
        label: picked.tag.clone(),
        text: picked.text.clone(),
        position: picked.rect,
        html_hint: String::new(),
        // The slide it was picked on, not the one shown now.
        slide_index: d.slide,
        note: note.clone(),
        status: Some(CommentStatus::Attached),
        attachments: d.images.clone(),
    };
    let sent = d.images;
    // By id: a rename while it is sent moves it to another file.
    let set_sending = move |on: bool| {
        ctx.comment_drafts.update(|d| {
            if let Some(d) = draft_by_id_mut(d, id) {
                d.sending = on;
            }
        });
    };
    set_sending(true);
    // The comment is kept even if the draft box goes meanwhile; the box
    // says why it failed only while it is there. Its images are its own
    // once saved: the page leaving meanwhile releases its drafts' images
    // only after it ([`super::drafts::Sends`]).
    let path = format!("{}/conversations/{}/comments", ctx.base(), api::seg(&cid));
    let (sends, session, base) = (
        ctx.shell.draft_sends,
        ctx.draft_session.get_value(),
        ctx.base(),
    );
    sends.update_value(|s| s.begin(&session));
    let work = async move {
        let res = api::post::<_, CommentResponse>(&path, &req).await;
        if sends.try_update_value(|s| s.end(&session)) == Some(true) {
            super::drafts::release(&base, &session);
        }
        res
    };
    ctx.tasks.spawn_then(work, move |res| match res {
        Ok(r) => {
            ctx.comments.update(|c| c.push(r.comment));
            // Sent: nothing left to ask about, shown or not, but images
            // or words that came meanwhile.
            ctx.comment_drafts
                .update(|d| sent_draft(d, id, &note, &sent));
        }
        Err(e) => {
            set_sending(false);
            let why = ctx.failed_to(T::CommentFailed, &e);
            if draft.alive() {
                error.set(Some(why));
            } else {
                ctx.notice.set(Some(why));
            }
        }
    });
}

#[component]
pub fn CommentLayer(ctx: StudioCtx) -> impl IntoView {
    let open_pin = RwSignal::new(None::<String>);
    let pins = move || {
        let file = ctx.active();
        // Not `deck.active`: a reload of the deck goes through slide 0.
        let slide = ctx.shown_slide();
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
    // Over a deck, only on the slide it was picked on (as the pins).
    // Keyed by the draft's id: a rename of its file keeps the box, and
    // what is typed in it, as it is. Over a deck that lost the draft's
    // slide, on whatever slide shows (`draft_here`).
    let draft_file = Memo::new(move |_| {
        let file = ctx.active()?;
        if !ctx.comment_mode.get() {
            return None;
        }
        let (id, on) = ctx
            .comment_drafts
            .with(|d| d.get(&file).map(|d| (d.id, d.slide)))?;
        // Not `deck.active`: a reload of the deck goes through slide 0.
        ctx.draft_here(on).map(|_| id)
    });
    // An element picked over another slide than the draft's: the box says
    // the draft waits there (its slide, 0-based), at the element picked.
    let elsewhere = Memo::new(move |_| {
        let file = ctx.active()?;
        let slide = ctx.shown_slide();
        if !ctx.comment_mode.get() {
            return None;
        }
        let (on, waits) = ctx.comment_drafts.with(|d| {
            let d = d.get(&file)?;
            Some((d.slide, d.waiting.as_ref()?.slide))
        })?;
        (ctx.draft_here(on).is_none() && on_slide(waits, slide))
            .then(|| (file.clone(), on.unwrap_or_default()))
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
            {move || draft_file.get().map(|id| view! { <DraftBox ctx=ctx id=id/> })}
            {move || elsewhere.get().map(|(file, slide)| view! { <DraftElsewhere ctx=ctx file=file slide=slide/> })}
        </div>
    }
}

#[component]
fn DraftBox(ctx: StudioCtx, id: u64) -> impl IntoView {
    let error = RwSignal::new(None::<String>);
    let tasks = Tasks::here();
    let view_height = super::bridge::main_frame_height();
    // Its fields, read from the drafts by id: they outlive the box, and
    // its file may be renamed while it shows.
    let with = move |f: &dyn Fn(&CommentDraft) -> Option<String>| {
        ctx.comment_drafts
            .with(|d| draft_by_id(d, id).and_then(|(_, d)| f(d)))
            .unwrap_or_default()
    };
    let note = move || with(&|d| Some(d.note.clone()));
    let flag = move |f: fn(&CommentDraft) -> bool| {
        ctx.comment_drafts
            .with(|d| draft_by_id(d, id).is_some_and(|(_, d)| f(d)))
    };
    // Its element gone, or the deck slide it was on.
    let slide = move || {
        ctx.comment_drafts
            .with(|d| draft_by_id(d, id).and_then(|(_, d)| d.slide))
    };
    let lost = move || flag(|d| d.lost) || ctx.draft_here(slide()) == Some(true);
    // An image on its way: Add waits for it, else it would miss the comment.
    let uploading = move || flag(|d| !d.uploads.is_empty());
    // Kept in the draft: a box shown again while it is sent knows it.
    let sending = move || flag(|d| d.sending);
    let style = move || with(&|d| Some(draft_style(&d.picked.rect, view_height)));
    let label = move || {
        with(&|d| {
            let p = &d.picked;
            Some(draft_label(&p.tag, p.label.as_deref().unwrap_or(&p.text)))
        })
    };
    // Each key typed goes to this draft, whatever its file is called now.
    let set_note = move |text: String| {
        ctx.comment_drafts.update(|d| {
            if let Some(d) = draft_by_id_mut(d, id) {
                d.note = text;
            }
        });
    };
    let file = move || {
        ctx.comment_drafts
            .with_untracked(|d| draft_by_id(d, id).map(|(f, _)| f.clone()))
    };
    let cancel = move || {
        if let Some(f) = file() {
            ctx.cancel_comment_draft(&f);
        }
    };
    let submit = move || {
        let text = note().trim().to_owned();
        if !text.is_empty() {
            add_comment(ctx, id, tasks, text, error);
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
            <super::comments_panel::DraftImages ctx=ctx id=id error=error/>
            {move || uploading().then(|| view! {
                <p class="muted small" data-testid="comment-uploading">{t(T::CommentUploading)}</p>
            })}
            {move || error.get().map(|e| view! { <p class="error-text">{e}</p> })}
            <div class="row end tight">
                <button class="btn small ghost" data-testid="comment-cancel" disabled=sending on:click=move |_| cancel()>{t(T::Cancel)}</button>
                <button
                    class="btn small primary"
                    data-testid="comment-add"
                    disabled=move || sending() || lost() || uploading() || note().trim().is_empty()
                    on:click=move |_| submit()
                >
                    {t(T::AddComment)}
                </button>
            </div>
        </div>
    }
}

/// Over another slide than `file`'s draft (on `slide`, 0-based), at the
/// element picked here: the draft waits there; go back to it, or give it
/// up for this element.
#[component]
fn DraftElsewhere(ctx: StudioCtx, file: String, slide: u32) -> impl IntoView {
    let view_height = super::bridge::main_frame_height();
    let file = StoredValue::new(file);
    let style = move || {
        ctx.comment_drafts.with(|d| {
            file.with_value(|file| {
                d.get(file)
                    .and_then(|d| d.waiting.as_ref())
                    .map(|w| draft_style(&w.picked.rect, view_height))
            })
            .unwrap_or_default()
        })
    };
    let sending = move || {
        ctx.comment_drafts
            .with(|d| file.with_value(|file| d.get(file).is_some_and(|d| d.sending)))
    };
    let n = (slide + 1).to_string();
    view! {
        <div class="comment-draft" style=style data-testid="comment-elsewhere">
            <p class="small">{tf(T::CommentDraftElsewhere, &[&n])}</p>
            <div class="row end tight">
                <button
                    class="btn small ghost"
                    data-testid="comment-elsewhere-discard"
                    disabled=sending
                    on:click=move |_| file.with_value(|f| ctx.give_up_for_waiting(f))
                >
                    {t(T::CommentDiscardDraft)}
                </button>
                <button
                    class="btn small primary"
                    data-testid="comment-elsewhere-go"
                    on:click=move |_| file.with_value(|f| ctx.go_to_draft(f))
                >
                    {tf(T::CommentGoToDraft, &[&n])}
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

    fn id(drafts: &CommentDrafts, file: &str) -> u64 {
        drafts[file].id
    }

    fn typed(drafts: &mut CommentDrafts, file: &str, note: &str, images: &[&str]) {
        let d = drafts.get_mut(file).expect("a draft");
        d.note = note.to_owned();
        d.images = images.iter().map(|&i| i.to_owned()).collect();
    }

    #[test]
    fn each_file_keeps_its_own_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Bigger", &["comments/ref.png"]);
        pick_draft(&mut drafts, "b.html", picked("p"), None, None);
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
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Bigger", &["comments/ref.png"]);
        drafts.get_mut("a.html").unwrap().lost = true;
        pick_draft(&mut drafts, "a.html", picked("h2"), None, None);
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
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
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
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        let gone = [found(draft_locate_id("a.html", "h1"), None)];
        assert!(locate_draft(&mut drafts, "a.html", &gone));
        assert!(drafts["a.html"].lost, "pick it again");
    }

    #[test]
    fn the_draft_keeps_the_slide_it_was_picked_on() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "deck.html", picked("h1"), Some(1), None);
        assert_eq!(drafts["deck.html"].slide, Some(1));
        // Shown over its own slide only, as the pins.
        assert!(on_slide(Some(1), Some(1)));
        assert!(!on_slide(Some(1), Some(0)), "slide 1 shown: the box hides");
        assert!(on_slide(Some(1), None), "deck not known yet");
        assert!(on_slide(None, Some(0)), "picked on no deck");
        pick_draft(&mut drafts, "deck.html", picked("h2"), Some(2), None);
        assert_eq!(drafts["deck.html"].slide, Some(2), "picked again there");
    }

    #[test]
    fn a_draft_being_sent_is_not_cancelled_or_picked_over() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Bigger", &["comments/ref.png"]);
        drafts.get_mut("a.html").unwrap().sending = true;
        assert!(
            cancel_draft(&mut drafts, "a.html").is_none(),
            "its images are the comment's"
        );
        pick_draft(&mut drafts, "a.html", picked("h2"), None, None);
        assert_eq!(drafts["a.html"].picked.selector, "h1", "sent as picked");
        drafts.get_mut("a.html").unwrap().sending = false;
        assert!(cancel_draft(&mut drafts, "a.html").is_some());
        assert!(drafts.is_empty());
    }

    #[test]
    fn a_pick_over_another_slide_leaves_a_draft_hidden_there() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "deck.html", picked("h1"), Some(1), None);
        typed(&mut drafts, "deck.html", "About slide two", &[]);
        // Slide 0 shown: the note does not move to an element unseen.
        pick_draft(&mut drafts, "deck.html", picked("h2"), Some(0), None);
        let d = &drafts["deck.html"];
        assert_eq!((d.picked.selector.as_str(), d.slide), ("h1", Some(1)));
        let w = d.waiting.as_ref().expect("the pick waits");
        assert_eq!((w.picked.selector.as_str(), w.slide), ("h2", Some(0)));
        // Given up: a new draft on the element picked, the old one returned.
        let gone = take_waiting(&mut drafts, "deck.html").expect("given up");
        assert_eq!(gone.note, "About slide two");
        let d = &drafts["deck.html"];
        assert_eq!((d.picked.selector.as_str(), d.slide), ("h2", Some(0)));
        assert_eq!((d.note.as_str(), d.waiting.is_none()), ("", true));
        // Picked again on its own slide: the waiting pick goes.
        typed(&mut drafts, "deck.html", "About slide one", &[]);
        pick_draft(&mut drafts, "deck.html", picked("h3"), Some(2), None);
        pick_draft(&mut drafts, "deck.html", picked("p"), Some(0), None);
        let d = &drafts["deck.html"];
        assert_eq!(
            (d.picked.selector.as_str(), d.waiting.is_none()),
            ("p", true)
        );
        // Nothing typed yet: nothing to lose, the draft moves.
        typed(&mut drafts, "deck.html", " ", &[]);
        pick_draft(&mut drafts, "deck.html", picked("h1"), Some(2), None);
        assert_eq!(drafts["deck.html"].slide, Some(2));
        assert!(drafts["deck.html"].waiting.is_none());
    }

    #[test]
    fn a_draft_being_sent_is_not_given_up_for_a_pick_elsewhere() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "deck.html", picked("h1"), Some(1), None);
        drafts.get_mut("deck.html").unwrap().sending = true;
        pick_draft(&mut drafts, "deck.html", picked("h2"), Some(0), None);
        assert!(drafts["deck.html"].waiting.is_some(), "told where it is");
        assert!(take_waiting(&mut drafts, "deck.html").is_none());
        assert_eq!(drafts["deck.html"].picked.selector, "h1");
    }

    #[test]
    fn images_landing_while_it_is_sent_stay_in_the_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Bigger", &["comments/a.png"]);
        let sent = ["comments/a.png".to_owned()];
        let at = id(&drafts, "a.html");
        sent_draft(&mut drafts, at, "Bigger", &sent);
        assert!(drafts.is_empty(), "all sent: the draft goes");
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(
            &mut drafts,
            "a.html",
            "Bigger",
            &["comments/a.png", "comments/late.png"],
        );
        drafts.get_mut("a.html").unwrap().sending = true;
        let at = id(&drafts, "a.html");
        sent_draft(&mut drafts, at, "Bigger", &sent);
        let d = &drafts["a.html"];
        assert_eq!(d.images, ["comments/late.png"], "kept, not lost unseen");
        assert_eq!((d.note.as_str(), d.sending), ("", false));
        // One still on its way: the draft waits for it.
        drafts.get_mut("a.html").unwrap().images.clear();
        drafts.get_mut("a.html").unwrap().uploads.push(7);
        let at = id(&drafts, "a.html");
        sent_draft(&mut drafts, at, "", &[]);
        assert!(drafts.contains_key("a.html"));
    }

    #[test]
    fn words_typed_while_it_is_sent_stay_in_the_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Bigger ", &["comments/a.png"]);
        drafts.get_mut("a.html").unwrap().sending = true;
        // Typed on while the comment was saved.
        drafts.get_mut("a.html").unwrap().note = "Bigger, and bolder".to_owned();
        let sent = ["comments/a.png".to_owned()];
        let at = id(&drafts, "a.html");
        sent_draft(&mut drafts, at, "Bigger", &sent);
        let d = drafts.get("a.html").expect("not dropped unseen");
        assert_eq!(
            (d.note.as_str(), d.images.len(), d.sending),
            ("Bigger, and bolder", 0, false)
        );
        // Words and a late image: both stay.
        typed(
            &mut drafts,
            "a.html",
            "Again more",
            &["comments/a.png", "comments/b.png"],
        );
        let at = id(&drafts, "a.html");
        sent_draft(&mut drafts, at, "Again", &sent);
        let d = &drafts["a.html"];
        assert_eq!(
            (d.note.as_str(), d.images.as_slice()),
            ("Again more", &["comments/b.png".to_owned()][..])
        );
        // Only spaces added: nothing to keep.
        typed(&mut drafts, "a.html", "Same  ", &[]);
        let at = id(&drafts, "a.html");
        sent_draft(&mut drafts, at, "Same", &[]);
        assert!(drafts.is_empty());
    }

    #[test]
    fn an_image_on_its_way_is_work_not_sent() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        assert!(
            unsent_files(&drafts).is_empty(),
            "an element picked, nothing more"
        );
        drafts.get_mut("a.html").unwrap().uploads.push(3);
        assert_eq!(
            unsent_files(&drafts),
            ["a.html"],
            "leaving would drop the image"
        );
    }

    #[test]
    fn a_draft_follows_its_file_renamed_or_moved() {
        use super::super::drafts::Move;
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Keep this", &["comments/a.png"]);
        pick_draft(&mut drafts, "pages/b.html", picked("p"), None, None);
        pick_draft(&mut drafts, "z.html", picked("p"), None, None);
        let (a, b) = (id(&drafts, "a.html"), id(&drafts, "pages/b.html"));
        drafts.get_mut("a.html").unwrap().sending = true;
        rename_drafts(
            &mut drafts,
            &[
                Move {
                    from: "a.html".into(),
                    to: "c.html".into(),
                    folder: false,
                },
                Move {
                    from: "pages".into(),
                    to: "site/pages".into(),
                    folder: true,
                },
            ],
        );
        assert_eq!(
            drafts.keys().collect::<Vec<_>>(),
            ["c.html", "site/pages/b.html", "z.html"]
        );
        let c = &drafts["c.html"];
        assert_eq!(
            (c.id, c.note.as_str(), c.images.len(), c.sending),
            (a, "Keep this", 1, true),
            "as it was, a send on its way included"
        );
        assert_eq!(drafts["site/pages/b.html"].id, b);
    }

    #[test]
    fn a_draft_renamed_while_it_is_sent_goes_when_it_is_saved() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        typed(&mut drafts, "a.html", "Bigger", &["comments/a.png"]);
        let at = id(&drafts, "a.html");
        drafts.get_mut("a.html").unwrap().sending = true;
        let d = drafts.remove("a.html").unwrap();
        drafts.insert("c.html".into(), d);
        sent_draft(&mut drafts, at, "Bigger", &["comments/a.png".to_owned()]);
        assert!(drafts.is_empty(), "found by id under its new name");
    }

    #[test]
    fn an_image_lands_in_its_draft_whatever_its_file_is_called_now() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        drafts.get_mut("a.html").unwrap().uploads.push(7);
        let d = drafts.remove("a.html").unwrap();
        drafts.insert("c.html".into(), d);
        assert!(land_upload(&mut drafts, 7, &["comments/x.png".to_owned()]));
        let c = &drafts["c.html"];
        assert_eq!(
            (c.images.as_slice(), c.uploads.len()),
            (&["comments/x.png".to_owned()][..], 0)
        );
        // Its draft cancelled meanwhile: nothing holds them.
        assert!(!land_upload(&mut drafts, 8, &["comments/y.png".to_owned()]));
    }

    #[test]
    fn a_draft_on_a_slide_the_deck_lost_shows_on_the_slide_shown() {
        // Its own slide, any count: shown, not lost.
        assert_eq!(draft_slide_shown(Some(1), Some(1), Some(3)), Some(false));
        // Another slide that is there: it waits there.
        assert_eq!(draft_slide_shown(Some(1), Some(0), Some(3)), None);
        // The deck rewritten with 1 slide: shown here, to pick again.
        assert_eq!(draft_slide_shown(Some(1), Some(0), Some(1)), Some(true));
        // Count not known yet: as before.
        assert_eq!(draft_slide_shown(Some(1), Some(0), None), None);
        // Grown back: its own slide again, the slide kept.
        assert_eq!(draft_slide_shown(Some(1), Some(0), Some(2)), None);
        assert_eq!(draft_slide_shown(Some(1), Some(1), Some(2)), Some(false));
    }

    #[test]
    fn a_pick_over_a_deck_that_lost_the_draft_slide_takes_the_note() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "deck.html", picked("h1"), Some(1), Some(3));
        typed(&mut drafts, "deck.html", "Slide two note", &[]);
        pick_draft(&mut drafts, "deck.html", picked("h2"), Some(0), Some(1));
        let d = &drafts["deck.html"];
        assert_eq!(
            (
                d.picked.selector.as_str(),
                d.slide,
                d.note.as_str(),
                d.waiting.is_none()
            ),
            ("h2", Some(0), "Slide two note", true)
        );
    }

    #[test]
    fn a_file_being_renamed_keeps_its_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        pick_draft(&mut drafts, "pages/b.html", picked("h1"), None, None);
        pick_draft(&mut drafts, "gone.html", picked("h1"), None, None);
        // The watcher lists the files before the rename's answer came.
        let present = ["c.html".to_owned()];
        let moving = ["a.html".to_owned(), "pages/".to_owned()];
        let gone = drop_gone(&mut drafts, |p| {
            present.iter().any(|f| f == p) || super::super::drafts::is_moving(&moving, p)
        });
        assert_eq!(
            gone.into_iter().map(|(f, _)| f).collect::<Vec<_>>(),
            ["gone.html"]
        );
        assert_eq!(drafts.len(), 2);
    }

    #[test]
    fn a_file_gone_takes_its_draft() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "a.html", picked("h1"), None, None);
        pick_draft(&mut drafts, "b.html", picked("h1"), None, None);
        let gone = drop_gone(&mut drafts, |f| f == "b.html");
        assert_eq!(gone.len(), 1);
        assert_eq!(drafts.keys().collect::<Vec<_>>(), ["b.html"]);
    }

    #[test]
    fn only_images_nothing_holds_are_orphans() {
        let mut drafts = CommentDrafts::new();
        pick_draft(&mut drafts, "b.html", picked("p"), None, None);
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
