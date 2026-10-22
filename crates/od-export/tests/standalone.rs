//! Standalone HTML: the output must not reference any local file (spec 5.7
//! M5), and every limit maps to a typed error.

use base64::Engine;
use od_export::standalone::{self, MANIFEST_ID};
use od_export::{AssetMeta, AssetSource, Limits, Options, StandaloneError};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Default)]
struct Mem(HashMap<String, (String, Vec<u8>)>);

impl Mem {
    fn with(mut self, path: &str, mime: &str, body: impl Into<Vec<u8>>) -> Self {
        self.0
            .insert(path.to_owned(), (mime.to_owned(), body.into()));
        self
    }
}

impl AssetSource for Mem {
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>> {
        Ok(self.0.get(path).map(|(mime, body)| AssetMeta {
            mime: mime.clone(),
            size: body.len() as u64,
        }))
    }
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.0
            .get(path)
            .map(|(_, b)| b.clone())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

fn fixture() -> Mem {
    Mem::default()
        .with(
            "deck/index.html",
            "text/html",
            r##"<!doctype html><html><head>
<base href="./">
<link rel="stylesheet" href="css/main.css">
<link rel="icon" href="/favicon.png">
<link rel="modulepreload" href="js/app.js">
<style>.hero{background:url('img/hero.png')}</style>
<script src="js/classic.js"></script>
<script src="https://cdn.example.com/lib.js"></script>
<script type="module" src="js/app.js"></script>
<script type="module">import { a } from "./js/a.js"; console.log(a);</script>
</head><body>
<img src="img/hero.png" srcset="img/hero.png 1x, img/hero@2x.png 2x">
<picture><source srcset="img/hero@2x.png"></picture>
<div style="background-image: url(img/bg.png)"></div>
<video poster="img/poster.png" src="media/clip.mp4"></video>
<svg><image href="img/icon.svg"/></svg>
<iframe src="frame.html"></iframe>
<a href="other.html">next</a>
<a href="#top">top</a>
</body></html>"##,
        )
        .with(
            "deck/css/main.css",
            "text/css",
            r#"@import "theme.css";
/* url(missing-in-comment.png) */
@font-face { font-family: X; src: url("../fonts/x.woff2") format("woff2"); }
body { background: url(../img/bg.png) }"#,
        )
        .with("deck/css/theme.css", "text/css", ":root{--c:red} .t{background:url('../img/hero.png')}")
        .with("deck/fonts/x.woff2", "font/woff2", b"wOF2".to_vec())
        .with("deck/img/hero.png", "image/png", PNG)
        .with("deck/img/hero@2x.png", "image/png", PNG)
        .with("deck/img/bg.png", "image/png", PNG)
        .with("deck/img/poster.png", "image/png", PNG)
        .with("deck/img/icon.svg", "image/svg+xml", "<svg xmlns='http://www.w3.org/2000/svg'/>")
        .with("deck/media/clip.mp4", "video/mp4", b"mp4".to_vec())
        .with("favicon.png", "image/png", PNG)
        .with("deck/js/classic.js", "text/javascript", "window.classic = 1;")
        .with(
            "deck/js/app.js",
            "text/javascript",
            "import { a } from './a.js';\nimport('./lazy.js').then(() => {});\nexport * from \"./b.js\";",
        )
        .with("deck/js/a.js", "text/javascript", "import { b } from './b.js'; export const a = 1;")
        // b imports a: a module cycle, legal in ES modules.
        .with("deck/js/b.js", "text/javascript", "import { a } from './a.js'; export const b = 2;")
        .with("deck/js/lazy.js", "text/javascript", "export default 3;")
        .with("deck/frame.html", "text/html", r#"<html><body><img src="img/hero.png"></body></html>"#)
}

static DATA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"data:([a-z0-9.+/-]+)(?:;charset=utf-8)?;base64,([A-Za-z0-9+/=]+)").unwrap()
});
static CSS_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"url\(\s*["']?([^"')]+)["']?\s*\)|@import\s+["']([^"']+)["']"#).unwrap()
});
static ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s(src|href|poster|srcset|data)\s*=\s*"([^"]*)""#).unwrap());
static IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:from|import)\s*\(?\s*["']([^"']+)["']"#).unwrap());

fn is_local(url: &str) -> bool {
    let u = url.trim();
    !(u.is_empty()
        || u.starts_with('#')
        || u.starts_with("data:")
        || u.starts_with("about:")
        || u.starts_with("od-project:/")
        || u.starts_with("https:")
        || u.starts_with("http:")
        || u.starts_with("//"))
}

/// Every local reference left anywhere in `html`, including inside embedded
/// CSS, HTML and JavaScript `data:` payloads. `<a href>` is navigation, not a
/// load, and is ignored.
fn local_refs(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    scan(html, "text/html", &mut found);
    found
}

fn scan(text: &str, mime: &str, found: &mut Vec<String>) {
    let stripped = DATA_RE.replace_all(text, "data:");
    match mime {
        "text/html" => {
            for tag in Regex::new(r"<(\w+)[^>]*>")
                .unwrap()
                .captures_iter(&stripped)
            {
                if tag[1].eq_ignore_ascii_case("a") {
                    continue;
                }
                for attr in ATTR_RE.captures_iter(&tag[0]) {
                    let values: Vec<&str> = if &attr[1] == "srcset" {
                        attr[2]
                            .split(',')
                            .filter_map(|c| c.split_whitespace().next())
                            .collect()
                    } else {
                        vec![&attr[2]]
                    };
                    found.extend(
                        values
                            .into_iter()
                            .filter(|v| is_local(v))
                            .map(str::to_owned),
                    );
                }
            }
            for c in CSS_URL_RE.captures_iter(&stripped) {
                let v = c.get(1).or_else(|| c.get(2)).unwrap().as_str();
                if is_local(v) {
                    found.push(v.to_owned());
                }
            }
        }
        "text/css" => {
            for c in CSS_URL_RE.captures_iter(&stripped) {
                let v = c.get(1).or_else(|| c.get(2)).unwrap().as_str();
                if is_local(v) {
                    found.push(v.to_owned());
                }
            }
        }
        "text/javascript" => {
            for c in IMPORT_RE.captures_iter(&stripped) {
                if c[1].starts_with('.') || c[1].starts_with('/') {
                    found.push(c[1].to_owned());
                }
            }
        }
        _ => {}
    }
    for c in DATA_RE.captures_iter(text) {
        let inner_mime = &c[1];
        if matches!(inner_mime, "text/html" | "text/css" | "text/javascript") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&c[2])
                .unwrap();
            scan(&String::from_utf8(bytes).unwrap(), inner_mime, found);
        }
    }
    // Inline module scripts and the import map are JavaScript/JSON in HTML.
    if mime == "text/html" {
        for c in IMPORT_RE.captures_iter(&stripped) {
            if c[1].starts_with("./") || c[1].starts_with("../") {
                found.push(c[1].to_owned());
            }
        }
    }
}

fn bundle(src: &Mem, options: &Options) -> Result<od_export::StandaloneHtml, StandaloneError> {
    let html = String::from_utf8(src.0["deck/index.html"].1.clone()).unwrap();
    standalone::bundle("deck/index.html", &html, src, options)
}

#[test]
fn output_has_no_local_references() {
    let out = bundle(&fixture(), &Options::default()).unwrap();
    assert_eq!(local_refs(&out.html), Vec::<String>::new(), "{}", out.html);
    // The checker itself must see a relative URL when one is there.
    assert_eq!(local_refs(r#"<img src="x.png">"#), ["x.png"]);
    assert!(out.html.contains(r#"<a href="other.html">"#));
    assert!(!out.html.contains("<base"));
    assert!(!out.html.contains("modulepreload"));
}

#[test]
fn modules_go_through_one_import_map() {
    let out = bundle(&fixture(), &Options::default()).unwrap();
    let start = out.html.find(r#"<script type="importmap">"#).unwrap();
    let json_start = start + r#"<script type="importmap">"#.len();
    let json_end = json_start + out.html[json_start..].find("</script>").unwrap();
    let map: serde_json::Value = serde_json::from_str(&out.html[json_start..json_end]).unwrap();
    let imports = map["imports"].as_object().unwrap();
    let mut keys: Vec<&str> = imports.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "od-project:/deck/js/a.js",
            "od-project:/deck/js/app.js",
            "od-project:/deck/js/b.js",
            "od-project:/deck/js/lazy.js"
        ]
    );
    // The import map precedes every module script.
    assert!(start < out.html.find(r#"type="module""#).unwrap());
    assert!(
        out.html
            .contains(r#"import { a } from "od-project:/deck/js/a.js""#)
    );
}

#[test]
fn externals_are_listed_not_embedded() {
    let out = bundle(&fixture(), &Options::default()).unwrap();
    assert_eq!(
        out.external_dependencies,
        ["https://cdn.example.com/lib.js"]
    );
    assert!(out.html.contains(&format!(
        r#"<script type="application/json" id="{MANIFEST_ID}">{{"externalDependencies":["https://cdn.example.com/lib.js"]}}</script>"#
    )));
    assert!(out.html.contains(r#"src="https://cdn.example.com/lib.js""#));
    // 4 png + svg + mp4 + favicon + font + 2 css + 5 js + frame.
    assert_eq!(out.embedded, 16);
}

#[test]
fn missing_dependency_fails_unless_lenient() {
    let src = fixture().with(
        "deck/index.html",
        "text/html",
        r#"<img src="nope.png"><img src="img/bg.png">"#,
    );
    match bundle(&src, &Options::default()) {
        Err(StandaloneError::MissingDependency { dependency, chain }) => {
            assert_eq!(dependency, "deck/nope.png");
            assert_eq!(chain, ["deck/index.html"]);
        }
        other => panic!("expected missing dependency, got {other:?}"),
    }
    let lenient = Options {
        lenient: true,
        ..Options::default()
    };
    let out = bundle(&src, &lenient).unwrap();
    assert!(out.html.contains(r#"src="nope.png""#));
    assert!(!out.html.contains(r#"src="img/bg.png""#));
}

#[test]
fn escaping_the_project_is_rejected() {
    let src = Mem::default().with(
        "deck/index.html",
        "text/html",
        r#"<img src="../../etc/passwd">"#,
    );
    assert!(matches!(
        bundle(&src, &Options::default()),
        Err(StandaloneError::PathOutsideProject { .. })
    ));
}

#[test]
fn css_import_cycle_is_an_error() {
    let src = Mem::default()
        .with(
            "deck/index.html",
            "text/html",
            r#"<link rel="stylesheet" href="a.css">"#,
        )
        .with("deck/a.css", "text/css", "@import 'b.css';")
        .with("deck/b.css", "text/css", "@import url(a.css);");
    match bundle(&src, &Options::default()) {
        Err(StandaloneError::DependencyCycle { chain }) => {
            assert_eq!(
                chain,
                ["deck/index.html", "deck/a.css", "deck/b.css", "deck/a.css"]
            );
        }
        other => panic!("expected cycle, got {other:?}"),
    }
}

fn limit_of(result: Result<od_export::StandaloneHtml, StandaloneError>) -> &'static str {
    match result {
        Err(StandaloneError::LimitExceeded { limit, .. }) => limit,
        other => panic!("expected a limit error, got {other:?}"),
    }
}

#[test]
fn each_limit_is_enforced() {
    let base = Limits::default();
    let with = |limits: Limits| Options {
        limits,
        lenient: false,
    };
    assert_eq!(
        limit_of(bundle(
            &fixture(),
            &with(Limits {
                entry_bytes: 100,
                ..base
            })
        )),
        "entryBytes"
    );
    assert_eq!(
        limit_of(bundle(
            &fixture(),
            &with(Limits {
                graph_nodes: 5,
                ..base
            })
        )),
        "graphNodes"
    );
    assert_eq!(
        limit_of(bundle(
            &fixture(),
            &with(Limits {
                asset_bytes: 10,
                ..base
            })
        )),
        "assetBytes"
    );
    assert_eq!(
        limit_of(bundle(
            &fixture(),
            &with(Limits {
                output_bytes: 2_500,
                ..base
            })
        )),
        "outputBytes"
    );
    // Exactly at the limits passes.
    let out = bundle(&fixture(), &Options::default()).unwrap();
    let exact = Limits {
        graph_nodes: out.embedded + 1,
        ..base
    };
    assert!(bundle(&fixture(), &with(exact)).is_ok());
}

#[test]
fn defaults_match_spec() {
    let l = Limits::default();
    assert_eq!(
        (l.entry_bytes, l.graph_nodes, l.asset_bytes, l.output_bytes),
        (2 << 20, 2_500, 50 << 20, 100 << 20)
    );
}
