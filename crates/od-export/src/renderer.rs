//! The browser-engine boundary (spec 4.2): everything that needs a real
//! layout engine goes through [`Renderer`]. The daemon does not know what is
//! underneath (`od-render-cdp` drives Chrome; tests use a fake).

use futures_util::future::BoxFuture;
use std::path::PathBuf;

/// Default capture size: 1920×1080, the deck stage of the original app.
pub const DEFAULT_WIDTH: u32 = 1920;
pub const DEFAULT_HEIGHT: u32 = 1080;
/// Longest full-page capture (spec 4.2).
pub const MAX_PAGE_HEIGHT: u32 = 20_000;

/// CSS selector for slide surfaces; presenter clones are filtered out.
pub const SLIDE_SELECTOR: &str = ".slide, [data-screen-label], .deck-slide, .ppt-slide";
pub const SLIDE_CLONE_SELECTOR: &str = ".mini-slide, .overview, .notes-overlay, .thumb";
/// Deck chrome (progress bars, notes, nav) hidden during capture.
pub const HIDE_CHROME_SELECTOR: &str = ".progress-bar, .notes-overlay, aside.notes, .speaker-notes, .deck-nav, .deck-hint, .deck-counter";

/// Whether the page is a deck: decided by the caller or by finding slides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeckMode {
    #[default]
    Auto,
    Deck,
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

impl ImageFormat {
    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }

    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// A self-contained HTML file on disk to load (see `standalone`), plus the
/// viewport to lay it out in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTarget {
    pub html_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

impl RenderTarget {
    #[must_use]
    pub fn new(html_path: PathBuf) -> Self {
        Self {
            html_path,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

/// Output of [`Renderer::render_slides`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slides {
    /// `true` when slides were found (deck); `false` for page mode, where the
    /// images are consecutive viewport-high slices of the page.
    pub deck: bool,
    pub width: u32,
    pub height: u32,
    /// One PNG per slide (or page slice), in order.
    pub images: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureOptions {
    pub format: ImageFormat,
    /// Capture the whole scroll height (capped at [`MAX_PAGE_HEIGHT`]).
    pub full_page: bool,
}

/// Timeline capture for MP4 export (P2, spec 1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameOptions {
    pub fps: u32,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// No browser engine on this machine (spec decision 2: Chrome only).
    #[error("{0}")]
    Unavailable(String),
    /// The page has no slides but the caller asked for a deck.
    #[error("this artifact is not a slide deck; export it as PDF or an image instead")]
    NotADeck,
    #[error("{0} is not supported by this renderer")]
    Unsupported(&'static str),
    #[error("render failed: {0}")]
    Failed(String),
}

pub type RenderResult<T> = Result<T, RenderError>;

/// The four browser-engine operations of spec 4.2.
pub trait Renderer: Send + Sync {
    /// Deck → one PNG per slide. In [`DeckMode::Page`] (or `Auto` without
    /// slides) the page is cut into viewport-high slices instead.
    fn render_slides<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>>;

    /// Timeline → PNG frames at `fps`.
    fn render_frames<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: FrameOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<Vec<u8>>>>;

    /// Vector PDF through the print pipeline. Decks print one slide per
    /// 13.333×7.5 in page.
    fn print_pdf<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>>;

    /// One image of the page.
    fn capture_page<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>>;
}
