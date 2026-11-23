//! Gallery examples written for Contour (`od.origin: contour` in
//! SKILL.md): ten WebGL pages, fourteen decks and six landing pages. Each
//! one parses, is filed under the right type, ships a preview page and
//! Contour's Apache-2.0 LICENSE. No bundled design template or skill,
//! Contour's or copied, loads anything from outside loopback, in the pages
//! it ships or the snippets its instructions hand the agent.

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
    }
}

/// Written for Contour, not copied: no LICENSE of the 30 credits the
/// original app (the root LICENSE does, for the copied content).
#[test]
fn no_example_license_names_the_original_app() {
    let examples = contour_examples();
    assert_eq!(examples.len(), 30);
    let named: Vec<String> = examples
        .iter()
        .filter(|(dir, _)| {
            let license = std::fs::read_to_string(dir.join("LICENSE")).unwrap();
            ["Open Design", "OpenDesign", "nexu-io"]
                .iter()
                .any(|n| license.contains(n))
        })
        .map(|(dir, _)| folder(dir))
        .collect();
    assert!(
        named.is_empty(),
        "LICENSE names the original app: {named:?}"
    );
}

/// XML namespace names: identifiers an SVG or XHTML element is tagged with
/// (`xmlns`, `createElementNS`), never fetched.
const XML_NAMESPACES: [&str; 3] = [
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xlink",
    "http://www.w3.org/1999/xhtml",
];

/// The host of `scheme://host…` or `//host…`, lowercased: the authority without
/// user info or port, `[::1]` kept in brackets.
fn host_of(url: &str) -> String {
    let rest = url.split_once("//").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        host_port.split_inclusive(']').next().unwrap_or_default()
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    host.to_ascii_lowercase()
}

/// Every `http(s)://`, `ws(s)://` or protocol-relative `//host` URL in HTML, CSS or JS
/// text, except loopback (`localhost`, `127.0.0.1`, `[::1]`) and XML namespace names.
fn external_urls(text: &str) -> Vec<String> {
    let url = Regex::new(
        r#"(?i)(?:https?|wss?)://[^\s"'<>)]+|["'(=]\s*//[a-z0-9-]+(?:\.[a-z0-9-]+)+[^\s"'<>)]*"#,
    )
    .unwrap();
    url.find_iter(text)
        .map(|m| {
            m.as_str()
                .trim_start_matches(['"', '\'', '(', '='])
                .trim_start()
                .to_owned()
        })
        .filter(|u| !["localhost", "127.0.0.1", "[::1]"].contains(&host_of(u).as_str()))
        .filter(|u| !XML_NAMESPACES.contains(&u.as_str()))
        .collect()
}

/// The files a template's preview or its output loads: `example.html`, and
/// the HTML, CSS and JS under `examples/`, `assets/` and `templates/` (the
/// pages the agent copies). SKILL.md and
/// `references/` are instructions to the agent and may name URLs.
fn served_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.map(|e| e.unwrap().path()) {
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| ["html", "css", "js", "mjs", "svg"].contains(&e))
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    let example = dir.join("example.html");
    if example.is_file() {
        out.push(example);
    }
    walk(&dir.join("examples"), &mut out);
    walk(&dir.join("assets"), &mut out);
    walk(&dir.join("templates"), &mut out);
    out.sort();
    out
}

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/skills")
}

/// Every bundled folder with a SKILL.md: design templates and skills. An
/// agent copies pages from both into the project it previews.
fn bundled_folders() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in [templates_root(), skills_root()] {
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            if dir.join("SKILL.md").is_file() {
                out.push(dir);
            }
        }
    }
    out.sort();
    out
}

/// The preview CSP blocks every request outside loopback, so a CDN script,
/// stylesheet, web font or image would show as broken. Links point at `#`.
#[test]
fn no_bundled_page_loads_anything_from_outside_loopback() {
    let mut hits = Vec::new();
    let (mut templates, mut skills) = (0, 0);
    for dir in bundled_folders() {
        let is_skill = dir.starts_with(skills_root());
        for path in served_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap();
            if is_skill {
                skills += 1;
            } else {
                templates += 1;
            }
            for u in external_urls(&text) {
                hits.push(format!("{}: {u}", path.display()));
            }
            if text.contains("@import") {
                hits.push(format!("{}: @import", path.display()));
            }
        }
    }
    assert!(templates >= 200, "only {templates} template files checked");
    assert!(skills >= 50, "only {skills} skill files checked");
    assert!(hits.is_empty(), "{} external URLs: {hits:#?}", hits.len());
}

/// Info strings of fenced code blocks an agent writes into a page. A block
/// with no language counts too; shell, JSON and prose blocks do not.
const PAGE_CODE: [&str; 15] = [
    "",
    "html",
    "htm",
    "xhtml",
    "svg",
    "xml",
    "css",
    "scss",
    "js",
    "javascript",
    "mjs",
    "jsx",
    "ts",
    "tsx",
    "vue",
];

/// What a page loads from outside loopback in the fenced HTML, CSS and JS
/// blocks of a Markdown file: `<script src>`, `<link href>`, `@import`,
/// `url()` and ES `import`. Prose and Markdown links may name any URL; an
/// agent copies code blocks into the page.
fn code_block_loads(md: &str) -> Vec<String> {
    let fence = Regex::new(r"^\s{0,3}(`{3,}|~{3,})\s*([^\s`{]*)").unwrap();
    let load = Regex::new(
        r#"(?i)<script\b[^>]*?\bsrc\s*=\s*["']?([^"'\s>]+)|<link\b[^>]*?\bhref\s*=\s*["']?([^"'\s>]+)|@import\s+(?:url\(\s*)?["']?([^"'\s);]+)|\burl\(\s*["']?([^"'\s)]+)|\bimport\b[^;\n]*?\bfrom\s*["']([^"']+)|\bimport\s*\(?\s*["']([^"']+)"#,
    )
    .unwrap();
    let remote = Regex::new(r"(?i)^(?:(?:https?|wss?):)?//").unwrap();
    let mut out = Vec::new();
    // (fence marker, whether the block is page code)
    let mut open: Option<(String, bool)> = None;
    for (n, line) in md.lines().enumerate() {
        if let Some(c) = fence.captures(line) {
            let marker = &c[1];
            match &open {
                None => {
                    let lang = c[2].to_ascii_lowercase();
                    open = Some((marker.to_owned(), PAGE_CODE.contains(&lang.as_str())));
                    continue;
                }
                Some((m, _)) if marker.starts_with(m.as_str()) && c[2].is_empty() => {
                    open = None;
                    continue;
                }
                Some(_) => {}
            }
        }
        if !matches!(open, Some((_, true))) {
            continue;
        }
        for c in load.captures_iter(line) {
            let url = c.iter().skip(1).flatten().next().unwrap().as_str();
            if remote.is_match(url)
                && !["localhost", "127.0.0.1", "[::1]"].contains(&host_of(url).as_str())
            {
                out.push(format!("line {}: {url}", n + 1));
            }
        }
    }
    out
}

/// SKILL.md and every Markdown file under `references/`.
fn instruction_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.map(|e| e.unwrap().path()) {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }
    let mut out = vec![dir.join("SKILL.md")];
    walk(&dir.join("references"), &mut out);
    out.sort();
    out
}

/// What SKILL.md and `references/` tell the agent to paste in is what the
/// preview loads: a CDN in a snippet ends up blocked in the project.
#[test]
fn no_skill_snippet_loads_anything_from_outside_loopback() {
    let mut hits = Vec::new();
    let mut checked = 0;
    for dir in bundled_folders() {
        for path in instruction_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap();
            checked += 1;
            for h in code_block_loads(&text) {
                hits.push(format!("{} {h}", path.display()));
            }
        }
    }
    assert!(checked >= 450, "only {checked} Markdown files checked");
    assert!(hits.is_empty(), "{} snippets load: {hits:#?}", hits.len());
}

#[test]
fn the_snippet_check_reads_code_blocks_and_spares_prose() {
    let md = |lang: &str, body: &str| format!("Intro\n\n```{lang}\n{body}\n```\n");
    for (lang, body) in [
        (
            "html",
            r#"<script src="https://cdn.example.net/gsap.min.js"></script>"#,
        ),
        ("", r#"<script defer src=//cdn.example.net/a.js></script>"#),
        (
            "HTML",
            r#"<link rel="stylesheet" href="https://fonts.example.com/x.css">"#,
        ),
        ("css", r#"@import url("https://fonts.example.com/x.css");"#),
        ("css", "@import 'https://fonts.example.com/x.css';"),
        (
            "css",
            "body { background: url(https://img.example.com/bg.png) }",
        ),
        ("js", "import gsap from 'https://esm.example.sh/gsap';"),
        (
            "js",
            "const m = await import(\"https://esm.example.sh/three\");",
        ),
        (
            "jsx",
            r#"<img style={{background: "url('//img.example.com/a.png')"}} />"#,
        ),
    ] {
        assert_eq!(code_block_loads(&md(lang, body)).len(), 1, "{lang}: {body}");
    }
    assert_eq!(
        code_block_loads(&md("html", r#"<script src="https://a.example.net/x.js">"#)),
        ["line 4: https://a.example.net/x.js"]
    );
    // Spared: prose, Markdown links, shell and JSON blocks, loopback, files
    // next to the page, and links that load nothing.
    for text in [
        "See <script src=\"https://cdn.example.net/a.js\"> in the docs.".to_owned(),
        "[GSAP docs](https://gsap.com/docs/) and https://cdn.example.net/a.js".to_owned(),
        md(
            "bash",
            r#"curl -O https://cdn.example.net/a.js # <script src="https://cdn.example.net/a.js">"#,
        ),
        md("json", r#"{"url": "url(https://img.example.com/a.png)"}"#),
        md(
            "html",
            r#"<script src="http://127.0.0.1:7456/x.js"></script>"#,
        ),
        md(
            "html",
            r#"<script src="./gsap.min.js"></script><link href="style.css">"#,
        ),
        md(
            "css",
            "body { background: url(assets/bg.png) } @import 'base.css';",
        ),
        md("html", r#"<a href="https://example.com/">site</a>"#),
        md("js", "import { gsap } from 'gsap';"),
    ] {
        assert!(code_block_loads(&text).is_empty(), "{text}");
    }
    // A fence of four backticks holds a three-backtick line without closing.
    let nested = "````md\n```html\n<script src=\"https://a.example.net/x.js\">\n```\n````\n";
    assert!(code_block_loads(nested).is_empty());
    // After a non-page block closes, the next html block is read again.
    let after = format!(
        "{}{}",
        md("bash", "ls"),
        md("html", r#"<script src="https://a.example.net/x.js">"#)
    );
    assert_eq!(code_block_loads(&after).len(), 1);
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
    assert_eq!(
        external_urls("new WebSocket('wss://live.example.org/feed')").len(),
        1
    );
    assert!(external_urls("new WebSocket('ws://127.0.0.1:7456/ws')").is_empty());
    assert!(external_urls("fetch('http://localhost:7456/api')").is_empty());
    assert!(external_urls("fetch('HTTP://LOCALHOST/api')").is_empty());
    assert!(external_urls("fetch('http://[::1]:7456/api')").is_empty());
    // A host that only starts like loopback is somewhere else.
    for u in [
        "https://localhost.evil.com/x.js",
        "https://127.0.0.1.evil.com/x.js",
        "https://localhost@evil.com/x.js",
        "https://localhostevil.com/x.js",
    ] {
        assert_eq!(external_urls(&format!("<script src=\"{u}\">")), [u], "{u}");
    }
    assert!(external_urls("a // comment. not a url").is_empty());
    assert!(external_urls(r#"<svg xmlns="http://www.w3.org/2000/svg">"#).is_empty());
    assert_eq!(
        external_urls(r#"<image href="http://www.w3.org/2000/svg/logo.png">"#).len(),
        1
    );
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

/// Scripts a template runs write pages too (last30days renders its report
/// with `html_render.py`): no file a template ships may name Google Fonts,
/// whose requests the preview blocks.
#[test]
fn no_template_file_links_google_fonts() {
    fn walk(dir: &Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, hits);
            } else if path.extension().is_none_or(|e| e != "md") {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue; // binary assets
                };
                if text.contains("fonts.googleapis.com") || text.contains("fonts.gstatic.com") {
                    hits.push(path.display().to_string());
                }
            }
        }
    }
    let mut hits = Vec::new();
    walk(&templates_root(), &mut hits);
    assert!(hits.is_empty(), "Google Fonts in {hits:#?}");
}

/// The motion runtime that stands in for GSAP is inlined in every page
/// that used GSAP from a CDN; the copies stay the same, so a fix to one
/// reaches all.
#[test]
fn every_inlined_motion_runtime_is_the_same() {
    const START: &str = "/* Motion runtime written for Contour";
    let mut copies: Vec<(String, String)> = Vec::new();
    for dir in bundled_folders() {
        for path in served_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap();
            let Some(at) = text.find(START) else {
                continue;
            };
            let rest = &text[at..];
            let end = rest.find("})();").expect("runtime ends with })();") + 5;
            let body: String = rest[..end]
                .lines()
                .map(|l| l.trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            copies.push((path.display().to_string(), body));
        }
    }
    assert!(copies.len() >= 5, "{} copies", copies.len());
    for (path, body) in &copies[1..] {
        assert!(body == &copies[0].1, "{path} differs from {}", copies[0].0);
    }
}
