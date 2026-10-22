//! Raster PDF: one page per image, each image filling its page.
//!
//! A deliberately small PDF 1.4 writer: pages hold one Flate-compressed RGB
//! image XObject. Page width is 13.333 in (960 pt, the deck page of spec 4.2);
//! height follows the image aspect ratio.

use crate::image::{ImageError, decode_rgb};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::fmt::Write as _;
use std::io::Write as _;

/// Page width in points (13.333 in).
pub const PAGE_WIDTH_PT: f64 = 960.0;

/// Builds a PDF with one page per PNG, in order.
///
/// # Errors
/// PNG decode errors. An empty list is rejected.
pub fn pngs_to_pdf(pngs: &[Vec<u8>]) -> Result<Vec<u8>, ImageError> {
    if pngs.is_empty() {
        return Err(ImageError::Unsupported("no pages".to_owned()));
    }
    let mut w = PdfWriter::default();
    // 1: catalog, 2: pages; then per page: page, contents, image.
    let page_ids: Vec<usize> = (0..pngs.len()).map(|i| 3 + i * 3).collect();
    w.object(1, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    w.object(
        2,
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pngs.len()).as_bytes(),
    );
    for (png, &page_id) in pngs.iter().zip(&page_ids) {
        let rgb = decode_rgb(png)?;
        let width = f64::from(rgb.width.max(1));
        let height_pt = PAGE_WIDTH_PT * f64::from(rgb.height.max(1)) / width;
        let (contents_id, image_id) = (page_id + 1, page_id + 2);
        w.object(
            page_id,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH_PT:.3} {height_pt:.3}] \
                 /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {contents_id} 0 R >>"
            )
            .as_bytes(),
        );
        let ops = format!("q {PAGE_WIDTH_PT:.3} 0 0 {height_pt:.3} 0 0 cm /Im0 Do Q");
        w.stream(contents_id, "", ops.as_bytes());
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&rgb.pixels)?;
        let data = z.finish()?;
        w.stream(
            image_id,
            &format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /FlateDecode",
                rgb.width, rgb.height
            ),
            &data,
        );
    }
    Ok(w.finish(1))
}

/// Byte buffer plus the offset of each object, for the xref table.
#[derive(Default)]
struct PdfWriter {
    buf: Vec<u8>,
    offsets: Vec<(usize, usize)>,
}

impl PdfWriter {
    fn header(&mut self) {
        if self.buf.is_empty() {
            self.buf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        }
    }

    fn object(&mut self, id: usize, body: &[u8]) {
        self.header();
        self.offsets.push((id, self.buf.len()));
        self.buf
            .extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        self.buf.extend_from_slice(body);
        self.buf.extend_from_slice(b"\nendobj\n");
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.object(id, &body);
    }

    fn finish(mut self, root: usize) -> Vec<u8> {
        self.offsets.sort_unstable();
        let size = self.offsets.last().map_or(0, |(id, _)| id + 1);
        let xref_at = self.buf.len();
        let mut xref = format!("xref\n0 {size}\n0000000000 65535 f \n");
        let mut next = 1;
        for &(id, offset) in &self.offsets {
            while next < id {
                xref.push_str("0000000000 65535 f \n");
                next += 1;
            }
            let _ = writeln!(xref, "{offset:010} 00000 n ");
            next += 1;
        }
        let _ = write!(
            xref,
            "trailer\n<< /Size {size} /Root {root} 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        );
        self.buf.extend_from_slice(xref.as_bytes());
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::solid_png;

    #[test]
    fn one_page_per_image_and_parses() {
        let pngs: Vec<Vec<u8>> = (0..5)
            .map(|i| solid_png(16, 9, [i * 40, 0, 0]).unwrap())
            .collect();
        let pdf = pngs_to_pdf(&pngs).unwrap();
        let doc = lopdf::Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 5);
    }

    #[test]
    fn page_follows_aspect_ratio() {
        let pdf = pngs_to_pdf(&[solid_png(1920, 1080, [9, 9, 9]).unwrap()]).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains("/MediaBox [0 0 960.000 540.000]"),
            "{text:.400}"
        );
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(pngs_to_pdf(&[]).is_err());
    }
}
