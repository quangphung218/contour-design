//! Gallery examples written for Contour (`od.origin: contour` in
//! SKILL.md): ten WebGL pages, fourteen decks and six landing pages. Each
//! one parses, is filed under the right type, ships a preview page and
//! Contour's Apache-2.0 LICENSE, and loads nothing from outside loopback.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::{ContentIndex, ContentRoots};
use od_contracts::SkillMode;
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn templates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates")
}

/// `(folder, frontmatter)` of every template that says `od.origin: contour`.
fn contour_examples() -> Vec<(PathBuf, Value)> {
    let mut out: Vec<(PathBuf, Value)> = std::fs::read_dir(templates_root())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .filter_map(|dir| {
            let raw = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
            let (data, _) = od_content::frontmatter::parse(&raw)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            let data = Value::Object(data);
            (data["od"]["origin"] == "contour").then_some((dir, data))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn folder(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().into_owned()
}

/// What the Templates type filter shows for each group of new examples.
#[test]
fn thirty_examples_filed_under_the_right_type() {
    let examples = contour_examples();
    let idx = ContentIndex::build(&ContentRoots::bundled(&templates_root().join("..")));
    let mode_of = |id: &str| {
        idx.design_templates
            .iter()
            .find(|t| t.info.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the index"))
            .info
            .mode
    };
    let mut counts = [0usize; 3];
    for (dir, data) in &examples {
        let id = folder(dir);
        assert_eq!(data["name"], id.as_str(), "name matches the folder");
        let (slot, mode) = if id.starts_with("webgl-") {
            (0, SkillMode::Prototype)
        } else if id.starts_with("deck-") {
            (1, SkillMode::Deck)
        } else if id.starts_with("landing-") {
            (2, SkillMode::Prototype)
        } else {
            panic!("{id}: new examples are named webgl-*, deck-* or landing-*");
        };
        counts[slot] += 1;
        assert_eq!(mode_of(&id), mode, "{id}");
        assert!(SkillMode::parse(data["od"]["mode"].as_str().unwrap()) == Some(mode));
        for key in ["en_name", "en_description", "description"] {
            let text = data[key].as_str().unwrap_or_default();
            assert!(!text.trim().is_empty(), "{id}: no {key}");
        }
        assert!(data["triggers"].as_array().is_some_and(|t| !t.is_empty()));
    }
    assert_eq!(counts, [10, 14, 6], "webgl, deck, landing");
}

#[test]
fn every_example_ships_a_preview_and_contours_license() {
    for (dir, _) in contour_examples() {
        let id = folder(&dir);
        let html = std::fs::read_to_string(dir.join("example.html"))
            .unwrap_or_else(|_| panic!("{id}: no example.html"));
        assert!(html.starts_with("<!doctype html>"), "{id}");
        let license = std::fs::read_to_string(dir.join("LICENSE"))
            .unwrap_or_else(|_| panic!("{id}: no LICENSE"));
        assert!(license.contains("Apache License") && license.contains("Version 2.0"));
        assert!(license.contains("Copyright 2026 Quang Phung and contributors"));
        // Written for Contour: no upstream to credit.
        assert!(
            !license.contains("Open Design"),
            "{id}: LICENSE names the original app"
        );
    }
}

/// Every `http(s)://` or protocol-relative `//host` URL in HTML, CSS or JS
/// text, except loopback.
fn external_urls(text: &str) -> Vec<String> {
    let url =
        Regex::new(r#"(?i)https?://[^\s"'<>)]+|["'(=]\s*//[a-z0-9-]+(?:\.[a-z0-9-]+)+[^\s"'<>)]*"#)
            .unwrap();
    url.find_iter(text)
        .map(|m| {
            m.as_str()
                .trim_start_matches(['"', '\'', '(', '='])
                .trim_start()
                .to_owned()
        })
        .filter(|u| {
            let host = u.split("//").nth(1).unwrap_or_default();
            !(host.starts_with("127.0.0.1") || host.starts_with("localhost"))
        })
        .collect()
}

#[test]
fn examples_load_nothing_from_outside_loopback() {
    let mut hits = Vec::new();
    for (dir, _) in contour_examples() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if !["html", "css", "js"].contains(&ext) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for u in external_urls(&text) {
                hits.push(format!("{}: {u}", path.display()));
            }
            assert!(!text.contains("@import"), "{}: @import", path.display());
        }
    }
    assert!(hits.is_empty(), "external URLs: {hits:#?}");
}

#[test]
fn the_url_check_catches_external_and_spares_loopback() {
    assert_eq!(
        external_urls(r#"<link href="https://fonts.example.com/x.css">"#),
        ["https://fonts.example.com/x.css"]
    );
    assert_eq!(
        external_urls(r#"<script src="//cdn.example.net/three.js">"#).len(),
        1
    );
    assert!(external_urls("fetch('http://127.0.0.1:7456/api')").is_empty());
    assert!(external_urls("a // comment. not a url").is_empty());
}

/// The gallery runs no script, so the poster is what the card shows; it
/// also stays up when the browser has no WebGL.
#[test]
fn webgl_examples_keep_a_static_poster() {
    for (dir, _) in contour_examples() {
        let id = folder(&dir);
        if !id.starts_with("webgl-") {
            continue;
        }
        let html = std::fs::read_to_string(dir.join("example.html")).unwrap();
        let poster = html.find("data-webgl-fallback").expect("poster element");
        let canvas = html.find(r#"<canvas id="gl""#).expect("canvas");
        assert!(poster < canvas, "{id}: poster sits under the canvas");
        assert!(html.contains("getContext('webgl2'"), "{id}");
        assert!(html.contains("getContext('webgl'"), "{id}: WebGL1 fallback");
        assert!(
            html.contains("return fail('no WebGL context"),
            "{id}: no-context path"
        );
        assert!(
            html.contains("html.gl-on .fallback{visibility:hidden"),
            "{id}: poster hides only once a frame is drawn"
        );
    }
}

/// `design-templates/AGENTS.md`: decks navigate on their own in the
/// gallery iframe and print one slide per page.
#[test]
fn decks_follow_the_navigation_contract() {
    for (dir, _) in contour_examples() {
        let id = folder(&dir);
        if !id.starts_with("deck-") {
            continue;
        }
        let html = std::fs::read_to_string(dir.join("example.html")).unwrap();
        assert!(html.contains(r#"data-od-deck-protocol="1""#), "{id}");
        let slides = html.matches(r#"<section class="slide"#).count();
        assert!(slides >= 5, "{id}: {slides} slides");
        assert_eq!(
            html.matches(r#"<section class="slide active"#).count(),
            1,
            "{id}"
        );
        for needle in [
            "'ArrowRight'",
            "'ArrowDown'",
            "'PageDown'",
            "' '",
            "'ArrowLeft'",
            "'ArrowUp'",
            "'PageUp'",
            "'Home'",
            "'End'",
            "addEventListener('wheel'",
            "addEventListener('touchend'",
            "aria-current",
            "classList.toggle('active'",
            "localStorage",
            "od:slide-state",
            "@page{size:1920px 1080px",
        ] {
            assert!(html.contains(needle), "{id}: missing {needle}");
        }
        // Inactive slides hide only on screen once the runtime has booted.
        assert!(
            html.contains(
                "@media screen{html.deck-on .slide:not(.active){display:none!important}}"
            )
        );
        assert!(!html.contains("scrollIntoView"), "{id}");
    }
}
