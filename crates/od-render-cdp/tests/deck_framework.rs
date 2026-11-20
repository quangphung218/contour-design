//! Real Chrome exports a deck built on the shipped deck framework
//! (`DECK_FRAMEWORK_DIRECTIVE.md`: `.deck-stage` 1920×1080 holds the colour,
//! `body` holds the dark shell, its own `@media print` sets
//! `@page { size: 1920px 1080px }`) with `oklch()` theme tokens, as agents
//! write them (QA-120, QA-121).
//!
//! Needs Chrome (see `chrome.rs`); `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::image::decode_rgb;
use od_export::pptx_editable::{Element, layouts_to_pptx, parse_color};
use od_export::{DeckMode, RenderTarget, Renderer};
use od_render_cdp::find_chrome;

/// The tokens below in sRGB, as Chrome converts them.
const BG: [u8; 3] = [248, 237, 216];
const FG: [u8; 3] = [45, 29, 20];
const ACCENT: [u8; 3] = [183, 75, 33];
/// `--shell`, painted on `body` around the stage.
const SHELL: [u8; 3] = [8, 9, 13];

const DIRECTIVE: &str = include_str!("../../od-content/prompts/legacy/DECK_FRAMEWORK_DIRECTIVE.md");

/// The canonical skeleton with its slots filled: oklch tokens, 3 slides.
fn framework_deck() -> String {
    let start = DIRECTIVE.find("```html\n<!doctype html>").unwrap() + "```html\n".len();
    let len = DIRECTIVE[start..].find("\n```").unwrap();
    let mut html = DIRECTIVE[start..start + len].to_owned();
    let swap = |html: &mut String, from: &str, to: &str| {
        assert!(html.contains(from), "skeleton lost {from:?}");
        *html = html.replacen(from, to, 1);
    };
    // Cream slide, dark text, rust accent; the shell stays #08090d.
    swap(&mut html, "--bg: #ffffff;", "--bg: oklch(0.95 0.03 85);");
    swap(&mut html, "--fg: #1c1b1a;", "--fg: oklch(0.25 0.03 50);");
    swap(
        &mut html,
        "--accent: #c96442;",
        "--accent: oklch(0.55 0.15 40);",
    );
    let first = html.find("<section class=\"slide active\"").unwrap();
    let end = html.find("<!-- ... add as many").unwrap();
    let slides: String = (1..=3)
        .map(|i| {
            format!(
                r#"<section class="slide{}" data-screen-label="0{i}">
        <h1 style="margin:120px;font:96px sans-serif;color:var(--fg)">Bakery <span style="color:var(--accent)">{i}</span></h1>
        <div style="margin:0 120px;width:600px;height:200px;background:var(--accent);border:6px solid var(--fg)"></div>
        <div style="margin:40px 120px;width:600px;height:100px;background:oklch(0.5 0.1 30 / 0)"></div>
      </section>
      "#,
                if i == 1 { " active" } else { "" }
            )
        })
        .collect();
    html.replace_range(first..end, &slides);
    html
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_framework_deck_exports_as_the_user_sees_it() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deck.html");
    std::fs::write(&path, framework_deck()).unwrap();
    let deck = RenderTarget::new(path);

    // Vector PDF (QA-120): one 960×540 pt page per slide, filled by it.
    let pdf = renderer.print_pdf(&deck, DeckMode::Auto).await.unwrap();
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 3);
    for (n, id) in pages {
        let media = numbers(
            doc.get_object(id)
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"MediaBox")
                .unwrap(),
        );
        assert!(
            (media[2] - 960.0).abs() < 2.0 && (media[3] - 540.0).abs() < 2.0,
            "page {n}: {media:?}"
        );
        let content = doc.get_and_decode_page_content(id).unwrap();
        let painted = painted_box(&content.operations, media[2], media[3]);
        assert!(
            painted[0] < 2.0
                && painted[1] < 2.0
                && painted[2] > media[2] - 2.0
                && painted[3] > media[3] - 2.0,
            "page {n}: the slide covers only {painted:?} of {media:?}"
        );
        let texts = content
            .operations
            .iter()
            .filter(|o| o.operator == "BT")
            .count();
        assert_eq!(texts, 2, "page {n} holds one slide's title and number");
    }

    // Slide images (raster PDF, PNG, image PPTX): the stage colour edge to edge.
    let slides = renderer.render_slides(&deck, DeckMode::Auto).await.unwrap();
    assert_eq!(slides.images.len(), 3);
    for (i, png) in slides.images.iter().enumerate() {
        let rgb = decode_rgb(png).unwrap();
        assert_eq!((rgb.width, rgb.height), (1920, 1080));
        for (x, y) in [(2, 2), (1917, 1077), (1917, 2), (2, 1077)] {
            let at = (y * 1920 + x) * 3;
            let px = [rgb.pixels[at], rgb.pixels[at + 1], rgb.pixels[at + 2]];
            assert!(close(px, BG), "slide {i} at ({x}, {y}) is {px:?}");
        }
    }

    // Editable PPTX (QA-121): the stage's background, every run coloured.
    let layouts = renderer.extract_layouts(&deck).await.unwrap();
    assert_eq!(layouts.len(), 3);
    for (i, slide) in layouts.iter().enumerate() {
        let bg = slide.background.as_deref().and_then(parse_color);
        assert!(
            bg.is_some_and(|c| close(c.hex, BG) && !close(c.hex, SHELL)),
            "slide {i} background {:?}",
            slide.background
        );
        let shapes: Vec<_> = slide
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Shape { fill, line, .. } => Some((
                    fill.as_deref().and_then(parse_color).map(|c| c.hex),
                    line.as_deref().and_then(parse_color).map(|c| c.hex),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            shapes.len(),
            1,
            "slide {i}: only the card is painted: {shapes:?}"
        );
        assert!(
            shapes[0].0.is_some_and(|c| close(c, ACCENT))
                && shapes[0].1.is_some_and(|c| close(c, FG)),
            "slide {i}: {shapes:?}"
        );
        let runs: Vec<_> = slide
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Text { paragraphs, .. } => Some(paragraphs),
                _ => None,
            })
            .flatten()
            .flat_map(|p| p.runs.iter())
            .filter(|r| !r.text.trim().is_empty())
            .collect();
        let colour = |text: &str| {
            let run = runs
                .iter()
                .find(|r| r.text.trim() == text)
                .unwrap_or_else(|| panic!("no run {text:?}: {runs:#?}"));
            run.color.as_deref().and_then(parse_color).map(|c| c.hex)
        };
        assert!(
            colour("Bakery").is_some_and(|c| close(c, FG)),
            "slide {i}: {runs:#?}"
        );
        let number = (i + 1).to_string();
        assert!(
            colour(&number).is_some_and(|c| close(c, ACCENT)),
            "slide {i}: {runs:#?}"
        );
    }
    let pptx = layouts_to_pptx(&layouts, "deck").unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&pptx[..])).unwrap();
    let mut slide1 = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("ppt/slides/slide1.xml").unwrap(),
        &mut slide1,
    )
    .unwrap();
    assert!(
        slide1.contains("F8EDD8"),
        "background fill missing: {slide1}"
    );
    assert!(
        slide1.contains("2D1D14") && slide1.contains("B74B21"),
        "run colours missing: {slide1}"
    );
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
}

fn numbers(obj: &lopdf::Object) -> Vec<f64> {
    obj.as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v.as_float()
                .map(f64::from)
                .or_else(|_| v.as_i64().map(|n| n as f64))
                .unwrap()
        })
        .collect()
}

/// The box, in page points, that rectangles filled with a colour other
/// than white cover (Chrome paints the page white first), clamped to the
/// page. Follows `q`/`Q`/`cm` and `rg`; reads `re` then `f`/`f*`.
fn painted_box(ops: &[lopdf::content::Operation], w: f64, h: f64) -> [f64; 4] {
    type M = [f64; 6];
    let mul = |a: M, b: M| -> M {
        [
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
            a[4] * b[0] + a[5] * b[2] + b[4],
            a[4] * b[1] + a[5] * b[3] + b[5],
        ]
    };
    let mut ctm: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack = Vec::new();
    let mut white = false;
    let mut pending: Vec<[f64; 4]> = Vec::new();
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for op in ops {
        let args = || numbers(&lopdf::Object::Array(op.operands.clone()));
        match op.operator.as_str() {
            "q" => stack.push((ctm, white)),
            "Q" => (ctm, white) = stack.pop().unwrap(),
            "cm" => {
                let a = args();
                ctm = mul([a[0], a[1], a[2], a[3], a[4], a[5]], ctm);
            }
            "rg" => white = args().iter().all(|c| *c > 0.99),
            "sc" | "scn" | "k" | "g" | "cs" => white = false,
            "re" => {
                let a = args();
                let corners = [(a[0], a[1]), (a[0] + a[2], a[1] + a[3])];
                let pts: Vec<(f64, f64)> = corners
                    .iter()
                    .map(|&(x, y)| {
                        (
                            ctm[0] * x + ctm[2] * y + ctm[4],
                            ctm[1] * x + ctm[3] * y + ctm[5],
                        )
                    })
                    .collect();
                pending.push([
                    pts[0].0.min(pts[1].0),
                    pts[0].1.min(pts[1].1),
                    pts[0].0.max(pts[1].0),
                    pts[0].1.max(pts[1].1),
                ]);
            }
            "f" | "f*" | "F" => {
                for r in pending.drain(..) {
                    if !white {
                        out = [
                            out[0].min(r[0]),
                            out[1].min(r[1]),
                            out[2].max(r[2]),
                            out[3].max(r[3]),
                        ];
                    }
                }
            }
            "n" | "W" | "W*" | "S" | "B" | "b" => pending.clear(),
            _ => {}
        }
    }
    [
        out[0].max(0.0),
        out[1].max(0.0),
        out[2].min(w),
        out[3].min(h),
    ]
}
