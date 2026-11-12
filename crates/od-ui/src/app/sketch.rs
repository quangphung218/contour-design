//! Sketches (Studio files ▸ New sketch): a `*.sketch.json` file drawn on a
//! white board with the Draw tool's pen, box and text, saved as you draw,
//! and sent to the chat as a PNG for the agent to build from.

use super::api;
use super::file_state::{Autosave, ReadState, Save, read_error_banner};
use super::files_panel::{FilesUi, raw_url, replace_content};
use super::icons::{Glyph, icon};
use super::preview_tools::{DRAW_COLORS, DrawSurface, compose, draw_controls, save_png};
use super::studio::StudioCtx;
use super::tasks::Tasks;
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
        .map_err(|e| ctx.failed_to(T::FileCreateFailed, &e))
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
    ctx.tasks.spawn(async move {
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

/// The drawing in a sketch file's text, or why it cannot be read: a file
/// cut short by a write in progress, or not a sketch. Shown blank, the
/// next stroke would save over it (QA-084). An empty file holds nothing
/// to lose and opens blank.
fn read_sketch(text: &str) -> Result<SketchFile, String> {
    if text.trim().is_empty() {
        return Ok(SketchFile::blank());
    }
    serde_json::from_str(text).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveState {
    Clean,
    Saving,
    Done,
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
    // Loaded only once the file is read: it turns the autosave on.
    let read = RwSignal::new(ReadState::<()>::default());
    let saved = RwSignal::new(SaveState::Clean);
    let busy = RwSignal::new(false);
    let size = StoredValue::new((SketchFile::WIDTH, SketchFile::HEIGHT));
    let path = StoredValue::new(path);
    let tasks = Tasks::here();
    let load = move || {
        read.update(ReadState::retry);
        tasks.spawn(async move {
            let url = raw_url(ctx, &path.get_value());
            let file = match api::get_text(&url).await {
                Ok(text) => read_sketch(&text),
                Err(e) => Err(ctx.failed(&e)),
            };
            let res = file.map(|file| {
                size.set_value((file.width.max(1), file.height.max(1)));
                history.set(History::from_strokes(file.strokes));
            });
            read.update(|r| r.read(res));
        });
    };
    load();
    // Saved a moment after the last change, and at once when the view
    // goes (tab switch, leaving the Studio) with a change not saved yet.
    let autosave = Autosave::new(SAVE_DELAY_MS, move |save: Save| {
        saved.set(SaveState::Saving);
        let (width, height) = size.get_value();
        let file = SketchFile {
            version: 1,
            width,
            height,
            strokes: history.with_untracked(|h| h.strokes().to_vec()),
        };
        let body = match serde_json::to_string(&file) {
            Ok(body) => body,
            Err(_) => {
                saved.set(SaveState::Failed);
                return;
            }
        };
        // Started here: on leaving, the save runs after the view is disposed.
        let work = replace_content(ctx, &path.get_value(), &body, "Sketch", T::MdSaveFailed);
        tasks.spawn_then(work, move |res| {
            if res.is_err() {
                save.failed();
            }
            if save.is_latest() {
                saved.set(if res.is_ok() {
                    SaveState::Done
                } else {
                    SaveState::Failed
                });
            }
        });
    });
    Effect::new(move |first: Option<()>| {
        history.track();
        if first.is_none() || !read.with_untracked(ReadState::loaded) {
            return;
        }
        autosave.changed();
    });
    let to_chat = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let (w, h) = size.get_value();
        let strokes = history.with_untracked(|h| h.strokes().to_vec());
        tasks.spawn(async move {
            let stem = path.get_value();
            let stem = stem
                .strip_suffix(".sketch.json")
                .unwrap_or(&stem)
                .to_owned();
            let res = match compose(w, h, None, &strokes) {
                Ok(png) => {
                    save_png(
                        ctx,
                        T::SketchAttachFailed,
                        format!("{stem}-{}.png", js_sys::Date::now() as u64),
                        &png,
                    )
                    .await
                }
                Err(e) => Err(tf(T::SketchAttachFailed, &[&e])),
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
                        SaveState::Clean => String::new(),
                        SaveState::Saving => t(T::Saving).to_owned(),
                        SaveState::Done => t(T::Saved).to_owned(),
                        SaveState::Failed => t(T::NotSaved).to_owned(),
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
                    {read_error_banner(read, "sketch-error", "sketch", T::SketchReadFailed, load)}
                    <Show when=move || read.with(ReadState::loaded) fallback=move || read.with(ReadState::waiting).then(|| view! { <div class="skeleton sketch-skeleton"></div> })>
                        <DrawSurface history=history pen=pen color=color canvas_ref=canvas_ref size=size.get_value() class="sketch-canvas"/>
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sketch_is_read_with_its_strokes() {
        let text = r##"{"version":1,"width":800,"height":600,"strokes":[{"pen":"free","color":"#000","points":[[1.0,2.0]]}]}"##;
        let file = read_sketch(text).expect("a sketch");
        assert_eq!((file.width, file.height, file.strokes.len()), (800, 600, 1));
    }

    #[test]
    fn an_empty_file_opens_blank() {
        assert_eq!(read_sketch(" \n"), Ok(SketchFile::blank()));
    }

    #[test]
    fn a_file_that_is_not_a_sketch_is_not_shown_blank() {
        // Cut short by a write in progress, or another kind of file: blank,
        // the next stroke would save over it (QA-084).
        assert!(read_sketch(r#"{"version":1,"width":800,"strok"#).is_err());
        assert!(read_sketch("<html>").is_err());
    }
}
