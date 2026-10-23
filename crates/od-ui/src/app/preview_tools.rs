//! Preview tools (spec 1.2): inspect and override styles, edit text in
//! place, tweaks the artifact declares, palette presets, drawn annotations
//! sent with the next message, the anti-slop lint and the deliverable check
//! after a run. Edits are written back through `POST source-edits`, which
//! records a manual version.

use super::api;
use super::bridge;
use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, is_html_path};
use crate::i18n::{T, t, tf};
use crate::preview_bridge::{
    self as pb, BridgeMsg, Inspected, Override, Snapshot, TweakDef, TweakKind,
};
use leptos::prelude::*;
use od_contracts::{
    ContentEncoding, DeliverableResponse, DeliverableValidation, FileResponse, LintResponse,
    LintSeverity, SourceEdit, SourceEditRequest, SourceEditResponse, WriteFileRequest,
};
use std::collections::BTreeMap;
use wasm_bindgen::JsCast;

/// What the pointer does over the preview, or which side panel is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Inspect,
    EditText,
    Draw,
    Tweaks,
    Lint,
}

/// Properties the inspect panel shows, in order.
const INSPECT_PROPS: [&str; 12] = [
    "color",
    "background-color",
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "letter-spacing",
    "text-align",
    "padding",
    "margin",
    "border-radius",
    "opacity",
];

/// How long to wait for the bridge's snapshot.
const SNAPSHOT_WAIT_MS: u32 = 4000;
const DRAW_COLORS: [&str; 3] = ["#e5484d", "#1f6feb", "#1d7f4e"];
/// Folder (in the project) for annotation images.
const ANNOTATIONS_DIR: &str = "annotations";

/// State of the preview tools, shared through [`StudioCtx`].
#[derive(Clone, Copy)]
pub struct Tools {
    pub tool: RwSignal<Option<Tool>>,
    pub inspected: RwSignal<Option<Inspected>>,
    /// Overrides not yet written to the source; replayed after a reload.
    pub overrides: RwSignal<Vec<Override>>,
    pub extracted: RwSignal<Option<String>>,
    pub tweaks: RwSignal<Vec<TweakDef>>,
    /// Tweak values set for the shown file, replayed after a reload.
    pub tweak_values: RwSignal<BTreeMap<String, String>>,
    pub palette: RwSignal<Option<String>>,
    pub lint: RwSignal<Option<Result<LintResponse, String>>>,
    /// Number of the latest lint request; older answers are dropped.
    lint_seq: StoredValue<u32>,
    /// Outcome of the last tool action (saved, failed…).
    pub status: RwSignal<Option<String>>,
    /// Project files (annotation images) sent with the next message.
    pub attachments: RwSignal<Vec<String>>,
    pub snapshot: RwSignal<Option<Snapshot>>,
    /// The last run's deliverable check when it failed.
    pub deliverable: RwSignal<Option<DeliverableResponse>>,
    /// Id of the main frame's current load (`raw?…&key=<id>`).
    frame_kid: StoredValue<String>,
    /// The load id whose key was asked for, so it is asked once.
    claimed_kid: StoredValue<String>,
    /// That load's edit key, once claimed: bridge messages that write the
    /// source must carry it.
    edit_key: StoredValue<Option<String>>,
}

impl Tools {
    pub fn new() -> Self {
        Self {
            tool: RwSignal::new(None),
            inspected: RwSignal::new(None),
            overrides: RwSignal::new(Vec::new()),
            extracted: RwSignal::new(None),
            tweaks: RwSignal::new(Vec::new()),
            tweak_values: RwSignal::new(BTreeMap::new()),
            palette: RwSignal::new(None),
            lint: RwSignal::new(None),
            lint_seq: StoredValue::new(0),
            status: RwSignal::new(None),
            attachments: RwSignal::new(Vec::new()),
            snapshot: RwSignal::new(None),
            deliverable: RwSignal::new(None),
            frame_kid: StoredValue::new(String::new()),
            claimed_kid: StoredValue::new(String::new()),
            edit_key: StoredValue::new(None),
        }
    }

    /// A fresh id for the next load of the main frame; the old key goes.
    pub fn new_frame_kid(self) -> String {
        let part = || (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
        let kid = format!("load-{:08x}{:08x}{:08x}", part(), part(), part());
        self.frame_kid.set_value(kid.clone());
        self.edit_key.set_value(None);
        kid
    }

    /// Another file is shown: its edits, tweaks and lint start over.
    pub fn forget_document(self) {
        self.inspected.set(None);
        self.overrides.set(Vec::new());
        self.extracted.set(None);
        self.tweaks.set(Vec::new());
        self.tweak_values.set(BTreeMap::new());
        self.lint.set(None);
        self.status.set(None);
    }
}

/// `rgb(r, g, b)` / `rgba(…)` as `#rrggbb` for a color input.
pub fn rgb_to_hex(css: &str) -> Option<String> {
    let inner = css
        .trim()
        .strip_prefix("rgba(")
        .or_else(|| css.trim().strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<u8> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .take(3)
        .map(|p| {
            p.parse::<f64>()
                .ok()
                .map(|v| v.clamp(0.0, 255.0).round() as u8)
        })
        .collect::<Option<_>>()?;
    (parts.len() == 3).then(|| format!("#{:02x}{:02x}{:02x}", parts[0], parts[1], parts[2]))
}

/// Longest element text the bridge sends as a print (`plainText`).
const PRINT_CHARS: usize = 160;

/// Whether `selector` names its element by its own `data-od-id`, which a
/// script that rewrites the text does not move.
fn anchored(selector: &str) -> bool {
    selector
        .rsplit(" > ")
        .next()
        .is_some_and(|last| last.starts_with("[data-od-id="))
}

/// Groups overrides into one style edit per element, which the source
/// takes only if its element has the tag and text the preview showed (the
/// tag alone for an element anchored by its `data-od-id`).
pub fn style_edits(overrides: &[Override]) -> Vec<SourceEdit> {
    let mut by_selector: BTreeMap<&str, (&Override, BTreeMap<String, Option<String>>)> =
        BTreeMap::new();
    for o in overrides {
        by_selector
            .entry(&o.selector)
            .or_insert_with(|| (o, BTreeMap::new()))
            .1
            .insert(o.prop.clone(), Some(o.value.clone()));
    }
    by_selector
        .into_iter()
        .map(|(selector, (first, style))| SourceEdit {
            selector: selector.to_owned(),
            style,
            expect_tag: (!first.tag.is_empty()).then(|| first.tag.clone()),
            expect_text: (!first.tag.is_empty() && !anchored(selector)).then(|| first.text.clone()),
            ..SourceEdit::default()
        })
        .collect()
}

/// After a text edit was saved: the overrides of that element now expect
/// its new text (as the bridge prints it), or the next style save fails.
fn retext_overrides(overrides: &mut [Override], selector: &str, text: &str) {
    let print: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(PRINT_CHARS)
        .collect();
    for o in overrides.iter_mut().filter(|o| o.selector == selector) {
        o.text.clone_from(&print);
    }
}

/// Removes the overrides the frame reported it could not replay; returns
/// how many went.
fn drop_skipped(overrides: &mut Vec<Override>, skipped: &[pb::Skipped]) -> usize {
    let before = overrides.len();
    overrides.retain(|o| {
        !skipped
            .iter()
            .any(|s| s.selector == o.selector && s.prop == o.prop)
    });
    before - overrides.len()
}

/// Whether a bridge message carries the frame's edit key. The artifact's
/// own scripts share the bridge's window but never see the key.
fn keyed(msg_key: Option<&str>, key: Option<&str>) -> bool {
    key.is_some() && msg_key == key
}

/// A text edit the host should write: from the bridge (keyed), only while
/// the text tool is on and for the file the bridge was told about.
fn accepted_text_edit(
    e: &pb::TextEdit,
    tool: Option<Tool>,
    active: Option<&str>,
    key: Option<&str>,
) -> bool {
    keyed(e.key.as_deref(), key)
        && tool == Some(Tool::EditText)
        && active.is_some()
        && e.doc.as_deref() == active
}

#[derive(serde::Deserialize)]
struct EditKeyResponse {
    key: String,
}

/// The snapshot's image, if it is one: the URL is loaded by the host page.
fn snapshot_image(s: Snapshot) -> Option<String> {
    s.data_url.filter(|u| u.starts_with("data:image/"))
}

fn deliverable_message(d: &DeliverableResponse) -> Option<String> {
    let entry = d.entry_file.clone().unwrap_or_default();
    Some(match d.validation {
        DeliverableValidation::Valid | DeliverableValidation::NotSucceeded => return None,
        DeliverableValidation::NoArtifact => t(T::DeliverableNoArtifact).to_owned(),
        DeliverableValidation::ProjectMissing => t(T::DeliverableProjectMissing).to_owned(),
        DeliverableValidation::EntryMissing => t(T::DeliverableEntryMissing).to_owned(),
        DeliverableValidation::EntryNotTouched => tf(T::DeliverableNotTouched, &[&entry]),
        DeliverableValidation::EntryUnreadable => tf(T::DeliverableUnreadable, &[&entry]),
        DeliverableValidation::TypeMismatch => tf(T::DeliverableTypeMismatch, &[&entry]),
    })
}

fn palette_label(id: &str) -> &'static str {
    match id {
        "coral" => t(T::PaletteCoral),
        "electric" => t(T::PaletteElectric),
        "acid-forest" => t(T::PaletteAcidForest),
        "risograph" => t(T::PaletteRisograph),
        _ => t(T::PaletteMonoNoir),
    }
}

impl StudioCtx {
    /// Turns a tool on (`Some`) or off. Tools and comment mode exclude
    /// each other; the bridge's selection and edit modes follow.
    pub fn set_tool(self, tool: Option<Tool>) {
        if tool.is_some() && self.comment_mode.get_untracked() {
            self.set_comment_mode(false);
        }
        self.tools.tool.set(tool);
        bridge::post_main(&pb::select_mode(tool == Some(Tool::Inspect)));
        let doc = self.active_untracked();
        bridge::post_main(&pb::edit_mode(tool == Some(Tool::EditText), doc.as_deref()));
    }

    /// Leaves any tool for comment mode (which owns selection then).
    pub fn leave_tool(self) {
        if self.tools.tool.get_untracked().is_some() {
            self.tools.tool.set(None);
            bridge::post_main(&pb::edit_mode(false, None));
        }
    }

    pub fn toggle_tool(self, tool: Tool) {
        let on = self.tools.tool.get_untracked() == Some(tool);
        self.set_tool((!on).then_some(tool));
    }

    /// A new document in the frame: put modes, overrides, palette and
    /// tweaks back, and re-read the inspected element.
    pub fn tools_ready(self) {
        let tools = self.tools;
        match tools.tool.get_untracked() {
            Some(Tool::Inspect) => bridge::post_main(&pb::select_mode(true)),
            Some(Tool::EditText) => {
                let doc = self.active_untracked();
                bridge::post_main(&pb::edit_mode(true, doc.as_deref()));
            }
            _ => {}
        }
        let overrides = tools.overrides.get_untracked();
        if !overrides.is_empty() {
            bridge::post_main(&pb::inspect_replay(&overrides));
        }
        if let Some(p) = tools.palette.get_untracked() {
            bridge::post_main(&pb::palette(Some(&p)));
        }
        let kid = tools.frame_kid.get_value();
        if kid.is_empty() || tools.claimed_kid.get_value() == kid {
            if let Some(i) = tools.inspected.get_untracked() {
                bridge::post_main(&pb::inspect(&i.selector));
            }
            return;
        }
        // Take this load's edit key, then re-read the inspected element
        // (its answer counts only with the key).
        tools.claimed_kid.set_value(kid.clone());
        leptos::task::spawn_local(async move {
            let url = format!("{}/edit-keys/{}", self.base(), api::seg(&kid));
            if let Ok(r) = api::post::<_, EditKeyResponse>(&url, &serde_json::json!({})).await
                && tools.frame_kid.get_value() == kid
            {
                tools.edit_key.set_value(Some(r.key));
            }
            if let Some(i) = tools.inspected.get_untracked() {
                bridge::post_main(&pb::inspect(&i.selector));
            }
        });
    }

    /// Handles the tool messages of the main frame; `false` for others.
    pub fn on_tool_msg(self, msg: &BridgeMsg) -> bool {
        let tools = self.tools;
        match msg {
            BridgeMsg::InspectResult(i) => {
                if keyed(i.key.as_deref(), tools.edit_key.get_value().as_deref()) {
                    tools.inspected.set(Some(i.clone()));
                }
            }
            BridgeMsg::InspectOverrides { css, .. } => tools.extracted.set(Some(css.clone())),
            BridgeMsg::InspectReplaySkipped { items } => {
                let mut dropped = 0;
                tools
                    .overrides
                    .update(|list| dropped = drop_skipped(list, items));
                if dropped > 0 {
                    tools
                        .status
                        .set(Some(tf(T::OverridesDropped, &[&dropped.to_string()])));
                }
            }
            BridgeMsg::TextEdit(e) => {
                let active = self.active_untracked();
                let key = tools.edit_key.get_value();
                if accepted_text_edit(
                    e,
                    tools.tool.get_untracked(),
                    active.as_deref(),
                    key.as_deref(),
                ) {
                    self.save_edits(
                        vec![SourceEdit {
                            selector: e.selector.clone(),
                            text: Some(e.text.clone()),
                            expect_text: Some(e.before.clone()),
                            ..SourceEdit::default()
                        }],
                        false,
                    );
                }
            }
            BridgeMsg::Tweaks { tweaks } => {
                let defs = TweakDef::parse_list(tweaks);
                // Values set before the reload apply again.
                let values = tools.tweak_values.get_untracked();
                for (id, v) in &values {
                    if defs.iter().any(|d| &d.id == id) {
                        bridge::post_main(&pb::tweak_set(id, v));
                    }
                }
                if defs.is_empty() && tools.tool.get_untracked() == Some(Tool::Tweaks) {
                    tools.tool.set(None);
                }
                tools.tweaks.set(defs);
            }
            BridgeMsg::PaletteState { .. } => {}
            BridgeMsg::SnapshotResult(s) => tools.snapshot.set(Some(s.clone())),
            _ => return false,
        }
        true
    }

    /// An element picked while inspecting.
    pub fn inspect(self, selector: &str) {
        self.tools.extracted.set(None);
        bridge::post_main(&pb::inspect(selector));
    }

    fn set_override(self, selector: &str, prop: &str, value: &str) {
        let Some(el) = self
            .tools
            .inspected
            .get_untracked()
            .filter(|i| i.selector == selector && !i.missing)
        else {
            return;
        };
        bridge::post_main(&pb::inspect_set(selector, prop, value));
        self.tools.overrides.update(|list| {
            list.retain(|o| !(o.selector == selector && o.prop == prop));
            list.push(Override {
                selector: selector.to_owned(),
                prop: prop.to_owned(),
                value: value.to_owned(),
                tag: el.tag,
                text: el.text,
            });
        });
    }

    fn reset_override(self, selector: &str, prop: &str) {
        bridge::post_main(&pb::inspect_reset(Some(selector), Some(prop)));
        self.tools
            .overrides
            .update(|list| list.retain(|o| !(o.selector == selector && o.prop == prop)));
    }

    fn reset_all_overrides(self) {
        bridge::post_main(&pb::inspect_reset(None, None));
        self.tools.overrides.set(Vec::new());
        self.tools.extracted.set(None);
        if let Some(i) = self.tools.inspected.get_untracked() {
            self.inspect(&i.selector);
        }
    }

    /// Writes `edits` into the shown file as a manual version. Style saves
    /// drop the saved overrides; a refused text edit reloads the frame so it
    /// shows the source again.
    fn save_edits(self, edits: Vec<SourceEdit>, styles: bool) {
        let Some(file) = self.active_untracked() else {
            return;
        };
        let req = SourceEditRequest {
            file: file.clone(),
            edits,
            label: None,
        };
        let tools = self.tools;
        leptos::task::spawn_local(async move {
            let path = format!("{}/source-edits", self.base());
            match api::post::<_, SourceEditResponse>(&path, &req).await {
                Ok(r) => {
                    if styles {
                        tools.overrides.set(Vec::new());
                        tools.extracted.set(None);
                    } else {
                        tools.overrides.update(|list| {
                            for e in &req.edits {
                                if let Some(text) = &e.text {
                                    retext_overrides(list, &e.selector, text);
                                }
                            }
                        });
                    }
                    tools.status.set(Some(tf(
                        T::SavedVersion,
                        &[&r.file.path, &r.version.version.to_string()],
                    )));
                }
                Err(e) => {
                    tools.status.set(Some(tf(T::SaveFailed, &[&e.message])));
                    if !styles {
                        self.reload_preview();
                    }
                }
            }
        });
    }

    fn save_overrides(self) {
        let edits = style_edits(&self.tools.overrides.get_untracked());
        if !edits.is_empty() {
            self.save_edits(edits, true);
        }
    }

    fn set_tweak(self, id: &str, value: String) {
        bridge::post_main(&pb::tweak_set(id, &value));
        self.tools
            .tweak_values
            .update(|v| _ = v.insert(id.to_owned(), value));
    }

    fn reset_tweaks(self) {
        let defs = self.tools.tweaks.get_untracked();
        for d in &defs {
            bridge::post_main(&pb::tweak_set(&d.id, &d.initial()));
        }
        self.tools.tweak_values.set(BTreeMap::new());
    }

    pub fn set_palette(self, id: Option<String>) {
        bridge::post_main(&pb::palette(id.as_deref()));
        self.tools.palette.set(id);
    }

    /// Re-lints the shown HTML file.
    pub fn refresh_lint(self, path: String) {
        let tools = self.tools;
        tools.lint_seq.update_value(|n| *n = n.wrapping_add(1));
        let seq = tools.lint_seq.get_value();
        leptos::task::spawn_local(async move {
            let url = format!("{}/lint/{}", self.base(), api::file_path(&path));
            let res = api::get::<LintResponse>(&url)
                .await
                .map_err(|e| tf(T::LintFailed, &[&e.to_string()]));
            // A reload may start a new lint before this one answers, and
            // leaving the Studio disposes the tools (`try_`: no panic).
            if tools.lint_seq.try_get_value() == Some(seq) {
                let _ = tools.lint.try_set(Some(res));
            }
        });
    }

    /// After a successful run: warn when it did not deliver its entry.
    pub fn check_deliverable(self, run_id: String) {
        let tools = self.tools;
        leptos::task::spawn_local(async move {
            let url = format!("/api/runs/{}/deliverable", api::seg(&run_id));
            if let Ok(d) = api::get::<DeliverableResponse>(&url).await {
                let _ = tools.deliverable.try_set((!d.valid).then_some(d));
            }
        });
    }
}

/// The tool buttons, palette menu and lint badge under the preview head
/// (HTML files only), with the hint and status lines.
#[component]
pub fn ToolBar(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let tool_button = move |tool: Tool, label: T, glyph: Glyph, testid: &'static str| {
        view! {
            <button
                class="btn small ghost"
                class:active=move || tools.tool.get() == Some(tool)
                data-testid=testid
                aria-pressed=move || (tools.tool.get() == Some(tool)).to_string()
                on:click=move |_| ctx.toggle_tool(tool)
            >
                {icon(glyph)}
                {t(label)}
            </button>
        }
    };
    let lint_badge = move || {
        let counts = tools.lint.with(|l| match l {
            Some(Ok(r)) => Some((
                r.findings.len(),
                r.findings.iter().any(|f| f.severity == LintSeverity::P0),
            )),
            _ => None,
        });
        counts.map(|(n, p0)| {
            let label = if n == 0 {
                t(T::LintClean).to_owned()
            } else {
                tf(T::LintBadge, &[&n.to_string()])
            };
            view! {
                <button
                    class="btn small ghost lint-badge"
                    class:p0=p0
                    class:active=move || tools.tool.get() == Some(Tool::Lint)
                    data-testid="lint-badge"
                    data-count=n.to_string()
                    on:click=move |_| ctx.toggle_tool(Tool::Lint)
                >
                    {icon(if n == 0 { Glyph::LintOk } else { Glyph::Warning })}
                    {label}
                </button>
            }
        })
    };
    view! {
        <div class="tool-bar" data-testid="tool-bar">
            {tool_button(Tool::Inspect, T::ToolInspect, Glyph::Inspect, "inspect-mode")}
            {tool_button(Tool::EditText, T::ToolEditText, Glyph::EditText, "edit-mode")}
            {tool_button(Tool::Draw, T::ToolDraw, Glyph::Draw, "draw-mode")}
            <Show when=move || tools.tweaks.with(|t| !t.is_empty())>
                {tool_button(Tool::Tweaks, T::ToolTweaks, Glyph::Tweaks, "tweaks-toggle")}
            </Show>
            <span class="head-divider" aria-hidden="true"></span>
            <label class="palette-pick small">
                {icon(Glyph::Palette)}
                <span class="muted">{t(T::PaletteLabel)}</span>
                <select
                    class="input small"
                    data-testid="palette"
                    prop:value=move || tools.palette.get().unwrap_or_default()
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        ctx.set_palette((!v.is_empty()).then_some(v));
                    }
                >
                    <option value="">{t(T::PaletteOriginal)}</option>
                    {pb::PALETTES
                        .into_iter()
                        .map(|id| view! { <option value=id>{palette_label(id)}</option> })
                        .collect_view()}
                </select>
            </label>
            <span class="spacer"></span>
            {lint_badge}
        </div>
        {move || {
            let hint = match tools.tool.get() {
                Some(Tool::Inspect) => Some(t(T::InspectHint)),
                Some(Tool::EditText) => Some(t(T::EditTextHint)),
                Some(Tool::Draw) => Some(t(T::DrawHint)),
                _ => None,
            };
            hint.map(|h| view! { <p class="mode-hint">{icon(Glyph::Info)}{h}</p> })
        }}
        {move || tools.status.get().map(|s| view! {
            <p class="tool-status" data-testid="tool-status">
                {icon(Glyph::Info)}
                <span class="grow">{s}</span>
                <button class="chip-x" title=t(T::Close) aria-label=t(T::Close) on:click=move |_| tools.status.set(None)>{icon(Glyph::Close)}</button>
            </p>
        })}
    }
}

/// Side panel of the open tool: inspect, tweaks or lint.
#[component]
pub fn ToolPanel(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    move || {
        let body = match tools.tool.get() {
            Some(Tool::Inspect) => view! { <InspectPanel ctx=ctx/> }.into_any(),
            Some(Tool::Tweaks) => view! { <TweaksPanel ctx=ctx/> }.into_any(),
            Some(Tool::Lint) => view! { <LintPanel ctx=ctx/> }.into_any(),
            _ => return None,
        };
        Some(view! {
            <aside class="versions tool-panel" data-testid="tool-panel">
                {body}
            </aside>
        })
    }
}

fn panel_head(ctx: StudioCtx, title: String) -> impl IntoView {
    view! {
        <div class="pane-head">
            <span class="pane-title">{title}</span>
            <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| ctx.set_tool(None)>
                {icon(Glyph::Close)}
            </button>
        </div>
    }
}

#[component]
fn InspectPanel(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let rows = move || {
        let Some(i) = tools.inspected.get() else {
            return view! { <p class="empty small">{t(T::InspectEmpty)}</p> }.into_any();
        };
        if i.missing {
            return view! { <p class="empty small">{t(T::InspectMissing)}</p> }.into_any();
        }
        let selector = i.selector.clone();
        view! {
            <div class="inspect-target">
                <strong class="mono">{format!("<{}>", i.tag)}</strong>
                <span class="muted small mono" title=selector.clone()>{selector.clone()}</span>
            </div>
            <div class="inspect-props">
                {INSPECT_PROPS
                    .into_iter()
                    .map(|prop| {
                        let computed = i.styles.get(prop).cloned().unwrap_or_default();
                        let set = i.overrides.get(prop).cloned();
                        let shown = set.clone().unwrap_or_else(|| computed.clone());
                        let is_color = prop.ends_with("color");
                        let sel_text = selector.clone();
                        let sel_pick = selector.clone();
                        let sel_reset = selector.clone();
                        let picker = is_color.then(|| {
                            let hex = rgb_to_hex(&shown).unwrap_or_else(|| "#000000".to_owned());
                            view! {
                                <input
                                    type="color"
                                    class="swatch"
                                    data-testid=format!("inspect-{prop}-picker")
                                    prop:value=hex
                                    on:input=move |ev| ctx.set_override(&sel_pick, prop, &event_target_value(&ev))
                                />
                            }
                        });
                        view! {
                            <label class="inspect-row" class:overridden=set.is_some()>
                                <span class="mono small">{prop}</span>
                                <span class="inspect-value">
                                    {picker}
                                    <input
                                        class="input small"
                                        data-testid=format!("inspect-{prop}")
                                        prop:value=shown
                                        on:change=move |ev| {
                                            let v = event_target_value(&ev).trim().to_owned();
                                            if !v.is_empty() {
                                                ctx.set_override(&sel_text, prop, &v);
                                            }
                                        }
                                    />
                                    {set.is_some().then(|| view! {
                                        <button
                                            class="chip-x"
                                            title=t(T::ResetProp)
                                            aria-label=t(T::ResetProp)
                                            data-testid=format!("inspect-reset-{prop}")
                                            on:click=move |ev| {
                                                ev.prevent_default();
                                                ctx.reset_override(&sel_reset, prop);
                                            }
                                        >
                                            {icon(Glyph::Reset)}
                                        </button>
                                    })}
                                </span>
                            </label>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any()
    };
    let pending = move || tools.overrides.with(Vec::len);
    view! {
        {panel_head(ctx, t(T::ToolInspect).to_owned())}
        <div class="tool-body">
            {rows}
            <p class="muted small" data-testid="overrides-count">
                {move || match pending() {
                    0 => t(T::NoOverrides).to_owned(),
                    n => tf(T::UnsavedOverrides, &[&n.to_string()]),
                }}
            </p>
            <div class="row wrap">
                <button class="btn small" data-testid="inspect-reset-all" disabled=move || pending() == 0 on:click=move |_| ctx.reset_all_overrides()>
                    {t(T::ResetAll)}
                </button>
                <button class="btn small" data-testid="inspect-extract" on:click=move |_| bridge::post_main(&pb::inspect_extract())>
                    {t(T::ExtractCss)}
                </button>
                <button class="btn small primary" data-testid="inspect-save" disabled=move || pending() == 0 on:click=move |_| ctx.save_overrides()>
                    {t(T::SaveToSource)}
                </button>
            </div>
            {move || tools.extracted.get().filter(|c| !c.is_empty()).map(|css| view! {
                <pre class="extracted" data-testid="inspect-css">{css}</pre>
            })}
        </div>
    }
}

#[component]
fn TweaksPanel(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let controls = move || {
        let defs = tools.tweaks.get();
        if defs.is_empty() {
            return view! { <p class="empty small">{t(T::TweaksEmpty)}</p> }.into_any();
        }
        defs.into_iter()
            .map(|d| {
                let id = d.id.clone();
                let value = tools
                    .tweak_values
                    .with_untracked(|v| v.get(&d.id).cloned())
                    .unwrap_or_else(|| d.initial());
                let testid = format!("tweak-{}", d.id);
                let set = move |v: String| ctx.set_tweak(&id, v);
                let control = match d.kind {
                    TweakKind::Toggle => view! {
                        <input
                            type="checkbox"
                            data-testid=testid
                            prop:checked=value == "true"
                            on:change=move |ev| set(event_target_checked(&ev).to_string())
                        />
                    }
                    .into_any(),
                    TweakKind::Select => view! {
                        <select class="input small" data-testid=testid prop:value=value on:change=move |ev| set(event_target_value(&ev))>
                            {d.options
                                .iter()
                                .map(|o| view! { <option value=o.value().to_owned()>{o.label().to_owned()}</option> })
                                .collect_view()}
                        </select>
                    }
                    .into_any(),
                    TweakKind::Range => view! {
                        <input
                            type="range"
                            data-testid=testid
                            min=d.min.unwrap_or(0.0).to_string()
                            max=d.max.unwrap_or(1.0).to_string()
                            step=d.step.map_or_else(|| "any".to_owned(), |s| s.to_string())
                            prop:value=value
                            on:input=move |ev| set(event_target_value(&ev))
                        />
                    }
                    .into_any(),
                    TweakKind::Color => view! {
                        <input type="color" class="swatch" data-testid=testid prop:value=value on:input=move |ev| set(event_target_value(&ev))/>
                    }
                    .into_any(),
                    TweakKind::Text => view! {
                        <input class="input small" data-testid=testid prop:value=value on:change=move |ev| set(event_target_value(&ev))/>
                    }
                    .into_any(),
                };
                view! {
                    <label class="inspect-row">
                        <span class="small">{d.title().to_owned()}</span>
                        <span class="inspect-value">{control}</span>
                    </label>
                }
            })
            .collect_view()
            .into_any()
    };
    view! {
        {panel_head(ctx, t(T::ToolTweaks).to_owned())}
        <div class="tool-body" data-testid="tweaks-panel">
            {controls}
            <div class="row">
                <button class="btn small" data-testid="tweaks-reset" on:click=move |_| ctx.reset_tweaks()>
                    {t(T::TweaksReset)}
                </button>
            </div>
        </div>
    }
}

#[component]
fn LintPanel(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let body = move || match tools.lint.get() {
        None => view! { <p class="empty small">{t(T::Loading)}</p> }.into_any(),
        Some(Err(e)) => view! { <p class="error-text small">{e}</p> }.into_any(),
        Some(Ok(r)) if r.findings.is_empty() => view! {
            <div class="empty-state compact">
                <span class="empty-icon ok">{icon(Glyph::LintOk)}</span>
                <p>{t(T::LintNone)}</p>
            </div>
        }
        .into_any(),
        Some(Ok(r)) => {
            let message = r.agent_message.clone();
            view! {
                <ul class="lint-list">
                    {r.findings
                        .into_iter()
                        .map(|f| view! {
                            <li class="lint-finding" data-testid="lint-finding" data-rule=f.id.clone()>
                                <div class="version-head">
                                    <span class=format!("sev sev-{:?}", f.severity).to_lowercase()>{format!("{:?}", f.severity)}</span>
                                    <strong class="mono small">{f.id.clone()}</strong>
                                </div>
                                <p class="small">{f.message.clone()}</p>
                                <p class="small muted">{tf(T::LintFix, &[&f.fix])}</p>
                                {f.snippet.clone().map(|s| view! { <code class="snippet">{s}</code> })}
                            </li>
                        })
                        .collect_view()}
                </ul>
                <div class="row">
                    <button
                        class="btn small primary"
                        data-testid="lint-send"
                        on:click=move |_| ctx.send(message.clone(), ctx.shell)
                    >
                        {t(T::LintSend)}
                    </button>
                </div>
            }
            .into_any()
        }
    };
    view! {
        {panel_head(ctx, tf(T::LintTitle, &[&ctx.active().unwrap_or_default()]))}
        <div class="tool-body" data-testid="lint-panel">{body}</div>
    }
}

/// Pen or box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pen {
    Free,
    Box,
}

#[derive(Debug, Clone, PartialEq)]
struct Stroke {
    pen: Pen,
    color: &'static str,
    points: Vec<(f64, f64)>,
}

fn context_2d(canvas: &web_sys::HtmlCanvasElement) -> Option<web_sys::CanvasRenderingContext2d> {
    canvas.get_context("2d").ok()??.dyn_into().ok()
}

fn paint(g: &web_sys::CanvasRenderingContext2d, strokes: &[Stroke]) {
    g.set_line_width(3.0);
    g.set_line_cap("round");
    g.set_line_join("round");
    for s in strokes {
        g.set_stroke_style_str(s.color);
        match (s.pen, s.points.first(), s.points.last()) {
            (Pen::Box, Some(a), Some(b)) => g.stroke_rect(a.0, a.1, b.0 - a.0, b.1 - a.1),
            (Pen::Free, Some(a), _) => {
                g.begin_path();
                g.move_to(a.0, a.1);
                for p in &s.points[1..] {
                    g.line_to(p.0, p.1);
                }
                g.stroke();
            }
            _ => {}
        }
    }
}

/// White page, the snapshot (if any), then the strokes, as a PNG data URL.
fn compose(
    width: u32,
    height: u32,
    snapshot: Option<&web_sys::HtmlImageElement>,
    strokes: &[Stroke],
) -> Result<String, String> {
    let doc = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let canvas: web_sys::HtmlCanvasElement = doc
        .create_element("canvas")
        .map_err(|_| "no canvas")?
        .dyn_into()
        .map_err(|_| "no canvas")?;
    canvas.set_width(width);
    canvas.set_height(height);
    let g = context_2d(&canvas).ok_or("no 2d context")?;
    g.set_fill_style_str("#ffffff");
    g.fill_rect(0.0, 0.0, f64::from(width), f64::from(height));
    if let Some(img) = snapshot {
        g.draw_image_with_html_image_element_and_dw_and_dh(
            img,
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
        )
        .map_err(|_| "draw failed")?;
    }
    paint(&g, strokes);
    // Fails on a tainted canvas; the caller retries without the snapshot.
    canvas
        .to_data_url_with_type("image/png")
        .map_err(|_| "canvas export blocked".to_owned())
}

async fn load_image(url: &str) -> Option<web_sys::HtmlImageElement> {
    let img = web_sys::HtmlImageElement::new().ok()?;
    img.set_src(url);
    wasm_bindgen_futures::JsFuture::from(img.decode())
        .await
        .ok()?;
    Some(img)
}

/// Asks the frame for a snapshot and waits for it.
async fn take_snapshot(tools: Tools) -> Option<Snapshot> {
    let id = api::random_id();
    tools.snapshot.set(None);
    bridge::post_main(&pb::snapshot(&id));
    for _ in 0..SNAPSHOT_WAIT_MS / 50 {
        if let Some(s) = tools
            .snapshot
            .get_untracked()
            .filter(|s| s.request_id == id)
        {
            return Some(s);
        }
        gloo_timers::future::TimeoutFuture::new(50).await;
    }
    None
}

/// Composes the annotation, stores it in the project and attaches it to
/// the next message. Returns whether the page snapshot made it in.
async fn attach_annotation(
    ctx: StudioCtx,
    strokes: Vec<Stroke>,
    width: u32,
    height: u32,
) -> Result<bool, String> {
    let snapshot = take_snapshot(ctx.tools).await;
    let image = match snapshot.and_then(snapshot_image) {
        Some(url) => load_image(&url).await,
        None => None,
    };
    let (png, with_snapshot) = match image
        .as_ref()
        .map(|img| compose(width, height, Some(img), &strokes))
    {
        Some(Ok(png)) => (png, true),
        _ => (compose(width, height, None, &strokes)?, false),
    };
    let content = png
        .split_once(',')
        .map(|(_, b64)| b64.to_owned())
        .ok_or("bad image data")?;
    let req = WriteFileRequest {
        name: format!(
            "{ANNOTATIONS_DIR}/annotation-{}.png",
            js_sys::Date::now() as u64
        ),
        content,
        encoding: ContentEncoding::Base64,
        overwrite: Some(false),
        artifact_manifest: None,
    };
    let res = api::post::<_, FileResponse>(&format!("{}/files", ctx.base()), &req)
        .await
        .map_err(|e| e.to_string())?;
    ctx.tools
        .attachments
        .update(|a| a.push(res.file.path.clone()));
    Ok(with_snapshot)
}

/// Drawing surface over the preview frame (draw tool).
#[component]
pub fn DrawLayer(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let strokes = RwSignal::new(Vec::<Stroke>::new());
    let pen = RwSignal::new(Pen::Free);
    let color = RwSignal::new(DRAW_COLORS[0]);
    let drawing = StoredValue::new(false);
    let busy = RwSignal::new(false);

    let redraw = move || {
        let Some(canvas) = canvas_ref.get() else {
            return;
        };
        let (w, h) = (canvas.client_width(), canvas.client_height());
        let (w, h) = (u32::try_from(w).unwrap_or(0), u32::try_from(h).unwrap_or(0));
        if canvas.width() != w || canvas.height() != h {
            canvas.set_width(w);
            canvas.set_height(h);
        }
        if let Some(g) = context_2d(&canvas) {
            g.clear_rect(0.0, 0.0, f64::from(w), f64::from(h));
            strokes.with(|s| paint(&g, s));
        }
    };
    Effect::new(move |_| {
        strokes.track();
        redraw();
    });

    let point = |ev: &web_sys::PointerEvent| (f64::from(ev.offset_x()), f64::from(ev.offset_y()));
    let attach = move || {
        let Some(canvas) = canvas_ref.get_untracked() else {
            return;
        };
        let list = strokes.get_untracked();
        if list.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        let (w, h) = (canvas.width().max(1), canvas.height().max(1));
        leptos::task::spawn_local(async move {
            match attach_annotation(ctx, list, w, h).await {
                Ok(with_snapshot) => {
                    strokes.set(Vec::new());
                    tools.status.set(Some(
                        t(if with_snapshot {
                            T::DrawAttached
                        } else {
                            T::DrawAttachedNoSnapshot
                        })
                        .to_owned(),
                    ));
                    ctx.set_tool(None);
                }
                Err(e) => tools.status.set(Some(tf(T::DrawFailed, &[&e]))),
            }
            busy.set(false);
        });
    };
    view! {
        <div class="draw-layer" data-testid="draw-layer">
            <canvas
                node_ref=canvas_ref
                class="draw-canvas"
                data-testid="draw-canvas"
                on:pointerdown=move |ev: web_sys::PointerEvent| {
                    if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                        let _ = el.set_pointer_capture(ev.pointer_id());
                    }
                    drawing.set_value(true);
                    let p = point(&ev);
                    strokes.update(|s| s.push(Stroke { pen: pen.get_untracked(), color: color.get_untracked(), points: vec![p, p] }));
                }
                on:pointermove=move |ev: web_sys::PointerEvent| {
                    if !drawing.get_value() {
                        return;
                    }
                    let p = point(&ev);
                    strokes.update(|s| {
                        if let Some(last) = s.last_mut() {
                            match last.pen {
                                Pen::Free => last.points.push(p),
                                Pen::Box => last.points[1] = p,
                            }
                        }
                    });
                }
                on:pointerup=move |_| drawing.set_value(false)
            ></canvas>
            <div class="draw-tools">
                <div class="segmented small">
                    <button class="seg" class:active=move || pen.get() == Pen::Free data-testid="draw-pen" on:click=move |_| pen.set(Pen::Free)>{icon(Glyph::Draw)}{t(T::DrawPen)}</button>
                    <button class="seg" class:active=move || pen.get() == Pen::Box data-testid="draw-box" on:click=move |_| pen.set(Pen::Box)>{icon(Glyph::Rectangle)}{t(T::DrawBox)}</button>
                </div>
                {DRAW_COLORS
                    .into_iter()
                    .map(|c| view! {
                        <button
                            class="draw-color"
                            class:active=move || color.get() == c
                            style=format!("background:{c}")
                            aria-label=c
                            on:click=move |_| color.set(c)
                        ></button>
                    })
                    .collect_view()}
                <span class="head-divider" aria-hidden="true"></span>
                <button class="btn small ghost" data-testid="draw-clear" on:click=move |_| strokes.set(Vec::new())>{icon(Glyph::Delete)}{t(T::DrawClear)}</button>
                <button
                    class="btn small primary"
                    data-testid="draw-attach"
                    disabled=move || busy.get() || strokes.with(Vec::is_empty)
                    on:click=move |_| attach()
                >
                    {icon(Glyph::Attachment)}
                    {t(T::DrawAttach)}
                </button>
            </div>
        </div>
    }
}

/// Annotation images that go with the next message.
#[component]
pub fn AttachmentChips(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    move || {
        let files = tools.attachments.get();
        (!files.is_empty()).then(|| {
            view! {
                <div class="comment-chips" aria-label=t(T::FilesAttached)>
                    {files
                        .into_iter()
                        .map(|path| {
                            let remove = path.clone();
                            let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                            view! {
                                <span class="chip comment-chip" data-testid="attachment-chip" title=path.clone()>
                                    {icon(Glyph::Attachment)}
                                    {name}
                                    <button
                                        class="chip-x"
                                        title=t(T::Detach)
                                        aria-label=t(T::Detach)
                                        on:click=move |_| tools.attachments.update(|a| a.retain(|p| p != &remove))
                                    >
                                        {icon(Glyph::Close)}
                                    </button>
                                </span>
                            }
                        })
                        .collect_view()}
                </div>
            }
        })
    }
}

/// Warning when the last run did not deliver its entry file.
#[component]
pub fn DeliverableWarning(ctx: StudioCtx) -> impl IntoView {
    let tools = ctx.tools;
    move || {
        tools
            .deliverable
            .get()
            .and_then(|d| deliverable_message(&d))
            .map(|msg| {
                view! {
                    <p class="warn-text" data-testid="deliverable-warning">
                        {icon(Glyph::Warning)}
                        <span class="grow">{msg}</span>
                        <button class="chip-x" title=t(T::Close) aria-label=t(T::Close) on:click=move |_| tools.deliverable.set(None)>{icon(Glyph::Close)}</button>
                    </p>
                }
            })
    }
}

/// Lints the shown HTML file whenever it (re)loads.
pub fn watch_lint(ctx: StudioCtx) {
    Effect::new(move |_| {
        ctx.preview_rev.track();
        if let Some(path) = ctx.active().filter(|p| is_html_path(p)) {
            ctx.refresh_lint(path);
        }
    });
    // Another file: its tool state starts over.
    let shown = Memo::new(move |_| ctx.active());
    Effect::new(move |prev: Option<()>| {
        shown.track();
        if prev.is_some() {
            ctx.tools.forget_document();
            if ctx.tools.tool.get_untracked() == Some(Tool::Tweaks) {
                ctx.set_tool(None);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_strings_become_hex() {
        assert_eq!(rgb_to_hex("rgb(10, 125, 59)").as_deref(), Some("#0a7d3b"));
        assert_eq!(
            rgb_to_hex("rgba(255, 0, 0, 0.5)").as_deref(),
            Some("#ff0000")
        );
        assert_eq!(rgb_to_hex("rgb(1 2 3 / 50%)").as_deref(), Some("#010203"));
        assert_eq!(rgb_to_hex("transparent"), None);
        assert_eq!(rgb_to_hex("rgb(1, 2)"), None);
    }

    #[test]
    fn overrides_group_per_element() {
        let o = |s: &str, p: &str, v: &str| Override {
            selector: s.into(),
            prop: p.into(),
            value: v.into(),
            tag: s.into(),
            text: format!("{s} text"),
        };
        let edits = style_edits(&[
            o("h1", "color", "red"),
            o("p", "margin", "0"),
            o("h1", "font-size", "40px"),
        ]);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].selector, "h1");
        assert_eq!(edits[0].style.len(), 2);
        assert_eq!(edits[0].style["color"].as_deref(), Some("red"));
        assert!(edits[0].text.is_none());
        assert_eq!(edits[0].expect_tag.as_deref(), Some("h1"));
        assert_eq!(edits[0].expect_text.as_deref(), Some("h1 text"));
    }

    fn ov(selector: &str, prop: &str, text: &str) -> Override {
        Override {
            selector: selector.into(),
            prop: prop.into(),
            value: "red".into(),
            tag: "h1".into(),
            text: text.into(),
        }
    }

    #[test]
    fn an_element_anchored_by_its_od_id_is_checked_by_tag_only() {
        // Scripts may change its text; the id still names it.
        let edits = style_edits(&[
            ov("[data-od-id=\"x\"]", "color", "Welcome"),
            ov("[data-od-id=\"x\"] > h1:nth-of-type(1)", "color", "Hi"),
        ]);
        assert_eq!(edits[0].selector, "[data-od-id=\"x\"]");
        assert_eq!(edits[0].expect_tag.as_deref(), Some("h1"));
        assert_eq!(edits[0].expect_text, None);
        assert_eq!(edits[1].expect_text.as_deref(), Some("Hi"));
    }

    #[test]
    fn a_saved_text_edit_updates_the_overrides_of_its_element() {
        let mut list = vec![ov("h1", "color", "Hello"), ov("p", "color", "Body")];
        retext_overrides(&mut list, "h1", "  Hi\n there ");
        assert_eq!(list[0].text, "Hi there");
        assert_eq!(list[1].text, "Body");
        let edits = style_edits(&list);
        assert_eq!(edits[0].expect_text.as_deref(), Some("Hi there"));
        // The print is capped like the bridge's.
        retext_overrides(&mut list, "p", &"w".repeat(400));
        assert_eq!(list[1].text.chars().count(), 160);
    }

    #[test]
    fn overrides_the_frame_could_not_replay_are_dropped() {
        let mut list = vec![
            ov("h1", "color", "A"),
            ov("h1", "margin", "A"),
            ov("p", "color", "B"),
        ];
        let skipped = [pb::Skipped {
            selector: "h1".into(),
            prop: "color".into(),
        }];
        assert_eq!(drop_skipped(&mut list, &skipped), 1);
        assert_eq!(
            list.iter()
                .map(|o| (o.selector.as_str(), o.prop.as_str()))
                .collect::<Vec<_>>(),
            [("h1", "margin"), ("p", "color")]
        );
        assert_eq!(drop_skipped(&mut list, &skipped), 0);
    }

    #[test]
    fn text_edits_only_from_the_text_tool_on_the_shown_file() {
        const K: Option<&str> = Some("k1");
        let e = |doc: Option<&str>| pb::TextEdit {
            key: K.map(str::to_owned),
            selector: "h1".into(),
            before: "A".into(),
            text: "B".into(),
            doc: doc.map(str::to_owned),
        };
        let on = Some(Tool::EditText);
        assert!(accepted_text_edit(
            &e(Some("a.html")),
            on,
            Some("a.html"),
            K
        ));
        // A script posting while no tool (or another) is on.
        assert!(!accepted_text_edit(
            &e(Some("a.html")),
            None,
            Some("a.html"),
            K
        ));
        assert!(!accepted_text_edit(
            &e(Some("a.html")),
            Some(Tool::Inspect),
            Some("a.html"),
            K
        ));
        // The edit of the file left behind, landing after a switch.
        assert!(!accepted_text_edit(
            &e(Some("a.html")),
            on,
            Some("b.html"),
            K
        ));
        assert!(!accepted_text_edit(&e(None), on, Some("a.html"), K));
        assert!(!accepted_text_edit(&e(None), on, None, K));
    }

    #[test]
    fn only_the_bridge_holds_the_edit_key() {
        let forged = pb::TextEdit {
            key: None,
            selector: "h1".into(),
            before: "A".into(),
            text: "B".into(),
            doc: Some("a.html".into()),
        };
        let on = Some(Tool::EditText);
        assert!(!accepted_text_edit(&forged, on, Some("a.html"), Some("k1")));
        let guessed = pb::TextEdit {
            key: Some("k2".into()),
            ..forged.clone()
        };
        assert!(!accepted_text_edit(
            &guessed,
            on,
            Some("a.html"),
            Some("k1")
        ));
        // No key claimed yet: nothing is taken, not even a keyless edit.
        assert!(!accepted_text_edit(&forged, on, Some("a.html"), None));
        assert!(keyed(Some("k1"), Some("k1")));
        assert!(!keyed(None, None));
        assert!(!keyed(Some("k1"), None));
    }

    #[test]
    fn snapshots_must_be_images() {
        let s = |url: &str| Snapshot {
            request_id: "r".into(),
            data_url: Some(url.into()),
            ..Snapshot::default()
        };
        assert!(snapshot_image(s("data:image/png;base64,AAAA")).is_some());
        assert_eq!(snapshot_image(s("https://evil.example/x.png")), None);
        assert_eq!(snapshot_image(s("data:text/html,<b>x</b>")), None);
    }

    #[test]
    fn deliverable_messages() {
        let d = |v| DeliverableResponse {
            valid: v == DeliverableValidation::Valid,
            validation: v,
            entry_file: Some("index.html".into()),
            artifact_kind: None,
        };
        assert_eq!(deliverable_message(&d(DeliverableValidation::Valid)), None);
        assert_eq!(
            deliverable_message(&d(DeliverableValidation::NotSucceeded)),
            None
        );
        assert!(
            deliverable_message(&d(DeliverableValidation::EntryNotTouched))
                .unwrap()
                .contains("index.html")
        );
    }
}
