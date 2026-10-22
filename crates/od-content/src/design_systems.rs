//! Design-system packages: `<root>/<id>/DESIGN.md` plus optional
//! `manifest.json` and `metadata.json`. Title, category, summary, surface
//! and swatches follow the original fallback chains (spec 3.5).

use crate::frontmatter;
use crate::text::sorted_dir_entries;
use od_contracts::{ContentSource, DesignSystemStatus, DesignSystemSummary, Surface};
use rayon::prelude::*;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Id prefix for user design systems, so they never collide with bundled ids.
pub const USER_ID_PREFIX: &str = "user:";

const MANIFEST_SCHEMA: &str = "od-design-system-project/v1";

/// A loaded package: summary, raw `DESIGN.md`, validated manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignSystemEntry {
    pub summary: DesignSystemSummary,
    pub body: String,
    pub manifest: Option<Value>,
    pub dir: PathBuf,
}

/// How a root is listed.
#[derive(Debug, Clone, Copy)]
pub struct ListOptions<'a> {
    pub id_prefix: &'a str,
    pub source: ContentSource,
    pub is_editable: bool,
    pub default_status: DesignSystemStatus,
}

impl ListOptions<'_> {
    pub const BUILT_IN: Self = Self {
        id_prefix: "",
        source: ContentSource::BuiltIn,
        is_editable: false,
        default_status: DesignSystemStatus::Published,
    };
    pub const USER: ListOptions<'static> = ListOptions {
        id_prefix: USER_ID_PREFIX,
        source: ContentSource::User,
        is_editable: true,
        default_status: DesignSystemStatus::Draft,
    };
}

/// Lists every package folder with a `DESIGN.md`, sorted by folder name.
#[must_use]
pub fn list_design_systems(root: &Path, opts: ListOptions<'_>) -> Vec<DesignSystemEntry> {
    sorted_dir_entries(root)
        .par_iter()
        .filter_map(|(name, dir)| load_one(name, dir, opts))
        .collect()
}

/// Valid package ids: one safe path segment.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[derive(Debug, Default)]
struct UserMetadata {
    title: Option<String>,
    category: Option<String>,
    surface: Option<Surface>,
    status: Option<DesignSystemStatus>,
}

fn read_user_metadata(dir: &Path) -> UserMetadata {
    let Some(Value::Object(m)) = std::fs::read_to_string(dir.join("metadata.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    else {
        return UserMetadata::default();
    };
    let s = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_owned);
    UserMetadata {
        title: s("title"),
        category: s("category"),
        surface: m
            .get("surface")
            .and_then(Value::as_str)
            .and_then(Surface::parse),
        status: match m.get("status").and_then(Value::as_str) {
            Some("draft") => Some(DesignSystemStatus::Draft),
            Some("published") => Some(DesignSystemStatus::Published),
            _ => None,
        },
    }
}

/// `manifest.json` when it matches the v1 package schema for `expected_id`.
fn read_manifest(dir: &Path, expected_id: &str) -> Option<Value> {
    let raw = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let m = value.as_object()?;
    let non_empty = |k: &str| {
        m.get(k)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    let files = m.get("files")?.as_object()?;
    let file_is = |k: &str, want: &str| files.get(k).is_none_or(|v| v.as_str() == Some(want));
    let ok = m.get("schemaVersion").and_then(Value::as_str) == Some(MANIFEST_SCHEMA)
        && m.get("id").and_then(Value::as_str) == Some(expected_id)
        && non_empty("name")
        && non_empty("category")
        && m.get("description").is_none_or(Value::is_string)
        && files.get("design").and_then(Value::as_str) == Some("DESIGN.md")
        && files.get("tokens").and_then(Value::as_str) == Some("tokens.css")
        && file_is("designTokens", "design-tokens.json")
        && file_is("tailwind", "tailwind-v4.css")
        && file_is("components", "components.html");
    ok.then_some(value)
}

fn load_one(name: &str, dir: &Path, opts: ListOptions<'_>) -> Option<DesignSystemEntry> {
    let manifest = read_manifest(dir, name);
    let design_path = dir.join("DESIGN.md");
    if !std::fs::metadata(&design_path).ok()?.is_file() {
        return None;
    }
    let raw = std::fs::read_to_string(&design_path).ok()?;
    let metadata = read_user_metadata(dir);
    let (front, body) = frontmatter::parse(&raw).ok()?;
    let mf = |k: &str| {
        manifest
            .as_ref()
            .and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };

    let markdown_title = H1_RE
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| clean_title(m.as_str()))
        .unwrap_or_default();
    let fallback_title = [markdown_title, string_field(&front, "name")]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| name.to_owned());
    let title = clean_title(
        &metadata
            .title
            .clone()
            .or_else(|| mf("name"))
            .unwrap_or(fallback_title),
    );
    let category = metadata
        .category
        .clone()
        .or_else(|| mf("category"))
        .or_else(|| extract_category(body))
        .unwrap_or_else(|| string_field(&front, "category"));
    let category = if category.is_empty() {
        "Uncategorized".to_owned()
    } else {
        category
    };
    let summary = [
        mf("description")
            .map(|d| d.trim().to_owned())
            .unwrap_or_default(),
        summarize(body),
        string_field(&front, "description"),
    ]
    .into_iter()
    .find(|s| !s.is_empty())
    .unwrap_or_default();
    let swatches = pick_final_swatch_row(swatches_from_frontmatter(&front), extract_swatches(body));
    let surface = metadata
        .surface
        .or_else(|| extract_surface(body))
        .or_else(|| Surface::parse(&string_field(&front, "surface").to_lowercase()))
        .unwrap_or(Surface::Web);

    Some(DesignSystemEntry {
        summary: DesignSystemSummary {
            id: format!("{}{name}", opts.id_prefix),
            title,
            category,
            summary,
            swatches,
            surface,
            source: opts.source,
            status: metadata.status.unwrap_or(opts.default_status),
            is_editable: opts.is_editable,
        },
        body: raw,
        manifest,
        dir: dir.to_path_buf(),
    })
}

fn string_field(data: &Map<String, Value>, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

static H1_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#\s+(.+?)\s*$").expect("static regex"));
static CLEAN_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Design System (Inspired by|for)\s+").expect("static regex"));
static CATEGORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^>\s*Category:\s*(.+?)\s*$").expect("static regex"));
static SURFACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^>\s*Surface:\s*(.+?)\s*$").expect("static regex"));

/// Drops "Design System Inspired by" / "Design System for" boilerplate.
fn clean_title(raw: &str) -> String {
    CLEAN_TITLE_RE.replace(raw, "").trim().to_owned()
}

fn extract_category(body: &str) -> Option<String> {
    CATEGORY_RE
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

fn extract_surface(body: &str) -> Option<Surface> {
    let v = SURFACE_RE
        .captures(body)?
        .get(1)?
        .as_str()
        .trim()
        .to_lowercase();
    Surface::parse(&v)
}

static ANY_HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,6}\s+").expect("static regex"));
static H1_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#\s+").expect("static regex"));
static SUMMARY_STRIP_RES: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        Regex::new(r"(?im)^>\s*Category:.*$").expect("static regex"),
        Regex::new(r"(?im)^>\s*Surface:.*$").expect("static regex"),
        Regex::new(r"(?m)^>\s*").expect("static regex"),
    ]
});

/// First paragraph under the first H1, without blockquote metadata, at most
/// 240 characters.
fn summarize(body: &str) -> String {
    let lines: Vec<&str> = body
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let Some(first_h1) = lines.iter().position(|l| H1_LINE_RE.is_match(l)) else {
        return String::new();
    };
    let after = &lines[first_h1 + 1..];
    let end = after
        .iter()
        .position(|l| ANY_HEADING_RE.is_match(l))
        .unwrap_or(after.len());
    let mut window = after[..end].join("\n");
    for re in SUMMARY_STRIP_RES.iter() {
        window = re.replace_all(&window, "").into_owned();
    }
    let window = window.trim();
    window
        .split("\n\n")
        .next()
        .unwrap_or_default()
        .chars()
        .take(240)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColorToken {
    name: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SwatchRow {
    values: Vec<String>,
    filled_all_slots: bool,
}

/// `#rgb`, `#rgba`, `#rrggbb`… lowercased; 3 and 4 digit forms expanded.
fn normalize_hex(raw: &str) -> Option<String> {
    let hex = raw.trim().strip_prefix('#')?;
    if !(3..=8).contains(&hex.len()) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let expanded: String = if hex.len() == 3 || hex.len() == 4 {
        hex.chars().flat_map(|c| [c, c]).take(8).collect()
    } else {
        hex.to_owned()
    };
    Some(format!("#{}", expanded.to_lowercase()))
}

fn swatches_from_frontmatter(data: &Map<String, Value>) -> Option<SwatchRow> {
    let colors_map = data.get("colors")?.as_object()?;
    let mut seen = HashSet::new();
    let colors: Vec<ColorToken> = colors_map
        .iter()
        .filter_map(|(name, value)| {
            let hex = normalize_hex(value.as_str()?)?;
            let clean = WS_RE.replace_all(name, " ").trim().to_lowercase();
            seen.insert(format!("{clean}|{hex}")).then_some(ColorToken {
                name: clean,
                value: hex,
            })
        })
        .collect();
    (!colors.is_empty()).then(|| pick_swatch_row(&colors))
}

fn pick_final_swatch_row(front: Option<SwatchRow>, markdown: Vec<String>) -> Vec<String> {
    match front {
        Some(row) if row.filled_all_slots => row.values,
        _ if !markdown.is_empty() => markdown,
        Some(row) => row.values,
        None => Vec::new(),
    }
}

static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("static regex"));
static MARKS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*_`]+").expect("static regex"));
static FORM_A: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[\s>*-]*([A-Za-z][A-Za-z0-9 /&()+_-]{1,40}?)\s*[:：]?\s*\**\s*[:：]?\s*`?(#[0-9a-fA-F]{3,8})")
        .expect("static regex")
});
static FORM_B: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\*\*([A-Za-z][A-Za-z0-9 /&()+_-]{1,40}?)\*\*\s*\(?\s*`?(#[0-9a-fA-F]{3,8})")
        .expect("static regex")
});
static FORM_C: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*\|(.+)\|[ \t]*$").expect("static regex"));
static HEX_WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#[0-9a-fA-F]{3,8}(?-u:\b)").expect("static regex"));
static HEX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#[0-9a-fA-F]{3,8}").expect("static regex"));
static SEPARATOR_CELL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[-:\s]+$").expect("static regex"));

/// Four representative colors `[bg, support, fg, accent]` from prose:
/// `- **Name:** #hex` lines, `**Name** (#hex)`, and table rows. (The
/// original's SwiftUI `Color(...)` form is not ported: no bundled
/// `DESIGN.md` uses it.)
fn extract_swatches(body: &str) -> Vec<String> {
    let mut colors: Vec<ColorToken> = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |name: &str, value: &str| {
        let stripped = MARKS_RE.replace_all(name, "");
        let clean = WS_RE.replace_all(&stripped, " ").trim().to_lowercase();
        let Some(hex) = normalize_hex(value) else {
            return;
        };
        if clean.chars().count() > 60 || !seen.insert(format!("{clean}|{hex}")) {
            return;
        }
        colors.push(ColorToken {
            name: clean,
            value: hex,
        });
    };
    for re in [&*FORM_A, &*FORM_B] {
        for c in re.captures_iter(body) {
            push(
                c.get(1).map_or("", |m| m.as_str()),
                c.get(2).map_or("", |m| m.as_str()),
            );
        }
    }
    for c in FORM_C.captures_iter(body) {
        let cells: Vec<&str> = c
            .get(1)
            .map_or("", |m| m.as_str())
            .split('|')
            .map(str::trim)
            .collect();
        let Some(hex_cell) = cells.iter().find(|cell| HEX_WORD_RE.is_match(cell)) else {
            continue;
        };
        let hex = HEX_RE.find(hex_cell).map_or("", |m| m.as_str());
        let name = cells
            .iter()
            .find(|cell| {
                !cell.is_empty() && !HEX_RE.is_match(cell) && !SEPARATOR_CELL_RE.is_match(cell)
            })
            .copied()
            .unwrap_or("");
        push(name, hex);
    }
    if colors.is_empty() {
        return Vec::new();
    }
    pick_swatch_row(&colors).values
}

fn is_neutral(hex: &str) -> bool {
    let Some(digits) = hex.strip_prefix('#') else {
        return false;
    };
    if digits.len() != 6
        || !digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return false;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).unwrap_or(0);
    let (r, g, b) = (channel(0), channel(2), channel(4));
    r.max(g).max(b) - r.min(g).min(b) < 10
}

fn pick_swatch_row(colors: &[ColorToken]) -> SwatchRow {
    let pick = |hints: &[&str]| -> Option<String> {
        hints.iter().find_map(|h| {
            colors
                .iter()
                .find(|c| c.name.contains(h))
                .map(|c| c.value.clone())
        })
    };
    let bg_hit = pick(&[
        "page background",
        "background",
        "canvas",
        "paper",
        "surface",
    ]);
    let fg_hit = pick(&[
        "heading",
        "foreground",
        "ink",
        "fg",
        "text",
        "navy",
        "graphite",
    ]);
    let accent_hit = pick(&[
        "primary brand",
        "brand primary",
        "accent",
        "brand",
        "primary",
    ]);
    let support_hit = pick(&["border", "divider", "rule", "muted", "secondary", "subtle"]);
    let filled_all_slots =
        bg_hit.is_some() && fg_hit.is_some() && accent_hit.is_some() && support_hit.is_some();

    let bg = bg_hit.unwrap_or_else(|| "#ffffff".to_owned());
    let fg = fg_hit.unwrap_or_else(|| "#111111".to_owned());
    let accent = accent_hit
        .or_else(|| {
            colors
                .iter()
                .find(|c| !is_neutral(&c.value))
                .map(|c| c.value.clone())
        })
        .or_else(|| colors.first().map(|c| c.value.clone()))
        .unwrap_or_else(|| "#888888".to_owned());
    let support = support_hit
        .or_else(|| {
            colors
                .iter()
                .find(|c| is_neutral(&c.value) && c.value != bg && c.value != fg)
                .map(|c| c.value.clone())
        })
        .unwrap_or_else(|| "#cccccc".to_owned());
    SwatchRow {
        values: vec![bg, support, fg, accent],
        filled_all_slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_normalization() {
        assert_eq!(normalize_hex("#ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex(" #abcd ").as_deref(), Some("#aabbccdd"));
        assert_eq!(normalize_hex("#12345").as_deref(), Some("#12345"));
        assert_eq!(normalize_hex("#xyz"), None);
    }

    #[test]
    fn swatches_from_prose_and_tables() {
        let md = "# T\n\n- **Background:** `#FAFAFA`\n- **Text**: #111\n**Brand Purple** (`#533afd`)\n| Border | `--b` | `#e5e5e5` |\n|---|---|---|\n";
        assert_eq!(
            extract_swatches(md),
            ["#fafafa", "#e5e5e5", "#111111", "#533afd"]
        );
    }

    #[test]
    fn title_summary_category() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("acme");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("DESIGN.md"),
            "# Design System Inspired by Acme\n\n> Category: Fintech\n> Surface: web\n\nCalm and precise.\n\nMore.\n\n## Colors\n",
        )
        .unwrap();
        let list = list_design_systems(tmp.path(), ListOptions::BUILT_IN);
        assert_eq!(list.len(), 1);
        let s = &list[0].summary;
        assert_eq!((s.id.as_str(), s.title.as_str()), ("acme", "Acme"));
        assert_eq!(s.category, "Fintech");
        assert_eq!(s.summary, "Calm and precise.");
        assert_eq!(s.surface, Surface::Web);
    }
}
