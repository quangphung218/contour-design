//! Every HTML file in the bundled design templates goes through the strict
//! bundler: it either succeeds or fails with a typed reason, never panics.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_export::standalone::bundle;
use od_export::{AssetMeta, AssetSource, Options, StandaloneError};
use std::path::{Path, PathBuf};

struct Dir(PathBuf);

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        _ => "application/octet-stream",
    }
}

impl AssetSource for Dir {
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>> {
        Ok(std::fs::metadata(self.0.join(path))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| AssetMeta {
                mime: mime(path).to_owned(),
                size: m.len(),
            }))
    }
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.0.join(path))
    }
}

fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

#[test]
fn bundled_templates_bundle_or_fail_typed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates");
    let mut files = Vec::new();
    html_files(&root, &mut files);
    assert!(
        files.len() > 100,
        "found only {} template HTML files",
        files.len()
    );
    let (mut ok, mut missing, mut other) = (0, 0, Vec::new());
    for file in &files {
        // Each template folder is its own project root.
        let rel = file.strip_prefix(&root).unwrap();
        let template = root.join(rel.components().next().unwrap());
        let entry = file
            .strip_prefix(&template)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let html = std::fs::read_to_string(file).unwrap();
        match bundle(&entry, &html, &Dir(template.clone()), &Options::default()) {
            Ok(out) => {
                ok += 1;
                assert!(out.html.len() >= html.len() / 2, "{entry} shrank");
            }
            Err(StandaloneError::MissingDependency { .. }) => missing += 1,
            Err(e @ (StandaloneError::Io(_) | StandaloneError::InvalidSource { .. })) => {
                other.push(format!("{}: {e}", rel.display()));
            }
            Err(_) => {}
        }
    }
    println!(
        "{} files: {ok} bundled, {missing} with a missing local dependency",
        files.len()
    );
    assert!(other.is_empty(), "{other:#?}");
    assert!(ok * 2 > files.len(), "only {ok} of {} bundled", files.len());
}
