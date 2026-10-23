//! Plain text of Office files for the Studio viewer: Word paragraphs,
//! PowerPoint slides, Excel sheets (tab-separated rows). Reads the OOXML
//! parts out of the ZIP with a small tag scanner; formatting is dropped.

use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Largest XML part read, so a hostile file cannot balloon memory.
const MAX_PART_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    Document,
    Presentation,
    Spreadsheet,
}

impl OfficeKind {
    /// From the file extension (`docx`, `pptx`, `xlsx` and their macro
    /// and template variants).
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match ext.as_str() {
            "docx" | "docm" | "dotx" => Some(Self::Document),
            "pptx" | "pptm" | "potx" => Some(Self::Presentation),
            "xlsx" | "xlsm" | "xltx" => Some(Self::Spreadsheet),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Presentation => "presentation",
            Self::Spreadsheet => "spreadsheet",
        }
    }
}

/// One slide, sheet, or the whole document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// `1`, `2`… for slides, the sheet's part number for sheets, empty for
    /// a document.
    pub title: String,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum OfficeError {
    #[error("not an Office file (docx, pptx or xlsx)")]
    Unsupported,
    #[error("the file is not a readable Office package: {0}")]
    Corrupt(String),
}

/// Text of the Office file `name` holding `bytes`.
///
/// # Errors
/// [`OfficeError::Unsupported`] for other extensions,
/// [`OfficeError::Corrupt`] for a broken package.
pub fn extract(name: &str, bytes: &[u8]) -> Result<(OfficeKind, Vec<Section>), OfficeError> {
    let kind = OfficeKind::of(name).ok_or(OfficeError::Unsupported)?;
    let mut zip =
        ZipArchive::new(Cursor::new(bytes)).map_err(|e| OfficeError::Corrupt(e.to_string()))?;
    let sections = match kind {
        OfficeKind::Document => {
            let xml = part(&mut zip, "word/document.xml")?;
            vec![Section {
                title: String::new(),
                text: paragraphs(&xml, "w:t", "w:p"),
            }]
        }
        OfficeKind::Presentation => numbered(&mut zip, "ppt/slides/slide")?
            .into_iter()
            .map(|(n, xml)| Section {
                title: n.to_string(),
                text: paragraphs(&xml, "a:t", "a:p"),
            })
            .collect(),
        OfficeKind::Spreadsheet => {
            let shared = match part(&mut zip, "xl/sharedStrings.xml") {
                Ok(xml) => shared_strings(&xml),
                Err(_) => Vec::new(),
            };
            numbered(&mut zip, "xl/worksheets/sheet")?
                .into_iter()
                .map(|(n, xml)| Section {
                    title: n.to_string(),
                    text: sheet_rows(&xml, &shared),
                })
                .collect()
        }
    };
    Ok((kind, sections))
}

fn part(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, OfficeError> {
    let file = zip
        .by_name(name)
        .map_err(|e| OfficeError::Corrupt(format!("{name}: {e}")))?;
    let mut out = String::new();
    file.take(MAX_PART_BYTES)
        .read_to_string(&mut out)
        .map_err(|e| OfficeError::Corrupt(format!("{name}: {e}")))?;
    Ok(out)
}

/// Parts `<prefix><n>.xml`, in number order.
fn numbered(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    prefix: &str,
) -> Result<Vec<(u32, String)>, OfficeError> {
    let mut found: Vec<(u32, String)> = zip
        .file_names()
        .filter_map(|n| {
            let n = n.ok()?;
            let num = n.strip_prefix(prefix)?.strip_suffix(".xml")?.parse().ok()?;
            Some((num, n.into_owned()))
        })
        .collect();
    found.sort();
    found
        .into_iter()
        .map(|(num, name)| Ok((num, part(zip, &name)?)))
        .collect()
}

/// A tag: its name, and whether it closes (`</x>`) or is empty (`<x/>`).
struct Tag<'a> {
    name: &'a str,
    closing: bool,
    empty: bool,
    attrs: &'a str,
}

/// A piece of XML: a tag, or the text between tags.
enum Token<'a> {
    Tag(Tag<'a>),
    Text(&'a str),
}

/// The tags and texts of `xml`, in order (declarations and comments
/// skipped).
fn tokens(xml: &str) -> impl Iterator<Item = Token<'_>> {
    let mut rest = xml;
    std::iter::from_fn(move || {
        loop {
            if rest.is_empty() {
                return None;
            }
            let Some(lt) = rest.find('<') else {
                let text = rest;
                rest = "";
                return Some(Token::Text(text));
            };
            if lt > 0 {
                let text = &rest[..lt];
                rest = &rest[lt..];
                return Some(Token::Text(text));
            }
            let Some(gt) = rest.find('>') else {
                rest = "";
                return None;
            };
            let inner = &rest[1..gt];
            rest = &rest[gt + 1..];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue;
            }
            let body = inner.trim_start_matches('/').trim_end_matches('/');
            let (name, attrs) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
            return Some(Token::Tag(Tag {
                name,
                closing: inner.starts_with('/'),
                empty: inner.ends_with('/'),
                attrs,
            }));
        }
    })
}

fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=semi]),
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Text inside `text_tag` elements, one line per `para_tag`; empty
/// paragraphs dropped.
fn paragraphs(xml: &str, text_tag: &str, para_tag: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut in_text = false;
    for token in tokens(xml) {
        match token {
            Token::Text(text) if in_text => line.push_str(&unescape(text)),
            Token::Text(_) => {}
            Token::Tag(tag) if tag.name == text_tag => in_text = !tag.closing && !tag.empty,
            Token::Tag(tag) if tag.name == para_tag && (tag.closing || tag.empty) => {
                let l = line.trim();
                if !l.is_empty() {
                    lines.push(l.to_owned());
                }
                line.clear();
            }
            Token::Tag(tag) if !tag.closing && matches!(tag.name, "w:tab" | "a:tab") => {
                line.push('\t')
            }
            Token::Tag(tag) if !tag.closing && matches!(tag.name, "w:br" | "a:br") => {
                line.push('\n')
            }
            Token::Tag(_) => {}
        }
    }
    let l = line.trim();
    if !l.is_empty() {
        lines.push(l.to_owned());
    }
    lines.join("\n")
}

fn shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_item = false;
    let mut in_text = false;
    for token in tokens(xml) {
        match token {
            Token::Text(text) if in_text => current.push_str(&unescape(text)),
            Token::Text(_) => {}
            Token::Tag(tag) => match tag.name {
                "si" if !tag.closing => {
                    in_item = true;
                    current.clear();
                }
                "si" => {
                    in_item = false;
                    out.push(std::mem::take(&mut current));
                }
                "t" => in_text = in_item && !tag.closing && !tag.empty,
                _ => {}
            },
        }
    }
    out
}

/// Rows of a sheet as tab-separated values; shared strings resolved.
fn sheet_rows(xml: &str, shared: &[String]) -> String {
    let mut rows: Vec<String> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut cell_type = String::new();
    let mut capture = false;
    for token in tokens(xml) {
        let tag = match token {
            Token::Text(text) => {
                if capture {
                    cell.push_str(&unescape(text));
                }
                continue;
            }
            Token::Tag(tag) => tag,
        };
        match tag.name {
            "c" if !tag.closing => {
                cell.clear();
                cell_type = attr(tag.attrs, "t").unwrap_or_default().to_owned();
                if tag.empty {
                    cells.push(String::new());
                }
            }
            "c" => {
                let value = if cell_type == "s" {
                    cell.trim()
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| shared.get(i).cloned())
                        .unwrap_or_default()
                } else {
                    cell.clone()
                };
                cells.push(value);
            }
            "v" | "t" => capture = !tag.closing && !tag.empty,
            "row" if tag.closing || tag.empty => {
                let row = cells.join("\t");
                if !row.trim().is_empty() {
                    rows.push(row.trim_end().to_owned());
                }
                cells.clear();
            }
            _ => {}
        }
    }
    rows.join("\n")
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let at = attrs.find(&format!("{name}=\""))? + name.len() + 2;
    let end = attrs[at..].find('"')?;
    Some(&attrs[at..at + end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn word_paragraphs() {
        let doc = package(&[(
            "word/document.xml",
            r#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Tea &amp; </w:t></w:r><w:r><w:t xml:space="preserve">cakes</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>Open daily</w:t><w:tab/><w:t>9&#8211;5</w:t></w:r></w:p></w:body></w:document>"#,
        )]);
        let (kind, sections) = extract("menu.docx", &doc).unwrap();
        assert_eq!(kind, OfficeKind::Document);
        assert_eq!(sections[0].text, "Tea & cakes\nOpen daily\t9–5");
    }

    #[test]
    fn slides_in_order() {
        let pptx = package(&[
            (
                "ppt/slides/slide10.xml",
                "<p:sld><a:p><a:r><a:t>Ten</a:t></a:r></a:p></p:sld>",
            ),
            (
                "ppt/slides/slide2.xml",
                "<p:sld><a:p><a:r><a:t>Two</a:t></a:r></a:p><a:p><a:r><a:t>more</a:t></a:r></a:p></p:sld>",
            ),
            ("ppt/slides/_rels/slide2.xml.rels", "<x/>"),
        ]);
        let (_, sections) = extract("deck.pptx", &pptx).unwrap();
        let got: Vec<(&str, &str)> = sections
            .iter()
            .map(|s| (s.title.as_str(), s.text.as_str()))
            .collect();
        assert_eq!(got, [("2", "Two\nmore"), ("10", "Ten")]);
    }

    #[test]
    fn sheets_resolve_shared_strings() {
        let xlsx = package(&[
            (
                "xl/sharedStrings.xml",
                "<sst><si><t>Item</t></si><si><r><t>Pri</t></r><r><t>ce</t></r></si><si><t>Green tea</t></si></sst>",
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>4.5</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Note</t></is></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let (kind, sections) = extract("prices.xlsx", &xlsx).unwrap();
        assert_eq!(kind, OfficeKind::Spreadsheet);
        assert_eq!(sections[0].text, "Item\tPrice\nGreen tea\t4.5\nNote");
    }

    #[test]
    fn other_files_are_refused() {
        assert!(matches!(
            extract("a.pdf", b"%PDF"),
            Err(OfficeError::Unsupported)
        ));
        assert!(matches!(
            extract("a.docx", b"not a zip"),
            Err(OfficeError::Corrupt(_))
        ));
        assert_eq!(unescape("a &unknown; &#x41;"), "a &unknown; A");
    }
}
