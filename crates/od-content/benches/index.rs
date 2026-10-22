//! Spec 5.5 target: index all bundled content in ≤ 300 ms.
//!
//! `cargo bench -p od-content --bench index`

use criterion::{Criterion, criterion_group, criterion_main};
use od_content::{ContentIndex, ContentRoots};
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;

fn bench_index(c: &mut Criterion) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let roots = ContentRoots::bundled(&root);
    let index = ContentIndex::build(&roots);
    println!(
        "indexed {} skills, {} design templates, {} design systems, {} craft files",
        index.skills.len(),
        index.design_templates.len(),
        index.design_systems.len(),
        index.craft.len()
    );
    let mut group = c.benchmark_group("content");
    group
        .sample_size(30)
        .measurement_time(Duration::from_secs(10));
    group.bench_function("index_bundled", |b| {
        b.iter(|| black_box(ContentIndex::build(&roots)))
    });
    group.finish();
}

criterion_group!(benches, bench_index);
criterion_main!(benches);
