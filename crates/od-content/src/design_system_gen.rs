//! Creating a design system from a brief (spec 1.4 "Tạo design system"):
//! an agent writes `DESIGN.md` and `tokens.css` into a staging folder,
//! then [`install`] checks them, adds `manifest.json` / `metadata.json`
//! and moves the package into the user root (`<data>/design-systems`),
//! where the index lists it as `user:<id>` next to the bundled ones.

use crate::design_systems::{USER_ID_PREFIX, read_package_file};
use crate::nofollow::{ReadFile, Tree};
use od_fs::user_store::UserRoot;
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

impl From<std::io::Error> for GenError {
    fn from(e: std::io::Error) -> Self {
        io(e)
    }
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

/// Where an agent finds the material a design system is derived from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sources {
    /// Websites to study (a brand's site, a product page).
    pub urls: Vec<String>,
    /// A GitHub repository (`owner/repo` or its URL).
    pub github: Option<String>,
    /// A Figma file or frame URL.
    pub figma: Option<String>,
    /// Files the user attached, saved under `source/` in the working folder.
    pub files: Vec<String>,
}

impl Sources {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty()
            && self.github.is_none()
            && self.figma.is_none()
            && self.files.is_empty()
    }
}

/// The brief used when only sources were given.
pub const SOURCES_ONLY_BRIEF: &str = "Derive the design system from the sources below: its colours, type, spacing, components and voice.";

/// [`generation_prompt`] plus a `# Sources` section the agent studies
/// first (it fetches URLs and clones repositories with its own tools).
#[must_use]
pub fn generation_prompt_with_sources(req: &GenerateRequest, sources: &Sources) -> String {
    let mut out = generation_prompt(req);
    if sources.is_empty() {
        return out;
    }
    let mut lines = vec![
        String::new(),
        "# Sources".to_owned(),
        String::new(),
        "Study these before writing. Take real values (hex colours, font families, sizes, radii) from them; do not invent a palette they contradict.".to_owned(),
        String::new(),
    ];
    for u in &sources.urls {
        lines.push(format!("- Website: {u} (fetch the page and its CSS)"));
    }
    if let Some(g) = &sources.github {
        lines.push(format!(
            "- GitHub repository: {g} (clone it shallowly into a temporary folder outside the current one; read its theme, CSS variables, Tailwind config and components)"
        ));
    }
    if let Some(f) = &sources.figma {
        lines.push(format!(
            "- Figma: {f} (use a Figma tool if you have one; otherwise rely on the other sources)"
        ));
    }
    for f in &sources.files {
        lines.push(format!("- Attached file: `source/{f}`"));
    }
    out.push_str(&lines.join("\n"));
    out.push('\n');
    out
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

/// A non-empty file the agent wrote in `staging`; one that links out of it
/// counts as missing.
fn staged(staging: &Path, name: &'static str) -> Result<String, GenError> {
    read_package_file(staging, name)
        .filter(|t| !t.trim().is_empty())
        .ok_or(GenError::Missing(name))
}

/// The package files read from `staging`, once: what was checked is what
/// gets installed.
struct Staged {
    design: String,
    tokens: String,
}

/// Checks the agent's files in `staging`.
///
/// # Errors
/// [`GenError::Missing`], [`GenError::TooFewSections`],
/// [`GenError::MissingTokens`].
pub fn check_staging(staging: &Path) -> Result<(), GenError> {
    read_checked(staging).map(|_| ())
}

fn read_checked(staging: &Path) -> Result<Staged, GenError> {
    let design = staged(staging, "DESIGN.md")?;
    let found = count_h2(&design);
    if found < MIN_SECTIONS {
        return Err(GenError::TooFewSections { found });
    }
    let tokens = staged(staging, "tokens.css")?;
    let missing: Vec<String> = REQUIRED_TOKENS
        .iter()
        .filter(|t| !defines(&tokens, t))
        .map(|t| (*t).to_owned())
        .collect();
    if !missing.is_empty() {
        return Err(GenError::MissingTokens(missing));
    }
    Ok(Staged { design, tokens })
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

/// Folder names for `slug`: `<slug>`, `<slug>-2`, …
pub fn folder_names(slug: &str) -> impl Iterator<Item = String> + '_ {
    (1..).map(move |n| {
        if n == 1 {
            slug.to_owned()
        } else {
            format!("{slug}-{n}")
        }
    })
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
    let files = read_checked(staging)?;
    install_checked(&files, user_root, req, "generated from a brief by an agent")
}

/// Installs a package the user brought (pasted `DESIGN.md`, imported CSS
/// variables): `tokens.css` must define the A1 tokens, `DESIGN.md` only has
/// to exist (the original accepts any pasted document).
///
/// # Errors
/// [`GenError::Missing`], [`GenError::MissingTokens`], [`GenError::Io`].
pub fn install_imported(
    staging: &Path,
    user_root: &Path,
    req: &GenerateRequest,
    origin: &str,
) -> Result<Installed, GenError> {
    let design = staged(staging, "DESIGN.md")?;
    let tokens = staged(staging, "tokens.css")?;
    let missing: Vec<String> = REQUIRED_TOKENS
        .iter()
        .filter(|t| !defines(&tokens, t))
        .map(|t| (*t).to_owned())
        .collect();
    if !missing.is_empty() {
        return Err(GenError::MissingTokens(missing));
    }
    install_checked(&Staged { design, tokens }, user_root, req, origin)
}

/// Puts the package in the first free `<slug>` folder of `user_root`
/// through [`UserRoot`]'s `put_new`: the name picked and taken under the
/// root's lock (two jobs for one name land in two folders), the package
/// built in a hidden folder through its descriptor (no link an agent
/// plants there written through), `DESIGN.md` last, then renamed in: the
/// index never lists a half-written folder.
fn install_checked(
    files: &Staged,
    user_root: &Path,
    req: &GenerateRequest,
    origin: &str,
) -> Result<Installed, GenError> {
    let slug = slugify(&req.name);
    let category = req.category.clone().unwrap_or_else(|| "Custom".to_owned());
    let root = UserRoot::open(user_root, crate::user_content::MAX_PLUGIN_BYTES);
    // Every name `folder_names` gives is a valid id (a test holds it).
    let package = |id: &str| -> Result<Tree, GenError> {
        let manifest = json!({
            "schemaVersion": "od-design-system-project/v1",
            "id": id,
            "name": req.name,
            "category": category,
            "description": first_line(&req.brief),
            "source": { "type": "local", "origin": origin },
            "files": { "design": "DESIGN.md", "tokens": "tokens.css" },
            "importMode": "normalized",
            "craft": { "applies": [], "suggested": ["color", "typography"], "exemptions": [] },
        });
        let metadata = json!({ "title": req.name, "category": category, "status": "draft" });
        let json = |v: &serde_json::Value| {
            serde_json::to_vec_pretty(v).map_err(|e| GenError::Io(e.to_string()))
        };
        let file = |bytes: Vec<u8>| ReadFile { bytes, mode: 0o644 };
        Ok(Tree {
            dirs: Vec::new(),
            files: vec![
                (
                    PathBuf::from("tokens.css"),
                    file(files.tokens.clone().into_bytes()),
                ),
                (PathBuf::from("manifest.json"), file(json(&manifest)?)),
                (PathBuf::from("metadata.json"), file(json(&metadata)?)),
                (
                    PathBuf::from("DESIGN.md"),
                    file(files.design.clone().into_bytes()),
                ),
            ],
        })
    };
    let id = root
        .lock()
        .put_new(folder_names(&slug), package, Some(Path::new("DESIGN.md")))?;
    Ok(Installed {
        dir: user_root.join(&id),
        id: format!("{USER_ID_PREFIX}{id}"),
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
    fn sources_are_listed_after_the_files_to_write() {
        let r = GenerateRequest::new("Tea", SOURCES_ONLY_BRIEF, None).unwrap();
        assert_eq!(
            generation_prompt_with_sources(&r, &Sources::default()),
            generation_prompt(&r)
        );
        let p = generation_prompt_with_sources(
            &r,
            &Sources {
                urls: vec!["https://tea.example".into()],
                github: Some("acme/ui".into()),
                figma: Some("https://www.figma.com/file/x".into()),
                files: vec!["logo.png".into()],
            },
        );
        let at = p.find("# Sources").unwrap();
        assert!(p.find("# Files to write").unwrap() < at);
        assert!(p.contains("- Website: https://tea.example"));
        assert!(p.contains("- GitHub repository: acme/ui"));
        assert!(p.contains("- Figma: https://www.figma.com/file/x"));
        assert!(p.contains("`source/logo.png`"));
    }

    #[test]
    fn staging_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let s = tmp.path();
        assert_eq!(check_staging(s), Err(GenError::Missing("DESIGN.md")));
        std::fs::write(
            s.join("DESIGN.md"),
            "# X\n\n## One\n\n```\n## not a heading\n```\n",
        )
        .unwrap();
        assert_eq!(check_staging(s), Err(GenError::TooFewSections { found: 1 }));
        std::fs::write(s.join("DESIGN.md"), good_design("X")).unwrap();
        assert_eq!(check_staging(s), Err(GenError::Missing("tokens.css")));
        std::fs::write(
            s.join("tokens.css"),
            ":root { --bg: #fff; --bg-2: #eee; --fg: #000; }",
        )
        .unwrap();
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

    /// The agent writes the staging folder: a `DESIGN.md` or `tokens.css`
    /// that links out of it is missing, never read or copied in.
    #[cfg(unix)]
    #[test]
    fn staging_files_never_follow_a_link_out() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.md"), good_design("Leak")).unwrap();
        std::fs::write(outside.join("secret.css"), TOKENS).unwrap();
        let user = tmp.path().join("design-systems");
        let req = GenerateRequest::new("Tea", "Warm.", None).unwrap();
        for (file, target, real) in [
            ("DESIGN.md", "secret.md", ("tokens.css", TOKENS.to_owned())),
            (
                "tokens.css",
                "secret.css",
                ("DESIGN.md", good_design("Tea")),
            ),
        ] {
            let staging = tempfile::tempdir().unwrap();
            let s = staging.path();
            std::os::unix::fs::symlink(outside.join(target), s.join(file)).unwrap();
            std::fs::write(s.join(real.0), &real.1).unwrap();
            let missing = Err(GenError::Missing(file));
            assert_eq!(check_staging(s), missing);
            assert_eq!(install(s, &user, &req).map(|_| ()), missing);
            assert_eq!(install_imported(s, &user, &req, "x").map(|_| ()), missing);
        }
        assert!(!user.exists());
    }

    /// Two jobs for the same name at once: each builds in its own folder,
    /// both land, and a half-built folder never shows in the list.
    #[test]
    fn concurrent_installs_of_one_name_both_land() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("design-systems");
        std::fs::create_dir_all(user.join(".tra-da.installing")).unwrap();
        std::fs::write(
            user.join(".tra-da.installing/DESIGN.md"),
            good_design("Trà Đá"),
        )
        .unwrap();
        let req = GenerateRequest::new("Trà Đá", "Street tea stall.", None).unwrap();
        let barrier = std::sync::Barrier::new(8);
        let results: Vec<Result<Installed, GenError>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        let staging = tempfile::tempdir().unwrap();
                        std::fs::write(staging.path().join("DESIGN.md"), good_design("Trà Đá"))
                            .unwrap();
                        std::fs::write(staging.path().join("tokens.css"), TOKENS).unwrap();
                        barrier.wait();
                        install(staging.path(), &user, &req)
                    })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        let mut ids: Vec<String> = results.into_iter().map(|r| r.unwrap().id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8, "{ids:?}");
        let left: Vec<String> = std::fs::read_dir(&user)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".installing-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let listed = list_design_systems(&user, ListOptions::USER);
        assert_eq!(listed.len(), 8);
        assert!(
            listed.iter().all(|d| !d.summary.id.starts_with("user:.")),
            "{:?}",
            listed.iter().map(|d| &d.summary.id).collect::<Vec<_>>()
        );
        for d in &listed {
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(d.dir.join("manifest.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                format!("user:{}", manifest["id"].as_str().unwrap()),
                d.summary.id,
                "manifest id matches its folder"
            );
        }
    }

    /// A folder name is always an id the index takes, whatever the name
    /// given: the package is placed under it with no check of its own.
    #[test]
    fn every_folder_name_is_a_valid_id() {
        let long = "Ā".repeat(200);
        for name in [
            "Trà Đá",
            "!!!",
            "🎨 palette",
            "..",
            "a/b\\c",
            "_x",
            long.as_str(),
        ] {
            for folder in folder_names(&slugify(name)).take(12) {
                assert!(
                    crate::design_systems::is_valid_id(&folder),
                    "{name:?} gives {folder:?}"
                );
                assert!(
                    !folder.starts_with(['.', '_', '-']),
                    "{name:?} gives {folder:?}"
                );
            }
        }
    }

    /// A link an agent put at the first free name is skipped, never
    /// written through, and the package lands whole in the next.
    #[cfg(unix)]
    #[test]
    fn a_link_at_the_free_name_is_never_written_through() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("design-systems");
        std::fs::create_dir_all(&user).unwrap();
        let away = tmp.path().join("away");
        std::fs::create_dir(&away).unwrap();
        std::os::unix::fs::symlink(&away, user.join("tra-da")).unwrap();
        let req = GenerateRequest::new("Trà Đá", "Street tea stall.", None).unwrap();
        let files = Staged {
            design: good_design("Trà Đá"),
            tokens: TOKENS.to_owned(),
        };
        let got = install_checked(&files, &user, &req, "x").unwrap();
        assert_eq!(got.id, "user:tra-da-2");
        assert_eq!(std::fs::read_dir(&away).unwrap().count(), 0);
        let mut names: Vec<_> = std::fs::read_dir(&user)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["tra-da", "tra-da-2"], "nothing hidden left");
    }

    /// Jobs of one name at once land in folders of their own.
    #[test]
    fn installs_of_one_name_at_once_take_a_folder_each() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("design-systems");
        let req = GenerateRequest::new("Trà Đá", "Street tea stall.", None).unwrap();
        let files = Staged {
            design: good_design("Trà Đá"),
            tokens: TOKENS.to_owned(),
        };
        let mut ids: Vec<String> = std::thread::scope(|s| {
            let runs: Vec<_> = (0..4)
                .map(|_| s.spawn(|| install_checked(&files, &user, &req, "x").unwrap().id))
                .collect();
            runs.into_iter().map(|r| r.join().unwrap()).collect()
        });
        ids.sort();
        assert_eq!(
            ids,
            [
                "user:tra-da",
                "user:tra-da-2",
                "user:tra-da-3",
                "user:tra-da-4"
            ]
        );
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
        assert!(
            first.manifest.is_some(),
            "manifest passes the v1 schema check"
        );
        assert!(
            first.summary.swatches.contains(&"#2f6b3a".to_owned()),
            "{:?}",
            first.summary.swatches
        );
        assert!(!first.dir.join("notes.txt").exists());
        assert!(std::fs::read_dir(&user).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".installing")
        }));
    }
}
