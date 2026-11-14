//! Manual edit (spec 1.2, upstream `ManualEditPanel`): the page's layers,
//! and for the picked element a Content tab (text, link, image with
//! upload), Style (inline style properties), Attributes (JSON) and Html
//! (its source HTML), plus delete, undo and redo. Every change is written
//! to the source through `POST source-edits` as a manual version, checked
//! against the tag and text the preview showed; undo and redo restore the
//! version before or after an edit, only while the file is still in the
//! state that edit left (`expectVersionId`).

use super::api;
use super::bridge;
use super::icons::{Glyph, icon};
use super::preview_tools::{panel_head, style_edits};
use super::studio::StudioCtx;
use super::tasks::live;
use crate::i18n::{T, t, tf};
use crate::manual_model::{self as mm, History, Kind, Step};
use crate::preview_bridge::{self as pb, Inspected, Layer};
use leptos::prelude::*;
use od_contracts::{
    RestoreVersionRequest, RestoreVersionResponse, SourceEdit, SourceEditRequest,
    SourceEditResponse, SourceElementRequest, SourceElementResponse,
};
use std::collections::BTreeMap;

/// Label of the versions manual edits write.
const VERSION_LABEL: &str = "Manual edit";
/// Project folder for images uploaded from the Content tab.
const IMAGES_DIR: &str = "assets";
/// Inline style properties the Style tab offers (all reported by the
/// bridge's inspect).
const STYLE_PROPS: [&str; 14] = [
    "color",
    "background-color",
    "opacity",
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "letter-spacing",
    "text-align",
    "padding",
    "margin",
    "border-radius",
    "width",
    "height",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Content,
    Style,
    Attributes,
    Html,
}

/// Manual edit's state, kept in [`super::preview_tools::Tools`].
#[derive(Clone, Copy)]
pub struct Manual {
    pub layers: RwSignal<Vec<Layer>>,
    /// Selector of the picked element.
    pub selected: RwSignal<Option<String>>,
    /// The picked element as the source has it, once read.
    pub element: RwSignal<Option<Result<SourceElementResponse, String>>>,
    pub tab: RwSignal<Tab>,
    pub history: RwSignal<History>,
    pub busy: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    // Drafts of the Content, Attributes and Html tabs.
    text: RwSignal<String>,
    href: RwSignal<String>,
    src: RwSignal<String>,
    alt: RwSignal<String>,
    attrs: RwSignal<String>,
    html: RwSignal<String>,
    /// Number of the latest element read; older answers are dropped.
    read_seq: StoredValue<u32>,
    /// Which drafts are clean (match the source or were just written).
    book: StoredValue<mm::DraftBook>,
    /// Images uploaded from the Content tab and not written into a page
    /// yet: the server keeps them out of the project until a write takes
    /// them (`drafts`).
    uploaded: StoredValue<Vec<String>>,
    /// Other files' changes typed and not applied, by file: they come back
    /// when the file shows again, and the leave guard lists them.
    kept: RwSignal<BTreeMap<String, Kept>>,
}

/// A file's picked element with a change typed and not applied, kept while
/// another file shows.
#[derive(Clone)]
struct Kept {
    selected: String,
    element: Option<Result<SourceElementResponse, String>>,
    drafts: mm::Drafts,
    book: mm::DraftBook,
    tab: Tab,
}

impl Manual {
    pub fn new() -> Self {
        Self {
            layers: RwSignal::new(Vec::new()),
            selected: RwSignal::new(None),
            element: RwSignal::new(None),
            tab: RwSignal::new(Tab::Content),
            history: RwSignal::new(History::default()),
            busy: RwSignal::new(false),
            error: RwSignal::new(None),
            text: RwSignal::new(String::new()),
            href: RwSignal::new(String::new()),
            src: RwSignal::new(String::new()),
            alt: RwSignal::new(String::new()),
            attrs: RwSignal::new(String::new()),
            html: RwSignal::new(String::new()),
            read_seq: StoredValue::new(0),
            book: StoredValue::new(mm::DraftBook::default()),
            uploaded: StoredValue::new(Vec::new()),
            kept: RwSignal::new(BTreeMap::new()),
        }
    }

    /// `now` is shown instead of `last`: the layers, selection and history
    /// go. A change typed and not applied for `last`'s element is kept, and
    /// `now`'s comes back. Undo stays with the file it was made on: none.
    pub fn forget(self, last: Option<&str>, now: Option<&str>) {
        if let (Some(last), Some(selected)) = (last, self.selected.get_untracked())
            && self.unapplied()
        {
            let kept = Kept {
                selected,
                element: self.element.get_untracked(),
                drafts: self.drafts(),
                book: self.book.get_value(),
                tab: self.tab.get_untracked(),
            };
            self.kept.update(|k| _ = k.insert(last.to_owned(), kept));
        }
        // The last file's element, if its read answers now, is not this one's.
        self.read_seq.update_value(|n| *n = n.wrapping_add(1));
        self.layers.set(Vec::new());
        self.selected.set(None);
        self.element.set(None);
        self.history.update(History::clear);
        self.error.set(None);
        self.set_drafts(mm::Drafts::default());
        self.book.set_value(mm::DraftBook::default());
        let back = now
            .filter(|f| self.kept.with_untracked(|k| k.contains_key(*f)))
            .and_then(|f| {
                let mut out = None;
                self.kept.update(|k| out = k.remove(f));
                out
            });
        if let Some(k) = back {
            self.selected.set(Some(k.selected));
            self.element.set(k.element);
            self.set_drafts(k.drafts);
            self.book.set_value(k.book);
            self.tab.set(k.tab);
        }
    }

    /// Files with a change typed and not applied, `shown` the file on
    /// screen; tracks what the answer depends on.
    pub fn unapplied_files(self, shown: Option<&str>) -> Vec<String> {
        self.track_drafts();
        let mut files: Vec<String> = self.kept.with(|k| k.keys().cloned().collect());
        if let Some(file) = shown
            && self.selected.with(Option::is_some)
            && self.unapplied()
        {
            files.push(file.to_owned());
        }
        files
    }

    /// A write of `file` saved the drafts `sent` for `part`. `shown`: the
    /// file is on screen, its drafts are the fields'; else they are kept,
    /// and go once nothing is left to apply.
    fn written(self, file: &str, shown: bool, part: mm::Part, sent: &mm::Drafts) {
        if shown {
            self.book.update_value(|book| book.written(part, sent));
            return;
        }
        if !self.kept.with_untracked(|k| k.contains_key(file)) {
            return;
        }
        self.kept.update(|k| {
            if let Some(kept) = k.get_mut(file) {
                kept.book.written(part, sent);
                if !kept.book.dirty(&kept.drafts) {
                    k.remove(file);
                }
            }
        });
    }

    /// Drops the kept drafts of files `keep` refuses (deleted, or renamed
    /// outside the app); returns those files.
    pub fn drop_gone(self, keep: impl Fn(&str) -> bool) -> Vec<String> {
        let gone: Vec<String> = self
            .kept
            .with_untracked(|k| k.keys().filter(|f| !keep(f)).cloned().collect());
        if !gone.is_empty() {
            self.kept.update(|k| k.retain(|f, _| keep(f)));
        }
        gone
    }

    /// Files renamed or moved in the app (`to` says where a path went):
    /// their kept changes go with them. The shown file's change stays in
    /// the fields: the switch to its new name keeps them (`renamed`).
    pub fn rename(self, to: impl Fn(&str) -> Option<String>) {
        if self
            .kept
            .with_untracked(|k| k.keys().all(|f| to(f).is_none()))
        {
            return;
        }
        self.kept.update(|k| {
            let moved: Vec<(String, String)> =
                k.keys().filter_map(|f| Some((f.clone(), to(f)?))).collect();
            let taken: Vec<(String, Kept)> = moved
                .into_iter()
                .filter_map(|(from, next)| Some((next, k.remove(&from)?)))
                .collect();
            k.extend(taken);
        });
    }

    /// An image uploaded for `page`'s element, `shown` the file on screen
    /// now. True when `page` shows and its field took it, to apply now;
    /// else `page`'s kept draft takes it, to apply there (never here).
    fn take_upload(self, page: &str, shown: Option<&str>, src: String) -> bool {
        if shown == Some(page) {
            self.src.set(src);
            return true;
        }
        if self.kept.with_untracked(|k| k.contains_key(page)) {
            self.kept.update(|k| {
                if let Some(kept) = k.get_mut(page) {
                    kept.drafts.src = src;
                }
            });
        }
        false
    }

    fn drafts(self) -> mm::Drafts {
        mm::Drafts {
            text: self.text.get_untracked(),
            href: self.href.get_untracked(),
            src: self.src.get_untracked(),
            alt: self.alt.get_untracked(),
            attrs: self.attrs.get_untracked(),
            html: self.html.get_untracked(),
        }
    }

    /// Subscribes the running effect to the typed fields.
    fn track_drafts(self) {
        self.text.track();
        self.href.track();
        self.src.track();
        self.alt.track();
        self.attrs.track();
        self.html.track();
    }

    /// The picked element has a change typed and not applied.
    pub fn unapplied(self) -> bool {
        let typed = self.drafts();
        self.book
            .try_with_value(|book| book.dirty(&typed))
            .unwrap_or(false)
    }

    fn set_drafts(self, d: mm::Drafts) {
        self.text.set(d.text);
        self.href.set(d.href);
        self.src.set(d.src);
        self.alt.set(d.alt);
        self.attrs.set(d.attrs);
        self.html.set(d.html);
    }

    /// Fills the drafts from a read of the picked element. A read again of
    /// the same one (`m.element` is cleared on a new pick) keeps what the
    /// user changed since the fields were last clean.
    fn fill(self, el: &SourceElementResponse) {
        let same = self
            .element
            .with_untracked(|was| matches!(was, Some(Ok(_))));
        let typed = self.drafts();
        let drafts = self
            .book
            .try_update_value(|book| book.read(&typed, el, same))
            .unwrap_or_else(|| mm::Drafts::of(el));
        self.set_drafts(drafts);
    }
}

impl StudioCtx {
    pub(super) fn enter_manual(self) {
        self.tools.manual.error.set(None);
        // An element picked here before (its draft kept over a file
        // switch) is outlined and read again.
        self.manual_ready();
    }

    pub(super) fn leave_manual(self) {
        let m = self.tools.manual;
        m.selected.set(None);
        m.element.set(None);
        bridge::post_main(&pb::mark(None, false));
    }

    /// A new document in the frame (after an edit, an undo, a live
    /// reload): read the layers and the picked element again.
    pub(super) fn manual_ready(self) {
        bridge::post_main(&pb::layers_request());
        if let Some(sel) = self.tools.manual.selected.get_untracked() {
            bridge::post_main(&pb::mark(Some(&sel), false));
            bridge::post_main(&pb::inspect(&sel));
        }
    }

    /// An element picked in the preview or the layer list.
    pub(super) fn manual_select(self, selector: &str) {
        let m = self.tools.manual;
        m.selected.set(Some(selector.to_owned()));
        m.element.set(None);
        m.error.set(None);
        bridge::post_main(&pb::mark(Some(selector), true));
        self.inspect(selector);
    }

    /// The frame answered the inspect of the picked element: read it from
    /// the source, as the preview showed it.
    pub(super) fn manual_inspected(self, i: &Inspected) {
        let m = self.tools.manual;
        if m.selected.get_untracked().as_deref() != Some(i.selector.as_str()) {
            return;
        }
        if i.missing {
            m.element.set(Some(Err(t(T::ManualMissing).to_owned())));
            return;
        }
        let Some(file) = self.active_untracked() else {
            return;
        };
        m.read_seq.update_value(|n| *n = n.wrapping_add(1));
        let seq = m.read_seq.get_value();
        let req = SourceElementRequest {
            file,
            selector: i.selector.clone(),
            expect_tag: Some(i.tag.clone()),
            expect_text: Some(i.text.clone()),
        };
        self.tasks.spawn(async move {
            let res = api::post::<_, SourceElementResponse>(
                &format!("{}/source-element", self.base()),
                &req,
            )
            .await
            .map_err(|e| self.failed_to(T::ManualReadFailed, &e));
            if m.read_seq.try_get_value() != Some(seq) {
                return;
            }
            // A read again of the same source (a reload, a key claimed)
            // keeps what the user typed; a changed element refills.
            let same = m
                .element
                .with_untracked(|was| matches!((was, &res), (Some(Ok(a)), Ok(b)) if a == b));
            if same {
                return;
            }
            if let Ok(el) = &res {
                m.fill(el);
            }
            let _ = m.element.try_set(Some(res));
        });
    }

    /// The picked element as the preview showed it: what every write expects.
    fn manual_target(self) -> Option<SourceEdit> {
        let sel = self.tools.manual.selected.get_untracked()?;
        let i = self
            .tools
            .inspected
            .get_untracked()
            .filter(|i| i.selector == sel && !i.missing)?;
        Some(SourceEdit {
            selector: sel,
            expect_tag: Some(i.tag),
            expect_text: Some(i.text),
            ..SourceEdit::default()
        })
    }

    /// Writes `edits` as one manual version and records it for undo;
    /// `part` names the drafts it sends, clean once it is saved.
    fn manual_write(
        self,
        edits: Vec<SourceEdit>,
        label: String,
        part: mm::Part,
        after: impl FnOnce() + 'static,
    ) {
        let Some(file) = self.active_untracked() else {
            return;
        };
        let m = self.tools.manual;
        if edits.is_empty() || m.busy.get_untracked() {
            return;
        }
        m.busy.set(true);
        m.error.set(None);
        let req = SourceEditRequest {
            file,
            edits,
            label: Some(VERSION_LABEL.to_owned()),
        };
        let tools = self.tools;
        let file = req.file.clone();
        // As sent: what is typed while it saves is not written yet.
        let sent = m.drafts();
        // An image uploaded for this element and written now is the
        // page's: the server ships it from then on.
        let claim: Vec<String> = if part == mm::Part::Content {
            m.uploaded.with_value(|u| {
                u.iter()
                    .filter(|p| mm::relative_url(&file, p) == sent.src)
                    .cloned()
                    .collect()
            })
        } else {
            Vec::new()
        };
        let (base, session) = (self.base(), self.draft_session.get_value());
        // The write lands even if the Studio is left meanwhile; the panel
        // shows it only while the Studio is there.
        let path = format!("{base}/source-edits");
        let claimed = claim.clone();
        let work = async move {
            let res = api::post::<_, SourceEditResponse>(&path, &req).await;
            if res.is_ok() && !claimed.is_empty() {
                let url = format!("{base}/drafts/{}/claim", api::seg(&session));
                let body = od_contracts::ClaimDraftsRequest { paths: claimed };
                if let Err(e) = api::post::<_, od_contracts::OkResponse>(&url, &body).await {
                    leptos::logging::warn!("claim of uploaded images failed: {e:?}");
                }
            }
            res
        };
        self.tasks.spawn_then(work, move |res| {
            // Another file shown meanwhile: the panel, its history and its
            // selection are that file's; only `file`'s kept draft follows.
            let shown = self.active_untracked().as_deref() == Some(file.as_str());
            match res {
                Ok(r) => {
                    if shown && let Some(before) = r.previous_version_id {
                        m.history.update(|h| {
                            h.record(Step {
                                label: label.clone(),
                                before,
                                after: r.version.id.clone(),
                            });
                        });
                    }
                    tools.saved(&r.file.path, r.version.version);
                    m.written(&file, shown, part, &sent);
                    m.uploaded
                        .update_value(|u| u.retain(|p| !claim.contains(p)));
                    if shown {
                        after();
                    }
                }
                Err(e) => {
                    let why = self.failed_to(T::ManualApplyFailed, &e);
                    if shown {
                        m.error.set(Some(why));
                    } else {
                        self.notice.set(Some(why));
                    }
                }
            }
            m.busy.set(false);
        });
    }

    fn manual_edit(self, change: impl FnOnce(&mut SourceEdit), label: T, part: mm::Part) {
        let Some(mut edit) = self.manual_target() else {
            return;
        };
        change(&mut edit);
        self.manual_write(vec![edit], t(label).to_owned(), part, || {});
    }

    fn manual_save_content(self) {
        let m = self.tools.manual;
        let Some(Ok(el)) = m.element.get_untracked() else {
            return;
        };
        let kind = Kind::of(&el.tag, el.text_only);
        let attr = |name: &str| {
            el.attributes
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.value.clone())
                .unwrap_or_default()
        };
        let text = m.text.get_untracked();
        let mut attributes = BTreeMap::new();
        let mut set = |name: &str, value: String| {
            if value != attr(name) {
                attributes.insert(name.to_owned(), (!value.is_empty()).then_some(value));
            }
        };
        match kind {
            Kind::Link => set("href", m.href.get_untracked()),
            Kind::Image => {
                set("src", m.src.get_untracked());
                set("alt", m.alt.get_untracked());
            }
            _ => {}
        }
        let new_text = (matches!(kind, Kind::Text) || (kind == Kind::Link && el.text_only))
            && text.trim() != el.text;
        if attributes.is_empty() && !new_text {
            return;
        }
        self.manual_edit(
            |e| {
                e.attributes = attributes;
                if new_text {
                    e.text = Some(text);
                }
            },
            T::ManualApplyContent,
            mm::Part::Content,
        );
    }

    fn manual_save_style(self) {
        let sel = self.tools.manual.selected.get_untracked();
        let edits: Vec<SourceEdit> = style_edits(&self.tools.overrides.get_untracked())
            .into_iter()
            .filter(|e| Some(&e.selector) == sel.as_ref())
            .collect();
        let tools = self.tools;
        self.manual_write(
            edits,
            t(T::ManualApplyStyle).to_owned(),
            mm::Part::None,
            move || {
                let selector = sel.unwrap_or_default();
                tools
                    .overrides
                    .update(|list| list.retain(|o| o.selector != selector));
            },
        );
    }

    fn manual_save_attributes(self) {
        let m = self.tools.manual;
        let Some(Ok(el)) = m.element.get_untracked() else {
            return;
        };
        match mm::attribute_changes(&el.attributes, &m.attrs.get_untracked()) {
            Ok(changes) if changes.is_empty() => {}
            Ok(changes) => self.manual_edit(
                |e| e.attributes = changes,
                T::ManualApplyAttributes,
                mm::Part::Attributes,
            ),
            Err(e) => m.error.set(Some(tf(T::ManualInvalidAttributes, &[&e]))),
        }
    }

    fn manual_save_html(self) {
        let html = self.tools.manual.html.get_untracked();
        self.manual_edit(
            |e| e.outer_html = Some(html),
            T::ManualApplyHtml,
            mm::Part::Html,
        );
    }

    fn manual_delete(self) {
        let m = self.tools.manual;
        let Some(mut edit) = self.manual_target() else {
            return;
        };
        edit.remove = true;
        self.manual_write(
            vec![edit],
            t(T::ManualDelete).to_owned(),
            mm::Part::None,
            move || {
                m.selected.set(None);
                m.element.set(None);
                bridge::post_main(&pb::mark(None, false));
            },
        );
    }

    /// Restores the version before (`undo`) or after the last step.
    fn manual_history(self, undo: bool) {
        let m = self.tools.manual;
        let Some(file) = self.active_untracked() else {
            return;
        };
        let step = m
            .history
            .with_untracked(|h| if undo { h.to_undo() } else { h.to_redo() }.cloned());
        let Some(step) = step else { return };
        if m.busy.get_untracked() {
            return;
        }
        let (to, expect) = if undo {
            (step.before, step.after)
        } else {
            (step.after, step.before)
        };
        m.busy.set(true);
        m.error.set(None);
        self.tasks.spawn(async move {
            let url = format!(
                "{}/files/{}/versions/{}/restore",
                self.base(),
                api::file_path(&file),
                api::seg(&to)
            );
            let req = RestoreVersionRequest {
                prompt: None,
                expect_version_id: Some(expect),
            };
            let res = api::post::<_, RestoreVersionResponse>(&url, &req).await;
            // Another file shown meanwhile: the history is that file's.
            let shown = self.active_untracked().as_deref() == Some(file.as_str());
            match res {
                // The fields follow the restored source on its read,
                // except what was typed and not applied.
                Ok(_) if shown => {
                    m.history
                        .update(|h| if undo { h.undone() } else { h.redone() });
                }
                Ok(_) => {}
                Err(e) => {
                    let why = self.failed_to(T::ManualHistoryFailed, &e);
                    if shown {
                        m.error.set(Some(why));
                    } else {
                        self.notice.set(Some(why));
                    }
                }
            }
            m.busy.set(false);
            if shown {
                self.reload_preview();
            }
        });
    }

    fn manual_upload(self, file: web_sys::File) {
        let m = self.tools.manual;
        let Some(page) = self.active_untracked() else {
            return;
        };
        m.busy.set(true);
        // As this page's draft's: until a write takes it, it is no part of
        // the project, and it goes with the page (`drafts`).
        let session = self.draft_session.get_value();
        self.tasks.spawn(async move {
            match super::files_panel::upload(
                self,
                T::ManualUploadFailed,
                vec![file],
                IMAGES_DIR,
                Some(&session),
            )
            .await
            {
                Ok(paths) => {
                    m.busy.set(false);
                    self.refresh_files();
                    if let Some(path) = paths.first() {
                        m.uploaded.update_value(|u| u.push(path.clone()));
                        let src = mm::relative_url(&page, path);
                        let shown = self.active_untracked();
                        if m.take_upload(&page, shown.as_deref(), src) {
                            self.manual_save_content();
                        }
                    }
                }
                Err(e) => {
                    m.busy.set(false);
                    m.error.set(Some(e));
                }
            }
        });
    }
}

fn kind_glyph(kind: &str) -> Glyph {
    match kind {
        "link" => Glyph::Link,
        "image" => Glyph::Image,
        "text" => Glyph::EditText,
        _ => Glyph::Board,
    }
}

#[component]
pub fn ManualEditPanel(ctx: StudioCtx) -> impl IntoView {
    let m = ctx.tools.manual;
    let layers = move || {
        let list = m.layers.get();
        if list.is_empty() {
            return view! { <p class="empty small">{t(T::ManualNoLayers)}</p> }.into_any();
        }
        list.into_iter()
            .map(|l| {
                let sel = l.selector.clone();
                let is_selected = {
                    let sel = sel.clone();
                    move || m.selected.with(|s| s.as_deref() == Some(sel.as_str()))
                };
                let name = if l.label.trim().is_empty() {
                    format!("<{}>", l.tag)
                } else {
                    l.label.clone()
                };
                view! {
                    <li>
                        <button
                            class="layer"
                            class:selected=is_selected
                            class:hidden-layer=l.hidden
                            data-testid="manual-layer"
                            data-kind=l.kind.clone()
                            title=l.selector.clone()
                            style=format!("padding-left:calc(var(--space-2) + {}px)", l.depth.min(12) * 12)
                            on:click=move |_| ctx.manual_select(&sel)
                        >
                            {icon(kind_glyph(&l.kind))}
                            <span class="layer-name">{name}</span>
                            <span class="mono subtle small">{l.tag.clone()}</span>
                            {l.hidden.then(|| view! { <span class="badge">{t(T::ManualHidden)}</span> })}
                        </button>
                    </li>
                }
            })
            .collect_view()
            .into_any()
    };
    let tab_button = move |tab: Tab, label: T, testid: &'static str| {
        view! {
            <button
                class="seg"
                role="tab"
                data-testid=testid
                aria-selected=move || (m.tab.get() == tab).to_string() tabindex=move || super::focus::roving(m.tab.get() == tab)
                class:active=move || m.tab.get() == tab
                on:click=move |_| m.tab.set(tab)
            >
                {t(label)}
            </button>
        }
    };
    let can_undo = move || m.history.with(|h| h.to_undo().is_some());
    let can_redo = move || m.history.with(|h| h.to_redo().is_some());
    view! {
        {panel_head(ctx, t(T::ToolManualEdit).to_owned())}
        <div class="tool-body manual-edit" data-testid="manual-edit-panel">
            <section class="manual-section">
                <header class="manual-section-head">
                    {t(T::ManualLayers)}
                    <span class="subtle small">{move || m.layers.with(Vec::len)}</span>
                </header>
                <ul class="layer-list" data-testid="manual-layers">{layers}</ul>
            </section>
            {move || match m.element.get() {
                None if m.selected.with(Option::is_some) => view! { <div class="skeleton manual-skeleton"></div> }.into_any(),
                None => view! { <p class="empty small" data-testid="manual-empty">{t(T::ManualEmpty)}</p> }.into_any(),
                Some(Err(e)) => view! { <p class="error-text small" role="alert" data-testid="manual-read-error">{e}</p> }.into_any(),
                Some(Ok(el)) => {
                    let tag = el.tag.clone();
                    view! {
                    <section class="manual-section" data-testid="manual-element" data-tag=tag.clone()>
                        <header class="manual-section-head">
                            <strong class="mono">{format!("<{tag}>")}</strong>
                        </header>
                        <div class="segmented small manual-tabs" role="tablist" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::ToolManualEdit)>
                            {tab_button(Tab::Content, T::ManualTabContent, "manual-tab-content")}
                            {tab_button(Tab::Style, T::ManualTabStyle, "manual-tab-style")}
                            {tab_button(Tab::Attributes, T::ManualTabAttributes, "manual-tab-attributes")}
                            {tab_button(Tab::Html, T::ManualTabHtml, "manual-tab-html")}
                        </div>
                        {move || match m.tab.get() {
                            Tab::Content => view! { <ContentTab ctx=ctx el=el.clone()/> }.into_any(),
                            Tab::Style => view! { <StyleTab ctx=ctx/> }.into_any(),
                            Tab::Attributes => view! { <AttributesTab ctx=ctx/> }.into_any(),
                            Tab::Html => view! { <HtmlTab ctx=ctx/> }.into_any(),
                        }}
                    </section>
                    }
                    .into_any()
                }
            }}
            {move || m.error.get().map(|e| view! { <p class="error-text small" role="alert" data-testid="manual-error">{e}</p> })}
            <div class="manual-footer">
                <button class="icon-btn" aria-label=t(T::ManualUndo) data-tip=t(T::ManualUndo) data-testid="manual-undo"
                    disabled=move || m.busy.get() || !can_undo() on:click=move |_| ctx.manual_history(true)>
                    {icon(Glyph::Restore)}
                </button>
                <button class="icon-btn" aria-label=t(T::ManualRedo) data-tip=t(T::ManualRedo) data-testid="manual-redo"
                    disabled=move || m.busy.get() || !can_redo() on:click=move |_| ctx.manual_history(false)>
                    {icon(Glyph::Redo)}
                </button>
                <span class="spacer"></span>
                <button class="btn small danger-ghost" data-testid="manual-delete"
                    disabled=move || m.busy.get() || !m.element.with(|e| matches!(e, Some(Ok(_))))
                    on:click=move |_| ctx.manual_delete()>
                    {icon(Glyph::Delete)}{t(T::ManualDelete)}
                </button>
            </div>
        </div>
    }
}

fn field(
    label: T,
    value: RwSignal<String>,
    testid: &'static str,
    rows: Option<u32>,
) -> impl IntoView {
    let input = match rows {
        Some(r) => view! {
            <textarea class="input" rows=r data-testid=testid prop:value=move || value.get()
                on:input=live(move |ev| value.set(event_target_value(&ev)))></textarea>
        }
        .into_any(),
        None => view! {
            <input class="input small" data-testid=testid prop:value=move || value.get()
                on:input=live(move |ev| value.set(event_target_value(&ev)))/>
        }
        .into_any(),
    };
    view! {
        <label class="manual-field">
            <span class="small subtle">{t(label)}</span>
            {input}
        </label>
    }
}

#[component]
fn ContentTab(ctx: StudioCtx, el: SourceElementResponse) -> impl IntoView {
    let m = ctx.tools.manual;
    let kind = Kind::of(&el.tag, el.text_only);
    let upload = NodeRef::<leptos::html::Input>::new();
    let picked = move |_| {
        let Some(input) = upload.get() else { return };
        let file = input.files().and_then(|l| l.get(0));
        input.set_value("");
        if let Some(f) = file {
            ctx.manual_upload(f);
        }
    };
    let body = match kind {
        Kind::Text => field(T::ManualText, m.text, "manual-text", Some(4)).into_any(),
        Kind::Link => view! {
            {el.text_only.then(|| field(T::ManualText, m.text, "manual-text", Some(2)))}
            {field(T::ManualHref, m.href, "manual-href", None)}
        }
        .into_any(),
        Kind::Image => view! {
            {field(T::ManualImageUrl, m.src, "manual-src", None)}
            {field(T::ManualAlt, m.alt, "manual-alt", None)}
            <input type="file" accept="image/*" hidden node_ref=upload data-testid="manual-image-input" on:change=live(picked)/>
            <button class="btn small" data-testid="manual-upload" disabled=move || m.busy.get()
                on:click=move |_| { if let Some(i) = upload.get() { i.click(); } }>
                {icon(Glyph::Upload)}{t(T::ManualUploadImage)}
            </button>
        }
        .into_any(),
        Kind::Container => view! { <p class="small subtle">{t(T::ManualContainerHint)}</p> }.into_any(),
    };
    view! {
        <div class="manual-tab" role="tabpanel">
            {body}
            {(kind != Kind::Container).then(|| view! {
                <div class="row end">
                    <button class="btn small primary" data-testid="manual-apply-content" disabled=move || m.busy.get()
                        on:click=move |_| ctx.manual_save_content()>
                        {t(T::ManualApply)}
                    </button>
                </div>
            })}
        </div>
    }
}

#[component]
fn StyleTab(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let m = tools.manual;
    let rows = move || {
        let sel = m.selected.get()?;
        let i = tools
            .inspected
            .get()
            .filter(|i| i.selector == sel && !i.missing)?;
        Some(
            STYLE_PROPS
                .into_iter()
                .map(|prop| {
                    let computed = i.styles.get(prop).cloned().unwrap_or_default();
                    let set = i.overrides.get(prop).cloned();
                    let shown = set.clone().unwrap_or(computed);
                    let (s1, s2, s3) = (sel.clone(), sel.clone(), sel.clone());
                    let picker = prop.ends_with("color").then_some(super::preview_tools::color_swatch(
                        &shown,
                        None,
                        move |v| ctx.set_override(&s1, prop, &v),
                    ));
                    view! {
                        <label class="inspect-row" class:overridden=set.is_some()>
                            <span class="mono small">{prop}</span>
                            <span class="inspect-value">
                                {picker}
                                <input class="input small" data-testid=format!("manual-style-{prop}") prop:value=shown
                                    on:change=live(move |ev| {
                                        let v = event_target_value(&ev).trim().to_owned();
                                        if !v.is_empty() {
                                            ctx.set_override(&s2, prop, &v);
                                        }
                                    })/>
                                {set.is_some().then(|| view! {
                                    <button class="chip-x" title=t(T::ResetProp) aria-label=t(T::ResetProp)
                                        on:click=move |ev| { ev.prevent_default(); ctx.reset_override(&s3, prop); }>
                                        {icon(Glyph::Reset)}
                                    </button>
                                })}
                            </span>
                        </label>
                    }
                })
                .collect_view(),
        )
    };
    let pending = move || {
        let sel = m.selected.get();
        tools.overrides.with(|l| {
            l.iter()
                .filter(|o| Some(&o.selector) == sel.as_ref())
                .count()
        })
    };
    view! {
        <div class="manual-tab inspect-props" role="tabpanel">
            {rows}
            <div class="row end">
                <button class="btn small primary" data-testid="manual-apply-style"
                    disabled=move || m.busy.get() || pending() == 0
                    on:click=move |_| ctx.manual_save_style()>
                    {t(T::ManualApply)}
                </button>
            </div>
        </div>
    }
}

#[component]
fn AttributesTab(ctx: StudioCtx) -> impl IntoView {
    let m = ctx.tools.manual;
    view! {
        <div class="manual-tab" role="tabpanel">
            <label class="manual-field">
                <span class="small subtle">{t(T::ManualAttributesJson)}</span>
                <textarea class="input mono manual-code" rows="8" spellcheck="false" data-testid="manual-attributes"
                    prop:value=move || m.attrs.get() on:input=live(move |ev| m.attrs.set(event_target_value(&ev)))></textarea>
            </label>
            <div class="row end">
                <button class="btn small primary" data-testid="manual-apply-attributes" disabled=move || m.busy.get()
                    on:click=move |_| ctx.manual_save_attributes()>
                    {t(T::ManualApply)}
                </button>
            </div>
        </div>
    }
}

#[component]
fn HtmlTab(ctx: StudioCtx) -> impl IntoView {
    let m = ctx.tools.manual;
    view! {
        <div class="manual-tab" role="tabpanel">
            <label class="manual-field">
                <span class="small subtle">{t(T::ManualSelectedHtml)}</span>
                <textarea class="input mono manual-code" rows="10" spellcheck="false" data-testid="manual-html"
                    prop:value=move || m.html.get() on:input=live(move |ev| m.html.set(event_target_value(&ev)))></textarea>
            </label>
            <div class="row end">
                <button class="btn small primary" data-testid="manual-apply-html" disabled=move || m.busy.get()
                    on:click=move |_| ctx.manual_save_html()>
                    {t(T::ManualApply)}
                </button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod forget_tests {
    use super::*;
    use od_contracts::SourceElementResponse;

    fn lead() -> SourceElementResponse {
        SourceElementResponse {
            tag: "p".to_owned(),
            attributes: Vec::new(),
            outer_html: "<p class=\"lead\">was</p>".to_owned(),
            text: "was".to_owned(),
            text_only: true,
        }
    }

    /// `m` with `p.lead` of the shown file picked and read, and `typed` in
    /// its text field.
    fn picked(m: Manual, typed: &str) {
        let el = lead();
        m.selected.set(Some("p.lead".to_owned()));
        m.book.update_value(|book| {
            book.read(&mm::Drafts::of(&el), &el, false);
        });
        m.set_drafts(mm::Drafts::of(&el));
        m.element.set(Some(Ok(el)));
        m.text.set(typed.to_owned());
    }

    #[test]
    fn a_change_not_applied_comes_back_with_its_file() {
        let owner = Owner::new();
        owner.with(|| {
            let m = Manual::new();
            picked(m, "Typed not applied");
            m.tab.set(Tab::Html);
            assert_eq!(m.unapplied_files(Some("a.html")), ["a.html"]);
            m.forget(Some("a.html"), Some("b.html"));
            assert_eq!(m.text.get_untracked(), "", "b.html shows none of it");
            assert!(m.selected.get_untracked().is_none());
            assert!(!m.unapplied(), "b.html has nothing typed");
            assert_eq!(
                m.unapplied_files(Some("b.html")),
                ["a.html"],
                "the leave guard still lists a.html"
            );
            m.forget(Some("b.html"), Some("a.html"));
            assert_eq!(m.text.get_untracked(), "Typed not applied");
            assert_eq!(m.selected.get_untracked().as_deref(), Some("p.lead"));
            assert_eq!(m.tab.get_untracked(), Tab::Html);
            assert!(m.unapplied(), "still to apply, on a.html");
            assert_eq!(m.unapplied_files(Some("a.html")), ["a.html"]);
        });
    }

    #[test]
    fn a_file_gone_takes_its_kept_change() {
        let owner = Owner::new();
        owner.with(|| {
            let m = Manual::new();
            picked(m, "Typed");
            m.forget(Some("a.html"), Some("b.html"));
            m.drop_gone(|f| f != "a.html");
            assert!(
                m.unapplied_files(Some("b.html")).is_empty(),
                "a.html was deleted"
            );
        });
    }

    #[test]
    fn nothing_typed_keeps_nothing() {
        let owner = Owner::new();
        owner.with(|| {
            let m = Manual::new();
            picked(m, "was");
            m.forget(Some("a.html"), Some("b.html"));
            m.forget(Some("b.html"), Some("a.html"));
            assert!(
                m.selected.get_untracked().is_none(),
                "a clean pick starts over"
            );
            assert!(m.unapplied_files(Some("a.html")).is_empty());
        });
    }

    #[test]
    fn a_late_read_of_the_last_file_is_dropped() {
        let owner = Owner::new();
        owner.with(|| {
            let m = Manual::new();
            let seq = m.read_seq.get_value();
            m.forget(Some("a.html"), Some("b.html"));
            assert_ne!(
                m.read_seq.get_value(),
                seq,
                "a read sent for a.html is stale"
            );
        });
    }

    #[test]
    fn a_write_that_lands_after_a_switch_cleans_its_own_file_only() {
        let owner = Owner::new();
        owner.with(|| {
            let m = Manual::new();
            picked(m, "Sent");
            let sent = m.drafts();
            m.forget(Some("a.html"), Some("b.html"));
            // b.html's own element, typed too.
            picked(m, "On b");
            m.written("a.html", false, mm::Part::Content, &sent);
            assert!(m.unapplied(), "b.html's text is still not applied");
            assert_eq!(
                m.unapplied_files(Some("b.html")),
                ["b.html"],
                "a.html's draft was written: nothing left to ask about"
            );
        });
    }

    #[test]
    fn an_image_uploaded_after_a_switch_goes_to_its_own_file() {
        let owner = Owner::new();
        owner.with(|| {
            let m = Manual::new();
            picked(m, "Typed");
            m.forget(Some("a.html"), Some("b.html"));
            let now = m.take_upload("a.html", Some("b.html"), "assets/new.png".to_owned());
            assert!(!now, "nothing applied to b.html");
            assert_eq!(m.src.get_untracked(), "", "b.html's field untouched");
            m.forget(Some("b.html"), Some("a.html"));
            assert_eq!(m.src.get_untracked(), "assets/new.png");
        });
    }
}
