//! Creating a design system from a brief (spec 1.4 "Tạo design system"):
//! an agent writes `DESIGN.md` and `tokens.css` into a staging folder,
//! then [`install`] checks them, adds `manifest.json` / `metadata.json`
//! and moves the package into the user root (`<data>/design-systems`),
//! where the index lists it as `user:<id>` next to the bundled ones.

use crate::design_systems::{USER_ID_PREFIX, is_valid_id};
use serde_json::json;
use std::path::{Path, PathBuf};

/// H2 sections the agent is asked to write (spec 3.5: at least seven).
pub const SECTIONS: [&str; 9] = [
    "Visual Theme & Atmosphere",
    "Color",
    "Typography",
    "Spacing & Grid",
    "Layout & Composition",
    "Components",
    "Motion & Interaction",
    "Voice & Brand",
    "Anti-patterns",
];
pub const MIN_SECTIONS: usize = 7;

/// A1 tokens `tokens.css` must define (spec 3.5 token layers).
pub const REQUIRED_TOKENS: [&str; 6] = [
    "--bg",
    "--surface",
    "--fg",
    "--accent",
    "--font-display",
    "--font-body",
];

const MAX_NAME_CHARS: usize = 80;
const MAX_BRIEF_CHARS: usize = 8_000;

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateRequest {
    pub name: String,
    pub brief: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GenError {
    #[error("{0}")]
    BadRequest(String),
    #[error("the agent did not write {0}")]
    Missing(&'static str),
    #[error("DESIGN.md has {found} `## ` sections; at least {MIN_SECTIONS} are required")]
    TooFewSections { found: usize },
    #[error("tokens.css does not define {}", .0.join(", "))]
    MissingTokens(Vec<String>),
    #[error("installing the design system failed: {0}")]
    Io(String),
}

impl GenerateRequest {
    /// Trims and bounds the fields.
    ///
    /// # Errors
    /// [`GenError::BadRequest`] for an empty or oversized name or brief.
    pub fn new(name: &str, brief: &str, category: Option<&str>) -> Result<Self, GenError> {
        let name = name.trim();
        let brief = brief.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(GenError::BadRequest(format!(
                "name must be 1–{MAX_NAME_CHARS} characters"
            )));
        }
        if brief.is_empty() || brief.chars().count() > MAX_BRIEF_CHARS {
            return Err(GenError::BadRequest(format!(
                "brief must be 1–{MAX_BRIEF_CHARS} characters"
            )));
        }
        Ok(Self {
            name: name.to_owned(),
            brief: brief.to_owned(),
            category: category
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_owned),
        })
    }
}

/// Folder id for `name`: lowercase ASCII words joined by `-`; accents are
/// folded (`Trà Sữa` → `tra-sua`), anything else becomes `ds`.
#[must_use]
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(fold) {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(48).collect();
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() { "ds".to_owned() } else { out }
}

/// Vietnamese and common Latin accents to their base letter.
fn fold(c: char) -> Option<char> {
    const GROUPS: [(&str, char); 12] = [
        ("àáảãạăằắẳẵặâầấẩẫậäå", 'a'),
        ("ÀÁẢÃẠĂẰẮẲẴẶÂẦẤẨẪẬÄÅ", 'a'),
        ("èéẻẽẹêềếểễệë", 'e'),
        ("ÈÉẺẼẸÊỀẾỂỄỆË", 'e'),
        ("ìíỉĩịïî", 'i'),
        ("ÌÍỈĨỊÏÎ", 'i'),
        ("òóỏõọôồốổỗộơờớởỡợö", 'o'),
        ("ÒÓỎÕỌÔỒỐỔỖỘƠỜỚỞỠỢÖ", 'o'),
        ("ùúủũụưừứửữựüû", 'u'),
        ("ÙÚỦŨỤƯỪỨỬỮỰÜÛ", 'u'),
        ("ỳýỷỹỵÿ", 'y'),
        ("ỲÝỶỸỴ", 'y'),
    ];
    if c.is_ascii() {
        return Some(c);
    }
    if matches!(c, 'đ' | 'Đ') {
        return Some('d');
    }
    GROUPS
        .iter()
        .find(|(set, _)| set.contains(c))
        .map(|(_, base)| *base)
        .or(Some(' '))
}

/// The agent prompt: the brief plus the exact files to write in its cwd.
#[must_use]
pub fn generation_prompt(req: &GenerateRequest) -> String {
    let sections: Vec<String> = SECTIONS
        .iter()
        .enumerate()
        .map(|(i, s)| format!("## {}. {s}", i + 1))
        .collect();
    let category = req.category.as_deref().unwrap_or("Custom");
    format!(
        "# Design system brief\n\n\
         Create a design system named **{name}**.\n\n\
         {brief}\n\n\
         # Files to write\n\n\
         Write exactly these two files in the current working directory, then stop:\n\n\
         1. `DESIGN.md`: starts with `# {name}`, then a line `> Category: {category}`, a one-paragraph summary, and these sections (all of them, in order):\n\n\
         {sections}\n\n\
         The Color section lists each colour as `- **Name** `#rrggbb``. Prose, not placeholders.\n\n\
         2. `tokens.css`: one `:root {{ … }}` block defining at least {tokens}, plus a type scale `--text-xs` … `--text-4xl`, `--space-1` … `--space-12`, `--radius` and `--font-mono`. Values only: no comments that reference other brands.\n\n\
         Do not create any other file and do not ask questions.",
        name = req.name,
        brief = req.brief,
        sections = sections.join("\n"),
        tokens = REQUIRED_TOKENS
            .iter()
            .map(|t| format!("`{t}`"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Counts `## ` headings outside fenced code.
fn count_h2(markdown: &str) -> usize {
    let mut fenced = false;
    markdown
        .lines()
        .filter(|l| {
            if l.trim_start().starts_with("```") {
                fenced = !fenced;
                return false;
            }
            !fenced && l.starts_with("## ") && l.len() > 3
        })
        .count()
}

/// Token names defined in `css` (`--name:`).
fn defines(css: &str, token: &str) -> bool {
    css.match_indices(token).any(|(at, _)| {
        let rest = css[at + token.len()..].trim_start();
        let before = css[..at].chars().next_back();
        rest.starts_with(':') && !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// Checks the agent's files in `staging`.
///
/// # Errors
/// [`GenError::Missing`], [`GenError::TooFewSections`],
/// [`GenError::MissingTokens`].
pub fn check_staging(staging: &Path) -> Result<(), GenError> {
    let design = std::fs::read_to_string(staging.join("DESIGN.md"))
        .ok()
        .filter(|d| !d.trim().is_empty())
        .ok_or(GenError::Missing("DESIGN.md"))?;
    let found = count_h2(&design);
    if found < MIN_SECTIONS {
        return Err(GenError::TooFewSections { found });
    }
    let tokens = std::fs::read_to_string(staging.join("tokens.css"))
        .ok()
        .filter(|t| !t.trim().is_empty())
        .ok_or(GenError::Missing("tokens.css"))?;
    let missing: Vec<String> = REQUIRED_TOKENS
        .iter()
        .filter(|t| !defines(&tokens, t))
        .map(|t| (*t).to_owned())
        .collect();
    if !missing.is_empty() {
        return Err(GenError::MissingTokens(missing));
    }
    Ok(())
}

/// A package [`install`] put in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The listed id: `user:<folder>`.
    pub id: String,
    pub dir: PathBuf,
}

fn io(e: std::io::Error) -> GenError {
    GenError::Io(e.to_string())
}

/// First free folder name `<slug>`, `<slug>-2`, … under `root`.
fn free_folder(root: &Path, slug: &str) -> PathBuf {
    (1..)
        .map(|n| {
            if n == 1 {
                root.join(slug)
            } else {
                root.join(format!("{slug}-{n}"))
            }
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| root.join(slug))
}

/// Validates `staging`, keeps only the package files, writes the manifest
/// and metadata (status `draft`), and moves it to a free folder under
/// `user_root`. Other files the agent wrote are dropped.
///
/// # Errors
/// Any [`check_staging`] error, or [`GenError::Io`].
pub fn install(
    staging: &Path,
    user_root: &Path,
    req: &GenerateRequest,
) -> Result<Installed, GenError> {
    check_staging(staging)?;
    std::fs::create_dir_all(user_root).map_err(io)?;
    let slug = slugify(&req.name);
    let dest = free_folder(user_root, &slug);
    let id = dest
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_valid_id(n))
        .ok_or_else(|| GenError::Io("bad folder name".to_owned()))?
        .to_owned();
    // Build the package next to its destination, then rename: the index
    // never sees a half-written folder.
    let tmp = user_root.join(format!(".{id}.installing"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(io)?;
    }
    std::fs::create_dir_all(&tmp).map_err(io)?;
    for file in ["DESIGN.md", "tokens.css"] {
        std::fs::copy(staging.join(file), tmp.join(file)).map_err(io)?;
    }
    let category = req.category.clone().unwrap_or_else(|| "Custom".to_owned());
    let manifest = json!({
        "schemaVersion": "od-design-system-project/v1",
        "id": id,
        "name": req.name,
        "category": category,
        "description": first_line(&req.brief),
        "source": { "type": "local", "origin": "generated from a brief by an agent" },
        "files": { "design": "DESIGN.md", "tokens": "tokens.css" },
        "importMode": "normalized",
        "craft": { "applies": [], "suggested": ["color", "typography"], "exemptions": [] },
    });
    let metadata = json!({ "title": req.name, "category": category, "status": "draft" });
    for (file, value) in [("manifest.json", manifest), ("metadata.json", metadata)] {
        let text = serde_json::to_string_pretty(&value).map_err(|e| GenError::Io(e.to_string()))?;
        std::fs::write(tmp.join(file), text).map_err(io)?;
    }
    std::fs::rename(&tmp, &dest).map_err(io)?;
    Ok(Installed {
        id: format!("{USER_ID_PREFIX}{id}"),
        dir: dest,
    })
}

fn first_line(brief: &str) -> String {
    let line = brief.lines().next().unwrap_or("").trim();
    line.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_systems::{ListOptions, list_design_systems};

    pub(crate) fn good_design(name: &str) -> String {
        let mut s = format!("# {name}\n\n> Category: Food\n\nWarm tea shop palette.\n\n");
        for (i, sec) in SECTIONS.iter().enumerate() {
            s.push_str(&format!("## {}. {sec}\n\nText.\n\n", i + 1));
        }
        s.push_str("- **Leaf** `#2f6b3a`\n- **Cream** `#f6efe2`\n");
        s
    }

    const TOKENS: &str = ":root {\n  --bg: #f6efe2;\n  --surface: #fff;\n  --fg: #1d1b16;\n  --accent: #2f6b3a;\n  --font-display: Georgia, serif;\n  --font-body: system-ui;\n}\n";

    #[test]
    fn slugs_fold_accents() {
        assert_eq!(slugify("Trà Sữa Đà Lạt"), "tra-sua-da-lat");
        assert_eq!(slugify("  ACME  / Pro!! "), "acme-pro");
        assert_eq!(slugify("日本"), "ds");
    }

    #[test]
    fn request_bounds() {
        assert!(GenerateRequest::new(" ", "x", None).is_err());
        assert!(GenerateRequest::new("a", "  ", None).is_err());
        let r = GenerateRequest::new(" Tea ", " warm ", Some(" ")).unwrap();
        assert_eq!((r.name.as_str(), r.brief.as_str()), ("Tea", "warm"));
        assert_eq!(r.category, None);
        let p = generation_prompt(&r);
        assert!(p.contains("# Design system brief"));
        assert!(p.contains("## 9. Anti-patterns"));
        assert!(p.contains("`--font-body`"));
    }

    #[test]
    fn staging_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let s = tmp.path();
        assert_eq!(check_staging(s), Err(GenError::Missing("DESIGN.md")));
        std::fs::write(s.join("DESIGN.md"), "# X\n\n## One\n\n```\n## not a heading\n```\n").unwrap();
        assert_eq!(check_staging(s), Err(GenError::TooFewSections { found: 1 }));
        std::fs::write(s.join("DESIGN.md"), good_design("X")).unwrap();
        assert_eq!(check_staging(s), Err(GenError::Missing("tokens.css")));
        std::fs::write(s.join("tokens.css"), ":root { --bg: #fff; --bg-2: #eee; --fg: #000; }").unwrap();
        assert_eq!(
            check_staging(s),
            Err(GenError::MissingTokens(vec![
                "--surface".into(),
                "--accent".into(),
                "--font-display".into(),
                "--font-body".into()
            ]))
        );
        std::fs::write(s.join("tokens.css"), TOKENS).unwrap();
        assert_eq!(check_staging(s), Ok(()));
    }

    #[test]
    fn install_lists_next_to_bundled_with_a_free_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("design-systems");
        let req = GenerateRequest::new("Trà Đá", "Street tea stall.\nMore.", None).unwrap();
        for _ in 0..2 {
            let staging = tempfile::tempdir().unwrap();
            std::fs::write(staging.path().join("DESIGN.md"), good_design("Trà Đá")).unwrap();
            std::fs::write(staging.path().join("tokens.css"), TOKENS).unwrap();
            std::fs::write(staging.path().join("notes.txt"), "scratch").unwrap();
            install(staging.path(), &user, &req).unwrap();
        }
        let listed = list_design_systems(&user, ListOptions::USER);
        let ids: Vec<&str> = listed.iter().map(|d| d.summary.id.as_str()).collect();
        assert_eq!(ids, ["user:tra-da", "user:tra-da-2"]);
        let first = &listed[0];
        assert_eq!(first.summary.title, "Trà Đá");
        assert_eq!(first.summary.category, "Custom");
        assert_eq!(first.summary.summary, "Street tea stall.");
        assert!(first.manifest.is_some(), "manifest passes the v1 schema check");
        assert!(first.summary.swatches.contains(&"#2f6b3a".to_owned()), "{:?}", first.summary.swatches);
        assert!(!first.dir.join("notes.txt").exists());
        assert!(
            std::fs::read_dir(&user)
                .unwrap()
                .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".installing"))
        );
    }
}
