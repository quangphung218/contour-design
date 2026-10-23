//! Editable PPTX golden test (spec 1.3): a fixed two-slide layout becomes
//! native text boxes, shapes and pictures. The package is opened as a zip,
//! every XML part is parsed, the structure is checked part by part, and
//! the slide XML is compared byte for byte with `tests/golden/`
//! (`OD_UPDATE_GOLDEN=1` rewrites it). When LibreOffice (`soffice`) is
//! installed, it must open the file and print both slides to PDF.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use base64::Engine;
use od_export::image::solid_png;
use od_export::pptx_editable::{Element, Paragraph, Rect, SlideLayout, TextRun, layouts_to_pptx};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

fn run(text: &str, size: f64, color: &str, bold: bool) -> TextRun {
    TextRun {
        text: text.into(),
        size,
        color: Some(color.into()),
        bold,
        italic: false,
        underline: false,
        font: Some("Inter".into()),
    }
}

fn fixture() -> (Vec<SlideLayout>, Vec<u8>) {
    let png = solid_png(8, 6, [200, 30, 30]).unwrap();
    let src = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    let slides = vec![
        SlideLayout {
            width: 1920.0,
            height: 1080.0,
            background: Some("rgb(246, 239, 226)".into()),
            elements: vec![
                Element::Shape {
                    rect: Rect {
                        x: 80.0,
                        y: 80.0,
                        w: 800.0,
                        h: 400.0,
                    },
                    fill: Some("rgb(47, 107, 58)".into()),
                    line: Some("rgba(0, 0, 0, 0.5)".into()),
                    line_width: 2.0,
                    radius: 24.0,
                },
                Element::Text {
                    rect: Rect {
                        x: 120.0,
                        y: 120.0,
                        w: 720.0,
                        h: 160.0,
                    },
                    paragraphs: vec![Paragraph {
                        align: Some("center".into()),
                        line_height: Some(1.2),
                        runs: vec![
                            run("Trà & bánh ", 72.0, "rgb(255, 255, 255)", true),
                            run("<mới>", 72.0, "#f6c445", false),
                        ],
                    }],
                    valign: Some("middle".into()),
                },
                // Nothing to draw: skipped, ids stay dense.
                Element::Shape {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                    },
                    fill: Some("transparent".into()),
                    line: None,
                    line_width: 0.0,
                    radius: 0.0,
                },
            ],
        },
        SlideLayout {
            width: 1920.0,
            height: 1080.0,
            background: None,
            elements: vec![
                Element::Image {
                    rect: Rect {
                        x: 960.0,
                        y: 0.0,
                        w: 960.0,
                        h: 720.0,
                    },
                    src,
                },
                Element::Image {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                    },
                    src: "https://cdn.example/not-embedded.png".into(),
                },
                Element::Text {
                    rect: Rect {
                        x: 80.0,
                        y: 800.0,
                        w: 1200.0,
                        h: 200.0,
                    },
                    paragraphs: vec![Paragraph {
                        align: None,
                        line_height: None,
                        runs: vec![run("Line one\nLine two", 32.0, "rgb(29, 27, 22)", false)],
                    }],
                    valign: None,
                },
            ],
        },
    ];
    (slides, png)
}

fn parts(pptx: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        out.insert(f.name().unwrap().into_owned(), buf);
    }
    out
}

/// Parses `xml` fully; returns (element names in document order, text).
fn parse(name: &str, xml: &[u8]) -> (Vec<String>, String) {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let (mut names, mut text) = (Vec::new(), String::new());
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                names.push(String::from_utf8(e.name().as_ref().to_vec()).unwrap());
                for a in e.attributes() {
                    a.unwrap_or_else(|err| panic!("{name}: bad attribute: {err}"));
                }
            }
            Ok(Event::Text(t)) => text.push_str(&t.decode().unwrap()),
            Ok(Event::GeneralRef(r)) => {
                let entity = String::from_utf8(r.to_vec()).unwrap();
                text.push_str(match entity.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    other => panic!("{name}: unexpected entity {other}"),
                });
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!("{name} is not well-formed XML: {e}"),
            Ok(_) => {}
        }
        buf.clear();
    }
    (names, text)
}

fn count(names: &[String], tag: &str) -> usize {
    names.iter().filter(|n| *n == tag).count()
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

#[test]
fn editable_pptx_structure_and_golden() {
    let (slides, png) = fixture();
    let pptx = layouts_to_pptx(&slides, "Tea <deck>").unwrap();
    let parts = parts(&pptx);

    // Every XML part parses.
    for (name, body) in &parts {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            parse(name, body);
        }
    }
    let archive = zip::ZipArchive::new(Cursor::new(&pptx[..])).unwrap();
    let first = archive.file_names().next().unwrap().unwrap().into_owned();
    assert_eq!(first, "[Content_Types].xml");

    let (types, _) = parse("types", &parts["[Content_Types].xml"]);
    assert_eq!(count(&types, "Override"), 8, "6 package parts + 2 slides");
    let (pres, _) = parse("presentation", &parts["ppt/presentation.xml"]);
    assert_eq!(count(&pres, "p:sldId"), 2);
    let pres_xml = String::from_utf8(parts["ppt/presentation.xml"].clone()).unwrap();
    assert!(pres_xml.contains(r#"<p:sldSz cx="12192000" cy="6858000"/>"#));

    // Slide 1: background, a rounded shape with fill and outline, one text
    // box with two styled runs (escaped text, bold first run, 36 pt).
    let s1 = String::from_utf8(parts["ppt/slides/slide1.xml"].clone()).unwrap();
    let (names, text) = parse("slide1", s1.as_bytes());
    assert_eq!(count(&names, "p:sp"), 2);
    assert_eq!(count(&names, "p:pic"), 0);
    assert_eq!(count(&names, "p:bg"), 1);
    assert_eq!(count(&names, "a:r"), 2);
    assert_eq!(text, "Trà & bánh <mới>");
    assert!(s1.contains(r#"prst="roundRect""#));
    assert!(s1.contains(r#"<a:srgbClr val="2F6B3A"/>"#));
    assert!(s1.contains(r#"<a:alpha val="50000"/>"#));
    assert!(s1.contains(r#"<p:cNvSpPr txBox="1"/>"#));
    assert!(
        s1.contains(r#"sz="3600" b="1""#),
        "72 px on a 1920 px slide is 36 pt"
    );
    assert!(s1.contains(r#"anchor="ctr""#) && s1.contains(r#"algn="ctr""#));
    assert!(s1.contains(r#"<a:latin typeface="Inter"/>"#));

    // Slide 2: one embedded picture (the remote one is dropped), the text
    // split into two paragraphs.
    let s2 = String::from_utf8(parts["ppt/slides/slide2.xml"].clone()).unwrap();
    let (names, _) = parse("slide2", s2.as_bytes());
    assert_eq!(count(&names, "p:pic"), 1);
    assert_eq!(count(&names, "a:p"), 2);
    assert_eq!(count(&names, "p:bg"), 0);
    assert!(s2.contains(r#"<a:blip r:embed="rId2"/>"#));
    let rels = String::from_utf8(parts["ppt/slides/_rels/slide2.xml.rels"].clone()).unwrap();
    assert!(rels.contains(r#"Id="rId2""#) && rels.contains("../media/slide2-image1.png"));
    assert_eq!(
        parts["ppt/media/slide2-image1.png"], png,
        "the real image bytes"
    );
    assert_eq!(
        parts.keys().filter(|k| k.starts_with("ppt/media/")).count(),
        1
    );
    let core = String::from_utf8(parts["docProps/core.xml"].clone()).unwrap();
    assert!(core.contains("Tea &lt;deck&gt;"));

    for (name, xml) in [("editable-slide1.xml", &s1), ("editable-slide2.xml", &s2)] {
        let path = golden(name);
        if std::env::var("OD_UPDATE_GOLDEN").as_deref() == Ok("1") {
            std::fs::write(&path, xml).unwrap();
        }
        let want = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden {}; run with OD_UPDATE_GOLDEN=1",
                path.display()
            )
        });
        assert_eq!(xml, &want, "{name} differs from the golden file");
    }

    libreoffice_opens(&pptx);
}

fn soffice() -> Option<PathBuf> {
    let candidates = [
        "/opt/homebrew/bin/soffice",
        "/usr/local/bin/soffice",
        "/usr/bin/soffice",
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// LibreOffice must open the file and lay out both slides.
fn libreoffice_opens(pptx: &[u8]) {
    let Some(bin) = soffice() else {
        eprintln!("LibreOffice not installed: skipping the open-in-an-office-app check");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("deck.pptx");
    std::fs::write(&input, pptx).unwrap();
    let profile = format!("file://{}", dir.path().join("profile").display());
    let out = std::process::Command::new(bin)
        .arg(format!("-env:UserInstallation={profile}"))
        .args([
            "--headless",
            "--norestore",
            "--convert-to",
            "pdf",
            "--outdir",
        ])
        .arg(dir.path())
        .arg(&input)
        .output()
        .unwrap();
    let pdf = dir.path().join("deck.pdf");
    assert!(
        pdf.is_file(),
        "LibreOffice could not convert the deck: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let doc = lopdf::Document::load(&pdf).unwrap();
    assert_eq!(doc.get_pages().len(), 2, "LibreOffice laid out both slides");
}
