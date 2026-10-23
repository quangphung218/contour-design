//! `od export`: downloads an export from the daemon into a file.

use crate::client::Client;
use clap::{Args, ValueEnum};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// One self-contained HTML file.
    Html,
    /// Raster PDF, one page per slide (what you see; keeps CJK glyphs).
    Pdf,
    /// Vector PDF through the print pipeline.
    PdfVector,
    /// PowerPoint with one full-bleed image per slide.
    Pptx,
    /// PowerPoint with native text boxes, shapes and pictures.
    PptxEditable,
    /// PNG or JPEG: a slide, the stitched deck, or the full page.
    Image,
    /// ZIP of the project (or of --root).
    Zip,
    /// MP4 of a HyperFrames composition (needs Chrome and ffmpeg).
    Mp4,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Project id.
    project: String,
    /// HTML entry file (default: the project's entry file). Not used by zip.
    file: Option<String>,
    #[arg(long, value_enum, default_value_t = Format::Html)]
    format: Format,
    /// Output path, or `-` for stdout (default: the server's file name in
    /// the current directory).
    #[arg(long, short)]
    out: Option<PathBuf>,
    /// Download name and PPTX title.
    #[arg(long)]
    title: Option<String>,
    /// Treat the file as a deck (default: detect slides).
    #[arg(long, conflicts_with = "page")]
    deck: bool,
    /// Treat the file as an ordinary page.
    #[arg(long)]
    page: bool,
    /// Image export: this slide (0-based) instead of the whole deck.
    #[arg(long)]
    index: Option<usize>,
    /// Image export format.
    #[arg(long, value_parser = ["png", "jpeg"])]
    image_format: Option<String>,
    /// Viewport width in CSS pixels (default 1920).
    #[arg(long)]
    width: Option<u32>,
    /// Viewport height in CSS pixels (default 1080).
    #[arg(long)]
    height: Option<u32>,
    /// Zip export: only this folder.
    #[arg(long)]
    root: Option<String>,
}

pub fn run(client: &Client, args: ExportArgs, json: bool) -> anyhow::Result<()> {
    let (bytes, name, mime) = if args.format == Format::Zip {
        let mut query = Vec::new();
        if let Some(root) = &args.root {
            query.push(("root", root.as_str()));
        }
        client.get_download(&["api", "projects", &args.project, "archive"], &query)?
    } else {
        let route = match args.format {
            Format::Html => "html",
            Format::Pdf => "pdf-image",
            Format::PdfVector => "pdf",
            Format::Pptx => "pptx",
            Format::PptxEditable => "pptx-editable",
            Format::Image => "image",
            Format::Mp4 => "mp4",
            Format::Zip => unreachable!("handled above"),
        };
        let mut body = json!({});
        let fields = [
            ("fileName", args.file.as_ref().map(|f| json!(f))),
            ("title", args.title.as_ref().map(|t| json!(t))),
            ("deck", (args.deck || args.page).then(|| json!(args.deck))),
            ("index", args.index.map(|i| json!(i))),
            ("imageFormat", args.image_format.as_ref().map(|f| json!(f))),
            ("width", args.width.map(|w| json!(w))),
            ("height", args.height.map(|h| json!(h))),
        ];
        for (key, value) in fields {
            if let Some(value) = value {
                body[key] = value;
            }
        }
        client.post_download(&["api", "projects", &args.project, "export", route], &body)?
    };
    let target = match args.out {
        Some(p) if p.as_os_str() == "-" => {
            std::io::stdout().write_all(&bytes)?;
            return Ok(());
        }
        Some(p) => p,
        None => PathBuf::from(name.unwrap_or_else(|| "export".to_owned())),
    };
    std::fs::write(&target, &bytes)?;
    if json {
        println!(
            "{}",
            json!({ "path": target, "bytes": bytes.len(), "contentType": mime })
        );
    } else {
        println!("wrote {} ({} bytes)", target.display(), bytes.len());
    }
    Ok(())
}
