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
    #[arg(long, short, conflicts_with = "look")]
    out: Option<PathBuf>,
    /// Write under a new name into the agent turn's own folder
    /// (`$OD_LOOK_DIR`, outside the project, removed when the turn ends)
    /// and print the path: how an agent looks at its render.
    #[arg(long)]
    look: bool,
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
    let download = if args.format == Format::Zip {
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
    let crate::client::Download {
        bytes,
        name,
        mime,
        audio_tracks,
    } = download;
    let target = match args.out {
        _ if args.look => {
            let dir = std::env::var_os(LOOK_DIR_ENV).filter(|d| !d.is_empty());
            let dir = dir.ok_or_else(|| {
                anyhow::anyhow!("--look writes into ${LOOK_DIR_ENV}, which only an agent turn sets")
            })?;
            write_look(std::path::Path::new(&dir), name.as_deref(), &bytes)?
        }
        Some(p) if p.as_os_str() == "-" => {
            std::io::stdout().write_all(&bytes)?;
            return Ok(());
        }
        Some(p) => {
            std::fs::write(&p, &bytes)?;
            p
        }
        None => {
            let p = PathBuf::from(name.unwrap_or_else(|| "export".to_owned()));
            std::fs::write(&p, &bytes)?;
            p
        }
    };
    if json {
        println!(
            "{}",
            json!({ "path": target, "bytes": bytes.len(), "contentType": mime, "audioTracks": audio_tracks })
        );
    } else {
        println!(
            "wrote {} ({} bytes){}",
            target.display(),
            bytes.len(),
            audio_note(audio_tracks)
        );
    }
    Ok(())
}

/// The agent turn's own folder for `--look` pictures, set by the daemon.
const LOOK_DIR_ENV: &str = "OD_LOOK_DIR";

/// Writes `bytes` into `dir` under a new name (`look-<random>.<ext>`,
/// the extension of the server's file name): created exclusively, so a
/// file or link already there is never written over or through.
fn write_look(dir: &std::path::Path, name: Option<&str>, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    let ext = name
        .and_then(|n| std::path::Path::new(n).extension())
        .and_then(|e| e.to_str())
        .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map_or_else(String::new, |e| format!(".{e}"));
    let mut file = tempfile::Builder::new()
        .prefix("look-")
        .suffix(&ext)
        .tempfile_in(dir)
        .map_err(|e| anyhow::anyhow!("cannot write into {}: {e}", dir.display()))?;
    file.write_all(bytes)?;
    let (_, path) = file.keep()?;
    Ok(path)
}

/// ` · no audio` or ` · N audio tracks` for an MP4 export; empty for
/// other formats (no header).
fn audio_note(tracks: Option<u32>) -> String {
    match tracks {
        None => String::new(),
        Some(0) => " · no audio".to_owned(),
        Some(1) => " · 1 audio track".to_owned(),
        Some(n) => format!(" · {n} audio tracks"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--look` takes a new file each time, named by the server's
    /// extension, never one already there.
    #[test]
    fn a_look_is_a_new_file_in_the_turns_folder() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_look(dir.path(), Some("deck.png"), b"one").unwrap();
        let b = write_look(dir.path(), Some("deck.png"), b"two").unwrap();
        assert_ne!(a, b);
        for (path, body) in [(&a, "one"), (&b, "two")] {
            assert_eq!(path.parent(), Some(dir.path()));
            let name = path.file_name().unwrap().to_str().unwrap();
            assert!(
                name.starts_with("look-") && name.ends_with(".png"),
                "{name}"
            );
            assert_eq!(std::fs::read_to_string(path).unwrap(), body);
        }
        let odd = write_look(dir.path(), Some("x.p/ng"), b"x").unwrap();
        assert!(odd.extension().is_none(), "{}", odd.display());
        let none = write_look(dir.path(), None, b"x").unwrap();
        assert_eq!(none.parent(), Some(dir.path()));
        // The turn's folder gone (the turn ended): an error, nothing made.
        let gone = dir.path().join("gone");
        assert!(write_look(&gone, Some("a.png"), b"x").is_err());
        assert!(!gone.exists());
    }

    #[test]
    fn mp4_exports_say_how_much_sound_they_carry() {
        assert_eq!(audio_note(None), "");
        assert_eq!(audio_note(Some(0)), " · no audio");
        assert_eq!(audio_note(Some(1)), " · 1 audio track");
        assert_eq!(audio_note(Some(3)), " · 3 audio tracks");
    }
}
