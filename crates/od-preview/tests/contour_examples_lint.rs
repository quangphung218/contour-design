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
