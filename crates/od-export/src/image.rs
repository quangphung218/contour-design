//! PNG helpers: read dimensions, decode to RGB, stitch slides vertically.

use std::io::Cursor;

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("invalid PNG: {0}")]
    Decode(#[from] png::DecodingError),
    #[error("PNG encoding failed: {0}")]
    Encode(#[from] png::EncodingError),
    #[error("unsupported PNG layout: {0}")]
    Unsupported(String),
    #[error("cannot stitch images of different widths ({0} and {1})")]
    WidthMismatch(u32, u32),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// An 8-bit RGB raster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb {
    pub width: u32,
    pub height: u32,
    /// `width * height * 3` bytes, row-major.
    pub pixels: Vec<u8>,
}

/// Width and height from the IHDR chunk, without decoding pixels.
#[must_use]
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}

/// Decodes any 8-bit PNG to RGB, compositing alpha over white.
///
/// # Errors
/// Invalid or 16-bit/indexed-without-expansion PNGs.
pub fn decode_rgb(bytes: &[u8]) -> Result<Rgb, ImageError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let pixels = match info.color_type {
        png::ColorType::Rgb => buf,
        png::ColorType::Rgba => buf
            .chunks_exact(4)
            .flat_map(|p| {
                let a = u16::from(p[3]);
                let over = |c: u8| -> u8 {
                    // c*a + 255*(255-a), rounded, over 255.
                    let v = (u16::from(c) * a + 255 * (255 - a) + 127) / 255;
                    u8::try_from(v).unwrap_or(u8::MAX)
                };
                [over(p[0]), over(p[1]), over(p[2])]
            })
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| {
                let a = u16::from(p[1]);
                let v = (u16::from(p[0]) * a + 255 * (255 - a) + 127) / 255;
                let g = u8::try_from(v).unwrap_or(u8::MAX);
                [g, g, g]
            })
            .collect(),
        png::ColorType::Indexed => {
            return Err(ImageError::Unsupported("indexed colour".to_owned()));
        }
    };
    Ok(Rgb {
        width: w,
        height: h,
        pixels,
    })
}

/// Encodes an RGB raster as PNG.
///
/// # Errors
/// Encoder errors.
pub fn encode_png(image: &Rgb) -> Result<Vec<u8>, ImageError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.pixels)?;
    writer.finish()?;
    Ok(out)
}

/// A single-colour PNG (placeholders and tests).
///
/// # Errors
/// Encoder errors.
pub fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Result<Vec<u8>, ImageError> {
    let pixels = (0..u64::from(width) * u64::from(height))
        .flat_map(|_| rgb)
        .collect();
    encode_png(&Rgb {
        width,
        height,
        pixels,
    })
}

/// Stacks PNGs of equal width top to bottom into one PNG.
///
/// # Errors
/// Decode errors or differing widths.
pub fn stitch_vertical(pngs: &[Vec<u8>]) -> Result<Vec<u8>, ImageError> {
    let mut width = None;
    let mut height = 0;
    let mut pixels = Vec::new();
    for png in pngs {
        let rgb = decode_rgb(png)?;
        match width {
            None => width = Some(rgb.width),
            Some(w) if w != rgb.width => return Err(ImageError::WidthMismatch(w, rgb.width)),
            Some(_) => {}
        }
        height += rgb.height;
        pixels.extend_from_slice(&rgb.pixels);
    }
    encode_png(&Rgb {
        width: width.unwrap_or(0),
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_reads_ihdr() {
        assert_eq!(png_size(&solid_png(7, 3, [1, 2, 3]).unwrap()), Some((7, 3)));
        assert_eq!(png_size(b"not a png"), None);
    }

    #[test]
    fn stitch_stacks_rows() {
        let out = stitch_vertical(&[
            solid_png(4, 2, [255, 0, 0]).unwrap(),
            solid_png(4, 3, [0, 0, 255]).unwrap(),
        ])
        .unwrap();
        let rgb = decode_rgb(&out).unwrap();
        assert_eq!((rgb.width, rgb.height), (4, 5));
        assert_eq!(&rgb.pixels[..3], &[255, 0, 0]);
        assert_eq!(&rgb.pixels[rgb.pixels.len() - 3..], &[0, 0, 255]);
    }

    #[test]
    fn stitch_rejects_mixed_widths() {
        assert!(matches!(
            stitch_vertical(&[
                solid_png(4, 2, [0; 3]).unwrap(),
                solid_png(5, 2, [0; 3]).unwrap()
            ]),
            Err(ImageError::WidthMismatch(4, 5))
        ));
    }
}
