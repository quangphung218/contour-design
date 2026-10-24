//! Editable PPTX (spec 1.3 "PPTX (editable)"): each slide's laid-out DOM
//! becomes native PowerPoint objects (text boxes with runs, filled or
//! outlined shapes, embedded pictures) instead of one screenshot.
//!
//! The browser side ([`LAYOUT_SCRIPT`], run by the renderer on each shown
//! slide) measures the DOM into a [`SlideLayout`]; [`layouts_to_pptx`]
//! writes the OOXML. Coordinates are CSS pixels relative to the slide box.

use crate::pptx::{
    EMPTY_TREE, NS, R_DOC, SLIDE_WIDTH_EMU, THEME, XML_DECL, escape, rels, slide_height,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::{Cursor, Write};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// One measured slide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlideLayout {
    pub width: f64,
    pub height: f64,
    /// CSS colour of the slide background.
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub elements: Vec<Element>,
}

/// A box in slide pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Element {
    /// A filled and/or outlined rectangle (rounded with `radius`).
    Shape {
        #[serde(flatten)]
        rect: Rect,
        #[serde(default)]
        fill: Option<String>,
        #[serde(default)]
        line: Option<String>,
        #[serde(default, rename = "lineWidth")]
        line_width: f64,
        #[serde(default)]
        radius: f64,
    },
    /// A text box; `\n` inside a run starts a new paragraph.
    Text {
        #[serde(flatten)]
        rect: Rect,
        #[serde(default)]
        paragraphs: Vec<Paragraph>,
        /// `top`, `middle` or `bottom`.
        #[serde(default)]
        valign: Option<String>,
    },
    /// A picture from a `data:` URL (PNG, JPEG or GIF).
    Image {
        #[serde(flatten)]
        rect: Rect,
        src: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    /// `left`, `center`, `right` or `justify`.
    #[serde(default)]
    pub align: Option<String>,
    /// Line height as a multiple of the font size.
    #[serde(default, rename = "lineHeight")]
    pub line_height: Option<f64>,
    #[serde(default)]
    pub runs: Vec<TextRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    /// CSS pixels.
    #[serde(default = "default_size")]
    pub size: f64,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub font: Option<String>,
}

fn default_size() -> f64 {
    16.0
}

#[derive(Debug, thiserror::Error)]
pub enum EditableError {
    #[error("no slides")]
    Empty,
    #[error("slide {0} has no size")]
    BadSize(usize),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A parsed CSS colour: `RRGGBB` plus alpha in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub hex: [u8; 3],
    pub alpha: f64,
}

/// `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()` and `rgba()` (what
/// `getComputedStyle` returns). `None` for anything else or fully
/// transparent colours.
#[must_use]
pub fn parse_color(css: &str) -> Option<Color> {
    let s = css.trim();
    let color = if let Some(hex) = s.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
            .collect::<Option<_>>()?;
        match digits.len() {
            3 => Color {
                hex: [digits[0] * 17, digits[1] * 17, digits[2] * 17],
                alpha: 1.0,
            },
            6 | 8 => {
                let byte = |i: usize| digits[i] * 16 + digits[i + 1];
                Color {
                    hex: [byte(0), byte(2), byte(4)],
                    alpha: if digits.len() == 8 {
                        f64::from(byte(6)) / 255.0
                    } else {
                        1.0
                    },
                }
            }
            _ => return None,
        }
    } else {
        let inner = s
            .strip_prefix("rgba(")
            .or_else(|| s.strip_prefix("rgb("))?
            .strip_suffix(')')?;
        let parts: Vec<&str> = inner
            .split([',', ' ', '/'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let channel = |p: &str| -> Option<u8> {
            let v: f64 = p.parse().ok()?;
            // Clamped to 0..=255 first, so the cast cannot truncate.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Some(v.clamp(0.0, 255.0).round() as u8)
        };
        let alpha = match parts.get(3) {
            Some(a) if a.ends_with('%') => a.trim_end_matches('%').parse::<f64>().ok()? / 100.0,
            Some(a) => a.parse::<f64>().ok()?,
            None => 1.0,
        };
        Color {
            hex: [channel(parts[0])?, channel(parts[1])?, channel(parts[2])?],
            alpha: alpha.clamp(0.0, 1.0),
        }
    };
    (color.alpha > 0.001).then_some(color)
}

fn solid_fill(c: Color) -> String {
    let [r, g, b] = c.hex;
    if c.alpha >= 0.999 {
        format!(r#"<a:solidFill><a:srgbClr val="{r:02X}{g:02X}{b:02X}"/></a:solidFill>"#)
    } else {
        format!(
            r#"<a:solidFill><a:srgbClr val="{r:02X}{g:02X}{b:02X}"><a:alpha val="{}"/></a:srgbClr></a:solidFill>"#,
            pct(c.alpha)
        )
    }
}

/// 0..=1 as OOXML thousandths of a percent.
fn pct(v: f64) -> i64 {
    // Bounded by the clamp.
    #[allow(clippy::cast_possible_truncation)]
    let out = (v.clamp(0.0, 1.0) * 100_000.0).round() as i64;
    out
}

/// Pixel → EMU conversion for one deck.
#[derive(Debug, Clone, Copy)]
struct Scale {
    emu_per_px: f64,
    /// Font points per CSS pixel.
    pt_per_px: f64,
}

impl Scale {
    fn emu(self, px: f64) -> i64 {
        // Slide coordinates are bounded by the slide size (≤ 51 206 400 EMU).
        #[allow(clippy::cast_possible_truncation)]
        let out = (px * self.emu_per_px).round() as i64;
        out
    }

    fn extent(self, px: f64) -> i64 {
        self.emu(px.max(0.0)).max(1)
    }

    /// Hundredths of a point, as `sz` wants; at least 1 pt.
    fn font_size(self, px: f64) -> i64 {
        #[allow(clippy::cast_possible_truncation)]
        let out = (px * self.pt_per_px * 100.0).round() as i64;
        out.clamp(100, 400_000)
    }
}

/// Text safe for XML 1.0 (no control characters) and escaped.
fn xml_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();
    escape(&clean).replace('\'', "&apos;")
}

fn xfrm(scale: Scale, r: Rect) -> String {
    format!(
        r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
        scale.emu(r.x),
        scale.emu(r.y),
        scale.extent(r.w),
        scale.extent(r.h)
    )
}

fn shape_xml(
    id: usize,
    scale: Scale,
    rect: Rect,
    fill: Option<Color>,
    line: Option<(Color, f64)>,
    radius: f64,
) -> String {
    let short = rect.w.min(rect.h).max(1.0);
    let geom = if radius > 0.5 {
        let adj = (radius / short).min(0.5) * 100_000.0;
        // ≤ 50 000 by the min above.
        #[allow(clippy::cast_possible_truncation)]
        let adj = adj.round() as i64;
        format!(
            r#"<a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val {adj}"/></a:avLst></a:prstGeom>"#
        )
    } else {
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#.to_owned()
    };
    let fill = fill.map_or_else(|| "<a:noFill/>".to_owned(), solid_fill);
    let line = match line {
        Some((c, w)) => format!(r#"<a:ln w="{}">{}</a:ln>"#, scale.extent(w), solid_fill(c)),
        None => "<a:ln><a:noFill/></a:ln>".to_owned(),
    };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Shape {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{geom}{fill}{line}</p:spPr></p:sp>"#,
        xfrm(scale, rect)
    )
}

/// Splits runs on `\n` into extra paragraphs with the same formatting.
fn split_lines(paragraphs: &[Paragraph]) -> Vec<Paragraph> {
    let mut out = Vec::new();
    for p in paragraphs {
        let mut current = Paragraph {
            align: p.align.clone(),
            line_height: p.line_height,
            runs: Vec::new(),
        };
        for run in &p.runs {
            let mut pieces = run.text.split('\n').peekable();
            while let Some(piece) = pieces.next() {
                if !piece.is_empty() {
                    current.runs.push(TextRun {
                        text: piece.to_owned(),
                        ..run.clone()
                    });
                }
                if pieces.peek().is_some() {
                    let next = Paragraph {
                        align: p.align.clone(),
                        line_height: p.line_height,
                        runs: Vec::new(),
                    };
                    out.push(std::mem::replace(&mut current, next));
                }
            }
        }
        out.push(current);
    }
    out
}

fn run_xml(scale: Scale, run: &TextRun) -> String {
    let mut props = format!(
        r#"<a:rPr lang="en-US" sz="{}" b="{}" i="{}"{} dirty="0">"#,
        scale.font_size(run.size),
        u8::from(run.bold),
        u8::from(run.italic),
        if run.underline { r#" u="sng""# } else { "" },
    );
    if let Some(c) = run.color.as_deref().and_then(parse_color) {
        props.push_str(&solid_fill(c));
    }
    if let Some(font) = run
        .font
        .as_deref()
        .map(|f| f.trim().trim_matches(['"', '\'']))
        .filter(|f| !f.is_empty())
    {
        let font = xml_text(font);
        let _ = write!(
            props,
            r#"<a:latin typeface="{font}"/><a:ea typeface="{font}"/><a:cs typeface="{font}"/>"#
        );
    }
    props.push_str("</a:rPr>");
    format!("<a:r>{props}<a:t>{}</a:t></a:r>", xml_text(&run.text))
}

fn text_xml(
    id: usize,
    scale: Scale,
    rect: Rect,
    paragraphs: &[Paragraph],
    valign: Option<&str>,
) -> String {
    let anchor = match valign {
        Some("middle") => "ctr",
        Some("bottom") => "b",
        _ => "t",
    };
    let mut body = String::new();
    for p in split_lines(paragraphs) {
        let algn = match p.align.as_deref() {
            Some("center") => "ctr",
            Some("right" | "end") => "r",
            Some("justify") => "just",
            _ => "l",
        };
        body.push_str("<a:p>");
        let _ = write!(body, r#"<a:pPr algn="{algn}">"#);
        if let Some(lh) = p.line_height.filter(|l| *l > 0.0) {
            // PowerPoint's 100 % is about 1.2× the font size.
            // Clamped to 10 %..500 %, so the cast cannot truncate.
            #[allow(clippy::cast_possible_truncation)]
            let val = (lh / 1.2 * 100_000.0).clamp(10_000.0, 500_000.0).round() as i64;
            let _ = write!(body, r#"<a:lnSpc><a:spcPct val="{val}"/></a:lnSpc>"#);
        }
        body.push_str("</a:pPr>");
        let size = p.runs.first().map_or(default_size(), |r| r.size);
        for run in &p.runs {
            body.push_str(&run_xml(scale, run));
        }
        let _ = write!(
            body,
            r#"<a:endParaRPr lang="en-US" sz="{}" dirty="0"/></a:p>"#,
            scale.font_size(size)
        );
    }
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0" anchor="{anchor}"><a:noAutofit/></a:bodyPr><a:lstStyle/>{body}</p:txBody></p:sp>"#,
        xfrm(scale, rect)
    )
}

fn picture_xml(id: usize, scale: Scale, rect: Rect, rid: &str) -> String {
    format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Picture {id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
        xfrm(scale, rect)
    )
}

/// Decodes a `data:image/{png,jpeg,gif};base64,…` URL to (extension, bytes).
#[must_use]
pub fn decode_data_image(src: &str) -> Option<(&'static str, Vec<u8>)> {
    let rest = src.trim().strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let ext = match meta
        .trim_end_matches(";base64")
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/gif" => "gif",
        _ => return None,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    (!bytes.is_empty()).then_some((ext, bytes))
}

struct Media {
    name: String,
    bytes: Vec<u8>,
}

/// One slide's XML plus the pictures it embeds.
fn slide_xml(layout: &SlideLayout, scale: Scale, slide_no: usize) -> (String, Vec<Media>) {
    let mut tree = String::new();
    let mut media = Vec::new();
    let mut id = 2;
    for el in &layout.elements {
        match el {
            Element::Shape {
                rect,
                fill,
                line,
                line_width,
                radius,
            } => {
                let fill = fill.as_deref().and_then(parse_color);
                let line = line
                    .as_deref()
                    .and_then(parse_color)
                    .filter(|_| *line_width > 0.0)
                    .map(|c| (c, *line_width));
                if fill.is_none() && line.is_none() {
                    continue;
                }
                tree.push_str(&shape_xml(id, scale, *rect, fill, line, *radius));
            }
            Element::Text {
                rect,
                paragraphs,
                valign,
            } => {
                if paragraphs
                    .iter()
                    .all(|p| p.runs.iter().all(|r| r.text.trim().is_empty()))
                {
                    continue;
                }
                tree.push_str(&text_xml(id, scale, *rect, paragraphs, valign.as_deref()));
            }
            Element::Image { rect, src } => {
                let Some((ext, bytes)) = decode_data_image(src) else {
                    continue;
                };
                let rid = format!("rId{}", media.len() + 2);
                media.push(Media {
                    name: format!("slide{slide_no}-image{}.{ext}", media.len() + 1),
                    bytes,
                });
                tree.push_str(&picture_xml(id, scale, *rect, &rid));
            }
        }
        id += 1;
    }
    let bg = layout
        .background
        .as_deref()
        .and_then(parse_color)
        .map(|c| {
            format!(
                "<p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg>",
                solid_fill(c)
            )
        })
        .unwrap_or_default();
    let xml = format!(
        r#"{XML_DECL}<p:sld {NS}><p:cSld>{bg}<p:spTree>{EMPTY_TREE}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
    );
    (xml, media)
}

/// Builds an editable .pptx with one slide per layout. The slide size
/// follows the first layout's aspect ratio.
///
/// # Errors
/// [`EditableError::Empty`], [`EditableError::BadSize`], zip errors.
pub fn layouts_to_pptx(layouts: &[SlideLayout], title: &str) -> Result<Vec<u8>, EditableError> {
    let first = layouts.first().ok_or(EditableError::Empty)?;
    for (i, l) in layouts.iter().enumerate() {
        if !(l.width.is_finite() && l.height.is_finite() && l.width >= 1.0 && l.height >= 1.0) {
            return Err(EditableError::BadSize(i + 1));
        }
    }
    // Rounded to whole pixels only to pick the slide aspect ratio.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (w, h) = (first.width.round() as u32, first.height.round() as u32);
    let cx = SLIDE_WIDTH_EMU;
    let cy = slide_height(w, h);
    #[allow(clippy::cast_precision_loss)]
    let emu_per_px = cx as f64 / first.width;
    let scale = Scale {
        emu_per_px,
        pt_per_px: emu_per_px / 12_700.0,
    };
    let n = layouts.len();

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, body: &[u8], o: SimpleFileOptions| -> Result<(), EditableError> {
        zip.start_file(name, o)?;
        zip.write_all(body)?;
        Ok(())
    };

    let slides: Vec<(String, Vec<Media>)> = layouts
        .iter()
        .enumerate()
        .map(|(i, l)| slide_xml(l, scale, i + 1))
        .collect();

    let mut types = format!(
        r#"{XML_DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#
    );
    for i in 1..=n {
        let _ = write!(
            types,
            r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        );
    }
    types.push_str("</Types>");
    put("[Content_Types].xml", types.as_bytes(), opts)?;
    put(
        "_rels/.rels",
        rels(&[
            ("rId1", format!("{R_DOC}/officeDocument"), "ppt/presentation.xml".into()),
            (
                "rId2",
                "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties".into(),
                "docProps/core.xml".into(),
            ),
            ("rId3", format!("{R_DOC}/extended-properties"), "docProps/app.xml".into()),
        ])
        .as_bytes(),
        opts,
    )?;
    put(
        "docProps/core.xml",
        format!(
            r#"{XML_DECL}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Contour</dc:creator></cp:coreProperties>"#,
            xml_text(title)
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "docProps/app.xml",
        format!(
            r#"{XML_DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Contour</Application><Slides>{n}</Slides></Properties>"#
        )
        .as_bytes(),
        opts,
    )?;

    let mut presentation = format!(
        r#"{XML_DECL}<p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>"#
    );
    let mut pres_rels: Vec<(String, String, String)> = vec![
        (
            "rId1".into(),
            format!("{R_DOC}/slideMaster"),
            "slideMasters/slideMaster1.xml".into(),
        ),
        (
            "rId2".into(),
            format!("{R_DOC}/theme"),
            "theme/theme1.xml".into(),
        ),
    ];
    for i in 1..=n {
        let rid = format!("rId{}", i + 2);
        let _ = write!(presentation, r#"<p:sldId id="{}" r:id="{rid}"/>"#, 255 + i);
        pres_rels.push((
            rid,
            format!("{R_DOC}/slide"),
            format!("slides/slide{i}.xml"),
        ));
    }
    let _ = write!(
        presentation,
        r#"</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
    );
    put("ppt/presentation.xml", presentation.as_bytes(), opts)?;
    let pres_rels: Vec<(&str, String, String)> = pres_rels
        .iter()
        .map(|(id, k, t)| (id.as_str(), k.clone(), t.clone()))
        .collect();
    put(
        "ppt/_rels/presentation.xml.rels",
        rels(&pres_rels).as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            r#"{XML_DECL}<p:sldMaster {NS}><p:cSld><p:spTree>{EMPTY_TREE}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(&[
            (
                "rId1",
                format!("{R_DOC}/slideLayout"),
                "../slideLayouts/slideLayout1.xml".into(),
            ),
            (
                "rId2",
                format!("{R_DOC}/theme"),
                "../theme/theme1.xml".into(),
            ),
        ])
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            r#"{XML_DECL}<p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{EMPTY_TREE}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[(
            "rId1",
            format!("{R_DOC}/slideMaster"),
            "../slideMasters/slideMaster1.xml".into(),
        )])
        .as_bytes(),
        opts,
    )?;
    put("ppt/theme/theme1.xml", THEME.as_bytes(), opts)?;

    for (i, (xml, media)) in slides.iter().enumerate() {
        let i = i + 1;
        put(&format!("ppt/slides/slide{i}.xml"), xml.as_bytes(), opts)?;
        let mut entries: Vec<(String, String, String)> = vec![(
            "rId1".into(),
            format!("{R_DOC}/slideLayout"),
            "../slideLayouts/slideLayout1.xml".into(),
        )];
        for (k, m) in media.iter().enumerate() {
            entries.push((
                format!("rId{}", k + 2),
                format!("{R_DOC}/image"),
                format!("../media/{}", m.name),
            ));
        }
        let entries: Vec<(&str, String, String)> = entries
            .iter()
            .map(|(id, k, t)| (id.as_str(), k.clone(), t.clone()))
            .collect();
        put(
            &format!("ppt/slides/_rels/slide{i}.xml.rels"),
            rels(&entries).as_bytes(),
            opts,
        )?;
        for m in media {
            put(&format!("ppt/media/{}", m.name), &m.bytes, stored)?;
        }
    }
    Ok(zip.finish()?.into_inner())
}

/// Page script that measures the slide pinned at the viewport's top-left
/// (after the renderer's "show slide") into a [`SlideLayout`] JSON value.
/// Pictures (`<img>`, inline `<svg>`, `<canvas>`) are re-encoded through a
/// canvas into PNG `data:` URLs; text keeps its runs and styling.
pub const LAYOUT_SCRIPT: &str = include_str!("pptx_layout.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        let c = parse_color("rgb(255, 0, 16)").unwrap();
        assert_eq!((c.hex, c.alpha), ([255, 0, 16], 1.0));
        let c = parse_color("rgba(0, 0, 0, 0.5)").unwrap();
        assert!((c.alpha - 0.5).abs() < 1e-9);
        assert_eq!(parse_color("#abc").unwrap().hex, [0xaa, 0xbb, 0xcc]);
        assert_eq!(parse_color("#11223380").unwrap().hex, [0x11, 0x22, 0x33]);
        assert!(parse_color("rgba(0, 0, 0, 0)").is_none());
        assert!(parse_color("transparent").is_none());
        assert!(parse_color("#12").is_none());
        assert_eq!(
            solid_fill(parse_color("rgba(16,32,48,0.25)").unwrap()),
            r#"<a:solidFill><a:srgbClr val="102030"><a:alpha val="25000"/></a:srgbClr></a:solidFill>"#
        );
    }

    #[test]
    fn lines_become_paragraphs() {
        let run = |t: &str| TextRun {
            text: t.into(),
            size: 20.0,
            color: None,
            bold: true,
            italic: false,
            underline: false,
            font: None,
        };
        let out = split_lines(&[Paragraph {
            align: Some("center".into()),
            line_height: None,
            runs: vec![run("one\ntwo"), run(" three")],
        }]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].runs.len(), 1);
        assert_eq!(
            out[1]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "two three"
        );
        assert!(out[1].runs.iter().all(|r| r.bold));
    }

    #[test]
    fn data_images() {
        assert_eq!(
            decode_data_image("data:image/png;base64,AAEC").unwrap(),
            ("png", vec![0, 1, 2])
        );
        assert!(decode_data_image("data:image/svg+xml;base64,AAEC").is_none());
        assert!(decode_data_image("data:image/png,raw").is_none());
        assert!(decode_data_image("https://x/y.png").is_none());
    }

    #[test]
    fn rejects_empty_and_sizeless() {
        assert!(matches!(
            layouts_to_pptx(&[], "x"),
            Err(EditableError::Empty)
        ));
        let bad = SlideLayout {
            width: 0.0,
            height: 10.0,
            background: None,
            elements: vec![],
        };
        assert!(matches!(
            layouts_to_pptx(&[bad], "x"),
            Err(EditableError::BadSize(1))
        ));
    }
}
