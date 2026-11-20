//! Real Chrome exports the bundled `html-ppt-*` deck templates to a vector
//! PDF that looks like their slides: one page per slide, the slide's shape,
//! and each page, rasterised, matches the slide's screenshot.
//!
//! Each template is bundled as the daemon bundles an export
//! (`standalone::bundle`, lenient): the renderer loads the page alone, so a
//! template's `assets/` must be inlined first.
//!
//! By default a sample runs, one template per way a deck printed wrong;
//! `OD_DECK_TEMPLATES=all` runs every bundled `html-ppt-*` template
//! (47 in Nov 2026, about 6 min in a debug build), or a comma-separated list.
//!
//! Needs Chrome (see `chrome.rs`); `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use od_export::image::decode_rgb;
use od_export::standalone::bundle;
use od_export::{AssetMeta, AssetSource, DeckMode, Options, RenderTarget, Renderer};
use od_render_cdp::find_chrome;
use std::path::{Path, PathBuf};

/// One template per way a bundled deck printed unlike its slides.
const SAMPLE: &[&str] = &[
    // `.slide.active { display: flex }`: printed without the deck's active
    // class, the slide lost its layout.
    "html-ppt-zhangzara-block-frame",
    // A script sets the slide strip's width to N×100vw: the page came out
    // N slides wide and Chrome shrank it onto the paper.
    "html-ppt-zhangzara-mat",
    // A centred box (`top: 50%; translateY(-50%)`) reaches past the slide's
    // bottom before its transform: Chrome broke the slide across pages.
    "html-ppt-zhangzara-soft-editorial",
    // `<deck-stage>` (shadow DOM, its own print rules) from `assets/`.
    "html-ppt-zhangzara-pin-and-paper",
    // A slide painted by a gradient over a white parent.
    "html-ppt-graphify-dark-graph",
];

/// Cells per page compared: small enough to ignore antialiasing and font
/// hinting, large enough to see a line of text move.
const GRID: (usize, usize) = (240, 135);
/// A cell differs when its colour is off by more than this (0–255, mean of RGB).
const CELL_TOLERANCE: f64 = 40.0;
/// A page matches its slide when at most this share of cells differs.
/// The sample stays under 2 % and every bundled template under 4.5 % (Nov 2026);
/// the faults above reached 10–65 %.
const PAGE_TOLERANCE: f64 = 0.05;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bundled_deck_templates_print_as_their_slides_look() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates");
    let names = templates(&root);
    let scratch = tempfile::tempdir().unwrap();
    let mut wrong = Vec::new();
    for name in &names {
        let dir = root.join(name);
        let html = std::fs::read_to_string(dir.join("example.html")).unwrap();
        let options = Options {
            lenient: true,
            ..Options::default()
        };
        let bundled = bundle("example.html", &html, &Dir(dir), &options).unwrap();
        let path = scratch.path().join(format!("{name}.html"));
        std::fs::write(&path, bundled.html).unwrap();
        let deck = RenderTarget::new(path);

        let pdf = renderer.print_pdf(&deck, DeckMode::Auto).await.unwrap();
        let slides = renderer.render_slides(&deck, DeckMode::Auto).await.unwrap();
        assert!(slides.deck, "{name} is not seen as a deck");
        let pages = raster_pages(&pdf);
        if pages.len() != slides.images.len() {
            wrong.push(format!(
                "{name}: {} pages for {} slides",
                pages.len(),
                slides.images.len()
            ));
            continue;
        }
        let mut worst = 0.0_f64;
        for (i, (page, png)) in pages.iter().zip(&slides.images).enumerate() {
            let n = i + 1;
            let slide = decode_rgb(png).unwrap();
            // The paper is 13.333 in wide; its height keeps the slide's shape.
            let want = page.width_pt * f64::from(slides.height) / f64::from(slides.width);
            if (page.width_pt - 960.0).abs() > 1.0 || (page.height_pt - want).abs() > 1.0 {
                wrong.push(format!(
                    "{name} page {n}: {:.0}×{:.0} pt for a {}×{} slide",
                    page.width_pt, page.height_pt, slides.width, slides.height
                ));
                continue;
            }
            let (sw, sh) = (slide.width as usize, slide.height as usize);
            let shot = cells(&slide.pixels, sw, sh);
            let off = differing(&page.cells, &shot);
            worst = worst.max(off);
            if off > PAGE_TOLERANCE {
                wrong.push(format!(
                    "{name} page {n}: {:.1} % of the page differs from the slide",
                    off * 100.0
                ));
            }
        }
        eprintln!(
            "{name}: {} pages, worst {:.1} % off",
            pages.len(),
            worst * 100.0
        );
    }
    assert!(
        wrong.is_empty(),
        "PDF pages unlike their slides ({} templates checked):\n{}",
        names.len(),
        wrong.join("\n")
    );
}

/// The sample, every `html-ppt-*` template with `OD_DECK_TEMPLATES=all`, or
/// the comma-separated names it gives.
fn templates(root: &Path) -> Vec<String> {
    let all = || {
        let mut names: Vec<String> = std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("html-ppt-") && root.join(n).join("example.html").is_file())
            .collect();
        names.sort();
        names
    };
    match std::env::var("OD_DECK_TEMPLATES").ok().as_deref() {
        Some("all") => all(),
        Some(list) if !list.trim().is_empty() => {
            list.split(',').map(|s| s.trim().to_owned()).collect()
        }
        _ => {
            let names = all();
            for s in SAMPLE {
                assert!(names.iter().any(|n| n == s), "{s} is no longer bundled");
            }
            SAMPLE.iter().map(|s| (*s).to_owned()).collect()
        }
    }
}

struct Page {
    width_pt: f64,
    height_pt: f64,
    cells: Vec<[f64; 3]>,
}

/// Every page of `pdf` rasterised on white, as [`GRID`] cells.
fn raster_pages(pdf: &[u8]) -> Vec<Page> {
    let pdf = Pdf::new(pdf.to_vec()).unwrap();
    let cache = RenderCache::new();
    let interpreter = InterpreterSettings::default();
    let settings = RenderSettings::default();
    pdf.pages()
        .iter()
        .map(|page| {
            let (w, h) = page.render_dimensions();
            // Two pixels a cell across: enough for text to average out.
            #[allow(clippy::cast_precision_loss)] // a few hundred cells
            let scale = 2.0 * GRID.0 as f32 / w;
            let pixmap = hayro::render(
                page,
                &cache,
                &interpreter,
                &settings,
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: WHITE,
                },
            );
            let (pw, ph) = (usize::from(pixmap.width()), usize::from(pixmap.height()));
            let rgb: Vec<u8> = pixmap
                .data_as_u8_slice()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            Page {
                width_pt: f64::from(w),
                height_pt: f64::from(h),
                cells: cells(&rgb, pw, ph),
            }
        })
        .collect()
}

/// `rgb` (`w`×`h`, 3 bytes a pixel) averaged into [`GRID`] cells.
#[allow(clippy::cast_precision_loss)] // pixel counts and sums stay far below 2^52
fn cells(rgb: &[u8], w: usize, h: usize) -> Vec<[f64; 3]> {
    let (gw, gh) = GRID;
    let mut out = Vec::with_capacity(gw * gh);
    for gy in 0..gh {
        let (y0, y1) = (gy * h / gh, ((gy + 1) * h / gh).max(gy * h / gh + 1));
        for gx in 0..gw {
            let (x0, x1) = (gx * w / gw, ((gx + 1) * w / gw).max(gx * w / gw + 1));
            let mut sum = [0.0; 3];
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let at = (y * w + x) * 3;
                    for c in 0..3 {
                        sum[c] += f64::from(rgb[at + c]);
                    }
                }
            }
            let n = ((y1.min(h) - y0) * (x1.min(w) - x0)).max(1) as f64;
            out.push(sum.map(|s| s / n));
        }
    }
    out
}

/// The share of cells whose mean colour differs by more than [`CELL_TOLERANCE`].
#[allow(clippy::cast_precision_loss)]
fn differing(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
    let off = a
        .iter()
        .zip(b)
        .filter(|(p, q)| {
            p.iter()
                .zip(q.iter())
                .map(|(x, y)| (x - y).abs())
                .sum::<f64>()
                / 3.0
                > CELL_TOLERANCE
        })
        .count();
    off as f64 / a.len().max(1) as f64
}

/// A template folder read as the project it becomes.
struct Dir(PathBuf);

impl AssetSource for Dir {
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>> {
        let mime = match path.rsplit('.').next().unwrap_or("") {
            "html" | "htm" => "text/html",
            "css" => "text/css",
            "js" | "mjs" => "text/javascript",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        Ok(std::fs::metadata(self.0.join(path))
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map(|m| AssetMeta {
                mime: mime.to_owned(),
                size: m.len(),
            }))
    }
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.0.join(path))
    }
}
