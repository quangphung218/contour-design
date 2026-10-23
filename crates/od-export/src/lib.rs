//! Exports that need no browser engine (spec 4.2): one-file HTML, ZIP
//! archives, and assembling rendered slides into PDF and PPTX. Work that
//! needs a layout engine goes through the [`Renderer`] trait. Also the
//! plain text of Office files for the Studio viewer ([`office`]).

pub mod archive;
pub mod hyperframes;
pub mod image;
pub mod office;
pub mod pdf;
pub mod pptx;
pub mod pptx_editable;
pub mod renderer;
pub mod standalone;

pub use renderer::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, ImageFormat, RenderError, RenderResult,
    RenderTarget, Renderer, Slides,
};
pub use standalone::{AssetMeta, AssetSource, Limits, Options, StandaloneError, StandaloneHtml};
