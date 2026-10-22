//! One-file HTML export of a bundled deck template (html-ppt demo deck:
//! CSS, fonts, runtime JS). No spec 5.5 row covers export; this keeps the
//! bundler's cost visible.
//!
//! `cargo bench -p od-export --bench standalone`

use criterion::{Criterion, criterion_group, criterion_main};
use od_export::standalone::bundle;
use od_export::{AssetMeta, AssetSource, Options};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Files under a folder, typed by extension.
struct Dir(PathBuf);

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

impl AssetSource for Dir {
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>> {
        match std::fs::metadata(self.0.join(path)) {
            Ok(m) if m.is_file() => Ok(Some(AssetMeta {
                mime: mime(path).to_owned(),
                size: m.len(),
            })),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.0.join(path))
    }
}

fn bench_bundle(c: &mut Criterion) {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/design-templates/html-ppt");
    let entry = "examples/demo-deck/index.html";
    let html = std::fs::read_to_string(root.join(entry)).unwrap_or_default();
    let source = Dir(root);
    let options = Options {
        lenient: true,
        ..Options::default()
    };
    if let Ok(out) = bundle(entry, &html, &source, &options) {
        println!(
            "bundled {} files into {} bytes",
            out.embedded,
            out.html.len()
        );
    }
    let mut group = c.benchmark_group("export");
    group
        .sample_size(30)
        .measurement_time(Duration::from_secs(10));
    group.bench_function("standalone_html_ppt_demo", |b| {
        b.iter(|| black_box(bundle(entry, &html, &source, &options).is_ok()))
    });
    group.finish();
}

criterion_group!(benches, bench_bundle);
criterion_main!(benches);
