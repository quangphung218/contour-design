//! Replay 50,000 events from `events.jsonl` (spec 5.5: ≤ 500 ms).

#![allow(clippy::unwrap_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use od_contracts::{RunEndPayload, RunStatus, sse};
use od_runs::{RunMeta, Runs};
use serde_json::json;

fn replay_50k(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let runs = Runs::new(tmp.path());
    let (run, _) = runs
        .create_or_reuse(RunMeta {
            run_id: "bench".to_owned(),
            agent_id: "claude".to_owned(),
            ..RunMeta::default()
        })
        .unwrap();
    for i in 0..50_000 {
        run.emit(
            sse::AGENT,
            json!({"type": "text_delta", "delta": format!("chunk {i} of the streamed answer")}),
        );
    }
    run.finish(RunEndPayload {
        status: RunStatus::Succeeded,
        code: Some(0),
        signal: None,
        terminal_at: 0,
        artifact_paths: Vec::new(),
        resumable: false,
        retryable: false,
        failure_category: None,
        error: None,
    });
    drop(runs);
    let mut group = c.benchmark_group("runs");
    group.sample_size(10);
    group.bench_function("replay_50k_from_jsonl", |b| {
        b.iter(|| {
            let (runs, _) = Runs::load(tmp.path()).unwrap();
            let run = runs.get("bench").unwrap();
            runtime.block_on(async {
                let mut sub = run.subscribe(0);
                let mut n = 0u32;
                while sub.next().await.is_some() {
                    n += 1;
                }
                assert_eq!(n, 50_001);
            });
        });
    });
    group.finish();
}

criterion_group!(benches, replay_50k);
criterion_main!(benches);
