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

/// Makes one path segment filesystem-friendly: keeps Unicode letters and
/// digits, `.`, `_`, `-`; whitespace runs become `-`; the rest becomes `_`.
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
        if ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-') {
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
