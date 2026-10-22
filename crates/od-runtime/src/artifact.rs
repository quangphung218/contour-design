//! `<artifact>` blocks in plain-text answers become project files (spec
//! 2.3 "plain"; ports `runtimes/plain-stream.ts`).
//!
//! A block counts only when its open tag is a real tag (`<artifact` then
//! whitespace) outside Markdown code (fences and inline backticks), and
//! its type is HTML, CSS, SVG or Markdown. An untyped block counts when it
//! looks like an HTML document.

use std::path::{Path, PathBuf};

const OPEN_TAG: &str = "<artifact";
const CLOSE_TAG: &str = "</artifact>";
const MAX_ARTIFACTS_PER_RUN: usize = 50;

/// One block, ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub identifier: String,
    pub title: String,
    pub artifact_type: String,
    pub content: String,
    /// `<identifier or title, slugged>.<ext>`; unique within one answer.
    pub file_name: String,
}

fn extension(artifact_type: &str) -> Option<&'static str> {
    Some(match artifact_type {
        "html" | "text/html" => "html",
        "css" | "text/css" => "css",
        "svg" | "image/svg+xml" => "svg",
        "markdown" | "md" | "text/markdown" | "text/x-markdown" => "md",
        _ => return None,
    })
}

fn looks_like_html(content: &str) -> bool {
    let start = content.trim_start().to_ascii_lowercase();
    start.starts_with("<!doctype html") || start.starts_with("<html")
}

/// `name="v"` / `name='v'` pairs of an open tag.
fn attrs(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || b"_:.-".contains(&bytes[i])) {
            i += 1;
        }
        let name = &raw[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if name.is_empty() || i >= bytes.len() || bytes[i] != b'=' {
            i = i.max(start + 1);
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let Some(&quote) = bytes.get(i).filter(|q| **q == b'"' || **q == b'\'') else {
            continue;
        };
        let value_start = i + 1;
        let Some(len) = raw[value_start..].find(char::from(quote)) else {
            break;
        };
        out.push((
            name.to_owned(),
            raw[value_start..value_start + len].to_owned(),
        ));
        i = value_start + len + 1;
    }
    out
}

/// Byte ranges of fenced code blocks and inline code spans.
fn code_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut fence_start: Option<usize> = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let has_newline = line.ends_with('\n');
        let trimmed = line.trim_end();
        let is_fence = trimmed.starts_with("```")
            && trimmed[3..]
                .chars()
                .all(|c| c.is_alphanumeric() || "+-_".contains(c));
        match fence_start {
            None if is_fence && has_newline => fence_start = Some(pos),
            Some(start) if trimmed == "```" && has_newline => {
                ranges.push((start, pos + line.len()));
                fence_start = None;
            }
            None => {
                // Inline code: matching backtick runs within the line.
                let mut i = 0;
                let b = line.as_bytes();
                while i < b.len() {
                    if b[i] != b'`' {
                        i += 1;
                        continue;
                    }
                    let run = b[i..].iter().take_while(|c| **c == b'`').count();
                    let ticks = "`".repeat(run);
                    let after = i + run;
                    match line[after..].find(&ticks) {
                        Some(off) => {
                            ranges.push((pos + i, pos + after + off + run));
                            i = after + off + run;
                        }
                        None => i = after,
                    }
                }
            }
            Some(_) => {}
        }
        pos += line.len();
    }
    if let Some(start) = fence_start {
        ranges.push((start, text.len()));
    }
    ranges
}

fn slug(identifier: &str, title: &str) -> String {
    let source = if identifier.is_empty() {
        title
    } else {
        identifier
    };
    let mut out = String::new();
    for c in source.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "artifact".to_owned()
    } else {
        out.to_owned()
    }
}

/// Every well-formed block of `text`, in order.
#[must_use]
pub fn extract(text: &str) -> Vec<Artifact> {
    if !text.contains(OPEN_TAG) {
        return Vec::new();
    }
    let skip = code_ranges(text);
    let in_code = |i: usize| skip.iter().any(|(s, e)| i >= *s && i < *e);
    let real_open = |i: usize| {
        text[i + OPEN_TAG.len()..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
            && !in_code(i)
    };
    let next_open = |from: usize| {
        let mut cur = from;
        while let Some(off) = text.get(cur..).and_then(|t| t.find(OPEN_TAG)) {
            let i = cur + off;
            if real_open(i) {
                return Some(i);
            }
            cur = i + OPEN_TAG.len();
        }
        None
    };
    let mut out: Vec<Artifact> = Vec::new();
    let mut from = 0;
    while out.len() < MAX_ARTIFACTS_PER_RUN {
        let Some(open) = next_open(from) else { break };
        let after_tag = open + OPEN_TAG.len();
        // End of the open tag, skipping quoted `>`.
        let mut quote = None;
        let mut open_end = None;
        for (i, c) in text[after_tag..].char_indices() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => {
                    open_end = Some(after_tag + i + 1);
                    break;
                }
                _ => {}
            }
        }
        let following = next_open(after_tag);
        let Some(open_end) = open_end else {
            from = after_tag;
            continue;
        };
        if following.is_some_and(|n| n < open_end) {
            from = following.unwrap_or(after_tag);
            continue;
        }
        let Some(close) = text[open_end..].find(CLOSE_TAG).map(|o| open_end + o) else {
            from = after_tag;
            continue;
        };
        if let Some(n) = following.filter(|n| *n < close) {
            from = n;
            continue;
        }
        let attrs = attrs(&text[after_tag..open_end - 1]);
        let get = |k: &str| {
            attrs
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let content = text[open_end..close].to_owned();
        let declared = get("type").trim().to_lowercase();
        let artifact_type = if extension(&declared).is_some() {
            Some(declared)
        } else if declared.is_empty() && looks_like_html(&content) {
            Some("text/html".to_owned())
        } else {
            None
        };
        if let Some(artifact_type) = artifact_type {
            let identifier = get("identifier");
            let title = get("title");
            let ext = extension(&artifact_type).unwrap_or("html");
            let base = slug(&identifier, &title);
            let mut file_name = format!("{base}.{ext}");
            let mut n = 2;
            while out.iter().any(|a| a.file_name == file_name) {
                file_name = format!("{base}-{n}.{ext}");
                n += 1;
            }
            out.push(Artifact {
                identifier,
                title,
                artifact_type,
                content,
                file_name,
            });
        }
        from = close + CLOSE_TAG.len();
    }
    out
}

/// Writes each artifact into `dir`, replacing a file of the same name (the
/// next turn's `index.html` updates the preview). Returns the paths
/// written.
///
/// # Errors
/// The first write error.
pub fn persist(dir: &Path, artifacts: &[Artifact]) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::with_capacity(artifacts.len());
    for a in artifacts {
        let path = dir.join(&a.file_name);
        write_no_follow(&path, a.content.as_bytes())?;
        written.push(path);
    }
    Ok(written)
}

/// Writes `path` but refuses a symlink there: an imported folder (or an
/// earlier run) may have linked `index.html` to a file outside the project.
fn write_no_follow(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // The open itself fails (ELOOP) on a symlink: no check-then-open gap.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is a symlink", path.display()),
        ));
    }
    options.open(path)?.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn persist_never_writes_through_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, "keep me").unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::os::unix::fs::symlink(&outside, project.join("index.html")).unwrap();
        let artifacts =
            extract("<artifact identifier=\"index\" type=\"text/html\"><p>x</p></artifact>");
        assert_eq!(artifacts[0].file_name, "index.html");
        assert!(persist(&project, &artifacts).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep me");
    }

    #[test]
    fn extracts_typed_and_html_blocks() {
        let text = "Here you go.\n<artifact identifier=\"index\" type=\"text/html\" title=\"Home\">\
                    <!doctype html><p>a > b</p></artifact>\n\
                    <artifact title='Brand Notes' type=\"markdown\">\n# Notes\n</artifact>\n\
                    <artifact title=\"x\">\n<html></html></artifact>";
        let got = extract(text);
        let names: Vec<&str> = got.iter().map(|a| a.file_name.as_str()).collect();
        assert_eq!(names, ["index.html", "brand-notes.md", "x.html"]);
        assert_eq!(got[0].content, "<!doctype html><p>a > b</p>");
    }

    #[test]
    fn ignores_code_unknown_types_and_fake_tags() {
        let text = "```html\n<artifact type=\"html\">no</artifact>\n```\n\
                    Use `<artifact type=\"html\">x</artifact>` like this.\n\
                    <artifacts>no</artifacts>\n\
                    <artifact type=\"application/json\">{}</artifact>\n\
                    <artifact type=\"html\">unterminated";
        assert!(extract(text).is_empty());
    }

    #[test]
    fn duplicate_names_get_a_suffix_and_slugs_are_safe() {
        let text = "<artifact identifier=\"../../etc/passwd\" type=\"css\">a</artifact>\
                    <artifact identifier=\"../../etc/passwd\" type=\"css\">b</artifact>";
        let got = extract(text);
        assert_eq!(got[0].file_name, "etc-passwd.css");
        assert_eq!(got[1].file_name, "etc-passwd-2.css");
    }
}
