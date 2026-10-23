//! Real Chrome measures a deck for the editable PPTX (spec 1.3): text keeps
//! its runs and styles, painted boxes become shapes, `<img>` and inline
//! `<svg>` become embedded PNG pictures; speaker notes and deck chrome are
//! left out.
//!
//! Needs Chrome (see `chrome.rs`); `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use base64::Engine;
use od_export::image::solid_png;
use od_export::pptx_editable::{Element, layouts_to_pptx};
use od_export::{RenderTarget, Renderer};
use od_render_cdp::{CdpRenderer, find_chrome};

fn deck() -> String {
    let png = base64::engine::general_purpose::STANDARD.encode(solid_png(4, 4, [10, 120, 200]).unwrap());
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;background:#101010}}
.slide{{display:none;position:relative;width:1920px;height:1080px;background:#f6efe2;font:40px Helvetica,Arial,sans-serif;color:#1d1b16}}
.slide.active{{display:block}}
.card{{position:absolute;left:100px;top:300px;width:700px;height:300px;background:#2f6b3a;border-radius:24px;color:#fff;padding:40px;box-sizing:border-box}}
h1{{position:absolute;left:100px;top:80px;margin:0;font-size:96px;text-transform:uppercase}}
img{{position:absolute;left:1000px;top:100px;width:400px;height:400px}}
svg{{position:absolute;left:1000px;top:600px}}
.deck-counter{{position:fixed;right:0;bottom:0}}
</style></head><body>
<section class="slide active"><h1>Hello tea</h1><div class="card"><p>Fresh <b>leaves</b>, <em>daily</em></p></div><aside class="notes">secret notes</aside></section>
<section class="slide"><img src="data:image/png;base64,{png}"><svg width="200" height="100"><rect width="200" height="100" fill="#c33"/></svg><p style="position:absolute;left:100px;top:900px;margin:0">Line one<br>Line two</p></section>
<div class="deck-counter">1/2</div>
</body></html>"##
    )
}

fn texts(el: &[Element]) -> Vec<String> {
    el.iter()
        .filter_map(|e| match e {
            Element::Text { paragraphs, .. } => Some(
                paragraphs
                    .iter()
                    .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()))
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chrome_measures_slides_into_native_objects() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = CdpRenderer::with_executable(find_chrome().expect("Chrome is required"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deck.html");
    std::fs::write(&path, deck()).unwrap();
    let layouts = renderer
        .extract_layouts(&RenderTarget::new(path))
        .await
        .unwrap();
    assert_eq!(layouts.len(), 2);
    let s1 = &layouts[0];
    assert_eq!((s1.width, s1.height), (1920.0, 1080.0));
    assert_eq!(s1.background.as_deref(), Some("rgb(246, 239, 226)"));
    let t1 = texts(&s1.elements);
    assert_eq!(t1, ["HELLO TEA", "Fresh leaves, daily"], "{:#?}", s1.elements);
    let card = s1
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Shape { rect, fill, radius, .. } => Some((*rect, fill.clone(), *radius)),
            _ => None,
        })
        .unwrap();
    assert_eq!((card.0.x, card.0.y, card.0.w, card.0.h), (100.0, 300.0, 700.0, 300.0));
    assert_eq!(card.1.as_deref(), Some("rgb(47, 107, 58)"));
    assert_eq!(card.2, 24.0);
    let runs = s1
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Text { paragraphs, .. } if paragraphs[0].runs.len() > 1 => {
                Some(paragraphs[0].runs.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(runs.iter().any(|r| r.text == "leaves" && r.bold));
    assert!(runs.iter().any(|r| r.text == "daily" && r.italic));
    assert!(runs.iter().all(|r| r.color.as_deref() == Some("rgb(255, 255, 255)")));

    let s2 = &layouts[1];
    let images: Vec<&str> = s2
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Image { src, .. } => Some(src.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2, "img + svg: {:#?}", s2.elements);
    assert!(images.iter().all(|s| s.starts_with("data:image/png;base64,")));
    assert_eq!(texts(&s2.elements), ["Line one\nLine two"]);

    let pptx = layouts_to_pptx(&layouts, "deck").unwrap();
    assert_eq!(od_export::pptx::count_slides(&pptx).unwrap(), (2, 2));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&pptx[..])).unwrap();
    let media = zip
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| n.starts_with("ppt/media/"))
        .count();
    assert_eq!(media, 2);
    let mut slide1 = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide1.xml").unwrap(), &mut slide1).unwrap();
    assert!(!slide1.contains("secret notes"));
    assert!(slide1.contains("<a:t>HELLO TEA</a:t>"));
}
