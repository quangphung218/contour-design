//! Real Chrome exports the bundled decks to a vector PDF that looks like
//! their slides: one page per slide, the slide's shape, and each page,
//! rasterised, matches the slide's screenshot.
//!
//! Each deck is bundled as the daemon bundles an export
//! (`standalone::bundle`, lenient): the renderer loads the page alone, so a
//! template's `assets/` must be inlined first.
//!
//! By default a sample runs: one bundled template per way a deck printed
//! wrong, and decks written here for what no bundled one does.
//! `OD_DECK_TEMPLATES=all` runs every bundled deck: each `example.html` in
//! `content/design-templates` and `content/skills` with slide markup, found
//! as `od-preview`'s `bundled_decks_stay_decks` finds them (68 in Nov 2026,
//! about 8 min in a debug build; 8 of them still fail, see PR #285). A
//! comma-separated list of folder names runs those.
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

/// Folders under `content/` that hold bundled decks.
const GROUPS: &[&str] = &["design-templates", "skills"];

/// A slide that contains its own layout (`contain: layout`) holds its
/// `position: fixed` boxes: each slide shows only its own. Print lost that
/// when the slide's `contain` was replaced (review of #285): every page drew
/// every slide's box, at the page's corner.
const FIXED_IN_CONTAINED_SLIDES: &str = r#"<!doctype html><html><head><style>
html,body{margin:0;background:#222}
.slide{position:relative;width:1920px;height:1080px;background:#f4efe6;contain:layout;overflow:hidden}
.mark{position:fixed;width:400px;height:400px}
h1{margin:0;padding:120px;font:96px sans-serif;color:#1c1b1a}
</style></head><body>
<section class="slide"><h1>One</h1><div class="mark" style="left:100px;top:500px;background:#c0392b"></div></section>
<section class="slide"><h1>Two</h1><div class="mark" style="left:760px;top:340px;background:#2e86de"></div></section>
<section class="slide"><h1>Three</h1><div class="mark" style="left:1420px;top:600px;background:#27ae60"></div></section>
</body></html>"#;

/// A deck that moves its slide strip itself: a script watches each slide's
/// `.active` class and sets the strip's inline `transform`, as
/// `kami-deck` does for the host bridge. Giving every slide the class (to
/// print it, or to show one for its screenshot) set the script off, and its
/// inline write replaced the export's own inline `transform: none`
/// (review of #285): pages and screenshots showed the wrong slide or none.
const SCRIPT_FOLLOWS_ACTIVE: &str = r#"<!doctype html><html><head><style>
html,body{margin:0;overflow:hidden;background:#222}
#deck{display:flex;width:300vw;height:100vh}
.slide{flex:0 0 100vw;width:100vw;height:100vh;position:relative;overflow:hidden}
.s1{background:#1b3a5c;color:#faf9f5}.s2{background:#f4efe6;color:#1c1b1a}.s3{background:#7a2e2e;color:#faf9f5}
h1{margin:0;padding:120px;font:120px sans-serif}
.box{position:absolute;width:500px;height:300px;background:#e0b040}
</style></head><body><div id="deck">
<section class="slide s1 active"><h1>One</h1><div class="box" style="left:200px;top:500px"></div></section>
<section class="slide s2"><h1>Two</h1><div class="box" style="left:800px;top:600px"></div></section>
<section class="slide s3"><h1>Three</h1><div class="box" style="left:1300px;top:400px"></div></section>
</div><script>
const deck = document.getElementById('deck');
const slides = Array.from(document.querySelectorAll('.slide'));
let idx = 0;
function apply(n) {
  idx = n;
  deck.style.transform = 'translateX(' + (-n * 100) + 'vw)';
  slides.forEach((s, i) => s.classList.toggle('active', i === n));
}
const mo = new MutationObserver(() => {
  for (let i = 0; i < slides.length; i++) {
    if (slides[i].classList.contains('active') && i !== idx) { apply(i); return; }
  }
});
slides.forEach((s) => mo.observe(s, { attributes: true, attributeFilter: ['class'] }));
apply(0);
</script></body></html>"#;

/// A deck that shows only its active slide (`display: none` otherwise) and
/// whose script keeps `active` on one slide: when print adds the class to
/// every slide, the script takes it off all but one (review of #285).
const SCRIPT_KEEPS_ONE_ACTIVE: &str = r#"<!doctype html><html><head><style>
html,body{margin:0;overflow:hidden;background:#222}
.slide{display:none;width:100vw;height:100vh;position:relative;overflow:hidden}
.slide.active{display:block}
.s1{background:#1b3a5c;color:#faf9f5}.s2{background:#f4efe6;color:#1c1b1a}.s3{background:#7a2e2e;color:#faf9f5}
h1{margin:0;padding:120px;font:120px sans-serif}
</style></head><body>
<section class="slide s1 active"><h1>One</h1></section>
<section class="slide s2"><h1>Two</h1></section>
<section class="slide s3"><h1>Three</h1></section>
<script>
const slides = Array.from(document.querySelectorAll('.slide'));
let idx = 0;
function go(n) {
  idx = n;
  slides.forEach((s, i) => s.classList.toggle('active', i === n));
}
const mo = new MutationObserver(() => {
  for (let i = 0; i < slides.length; i++) {
    if (slides[i].classList.contains('active') && i !== idx) { go(i); return; }
  }
});
slides.forEach((s) => mo.observe(s, { attributes: true, attributeFilter: ['class'] }));
go(0);
</script></body></html>"#;

/// Cells per page compared: small enough to ignore antialiasing and font
/// hinting, large enough to see a line of text move.
const GRID: (usize, usize) = (240, 135);
/// A cell differs when its colour is off by more than this (0–255, mean of RGB).
const CELL_TOLERANCE: f64 = 40.0;
/// A page matches its slide when at most this share of cells differs.
/// The sample stays under 2 % and every bundled `html-ppt-*` template under
/// 4.5 % (Nov 2026); the faults above reached 10–65 %.
const PAGE_TOLERANCE: f64 = 0.05;

/// A deck to print: its HTML, bundled, and where it came from.
struct Deck {
    name: String,
    html: String,
    /// For a deck written here, each slide's background: the PDF page and
    /// the screenshot must both show it, since a fault on the path they
    /// share would make them match each other and still be wrong.
    backgrounds: Vec<[u8; 3]>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bundled_deck_templates_print_as_their_slides_look() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let decks = decks();
    let scratch = tempfile::tempdir().unwrap();
    let mut wrong = Vec::new();
    for Deck {
        name,
        html,
        backgrounds,
    } in &decks
    {
        let path = scratch.path().join(format!("{name}.html"));
        std::fs::write(&path, html).unwrap();
        let deck = RenderTarget::new(path);

        let pdf = renderer.print_pdf(&deck, DeckMode::Auto).await.unwrap();
        let slides = renderer.render_slides(&deck, DeckMode::Auto).await.unwrap();
        if !slides.deck {
            wrong.push(format!("{name}: not seen as a deck"));
            continue;
        }
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
            if let Some(want) = backgrounds.get(i) {
                // Bottom left, clear of the slides' text and boxes.
                let at = (GRID.1 - 3) * GRID.0 + 2;
                for (what, cell) in [("page", page.cells[at]), ("screenshot", shot[at])] {
                    if cell
                        .iter()
                        .zip(want)
                        .any(|(c, w)| (c - f64::from(*w)).abs() > 12.0)
                    {
                        wrong.push(format!(
                            "{name} {what} {n}: background {cell:.0?}, not {want:?}"
                        ));
                    }
                }
            }
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
        "PDF pages unlike their slides ({} decks checked):\n{}",
        decks.len(),
        wrong.join("\n")
    );
}

/// The sample and the decks written here; every bundled deck with
/// `OD_DECK_TEMPLATES=all`; or the comma-separated folders it names.
fn decks() -> Vec<Deck> {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let all = bundled(&content);
    let pick = |name: &str| {
        all.iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} is not a bundled deck"))
            .1
            .clone()
    };
    let load = |name: &str, dir: &Path| Deck {
        name: name.to_owned(),
        html: bundled_html(dir),
        backgrounds: Vec::new(),
    };
    let written = || {
        [
            (
                "fixed-in-contained-slides",
                FIXED_IN_CONTAINED_SLIDES,
                vec![[244, 239, 230]; 3],
            ),
            (
                "script-follows-active",
                SCRIPT_FOLLOWS_ACTIVE,
                vec![[27, 58, 92], [244, 239, 230], [122, 46, 46]],
            ),
            (
                "script-keeps-one-active",
                SCRIPT_KEEPS_ONE_ACTIVE,
                vec![[27, 58, 92], [244, 239, 230], [122, 46, 46]],
            ),
        ]
        .map(|(name, html, backgrounds)| Deck {
            name: name.to_owned(),
            html: html.to_owned(),
            backgrounds,
        })
    };
    match std::env::var("OD_DECK_TEMPLATES").ok().as_deref() {
        Some("all") => all
            .iter()
            .map(|(n, d)| load(n, d))
            .chain(written())
            .collect(),
        Some(list) if !list.trim().is_empty() => list
            .split(',')
            .map(str::trim)
            .map(|n| load(n, &pick(n)))
            .collect(),
        _ => SAMPLE
            .iter()
            .map(|n| load(n, &pick(n)))
            .chain(written())
            .collect(),
    }
}

/// Every bundled `example.html` with slide markup, by folder name.
fn bundled(content: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for group in GROUPS {
        for entry in std::fs::read_dir(content.join(group)).unwrap().flatten() {
            let dir = entry.path();
            let Ok(html) = std::fs::read_to_string(dir.join("example.html")) else {
                continue;
            };
            if has_slide_markup(&html) {
                out.push((entry.file_name().to_string_lossy().into_owned(), dir));
            }
        }
    }
    out.sort();
    out
}

/// `example.html` in `dir` with its local files inlined, as the export
/// route bundles a project's entry.
fn bundled_html(dir: &Path) -> String {
    let html = std::fs::read_to_string(dir.join("example.html")).unwrap();
    let options = Options {
        lenient: true,
        ..Options::default()
    };
    bundle("example.html", &html, &Dir(dir.to_owned()), &options)
        .unwrap()
        .html
}

/// An element whose class list has `slide`, or a `data-od-slide` attribute
/// (as `od-preview/tests/bridge_browser.rs` tells a bundled deck).
fn has_slide_markup(html: &str) -> bool {
    html.contains("data-od-slide")
        || html.match_indices("class=").any(|(at, _)| {
            let rest = &html[at + 6..];
            let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                return false;
            };
            let value = &rest[1..];
            value[..value.find(quote).unwrap_or(0)]
                .split_whitespace()
                .any(|c| c == "slide")
        })
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
