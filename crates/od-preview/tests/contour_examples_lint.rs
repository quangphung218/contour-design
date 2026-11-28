//! The gallery examples written for Contour (`od.origin: contour`) pass
//! Contour's own artifact lint with no P0: a Remix of one must not open in
//! the studio with a red lint badge.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_contracts::LintSeverity;
use od_preview::lint::lint_artifact;
use std::path::{Path, PathBuf};

fn contour_examples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates");
    let mut out: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|dir| {
            std::fs::read_to_string(dir.join("SKILL.md"))
                .is_ok_and(|s| s.replace("\r\n", "\n").contains("\n  origin: contour\n"))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn contour_examples_have_no_p0_lint() {
    let examples = contour_examples();
    assert_eq!(examples.len(), 30, "Contour's own gallery examples");
    let mut p0 = Vec::new();
    for dir in &examples {
        let html = std::fs::read_to_string(dir.join("example.html")).unwrap();
        for f in lint_artifact(&html) {
            if f.severity == LintSeverity::P0 {
                p0.push(format!(
                    "{}: {} ({})",
                    dir.file_name().unwrap().to_string_lossy(),
                    f.id,
                    f.message
                ));
            }
        }
    }
    assert!(p0.is_empty(), "P0 lint findings:\n{}", p0.join("\n"));
}

/// The check above is only as good as the lint it calls: an untagged
/// slide is still a P0.
#[test]
fn an_untagged_slide_is_still_p0() {
    let html = r#"<!doctype html><html><body><section class="slide light">a</section><section class="slide">b</section></body></html>"#;
    assert!(
        lint_artifact(html)
            .iter()
            .any(|f| f.id == "slide-theme-missing" && f.severity == LintSeverity::P0)
    );
}

/// Bundled decks whose P0 is the template's design, not a slip, each with
/// why it stays. Fixing one means recolouring the template.
const DECK_P0_KEPT: &[(&str, &str)] = &[(
    "html-ppt-obsidian-claude-gradient/example.html",
    "purple-gradient: the whole palette is violet (--oc-accent #7c3aed, #a855f7, #c084fc); a recolour is a design call",
)];

fn deck_files(dir: &Path, slide: &regex::Regex, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            deck_files(&path, slide, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            let html = std::fs::read_to_string(&path).unwrap();
            if slide.is_match(&html) {
                out.push(path);
            }
        }
    }
}

/// Every deck the app ships under `content/design-templates` (each
/// template's `example.html` and the decks its assets carry) passes the
/// lint with no P0: picking an official template must not end the first
/// turn with a red badge and a "Fix 1 issue" chip.
#[test]
fn every_bundled_deck_template_passes_lint_without_p0() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates");
    let slide = regex::Regex::new(r#"(?i)<section\s+class\s*=\s*["'][^"']*\bslide\b"#).unwrap();
    let mut decks = Vec::new();
    deck_files(&root, &slide, &mut decks);
    decks.sort();
    let examples = decks.iter().filter(|p| p.ends_with("example.html")).count();
    assert!(
        decks.len() >= 100 && examples >= 45,
        "found {} decks, {examples} example.html: the scan lost its input",
        decks.len()
    );
    let mut p0 = Vec::new();
    let mut kept_seen = Vec::new();
    for path in &decks {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let html = std::fs::read_to_string(path).unwrap();
        for f in lint_artifact(&html) {
            if f.severity != LintSeverity::P0 {
                continue;
            }
            let kept = DECK_P0_KEPT
                .iter()
                .any(|(file, why)| *file == rel && why.starts_with(&format!("{}:", f.id)));
            if kept {
                kept_seen.push(rel.clone());
            } else {
                p0.push(format!("{rel}: {} ({})", f.id, f.message));
            }
        }
    }
    assert!(p0.is_empty(), "P0 lint findings:\n{}", p0.join("\n"));
    for (file, _) in DECK_P0_KEPT {
        assert!(
            kept_seen.iter().any(|s| s == file),
            "{file} no longer has its P0: drop it from DECK_P0_KEPT"
        );
    }
}
