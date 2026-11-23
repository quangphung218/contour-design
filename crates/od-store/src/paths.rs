//! Project-relative path validation and symlink-aware resolution.
//!
//! Every file API call funnels through [`resolve_safe_real`]: the name is
//! validated lexically, joined under the project root, canonicalized, and
//! re-checked against the canonical root so a symlink inside the project
//! cannot reach outside it.

use crate::error::{Result, StoreError};
use od_contracts::FileKind;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Internal folders the file API refuses to touch.
pub const RESERVED_SEGMENTS: [&str; 3] = [".file-versions", ".live-artifacts", ".open-design"];

/// Case-insensitive: macOS and Windows file systems usually are, so
/// `.OPEN-DESIGN` names the same folder as `.open-design`.
#[must_use]
pub fn is_reserved_segment(segment: &str) -> bool {
    RESERVED_SEGMENTS
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(segment))
}

/// Generated, installed or cache trees skipped by listings.
const IGNORED_DIR_NAMES: [&str; 29] = [
    ".git",
    "node_modules",
    "vendor",
    ".od",
    "debug",
    "dist",
    "build",
    ".build",
    "deriveddata",
    "target",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    ".output",
    "out",
    "coverage",
    ".gradle",
    ".swiftpm",
    ".tmp",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".ruff_cache",
    ".file-versions",
    ".live-artifacts",
];

#[must_use]
pub fn is_ignored_dir_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    IGNORED_DIR_NAMES.contains(&lower.as_str()) || lower.starts_with("deriveddata-")
}

/// Ids used as a single path segment under a managed directory.
#[must_use]
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.bytes().all(|b| b == b'.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Normalizes a client path to `a/b/c` form or rejects it.
///
/// Rejects empty names, NUL, absolute and drive-letter paths, `.`/`..`
/// segments, and reserved internal folders.
///
/// # Errors
/// [`StoreError::InvalidPath`] or [`StoreError::ReservedPath`].
pub fn validate_project_path(raw: &str) -> Result<String> {
    if raw.trim().is_empty() || raw.contains('\0') {
        return Err(StoreError::InvalidPath);
    }
    let normalized = raw.replace('\\', "/");
    let bytes = normalized.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if has_drive || normalized.starts_with('/') {
        return Err(StoreError::InvalidPath);
    }
    let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|p| *p == "." || *p == "..") {
        return Err(StoreError::InvalidPath);
    }
    if parts.iter().any(|p| is_reserved_segment(p)) {
        return Err(StoreError::ReservedPath);
    }
    Ok(parts.join("/"))
}

/// Imported folders point at the user's own directory, so hidden segments
/// (`.ssh`, `.aws`…) are off limits there.
///
/// # Errors
/// [`StoreError::HiddenSegment`] when `imported` and any segment starts with `.`.
pub fn assert_visible_for_import(name: &str, imported: bool) -> Result<()> {
    if imported
        && name
            .replace('\\', "/")
            .split('/')
            .any(|segment| segment.starts_with('.'))
    {
        return Err(StoreError::HiddenSegment);
    }
    Ok(())
}

/// A combining accent: macOS may send `mẫu` decomposed (NFD), its accents
/// as marks after the letter, which are not alphanumeric themselves.
fn is_combining_mark(ch: char) -> bool {
    matches!(ch,
        '\u{300}'..='\u{36f}'
        | '\u{1ab0}'..='\u{1aff}'
        | '\u{1dc0}'..='\u{1dff}'
        | '\u{20d0}'..='\u{20ff}'
        | '\u{fe20}'..='\u{fe2f}')
}

/// Makes one path segment filesystem-friendly: keeps Unicode letters,
/// digits and accents, `.`, `_`, `-`; whitespace runs become `-`; the rest
/// becomes `_`.
#[must_use]
pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push('-');
            }
            in_space = true;
            continue;
        }
        in_space = false;
        if ch.is_alphanumeric() || is_combining_mark(ch) || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let leading_dots = out.bytes().take_while(|b| *b == b'.').count();
    if leading_dots > 0 {
        out.replace_range(..leading_dots, "_");
    }
    if out.is_empty() {
        format!("file-{}", crate::now_ms())
    } else {
        out
    }
}

/// The longest name, in bytes, [`upload_name`] gives: under the 255 every
/// disk allows, with room for the `-9999` an upload adds to a taken name.
pub const MAX_UPLOAD_NAME_BYTES: usize = 200;

/// Longest extension [`upload_name`] keeps whole when it shortens a name.
const MAX_KEPT_EXTENSION_BYTES: usize = 16;

/// The name a file the user sent is stored under, when the app picks it
/// from the file's own name: [`sanitize_name`], then shortened to
/// [`MAX_UPLOAD_NAME_BYTES`], keeping its extension and cutting on a
/// character boundary, never between a letter and its combining accent.
/// A name the disk refuses would fail the upload every time (QA-106).
#[must_use]
pub fn upload_name(raw: &str) -> String {
    let name = sanitize_name(raw);
    if name.len() <= MAX_UPLOAD_NAME_BYTES {
        return name;
    }
    let ext = name
        .rfind('.')
        .filter(|&i| i > 0 && name.len() - i <= MAX_KEPT_EXTENSION_BYTES)
        .map_or("", |i| &name[i..]);
    let stem = &name[..name.len() - ext.len()];
    let mut cut = MAX_UPLOAD_NAME_BYTES - ext.len();
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    // A decomposed accent (NFD, as macOS may send) stays with its letter.
    while cut > 0 && stem[cut..].chars().next().is_some_and(is_combining_mark) {
        cut = stem[..cut].char_indices().next_back().map_or(0, |(i, _)| i);
    }
    let stem = stem[..cut].trim_end_matches(['-', '_', '.']);
    let stem = if stem.is_empty() { "file" } else { stem };
    format!("{stem}{ext}")
}

/// [`validate_project_path`] then [`sanitize_name`] on every segment.
///
/// # Errors
/// Same as [`validate_project_path`].
pub fn sanitize_path(raw: &str) -> Result<String> {
    let normalized = validate_project_path(raw)?;
    Ok(normalized
        .split('/')
        .map(sanitize_name)
        .collect::<Vec<_>>()
        .join("/"))
}

pub use od_contracts::files::name_key;

/// The entry of folder `dir` that `name` names on a disk that ignores case
/// and Unicode form: the one called exactly so, else the one equal after
/// NFC, else the first (sorted) of the same [`name_key`]. On Linux,
/// `Logo.png` and `logo.png` can both be there already; the closest wins.
/// `None` when nothing there has that key, or `dir` cannot be read.
#[must_use]
pub fn entry_named(dir: &Path, name: &str) -> Option<String> {
    use unicode_normalization::UnicodeNormalization as _;
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    let nfc = |n: &str| n.nfc().collect::<String>();
    let key = name_key(name);
    names
        .iter()
        .find(|n| *n == name)
        .or_else(|| names.iter().find(|n| nfc(n) == nfc(name)))
        .or_else(|| names.iter().find(|n| name_key(n) == key))
        .cloned()
}

/// `rel` (validated) with each segment that names an entry already under
/// `root` spelled as that entry is ([`entry_named`]), so every disk takes
/// `Logo.png` for `logo.png` and `mẫu` NFD for NFC, as macOS and Windows
/// do; segments past the first that is not there are kept as given.
#[must_use]
pub fn as_on_disk(root: &Path, rel: &str) -> String {
    let mut dir = Some(root.to_path_buf());
    rel.split('/')
        .map(|seg| {
            let found = dir.as_deref().and_then(|d| entry_named(d, seg));
            let seg = found.unwrap_or_else(|| seg.to_owned());
            dir = dir
                .take()
                .map(|d| d.join(&seg))
                .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()));
            seg
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Resolves `name` under `root`, following symlinks, and proves the result
/// stays inside the canonical root. Works for paths that do not exist yet.
///
/// # Errors
/// Validation errors, [`StoreError::PathEscape`] when the real path leaves
/// the root (including through a dangling symlink), or I/O errors.
pub fn resolve_safe_real(root: &Path, name: &str) -> Result<PathBuf> {
    let rel = validate_project_path(name)?;
    let candidate = root.join(&rel);
    let root_real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let real = match std::fs::canonicalize(&candidate) {
        Ok(real) => real,
        Err(err) if err.kind() == io::ErrorKind::NotFound => resolve_existing_prefix(&candidate)?,
        Err(err) => return Err(err.into()),
    };
    let Ok(inside) = real.strip_prefix(&root_real) else {
        return Err(StoreError::PathEscape);
    };
    // Check reserved names again on the real path: case-insensitive file
    // systems fold more than ASCII (APFS maps `ſ` to `s` and `ﬁ` to `fi`), so
    // `.open-deſign` passes the lexical check yet names `.open-design`.
    // `canonicalize` returns the stored name for every existing component;
    // the missing tail was already checked lexically above.
    if inside.components().any(|c| match c {
        Component::Normal(part) => is_reserved_segment(&part.to_string_lossy()),
        _ => false,
    }) {
        return Err(StoreError::ReservedPath);
    }
    Ok(real)
}

/// Whether `rel` under `dir` goes through a symlink: is one, or is in a
/// folder that is one. What is written, deleted or moved at such a path
/// would change the file or folder the link points to.
#[must_use]
pub fn through_link(dir: &Path, rel: &str) -> bool {
    let mut path = dir.to_path_buf();
    rel.split('/').any(|part| {
        path.push(part);
        std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_symlink())
    })
}

/// Canonicalizes the longest existing prefix of `path` and re-appends the
/// missing tail. A missing component that is itself a (dangling) symlink is
/// rejected: writing through it would land wherever it points.
fn resolve_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut current = path;
    loop {
        match std::fs::canonicalize(current) {
            Ok(real) => {
                let mut out = real;
                for part in tail.iter().rev() {
                    out.push(part);
                }
                return Ok(out);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                match std::fs::symlink_metadata(current) {
                    // A dangling symlink: writing through it would escape.
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(StoreError::PathEscape);
                    }
                    // Created by a concurrent writer since `canonicalize`
                    // failed: resolve it again.
                    Ok(_) => continue,
                    Err(_) => {}
                }
                match (current.parent(), current.components().next_back()) {
                    (Some(parent), Some(Component::Normal(part))) => {
                        tail.push(part);
                        current = parent;
                    }
                    _ => return Ok(path.to_path_buf()),
                }
            }
            Err(err) => return Err(err.into()),
        }
    }
}

/// Converts an absolute path under `root` to `a/b/c` form.
#[must_use]
pub fn to_project_path(root: &Path, full: &Path) -> String {
    full.strip_prefix(root)
        .unwrap_or(full)
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn extension_lower(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Content-Type for a project file, by extension.
#[must_use]
pub fn mime_for(name: &str) -> &'static str {
    match extension_lower(name).as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" | "jsx" | "tsx" => "text/javascript; charset=utf-8",
        "ts" => "text/typescript; charset=utf-8",
        "py" => "text/x-python; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

/// Viewer bucket for a project file, by extension.
#[must_use]
pub fn kind_for(name: &str) -> FileKind {
    if name.ends_with(".sketch.json") {
        return FileKind::Sketch;
    }
    match extension_lower(name).as_str() {
        "html" | "htm" => FileKind::Html,
        "svg" => FileKind::Sketch,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" => {
            if name.starts_with("sketch-") {
                FileKind::Sketch
            } else {
                FileKind::Image
            }
        }
        "mp4" | "mov" | "webm" => FileKind::Video,
        "mp3" | "wav" | "m4a" => FileKind::Audio,
        "md" | "txt" => FileKind::Text,
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "json" | "css" | "py" => FileKind::Code,
        "pdf" => FileKind::Pdf,
        "docx" => FileKind::Document,
        "pptx" => FileKind::Presentation,
        "xlsx" => FileKind::Spreadsheet,
        _ => FileKind::Binary,
    }
}

#[must_use]
pub fn is_html_name(name: &str) -> bool {
    matches!(extension_lower(name).as_str(), "html" | "htm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_traversal_and_reserved() {
        for bad in [
            "",
            "  ",
            "../x",
            "a/../../x",
            "/etc/passwd",
            "C:/win",
            "a/./b",
            "a\0b",
            "..\\x",
        ] {
            assert!(
                validate_project_path(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
        assert!(matches!(
            validate_project_path(".file-versions/x/manifest.json"),
            Err(StoreError::ReservedPath)
        ));
        assert!(matches!(
            validate_project_path("a/.open-design/project.json"),
            Err(StoreError::ReservedPath)
        ));
        for mixed in [
            ".OPEN-DESIGN/project.json",
            ".File-Versions/x/manifest.json",
            "a/.Live-Artifacts/x",
        ] {
            assert!(
                matches!(validate_project_path(mixed), Err(StoreError::ReservedPath)),
                "{mixed} must be reserved"
            );
        }
        assert_eq!(validate_project_path("a//b\\c.html").unwrap(), "a/b/c.html");
    }

    #[test]
    fn sanitize_keeps_unicode_and_replaces_punctuation() {
        assert_eq!(sanitize_name("测试 文档.docx"), "测试-文档.docx");
        assert_eq!(sanitize_name("..hidden"), "_hidden");
        assert_eq!(sanitize_name("a<b>c"), "a_b_c");
        // Decomposed (NFD) Vietnamese keeps its accents.
        assert_eq!(
            sanitize_name("ma\u{302}\u{303}u.png"),
            "ma\u{302}\u{303}u.png"
        );
    }

    /// QA-106: a name longer than the disk allows is shortened, keeping
    /// its extension and whole letters; a short one is left as it is.
    #[test]
    fn upload_names_fit_the_disk() {
        assert_eq!(upload_name("ảnh chụp.png"), "ảnh-chụp.png");
        let fit = format!("{}.png", "a".repeat(MAX_UPLOAD_NAME_BYTES - 4));
        assert_eq!(upload_name(&fit), fit);
        for raw in [
            format!("{}.png", "anh-chup-man-hinh-".repeat(17)),
            format!("{}.png", "ảnh chụp màn hình ".repeat(17)),
        ] {
            let name = upload_name(&raw);
            assert!(name.len() <= MAX_UPLOAD_NAME_BYTES, "{} bytes", name.len());
            assert!(
                name.len() > MAX_UPLOAD_NAME_BYTES - 8,
                "cut too much: {name}"
            );
            assert!(name.ends_with(".png"), "{name}");
            assert!(
                !name.trim_end_matches(".png").ends_with(['-', '.']),
                "{name}"
            );
        }
        // Decomposed accents (NFD): the limit falls inside `ẫ` (a + two
        // marks, 5 bytes), which goes whole.
        let nfd = upload_name(&format!("x{}.jpeg", "a\u{302}\u{303}".repeat(120)));
        assert!(nfd.len() <= MAX_UPLOAD_NAME_BYTES, "{} bytes", nfd.len());
        let stem = nfd.trim_end_matches(".jpeg");
        assert!(
            stem.ends_with("a\u{302}\u{303}"),
            "accent cut from its letter: {stem:?}"
        );
        // An "extension" too long to be one is cut like the rest.
        let odd = upload_name(&format!("a.{}", "b".repeat(300)));
        assert_eq!(odd.len(), MAX_UPLOAD_NAME_BYTES);
        // Nothing left of the stem: still a name.
        assert_eq!(
            upload_name(&format!("{}.png", "\u{301}".repeat(150))),
            "file.png"
        );
    }

    #[test]
    fn safe_id_rules() {
        assert!(is_safe_id("abc-1.2_x"));
        assert!(!is_safe_id(".."));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id(""));
    }

    #[test]
    fn kind_and_mime() {
        assert_eq!(kind_for("index.html"), FileKind::Html);
        assert_eq!(kind_for("a.sketch.json"), FileKind::Sketch);
        assert_eq!(kind_for("sketch-1.png"), FileKind::Sketch);
        assert_eq!(mime_for("x.tsx"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("x.bin"), "application/octet-stream");
    }
}
