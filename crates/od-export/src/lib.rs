//! Exports that need no browser engine (spec 4.2): one-file HTML, ZIP
//! archives, and assembling rendered slides into PDF and PPTX. Work that
//! needs a layout engine goes through the [`Renderer`] trait.

pub mod archive;
pub mod image;
pub mod pdf;
pub mod pptx;
pub mod renderer;
pub mod standalone;

pub use renderer::{
    CaptureOptions, DeckMode, FrameOptions, ImageFormat, RenderError, RenderResult, RenderTarget,
    Renderer, Slides,
};
pub use standalone::{AssetMeta, AssetSource, Limits, Options, StandaloneError, StandaloneHtml};
