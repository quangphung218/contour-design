//! Image PPTX: one full-bleed picture per slide (spec 1.3, 4.2).
//!
//! Minimal hand-written OOXML: one master, one blank layout, one theme, then
//! a slide and a PNG part per image. Slide width is 13.333 in; height follows
//! the image aspect ratio.

use crate::image::png_size;
use std::io::{Cursor, Write};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// 13.333 in in EMU.
pub const SLIDE_WIDTH_EMU: u64 = 12_192_000;
const MIN_EMU: u64 = 914_400;
const MAX_EMU: u64 = 51_206_400;

pub const PPTX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

#[derive(Debug, thiserror::Error)]
pub enum PptxError {
    #[error("no slides")]
    Empty,
    #[error("slide {0} is not a PNG")]
    NotPng(usize),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub(crate) const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
pub(crate) const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
pub(crate) const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(crate) const R_DOC: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const EMPTY_TREE: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn rels(entries: &[(&str, String, String)]) -> String {
    let mut out = format!(r#"{XML_DECL}<Relationships xmlns="{REL_NS}">"#);
    for (id, kind, target) in entries {
        out.push_str(&format!(
            r#"<Relationship Id="{id}" Type="{kind}" Target="{target}"/>"#
        ));
    }
    out.push_str("</Relationships>");
    out
}

/// Slide height in EMU for an image of `width`×`height` px.
pub(crate) fn slide_height(width: u32, height: u32) -> u64 {
    let h = SLIDE_WIDTH_EMU * u64::from(height.max(1)) / u64::from(width.max(1));
    h.clamp(MIN_EMU, MAX_EMU)
}

/// Builds a .pptx with one slide per PNG. The slide size comes from the first
/// image; every picture is stretched to fill its slide.
///
/// # Errors
/// [`PptxError::Empty`], [`PptxError::NotPng`], zip errors.
pub fn pngs_to_pptx(pngs: &[Vec<u8>], title: &str) -> Result<Vec<u8>, PptxError> {
    let first = pngs.first().ok_or(PptxError::Empty)?;
    for (i, png) in pngs.iter().enumerate() {
        png_size(png).ok_or(PptxError::NotPng(i + 1))?;
    }
    let (w, h) = png_size(first).ok_or(PptxError::NotPng(1))?;
    let cx = SLIDE_WIDTH_EMU;
    let cy = slide_height(w, h);
    let n = pngs.len();

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let put = |zip: &mut ZipWriter<Cursor<Vec<u8>>>,
               name: &str,
               body: &[u8],
               o|
     -> Result<(), PptxError> {
        zip.start_file(name, o)?;
        zip.write_all(body)?;
        Ok(())
    };

    // [Content_Types].xml must come first for some readers.
    let mut types = format!(
        r#"{XML_DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#
    );
    for i in 1..=n {
        types.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
    }
    types.push_str("</Types>");
    put(&mut zip, "[Content_Types].xml", types.as_bytes(), opts)?;

    put(
        &mut zip,
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
        &mut zip,
        "docProps/core.xml",
        format!(
            r#"{XML_DECL}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Open Design</dc:creator></cp:coreProperties>"#,
            escape(title)
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "docProps/app.xml",
        format!(
            r#"{XML_DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Open Design</Application><Slides>{n}</Slides></Properties>"#
        )
        .as_bytes(),
        opts,
    )?;

    let mut presentation = format!(
        r#"{XML_DECL}<p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>"#
    );
    let mut pres_rels = vec![
        (
            "rId1".to_owned(),
            format!("{R_DOC}/slideMaster"),
            "slideMasters/slideMaster1.xml".to_owned(),
        ),
        (
            "rId2".to_owned(),
            format!("{R_DOC}/theme"),
            "theme/theme1.xml".to_owned(),
        ),
    ];
    for i in 1..=n {
        let rid = format!("rId{}", i + 2);
        presentation.push_str(&format!(r#"<p:sldId id="{}" r:id="{rid}"/>"#, 255 + i));
        pres_rels.push((
            rid,
            format!("{R_DOC}/slide"),
            format!("slides/slide{i}.xml"),
        ));
    }
    presentation.push_str(&format!(
        r#"</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
    ));
    put(
        &mut zip,
        "ppt/presentation.xml",
        presentation.as_bytes(),
        opts,
    )?;
    let pres_rels: Vec<(&str, String, String)> = pres_rels
        .iter()
        .map(|(id, k, t)| (id.as_str(), k.clone(), t.clone()))
        .collect();
    put(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        rels(&pres_rels).as_bytes(),
        opts,
    )?;

    put(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            r#"{XML_DECL}<p:sldMaster {NS}><p:cSld><p:spTree>{EMPTY_TREE}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
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
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            r#"{XML_DECL}<p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{EMPTY_TREE}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[(
            "rId1",
            format!("{R_DOC}/slideMaster"),
            "../slideMasters/slideMaster1.xml".into(),
        )])
        .as_bytes(),
        opts,
    )?;
    put(&mut zip, "ppt/theme/theme1.xml", THEME.as_bytes(), opts)?;

    for (i, png) in pngs.iter().enumerate() {
        let i = i + 1;
        let slide = format!(
            r#"{XML_DECL}<p:sld {NS}><p:cSld><p:spTree>{EMPTY_TREE}<p:pic><p:nvPicPr><p:cNvPr id="2" name="Slide {i}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        );
        put(
            &mut zip,
            &format!("ppt/slides/slide{i}.xml"),
            slide.as_bytes(),
            opts,
        )?;
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{i}.xml.rels"),
            rels(&[
                (
                    "rId1",
                    format!("{R_DOC}/slideLayout"),
                    "../slideLayouts/slideLayout1.xml".into(),
                ),
                (
                    "rId2",
                    format!("{R_DOC}/image"),
                    format!("../media/image{i}.png"),
                ),
            ])
            .as_bytes(),
            opts,
        )?;
        // PNG is already compressed.
        put(&mut zip, &format!("ppt/media/image{i}.png"), png, stored)?;
    }
    Ok(zip.finish()?.into_inner())
}

pub(crate) const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Open Design"><a:themeElements><a:clrScheme name="Open Design"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F2937"/></a:dk2><a:lt2><a:srgbClr val="F3F4F6"/></a:lt2><a:accent1><a:srgbClr val="2563EB"/></a:accent1><a:accent2><a:srgbClr val="DC2626"/></a:accent2><a:accent3><a:srgbClr val="16A34A"/></a:accent3><a:accent4><a:srgbClr val="CA8A04"/></a:accent4><a:accent5><a:srgbClr val="7C3AED"/></a:accent5><a:accent6><a:srgbClr val="0891B2"/></a:accent6><a:hlink><a:srgbClr val="2563EB"/></a:hlink><a:folHlink><a:srgbClr val="7C3AED"/></a:folHlink></a:clrScheme><a:fontScheme name="Open Design"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Open Design"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#;

/// Reads a .pptx back: number of slides listed in `presentation.xml` and the
/// number of slide parts. For tests and sanity checks.
///
/// # Errors
/// Zip or I/O errors.
pub fn count_slides(pptx: &[u8]) -> Result<(usize, usize), PptxError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(pptx))?;
    let mut presentation = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("ppt/presentation.xml")?,
        &mut presentation,
    )?;
    let listed = presentation.matches("<p:sldId ").count();
    let parts = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .count();
    Ok((listed, parts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::solid_png;

    #[test]
    fn five_slides_round_trip() {
        let pngs: Vec<Vec<u8>> = (0..5)
            .map(|i| solid_png(32, 18, [0, i * 50, 0]).unwrap())
            .collect();
        let pptx = pngs_to_pptx(&pngs, "Deck <5>").unwrap();
        assert_eq!(count_slides(&pptx).unwrap(), (5, 5));
        let mut archive = zip::ZipArchive::new(Cursor::new(&pptx[..])).unwrap();
        assert_eq!(
            archive.file_names().next().unwrap().unwrap(),
            "[Content_Types].xml"
        );
        let mut core = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("docProps/core.xml").unwrap(),
            &mut core,
        )
        .unwrap();
        assert!(core.contains("Deck &lt;5&gt;"));
        assert_eq!(
            archive.by_name("ppt/media/image5.png").unwrap().size(),
            pngs[4].len() as u64
        );
    }

    #[test]
    fn slide_size_follows_aspect() {
        assert_eq!(slide_height(1920, 1080), 6_858_000);
        assert_eq!(slide_height(1, 1000), MAX_EMU);
    }

    #[test]
    fn rejects_empty_and_non_png() {
        assert!(matches!(pngs_to_pptx(&[], "x"), Err(PptxError::Empty)));
        assert!(matches!(
            pngs_to_pptx(&[b"jpeg?".to_vec()], "x"),
            Err(PptxError::NotPng(1))
        ));
    }
}
