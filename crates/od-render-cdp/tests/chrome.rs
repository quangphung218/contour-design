//! Real Chrome (spec 5.7 M5): a 5-slide deck becomes a 5-page PDF (raster
//! and vector), a 5-slide PPTX and 1920×1080 PNGs; a standalone export loads
//! with no network and no sibling files.
//!
//! Needs Chrome, Edge or Chromium (`OD_CHROME_PATH` or a standard install).
//! Missing browser = failure, unless `OD_SKIP_CHROME_TESTS=1` is set.
//! Everything runs in one test so only one browser is alive at a time.

use od_export::image::{decode_rgb, png_size, solid_png};
use od_export::standalone;
use od_export::{
    AssetMeta, AssetSource, CaptureOptions, DeckMode, Options, RenderTarget, Renderer,
};
use od_render_cdp::{CdpRenderer, find_chrome};
use std::collections::HashMap;
use std::path::Path;

const COLORS: [[u8; 3]; 5] = [
    [230, 30, 30],
    [30, 200, 30],
    [30, 30, 230],
    [220, 200, 20],
    [140, 20, 160],
];

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A deck in the common "only `.active` is shown" style, scaled by a
/// transformed stage like many templates.
fn deck_html() -> String {
    let slides: String = COLORS
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                r#"<section class="slide{}" style="background:{}"><h1>Slide {}</h1></section>"#,
                if i == 0 { " active" } else { "" },
                hex(*c),
                i + 1
            )
        })
        .collect();
    format!(
        r#"<!doctype html><html><head><style>
html,body{{margin:0;height:100%;overflow:hidden;background:#000}}
.stage{{width:1920px;height:1080px;transform:scale(0.5);transform-origin:0 0}}
.slide{{display:none;width:1920px;height:1080px;box-sizing:border-box;padding:80px;color:#fff;font:64px sans-serif}}
.slide.active{{display:flex}}
.deck-counter{{position:fixed;right:10px;bottom:10px;width:300px;height:300px;background:#fff}}
</style></head><body><div class="stage">{slides}</div><div class="deck-counter">1/5</div></body></html>"#
    )
}

#[derive(Default)]
struct Mem(HashMap<String, (String, Vec<u8>)>);

impl AssetSource for Mem {
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>> {
        Ok(self.0.get(path).map(|(m, b)| AssetMeta {
            mime: m.clone(),
            size: b.len() as u64,
        }))
    }
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        Ok(self.0[path].1.clone())
    }
}

fn write(dir: &Path, name: &str, html: &str) -> RenderTarget {
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    RenderTarget::new(path)
}

fn center(png: &[u8]) -> [u8; 3] {
    let rgb = decode_rgb(png).unwrap();
    let (x, y) = (rgb.width as usize / 2, rgb.height as usize * 3 / 4);
    let i = (y * rgb.width as usize + x) * 3;
    [rgb.pixels[i], rgb.pixels[i + 1], rgb.pixels[i + 2]]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 8)
}

fn pdf_pages(pdf: &[u8]) -> usize {
    lopdf::Document::load_mem(pdf).unwrap().get_pages().len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chrome_exports() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let chrome = find_chrome().expect("Chrome/Edge/Chromium is required for these tests");
    let renderer = CdpRenderer::with_executable(chrome);
    let dir = tempfile::tempdir().unwrap();

    // --- Deck: one PNG per slide, 1920×1080, the right slide each time.
    let deck = write(dir.path(), "deck.html", &deck_html());
    let slides = renderer.render_slides(&deck, DeckMode::Auto).await.unwrap();
    assert!(slides.deck);
    assert_eq!((slides.width, slides.height), (1920, 1080));
    assert_eq!(slides.images.len(), 5);
    for (i, png) in slides.images.iter().enumerate() {
        assert_eq!(png_size(png), Some((1920, 1080)), "slide {i}");
        assert!(
            close(center(png), COLORS[i]),
            "slide {i} shows {:?}",
            center(png)
        );
    }
    // Deck chrome (the white counter box) is hidden in captures.
    let rgb = decode_rgb(&slides.images[0]).unwrap();
    let corner = ((1060 * 1920 + 1900) * 3) as usize;
    assert!(close(
        [
            rgb.pixels[corner],
            rgb.pixels[corner + 1],
            rgb.pixels[corner + 2]
        ],
        COLORS[0]
    ));

    // Raster PDF and image PPTX assembled from the slides.
    assert_eq!(
        pdf_pages(&od_export::pdf::pngs_to_pdf(&slides.images).unwrap()),
        5
    );
    let pptx = od_export::pptx::pngs_to_pptx(&slides.images, "deck").unwrap();
    assert_eq!(od_export::pptx::count_slides(&pptx).unwrap(), (5, 5));

    // Vector PDF: one page per slide, no trailing blank page, 13.333 in wide.
    let vector = renderer.print_pdf(&deck, DeckMode::Auto).await.unwrap();
    assert_eq!(pdf_pages(&vector), 5);
    let doc = lopdf::Document::load_mem(&vector).unwrap();
    let first = doc.get_pages()[&1];
    let media = doc
        .get_object(first)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v.as_float()
                .map(f64::from)
                .or_else(|_| v.as_i64().map(|n| n as f64))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        (media[2] - 960.0).abs() < 2.0 && (media[3] - 540.0).abs() < 2.0,
        "{media:?}"
    );

    // Viewport capture of a deck is 1920×1080 too.
    let shot = renderer
        .capture_page(&deck, CaptureOptions::default())
        .await
        .unwrap();
    assert_eq!(png_size(&shot), Some((1920, 1080)));

    // Forcing deck mode on a page without slides is a clear error.
    let page = write(
        dir.path(),
        "page.html",
        "<!doctype html><body style='margin:0'><div style='height:2500px;background:#123456'></div></body>",
    );
    assert!(matches!(
        renderer.render_slides(&page, DeckMode::Deck).await,
        Err(od_export::RenderError::NotADeck)
    ));
    // Page mode: viewport-high slices of the scroll height.
    let sliced = renderer.render_slides(&page, DeckMode::Auto).await.unwrap();
    assert!(!sliced.deck);
    let heights: Vec<u32> = sliced
        .images
        .iter()
        .map(|p| png_size(p).unwrap().1)
        .collect();
    assert_eq!(heights, [1080, 1080, 340]);
    let full = renderer
        .capture_page(
            &page,
            CaptureOptions {
                full_page: true,
                ..CaptureOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(png_size(&full), Some((1920, 2500)));

    // --- Standalone HTML opened offline: no sibling files, no requests.
    let png = solid_png(40, 30, [10, 120, 200]).unwrap();
    let mut src = Mem::default();
    let index = r#"<!doctype html><html><head><link rel="stylesheet" href="css/site.css"></head>
<body><img id="logo" src="img/logo.png"><div id="bg"></div>
<script type="module">import { mark } from "./js/mark.js"; mark();</script></body></html>"#;
    for (path, mime, body) in [
        ("index.html", "text/html", index.as_bytes().to_vec()),
        ("css/site.css", "text/css", b"#bg{width:40px;height:30px;background:url(../img/logo.png)}".to_vec()),
        ("img/logo.png", "image/png", png.clone()),
        ("js/mark.js", "text/javascript", b"import { flag } from './flag.js'; export function mark() { document.body.dataset.ran = flag; }".to_vec()),
        ("js/flag.js", "text/javascript", b"export const flag = 'yes';".to_vec()),
    ] {
        src.0.insert(path.to_owned(), (mime.to_owned(), body));
    }
    let bundled = standalone::bundle("index.html", index, &src, &Options::default()).unwrap();
    let offline = tempfile::tempdir().unwrap();
    let target = write(offline.path(), "standalone.html", &bundled.html);
    assert_eq!(std::fs::read_dir(offline.path()).unwrap().count(), 1);
    let report = renderer
        .evaluate(
            &target,
            r#"(async () => {
  for (let i = 0; i < 50 && !document.body.dataset.ran; i++) await new Promise((r) => setTimeout(r, 20));
  const img = document.getElementById('logo');
  return {
    requests: performance.getEntriesByType('resource').map((e) => e.name).filter((n) => !n.startsWith('data:')),
    imageWidth: img.naturalWidth,
    background: getComputedStyle(document.getElementById('bg')).backgroundImage.slice(0, 15),
    moduleRan: document.body.dataset.ran || null,
  };
})()"#,
        )
        .await
        .unwrap();
    assert_eq!(report["requests"], serde_json::json!([]), "{report}");
    assert_eq!(report["imageWidth"], 40, "{report}");
    assert_eq!(report["background"], "url(\"data:image", "{report}");
    assert_eq!(report["moduleRan"], "yes", "{report}");
}
