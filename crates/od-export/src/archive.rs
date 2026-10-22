//! ZIP of a project (or one folder of it), written to a non-seekable sink so
//! the HTTP layer can stream it.

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Already-compressed formats are stored, everything else deflated.
fn is_compressed(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "avif"
            | "zip"
            | "gz"
            | "mp4"
            | "webm"
            | "mov"
            | "mp3"
            | "m4a"
            | "woff"
            | "woff2"
            | "pdf"
            | "pptx"
            | "docx"
            | "xlsx"
    )
}

/// Writes `entries` (`zip name`, file on disk) as a ZIP to `out`.
///
/// # Errors
/// I/O or zip errors; a file vanishing mid-archive is an error.
pub fn write_zip<W: Write>(out: W, entries: &[(String, PathBuf)]) -> io::Result<W> {
    let mut zip = ZipWriter::new_stream(out);
    for (name, path) in entries {
        let method = if is_compressed(name) {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(std::fs::metadata(path)?.len() >= u64::from(u32::MAX));
        zip.start_file(name.as_str(), options)
            .map_err(io::Error::other)?;
        io::copy(&mut File::open(path)?, &mut zip)?;
    }
    Ok(zip.finish().map_err(io::Error::other)?.into_inner())
}

/// Keeps file names ASCII-safe for `Content-Disposition` while the
/// `filename*` form carries the real name.
#[must_use]
pub fn sanitize_filename(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ', '-']).to_owned();
    cleaned.chars().take(120).collect()
}

/// `attachment; filename="ascii"; filename*=UTF-8''percent-encoded`.
#[must_use]
pub fn content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if (' '..='~').contains(&c) && c != '"' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for b in filename.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    /// A sink without `Seek`, like the HTTP body channel.
    struct Sink(Vec<u8>);
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn streams_readable_zip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<h1>hi</h1>").unwrap();
        std::fs::write(dir.path().join("a.png"), [0u8; 64]).unwrap();
        let entries = vec![
            ("index.html".to_owned(), dir.path().join("index.html")),
            ("img/a.png".to_owned(), dir.path().join("a.png")),
        ];
        let sink = write_zip(Sink(Vec::new()), &entries).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(sink.0)).unwrap();
        let mut html = String::new();
        zip.by_name("index.html")
            .unwrap()
            .read_to_string(&mut html)
            .unwrap();
        assert_eq!(html, "<h1>hi</h1>");
        assert_eq!(zip.by_name("img/a.png").unwrap().size(), 64);
    }

    #[test]
    fn disposition_has_ascii_fallback() {
        assert_eq!(
            content_disposition("Bản đồ.zip"),
            "attachment; filename=\"B_n __.zip\"; filename*=UTF-8''B%E1%BA%A3n%20%C4%91%E1%BB%93.zip"
        );
        assert_eq!(sanitize_filename(" a/b:c. "), "a-b-c");
    }
}
