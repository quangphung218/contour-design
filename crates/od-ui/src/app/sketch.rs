//! Sketches (Studio files ▸ New sketch): a `*.sketch.json` file drawn on a
//! white board with the Draw tool's pen, box and text, saved as you draw,
//! and sent to the chat as a PNG for the agent to build from.

use super::api;
use super::files_panel::{FilesUi, raw_url, replace_content};
use super::icons::{Glyph, icon};
use super::preview_tools::{DRAW_COLORS, DrawSurface, compose, draw_controls, save_png};
use super::studio::StudioCtx;
use crate::file_tree as ft;
use crate::i18n::{T, t, tf};
use crate::sketch_model::{History, Pen, SketchFile};
use leptos::prelude::*;
use od_contracts::{ContentEncoding, FileResponse, WriteFileRequest};

/// Quiet time before a sketch is saved, ms.
const SAVE_DELAY_MS: u32 = 700;

#[must_use]
pub fn is_sketch(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".sketch.json")
}

async fn create(ctx: StudioCtx, path: String, sketch: &SketchFile) -> Result<String, String> {
    let req = WriteFileRequest {
        name: path,
        content: serde_json::to_string(sketch).map_err(|e| e.to_string())?,
        encoding: ContentEncoding::Utf8,
        overwrite: Some(false),
        artifact_manifest: None,
    };
    api::post::<_, FileResponse>(&format!("{}/files", ctx.base()), &req)
        .await
        .map(|r| r.file.path)
        .map_err(|e| e.to_string())
}

/// Creates a blank sketch in `dir` (`sketches/` at the root) and opens it.
pub fn new_sketch(ctx: StudioCtx, ui: FilesUi, dir: String) {
    let dir = if dir.is_empty() {
        "sketches".to_owned()
    } else {
        dir
    };
    let existing: Vec<String> = ctx
        .files
        .with_untracked(|f| f.iter().map(|f| f.path.clone()).collect());
    let path = ft::free_name(&existing, &dir, "sketch", "sketch.json");
    leptos::task::spawn_local(async move {
        match create(ctx, path, &SketchFile::blank()).await {
            Ok(p) => {
                ctx.refresh_files();
                ui.refresh(ctx);
                ui.open.update(|o| {
                    o.insert(ft::parent(&p).to_owned());
                });
                ctx.open_file(&p);
            }
            Err(e) => ctx.notice.set(Some(e)),
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Saved {
    Clean,
    Saving,
    Saved,
    Failed,
}

/// The sketch editor shown in place of the preview.
#[component]
pub fn SketchView(ctx: StudioCtx, path: String) -> impl IntoView {
    let ui = FilesUi::get();
    let history = RwSignal::new(History::default());
    let pen = RwSignal::new(Pen::Free);
    let color = RwSignal::new(DRAW_COLORS[0]);
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let loaded = RwSignal::new(false);
    let saved = RwSignal::new(Saved::Clean);
    let busy = RwSignal::new(false);
    let size = StoredValue::new((SketchFile::WIDTH, SketchFile::HEIGHT));
    let path = StoredValue::new(path);
    leptos::task::spawn_local(async move {
        let file = match api::get_text(&raw_url(ctx, &path.get_value())).await {
            Ok(text) => SketchFile::parse(&text),
            Err(_) => SketchFile::blank(),
        };
        size.set_value((file.width.max(1), file.height.max(1)));
        history.set(History::from_strokes(file.strokes));
        loaded.set(true);
    });
    // Saved a moment after the last change.
    let seq = StoredValue::new(0u32);
    Effect::new(move |first: Option<()>| {
        history.track();
        if first.is_none() || !loaded.get_untracked() {
            return;
        }
        let n = seq.get_value().wrapping_add(1);
        seq.set_value(n);
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(SAVE_DELAY_MS).await;
            if seq.try_get_value() != Some(n) {
                return;
            }
            saved.set(Saved::Saving);
            let (width, height) = size.get_value();
            let file = SketchFile {
                version: 1,
                width,
                height,
                strokes: history.with_untracked(|h| h.strokes().to_vec()),
            };
            let res = match serde_json::to_string(&file) {
                Ok(body) => replace_content(ctx, &path.get_value(), &body, "Sketch")
                    .await
                    .map(|_| ()),
                Err(e) => Err(e.to_string()),
            };
            if seq.try_get_value() == Some(n) {
                saved.set(if res.is_ok() {
                    Saved::Saved
                } else {
                    Saved::Failed
                });
            }
        });
    });
    let to_chat = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let (w, h) = size.get_value();
        let strokes = history.with_untracked(|h| h.strokes().to_vec());
        leptos::task::spawn_local(async move {
            let stem = path.get_value();
            let stem = stem
                .strip_suffix(".sketch.json")
                .unwrap_or(&stem)
                .to_owned();
            let res = match compose(w, h, None, &strokes) {
                Ok(png) => {
                    save_png(
                        ctx,
                        format!("{stem}-{}.png", js_sys::Date::now() as u64),
                        &png,
                    )
                    .await
                }
                Err(e) => Err(e),
            };
            busy.set(false);
            match res {
                Ok(p) => {
                    ctx.tools.attachments.update(|a| a.push(p.clone()));
                    ctx.refresh_files();
                    ui.say(tf(T::SketchAttached, &[ft::base(&p)]));
                }
                Err(e) => ctx.notice.set(Some(e)),
            }
        });
    };
    let (w, h) = size.get_value();
    view! {
        <div class="sketch-view" data-testid="sketch-view">
            <div class="draw-tools sketch-tools">
                {draw_controls(history, pen, color)}
                <span class="spacer"></span>
                <span class="subtle sketch-status" data-testid="sketch-status">
                    {move || match saved.get() {
                        Saved::Clean => String::new(),
                        Saved::Saving => t(T::Saving).to_owned(),
                        Saved::Saved => t(T::Saved).to_owned(),
                        Saved::Failed => t(T::NotSaved).to_owned(),
                    }}
                </span>
                <button class="btn small primary" data-testid="sketch-to-chat"
                    disabled=move || busy.get() || history.with(History::is_empty)
                    on:click=to_chat>
                    {icon(Glyph::Attachment)}{t(T::SketchToChat)}
                </button>
            </div>
            <div class="sketch-board">
                <div class="sketch-paper" style=format!("aspect-ratio:{w}/{h}")>
                    <Show when=move || loaded.get() fallback=|| view! { <div class="skeleton sketch-skeleton"></div> }>
                        <DrawSurface history=history pen=pen color=color canvas_ref=canvas_ref size=size.get_value() class="sketch-canvas"/>
                    </Show>
                </div>
            </div>
        </div>
    }
}
