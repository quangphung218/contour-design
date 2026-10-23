//! Scheduler with a mock launcher: fire when due, history, no overlap,
//! interrupted runs after a crash.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use futures_util::FutureExt;
use od_automation::{
    AutomationError, Launched, Launcher, NewRoutine, Routine, RoutinePatch, RoutineStore, RunEnd,
    RunState, Scheduler, TargetMode, Trigger,
};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Each launch hands its "run end" sender to the test.
#[derive(Default)]
struct Mock {
    ends: Mutex<Vec<oneshot::Sender<RunEnd>>>,
    launched: Mutex<Vec<String>>,
    fail: bool,
}

impl Launcher for Mock {
    fn launch(
        &self,
        routine: Routine,
    ) -> futures_util::future::BoxFuture<'static, Result<Launched, String>> {
        if self.fail {
            return async { Err("agent not installed".to_owned()) }.boxed();
        }
        let (tx, rx) = oneshot::channel();
        self.ends.lock().unwrap().push(tx);
        let n = {
            let mut l = self.launched.lock().unwrap();
            l.push(routine.id.clone());
            l.len()
        };
        async move {
            Ok(Launched {
                run_id: format!("run-{n}"),
                conversation_id: format!("conv-{n}"),
                project_id: routine.project_id.clone(),
                done: rx
                    .map(|r| {
                        r.unwrap_or(RunEnd {
                            status: RunState::Failed,
                            error: Some("dropped".into()),
                        })
                    })
                    .boxed(),
            })
        }
        .boxed()
    }
}

fn new_routine(schedule: &str) -> NewRoutine {
    NewRoutine {
        name: "Nightly".into(),
        project_id: "p1".into(),
        target_mode: TargetMode::Reuse,
        agent_id: "claude".into(),
        prompt: "Refresh the dashboard".into(),
        schedule: schedule.into(),
        enabled: true,
    }
}

async fn settle(s: &Scheduler, id: &str) {
    for _ in 0..200 {
        if !s.is_running(id) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("routine still running");
}

#[tokio::test]
async fn fires_when_due_records_history_and_never_overlaps() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(RoutineStore::open(&dir.path().join("automations.sqlite")).unwrap());
    let mock = Arc::new(Mock::default());
    let sched = Arc::new(Scheduler::new(Arc::clone(&store), mock.clone()));
    let r = store
        .create(&new_routine("* * * * *"), |_| Some(1_000))
        .unwrap();
    assert_eq!(r.next_run_at, Some(1_000));

    assert!(sched.tick(999).await.unwrap().is_empty(), "not due yet");
    let fired = sched.tick(1_000).await.unwrap();
    assert_eq!(fired.len(), 1);
    assert_eq!(
        (fired[0].status, fired[0].trigger),
        (RunState::Running, Trigger::Schedule)
    );
    assert_eq!(fired[0].run_id.as_deref(), Some("run-1"));
    let next = store.get(&r.id).unwrap().next_run_at.unwrap();
    assert!(next > 1_000, "moved to the next minute");

    // Due again while run-1 still goes: skipped, nothing launched.
    store.set_next_run(&r.id, Some(2_000)).unwrap();
    let skipped = sched.tick(2_000).await.unwrap();
    assert_eq!(skipped[0].status, RunState::Skipped);
    assert_eq!(mock.launched.lock().unwrap().len(), 1);
    assert!(matches!(
        sched.run_now(&r.id).await,
        Err(AutomationError::Busy)
    ));

    mock.ends
        .lock()
        .unwrap()
        .remove(0)
        .send(RunEnd {
            status: RunState::Succeeded,
            error: None,
        })
        .unwrap();
    settle(&sched, &r.id).await;
    let manual = sched.run_now(&r.id).await.unwrap();
    assert_eq!(manual.trigger, Trigger::Manual);
    mock.ends
        .lock()
        .unwrap()
        .remove(0)
        .send(RunEnd {
            status: RunState::Failed,
            error: Some("boom".into()),
        })
        .unwrap();
    settle(&sched, &r.id).await;

    let history = store.runs(&r.id, 10).unwrap();
    let states: Vec<_> = history.iter().map(|h| (h.trigger, h.status)).collect();
    assert_eq!(
        states,
        [
            (Trigger::Manual, RunState::Failed),
            (Trigger::Schedule, RunState::Skipped),
            (Trigger::Schedule, RunState::Succeeded),
        ]
    );
    assert_eq!(history[0].error.as_deref(), Some("boom"));
    assert!(history.iter().all(|h| h.ended_at.is_some()));
    assert_eq!(
        store.get(&r.id).unwrap().last_run.unwrap().id,
        history[0].id
    );
}

#[tokio::test]
async fn disabled_routines_do_not_fire_and_launch_errors_are_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(RoutineStore::open(&dir.path().join("a.sqlite")).unwrap());
    let mock = Arc::new(Mock {
        fail: true,
        ..Mock::default()
    });
    let sched = Arc::new(Scheduler::new(Arc::clone(&store), mock));
    let r = store.create(&new_routine("@hourly"), |_| Some(10)).unwrap();
    let off = store
        .update(
            &r.id,
            &RoutinePatch {
                enabled: Some(false),
                ..RoutinePatch::default()
            },
            |_| Some(10),
        )
        .unwrap();
    assert_eq!((off.enabled, off.next_run_at), (false, None));
    assert!(sched.tick(i64::MAX).await.unwrap().is_empty());

    let on = store
        .update(
            &r.id,
            &RoutinePatch {
                enabled: Some(true),
                ..RoutinePatch::default()
            },
            |_| Some(10),
        )
        .unwrap();
    assert_eq!(on.next_run_at, Some(10));
    let fired = sched.tick(10).await.unwrap();
    assert_eq!(fired[0].status, RunState::Failed);
    assert_eq!(fired[0].error.as_deref(), Some("agent not installed"));
    assert!(
        !sched.is_running(&r.id),
        "a failed launch frees the routine"
    );
}

#[tokio::test]
async fn invalid_input_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = RoutineStore::open(&dir.path().join("a.sqlite")).unwrap();
    let cases = [
        NewRoutine {
            schedule: "61 * * * *".into(),
            ..new_routine("")
        },
        NewRoutine {
            name: "  ".into(),
            ..new_routine("@daily")
        },
        NewRoutine {
            name: "x".repeat(121),
            ..new_routine("@daily")
        },
        NewRoutine {
            name: "a\u{7}b".into(),
            ..new_routine("@daily")
        },
        NewRoutine {
            prompt: " ".into(),
            ..new_routine("@daily")
        },
        NewRoutine {
            prompt: "x".repeat(20_001),
            ..new_routine("@daily")
        },
        NewRoutine {
            agent_id: "../claude".into(),
            ..new_routine("@daily")
        },
        NewRoutine {
            project_id: "p 1".into(),
            ..new_routine("@daily")
        },
    ];
    for c in cases {
        assert!(
            matches!(store.create(&c, |_| None), Err(AutomationError::Invalid(_))),
            "{c:?}"
        );
    }
    let r = store.create(&new_routine("@daily"), |_| None).unwrap();
    let bad = RoutinePatch {
        schedule: Some("* *".into()),
        ..RoutinePatch::default()
    };
    assert!(matches!(
        store.update(&r.id, &bad, |_| None),
        Err(AutomationError::Invalid(_))
    ));
    assert!(matches!(store.get("nope"), Err(AutomationError::NotFound)));
    assert!(store.delete(&r.id).unwrap());
    assert!(!store.delete(&r.id).unwrap());
}

#[tokio::test]
async fn a_crash_leaves_no_run_marked_running_and_resync_skips_missed_times() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sqlite");
    let id = {
        let store = Arc::new(RoutineStore::open(&path).unwrap());
        let mock = Arc::new(Mock::default());
        let sched = Arc::new(Scheduler::new(Arc::clone(&store), mock.clone()));
        let r = store
            .create(&new_routine("* * * * *"), |_| Some(5))
            .unwrap();
        sched.tick(5).await.unwrap();
        // Daemon dies with the run in flight; leak the sender.
        std::mem::forget(mock.ends.lock().unwrap().pop());
        store.set_next_run(&r.id, Some(6)).unwrap();
        r.id
    };
    let store = Arc::new(RoutineStore::open(&path).unwrap());
    let h = store.runs(&id, 10).unwrap();
    assert_eq!(h[0].status, RunState::Failed);
    assert!(h[0].error.as_deref().unwrap().contains("interrupted"));

    let sched = Arc::new(Scheduler::new(
        Arc::clone(&store),
        Arc::new(Mock::default()),
    ));
    let now = chrono::Local::now().timestamp_millis();
    sched.resync(now).unwrap();
    assert!(store.get(&id).unwrap().next_run_at.unwrap() > now);
    assert!(
        sched.tick(now).await.unwrap().is_empty(),
        "the missed time is not caught up"
    );
}

#[test]
fn create_each_run_needs_no_project_and_edits_switch_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let store = RoutineStore::open(&tmp.path().join("a.sqlite")).unwrap();
    let r = store
        .create(
            &NewRoutine {
                project_id: String::new(),
                target_mode: TargetMode::CreateEachRun,
                ..new_routine("CRON_TZ=Europe/Berlin 30 8 * * 1-5")
            },
            |_| Some(1),
        )
        .unwrap();
    assert_eq!(r.target_mode, TargetMode::CreateEachRun);
    assert_eq!(r.project_id, "");
    assert_eq!(r.schedule, "CRON_TZ=Europe/Berlin 30 8 * * 1-5");
    // Back to one project: the id is required then.
    let patch = RoutinePatch {
        target_mode: Some(TargetMode::Reuse),
        ..RoutinePatch::default()
    };
    assert!(matches!(
        store.update(&r.id, &patch, |_| None),
        Err(AutomationError::Invalid(_))
    ));
    let patch = RoutinePatch {
        target_mode: Some(TargetMode::Reuse),
        project_id: Some("p9".into()),
        name: Some("Renamed".into()),
        ..RoutinePatch::default()
    };
    let r = store.update(&r.id, &patch, |_| None).unwrap();
    assert_eq!((r.project_id.as_str(), r.name.as_str()), ("p9", "Renamed"));
    assert_eq!(r.target_mode, TargetMode::Reuse);
}

#[test]
fn an_old_database_gains_the_new_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.sqlite");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE routines (id TEXT PRIMARY KEY, name TEXT NOT NULL, project_id TEXT NOT NULL, \
             agent_id TEXT NOT NULL, prompt TEXT NOT NULL, schedule TEXT NOT NULL, enabled INTEGER NOT NULL, \
             next_run_at INTEGER, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL); \
             CREATE TABLE routine_runs (id TEXT PRIMARY KEY, routine_id TEXT NOT NULL, trigger TEXT NOT NULL, \
             status TEXT NOT NULL, run_id TEXT, conversation_id TEXT, error TEXT, started_at INTEGER NOT NULL, \
             ended_at INTEGER); \
             INSERT INTO routines VALUES ('r1', 'Old', 'p1', 'claude', 'x', '@daily', 1, NULL, 1, 1); \
             INSERT INTO routine_runs VALUES ('h1', 'r1', 'manual', 'succeeded', 'run', 'c', NULL, 2, 3);",
        )
        .unwrap();
    }
    // Twice: the upgrade is idempotent.
    drop(RoutineStore::open(&path).unwrap());
    let store = RoutineStore::open(&path).unwrap();
    let r = store.get("r1").unwrap();
    assert_eq!(r.target_mode, TargetMode::Reuse);
    let runs = store.runs("r1", 10).unwrap();
    assert_eq!(runs[0].project_id, None);
}
