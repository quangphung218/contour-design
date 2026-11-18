//! Real Chrome (QA-113): exports show a page as it looks once its entrance
//! animations end, not the first frame of them. Agent-made pages commonly
//! fade or slide their hero in; the PDF lost the title and the PNG came out
//! faded. A motion export still plays CSS animations frame by frame.
//!
//! Needs Chrome, Edge or Chromium (`OD_CHROME_PATH` or a standard install).
//! Missing browser = failure, unless `OD_SKIP_CHROME_TESTS=1` is set.
//! Everything runs in one test so only one browser is alive at a time.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::image::decode_rgb;
use od_export::renderer::frame_file_name;
use od_export::{CaptureOptions, DeckMode, FrameOptions, RenderTarget, Renderer};
use od_render_cdp::find_chrome;
use std::path::Path;

const TITLE: [u8; 3] = [230, 30, 30];
const LEDE: [u8; 3] = [30, 30, 230];

/// The entrance as agents write it (`animation: … both`, starting hidden
/// and lower), delayed so a capture taken at once sees it fully hidden; a
/// lede faded in by a transition that a script starts once the title's
/// animation ends (so only after the title is finished); and a
/// spinner that never ends.
const ENTRANCE_CSS: &str = r"
@keyframes rise { from { opacity: 0; transform: translateY(400px); } }
@keyframes spin { to { transform: rotate(360deg); } }
.title { animation: rise 2s cubic-bezier(.2,.7,.2,1) .5s both; }
.lede { opacity: 0; transition: opacity 2s .5s; }
.lede.in { opacity: 1; }
.spinner { width: 80px; height: 80px; border: 8px solid #333; border-top-color: transparent;
  border-radius: 50%; animation: spin 1s linear infinite; }
";

const START_LEDE: &str = "<script>addEventListener('animationend', (e) => { \
     if (e.target.classList.contains('title')) \
       document.querySelectorAll('.lede').forEach((el) => el.classList.add('in')); });</script>";

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn page_html() -> String {
    format!(
        r#"<!doctype html><html><head><style>
html,body{{margin:0;background:#fff}}
.title{{position:absolute;left:100px;top:100px;width:800px;height:200px;margin:0;background:{t};color:#fff;font:64px sans-serif}}
.lede{{position:absolute;left:100px;top:400px;width:800px;height:120px;margin:0;background:{l};color:#fff;font:32px sans-serif}}
.spinner{{position:absolute;left:1500px;top:100px}}
{ENTRANCE_CSS}
</style>{START_LEDE}</head><body>
<h1 class="title">Quarterly results</h1><p class="lede">Revenue up</p><div class="spinner"></div>
</body></html>"#,
        t = hex(TITLE),
        l = hex(LEDE),
    )
}

/// A deck where every slide's title rises in when the slide is shown: the
/// per-slide capture and the PDF show slides after settling, so their
/// animations start then.
fn deck_html() -> String {
    format!(
        r#"<!doctype html><html><head><style>
html,body{{margin:0;height:100%;overflow:hidden;background:#000}}
.slide{{display:none;position:relative;width:1920px;height:1080px;background:#fff}}
.slide.active{{display:block}}
.title{{position:absolute;left:100px;top:100px;width:800px;height:200px;margin:0;background:{t};color:#fff;font:64px sans-serif}}
{ENTRANCE_CSS}
</style></head><body>
<section class="slide active"><h1 class="title">Slide one</h1></section>
<section class="slide"><h1 class="title">Slide two</h1></section>
</body></html>"#,
        t = hex(TITLE),
    )
}

/// A film without a timeline script: the CSS animation (fill `none`, so a
/// finished one leaves `document.getAnimations()`) is the motion.
fn film_html() -> String {
    r#"<!doctype html><html><head><style>
html,body{margin:0;background:#fff}
@keyframes slide { from { transform: translateX(0); } to { transform: translateX(1200px); } }
.box{position:absolute;left:0;top:300px;width:300px;height:300px;background:#1e1ee6;animation:slide 2s linear}
</style></head><body>
<div data-composition-id="main" data-duration="2" data-width="1920" data-height="1080">
<div class="box"></div></div></body></html>"#
        .to_owned()
}

fn write(dir: &Path, name: &str, html: &str) -> RenderTarget {
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    RenderTarget::new(path)
}

fn pixel(png: &[u8], x: usize, y: usize) -> [u8; 3] {
    let rgb = decode_rgb(png).unwrap();
    let i = (y * rgb.width as usize + x) * 3;
    [rgb.pixels[i], rgb.pixels[i + 1], rgb.pixels[i + 2]]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 8)
}

fn pdf_text(pdf: &[u8]) -> String {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    doc.extract_text(&pages).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exports_wait_for_entrance_animations() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    let renderer = common::renderer(&chrome);
    let dir = tempfile::tempdir().unwrap();

    // --- A page: the title and lede are in their final place and colour.
    let page = write(dir.path(), "page.html", &page_html());
    let pdf = renderer.print_pdf(&page, DeckMode::Auto).await.unwrap();
    let text = pdf_text(&pdf);
    assert!(text.contains("Quarterly results"), "PDF text: {text:?}");
    assert!(text.contains("Revenue up"), "PDF text: {text:?}");
    for format in [
        od_export::ImageFormat::Png,
        od_export::ImageFormat::Jpeg,
        od_export::ImageFormat::Webp,
    ] {
        let shot = renderer
            .capture_page(
                &page,
                CaptureOptions {
                    format,
                    ..CaptureOptions::default()
                },
            )
            .await
            .unwrap();
        if format != od_export::ImageFormat::Png {
            continue; // decode_rgb reads PNG; the path is the same
        }
        let title = pixel(&shot, 500, 200);
        assert!(close(title, TITLE), "title pixel {title:?}");
        let lede = pixel(&shot, 500, 460);
        assert!(close(lede, LEDE), "lede pixel {lede:?}");
    }
    let slices = renderer.render_slides(&page, DeckMode::Page).await.unwrap();
    let title = pixel(&slices.images[0], 500, 200);
    assert!(close(title, TITLE), "page slice title pixel {title:?}");

    // Finite animations ended (`both` keeps one listed as finished), the
    // spinner holds still.
    let states = renderer
        .evaluate(
            &page,
            "document.getAnimations().filter((a) => a.playState !== 'finished').map((a) => [a.animationName || a.transitionProperty, a.playState])",
        )
        .await
        .unwrap();
    assert_eq!(
        states,
        serde_json::json!([["spin", "paused"]]),
        "animations after settling"
    );

    // --- A deck: each slide's title is in place when that slide is taken.
    let deck = write(dir.path(), "deck.html", &deck_html());
    let slides = renderer.render_slides(&deck, DeckMode::Auto).await.unwrap();
    assert_eq!(slides.images.len(), 2);
    for (i, png) in slides.images.iter().enumerate() {
        let title = pixel(png, 500, 200);
        assert!(close(title, TITLE), "slide {i} title pixel {title:?}");
    }
    let pdf = renderer.print_pdf(&deck, DeckMode::Auto).await.unwrap();
    let text = pdf_text(&pdf);
    assert!(
        text.contains("Slide one") && text.contains("Slide two"),
        "deck PDF text: {text:?}"
    );

    // --- A film still moves: CSS animations play frame by frame.
    let film = write(dir.path(), "film.html", &film_html());
    let frames_dir = dir.path().join("frames");
    let frames = renderer
        .render_frames(
            &film,
            FrameOptions {
                fps: 5,
                duration_ms: 2000,
                require_timeline: false,
            },
            &frames_dir,
        )
        .await
        .unwrap();
    assert_eq!(frames.count, 10);
    let first = std::fs::read(frames_dir.join(frame_file_name(0))).unwrap();
    let last = std::fs::read(frames_dir.join(frame_file_name(frames.count - 1))).unwrap();
    // At 0 s the box is at the left edge; at 1.8 s it has moved 1080 px.
    assert!(close(pixel(&first, 150, 450), [30, 30, 230]), "first frame");
    assert!(close(pixel(&last, 150, 450), [255, 255, 255]), "last frame");
    assert!(close(pixel(&last, 1230, 450), [30, 30, 230]), "last frame");
}
