//! Replay and restart (spec 2.2, 5.7 M2).

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use od_contracts::{FailureCategory, RunEndPayload, RunStatus, sse};
use od_runs::{RING_CAPACITY, Run, RunMeta, Runs};
use serde_json::json;
use std::sync::Arc;

fn meta(id: &str) -> RunMeta {
    RunMeta {
        run_id: id.to_owned(),
        agent_id: "claude".to_owned(),
        created_at: od_runs::now_ms(),
        prompt: None,
        ..RunMeta::default()
    }
}

fn end(status: RunStatus) -> RunEndPayload {
    RunEndPayload {
        status,
        code: Some(0),
        signal: None,
        terminal_at: od_runs::now_ms(),
        artifact_paths: Vec::new(),
        resumable: false,
        retryable: false,
        failure_category: None,
        error: None,
    }
}

async fn drain_ids(run: &Arc<Run>, after: u64) -> Vec<u64> {
    let mut sub = run.subscribe(after);
    let mut ids = Vec::new();
    while let Some(ev) = sub.next().await {
        ids.push(ev.id);
    }
    ids
}

#[tokio::test]
async fn reconnect_after_k_gets_exactly_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = Runs::new(tmp.path());
    let (run, reused) = runs.create_or_reuse(meta("r1")).unwrap();
    assert!(!reused);
    // More than the ring holds, so the replay must stitch disk + ring.
    let total = (RING_CAPACITY + 500) as u64;
    for i in 0..total {
        run.emit(
            sse::AGENT,
            json!({"type": "text_delta", "delta": i.to_string()}),
        );
    }
    run.finish(end(RunStatus::Succeeded));
    let last = total + 1;
    for k in [0, 1, 7, 499, 500, 501, total - 3, total, last] {
        let ids = drain_ids(&run, k).await;
        let want: Vec<u64> = (k + 1..=last).collect();
        assert_eq!(ids, want, "after={k}");
    }
}

#[tokio::test]
async fn live_events_during_replay_are_neither_lost_nor_duplicated() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = Runs::new(tmp.path());
    let (run, _) = runs.create_or_reuse(meta("r2")).unwrap();
    for i in 0..3000 {
        run.emit(sse::STDOUT, json!({"chunk": i.to_string()}));
    }
    let producer = {
        let run = Arc::clone(&run);
        tokio::spawn(async move {
            for i in 0..3000 {
                run.emit(sse::STDOUT, json!({"chunk": i.to_string()}));
                if i % 100 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            run.finish(end(RunStatus::Succeeded));
        })
    };
    // Disconnect at k, reconnect with Last-Event-ID: k while the run is live.
    let mut first = run.subscribe(0);
    let mut seen = Vec::new();
    while let Some(ev) = first.next().await {
        seen.push(ev.id);
        if ev.id == 1234 {
            break;
        }
    }
    drop(first);
    let mut second = run.subscribe(1234);
    while let Some(ev) = second.next().await {
        seen.push(ev.id);
    }
    producer.await.unwrap();
    let want: Vec<u64> = (1..=6001).collect();
    assert_eq!(seen, want);
}

#[tokio::test]
async fn restart_mid_run_ends_it_failed_and_keeps_history() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let runs = Runs::new(tmp.path());
        let mut m = meta("r3");
        m.client_request_id = Some("req-1".to_owned());
        let (run, _) = runs.create_or_reuse(m).unwrap();
        run.emit(sse::START, json!({"runId": "r3"}));
        run.set_process(Some(999_999), Some("/bin/claude".to_owned()));
        for i in 0..10 {
            run.emit(
                sse::AGENT,
                json!({"type": "text_delta", "delta": i.to_string()}),
            );
        }
        // Simulated crash: no `end`. The periodic flusher got the buffered
        // lines out; then the process vanished.
        std::thread::sleep(od_runs::FLUSH_INTERVAL);
        run.flush_if_due();
    }
    let (runs, report) = Runs::load(tmp.path()).unwrap();
    assert_eq!(report.loaded, 1);
    assert_eq!(report.interrupted.len(), 1);
    let orphan = report.interrupted[0].1.clone().unwrap();
    assert_eq!(orphan.pgid, 999_999);
    let run = runs.get("r3").unwrap();
    assert_eq!(run.status(), RunStatus::Failed);
    let end = run.end_payload().unwrap();
    assert_eq!(end.failure_category, Some(FailureCategory::DaemonRestart));

    let mut sub = run.subscribe(0);
    let mut events = Vec::new();
    while let Some(ev) = sub.next().await {
        events.push((ev.id, ev.event.clone()));
    }
    let names: Vec<&str> = events.iter().map(|(_, e)| e.as_str()).collect();
    assert_eq!(names.first(), Some(&sse::START));
    assert_eq!(&names[names.len() - 2..], [sse::ERROR, sse::END]);
    let ids: Vec<u64> = events.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, (1..=13).collect::<Vec<_>>());

    // The clientRequestId index survives the restart.
    let mut again = meta("other");
    again.client_request_id = Some("req-1".to_owned());
    let (same, reused) = runs.create_or_reuse(again).unwrap();
    assert!(reused);
    assert_eq!(same.id(), "r3");

    // A second restart does not end it again.
    drop(runs);
    let (_, report) = Runs::load(tmp.path()).unwrap();
    assert!(report.interrupted.is_empty());
}

#[tokio::test]
async fn client_request_id_with_other_request_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = Runs::new(tmp.path());
    let mut a = meta("a");
    a.client_request_id = Some("k".to_owned());
    a.request_hash = Some("h1".to_owned());
    runs.create_or_reuse(a).unwrap();
    let mut b = meta("b");
    b.client_request_id = Some("k".to_owned());
    b.request_hash = Some("h2".to_owned());
    assert!(matches!(
        runs.create_or_reuse(b),
        Err(od_runs::RunsError::IdempotencyConflict)
    ));
}

#[tokio::test]
async fn nothing_follows_end() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = Runs::new(tmp.path());
    let (run, _) = runs.create_or_reuse(meta("e")).unwrap();
    run.finish(end(RunStatus::Canceled));
    assert!(run.finish(end(RunStatus::Succeeded)).is_none());
    assert!(run.emit(sse::AGENT, json!({})).is_none());
    assert_eq!(run.status(), RunStatus::Canceled);
}

#[test]
fn a_run_saved_with_the_whole_message_lists_only_its_first_line() {
    let tmp = tempfile::tempdir().unwrap();
    let asked = "\nA tea shop landing page\nWarm colours, a menu, opening hours.";
    {
        let runs = Runs::new(tmp.path());
        let mut m = meta("old");
        m.prompt = Some(asked.to_owned());
        let (run, _) = runs.create_or_reuse(m).unwrap();
        assert_eq!(
            run.info().prompt.as_deref(),
            Some("A tea shop landing page")
        );
        run.finish(end(RunStatus::Succeeded));
    }
    // A daemon from before kept the whole message in state.json.
    let state = tmp.path().join("old/state.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    v["prompt"] = json!(asked);
    std::fs::write(&state, serde_json::to_vec(&v).unwrap()).unwrap();
    let (runs, _) = Runs::load(tmp.path()).unwrap();
    let run = runs.get("old").unwrap();
    assert_eq!(
        run.info().prompt.as_deref(),
        Some("A tea shop landing page")
    );
    assert_eq!(
        run.meta().prompt.as_deref(),
        Some("A tea shop landing page")
    );
}
