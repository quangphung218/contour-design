//! The media task queue (spec 1.5 media gen): generations run in the
//! background, at most `concurrency` at a time, in submit order. A task can
//! be canceled while queued or running (the provider request is dropped).
//!
//! Every state change is persisted through [`TaskPersist`]. After a
//! restart, [`TaskQueue::restore`] queues the tasks that never started
//! again and marks the ones that were running `interrupted`: the provider
//! may already have charged for them, so they are not repeated silently.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};
use tokio_util::sync::CancellationToken;

/// Default number of generations running at once.
pub const DEFAULT_CONCURRENCY: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
    /// Was running when the daemon stopped.
    Interrupted,
}

impl TaskStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Interrupted => "interrupted",
        }
    }
}

/// One task as stored and as returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub project_id: String,
    pub status: TaskStatus,
    /// The `POST …/media/generate` body.
    pub request: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
}

/// Where task records survive a restart.
pub trait TaskPersist: Send + Sync {
    fn save(&self, record: &TaskRecord);
    fn load_all(&self) -> Vec<TaskRecord>;
}

/// In-memory [`TaskPersist`], for tests and embedders without a database.
#[derive(Debug, Default)]
pub struct MemoryPersist(Mutex<HashMap<String, TaskRecord>>);

impl TaskPersist for MemoryPersist {
    fn save(&self, record: &TaskRecord) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(record.id.clone(), record.clone());
    }

    fn load_all(&self) -> Vec<TaskRecord> {
        let mut all: Vec<TaskRecord> = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        all.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        all
    }
}

pub type WorkFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;
/// Runs one task: generate, write the file, return the result JSON.
pub type Worker = Arc<dyn Fn(TaskRecord) -> WorkFuture + Send + Sync>;

struct Entry {
    tx: watch::Sender<TaskRecord>,
    cancel: CancellationToken,
}

struct Inner {
    tasks: Mutex<HashMap<String, Entry>>,
    slots: Arc<Semaphore>,
    persist: Arc<dyn TaskPersist>,
    worker: Worker,
}

/// Cheap to clone.
#[derive(Clone)]
pub struct TaskQueue {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for TaskQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskQueue")
            .field("free_slots", &self.inner.slots.available_permits())
            .finish_non_exhaustive()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Strictly increasing creation stamps, so submit order survives a
/// restart even for tasks queued in the same millisecond.
fn next_created_at() -> i64 {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = now_ms();
    let prev = LAST
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
            Some(now.max(last + 1))
        })
        .unwrap_or(now);
    now.max(prev + 1)
}

impl TaskQueue {
    #[must_use]
    pub fn new(concurrency: usize, persist: Arc<dyn TaskPersist>, worker: Worker) -> Self {
        Self {
            inner: Arc::new(Inner {
                tasks: Mutex::default(),
                slots: Arc::new(Semaphore::new(concurrency.max(1))),
                persist,
                worker,
            }),
        }
    }

    fn tasks(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.inner
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Persists and publishes a change to task `id`.
    fn update(&self, id: &str, f: impl FnOnce(&mut TaskRecord)) -> Option<TaskRecord> {
        let tasks = self.tasks();
        let entry = tasks.get(id)?;
        let mut record = entry.tx.borrow().clone();
        f(&mut record);
        self.inner.persist.save(&record);
        entry.tx.send_replace(record.clone());
        Some(record)
    }

    /// Queues a new task; it starts when a slot is free.
    #[must_use]
    pub fn submit(&self, project_id: &str, request: Value) -> TaskRecord {
        let record = TaskRecord {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            status: TaskStatus::Queued,
            request,
            result: None,
            error: None,
            created_at: next_created_at(),
            started_at: None,
            ended_at: None,
        };
        self.inner.persist.save(&record);
        self.spawn(record.clone());
        record
    }

    fn spawn(&self, record: TaskRecord) {
        let (tx, _) = watch::channel(record.clone());
        let cancel = CancellationToken::new();
        self.tasks().insert(
            record.id.clone(),
            Entry {
                tx,
                cancel: cancel.clone(),
            },
        );
        if record.status.is_terminal() {
            return;
        }
        let queue = self.clone();
        tokio::spawn(async move { queue.drive(record.id, cancel).await });
    }

    async fn drive(self, id: String, cancel: CancellationToken) {
        let permit = tokio::select! {
            () = cancel.cancelled() => return,
            p = Arc::clone(&self.inner.slots).acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => return,
            },
        };
        // A cancel can land between the slot and this check.
        if cancel.is_cancelled() {
            return;
        }
        // Or between the check and here: `cancel` persists before it fires
        // the token, and the canceled record must stay canceled.
        let mut started = false;
        let Some(record) = self.update(&id, |r| {
            if r.status.is_terminal() {
                return;
            }
            r.status = TaskStatus::Running;
            r.started_at = Some(now_ms());
            started = true;
        }) else {
            return;
        };
        if !started {
            return;
        }
        let work = (self.inner.worker)(record);
        let outcome = tokio::select! {
            () = cancel.cancelled() => None,
            out = work => Some(out),
        };
        drop(permit);
        let Some(outcome) = outcome else {
            return;
        };
        self.update(&id, |r| {
            if r.status.is_terminal() {
                return;
            }
            r.ended_at = Some(now_ms());
            match outcome {
                Ok(v) => {
                    r.status = TaskStatus::Done;
                    r.result = Some(v);
                }
                Err(e) => {
                    r.status = TaskStatus::Failed;
                    r.error = Some(e);
                }
            }
        });
    }

    /// Cancels a queued or running task. Returns the record, or `None` for
    /// an unknown id. Terminal tasks are returned unchanged.
    #[must_use]
    pub fn cancel(&self, id: &str) -> Option<TaskRecord> {
        let token = self.tasks().get(id)?.cancel.clone();
        let record = self.update(id, |r| {
            if !r.status.is_terminal() {
                r.status = TaskStatus::Canceled;
                r.ended_at = Some(now_ms());
            }
        })?;
        token.cancel();
        Some(record)
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<TaskRecord> {
        self.tasks().get(id).map(|e| e.tx.borrow().clone())
    }

    /// Tasks, oldest first; optionally one project's.
    #[must_use]
    pub fn list(&self, project_id: Option<&str>) -> Vec<TaskRecord> {
        let mut all: Vec<TaskRecord> = self
            .tasks()
            .values()
            .map(|e| e.tx.borrow().clone())
            .filter(|r| project_id.is_none_or(|p| r.project_id == p))
            .collect();
        all.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        all
    }

    /// Waits until the task is terminal or `timeout` passes; returns its
    /// latest record either way.
    pub async fn wait(&self, id: &str, timeout: Duration) -> Option<TaskRecord> {
        let mut rx = self.tasks().get(id)?.tx.subscribe();
        let done = tokio::time::timeout(timeout, rx.wait_for(|r| r.status.is_terminal())).await;
        drop(done);
        let latest = rx.borrow().clone();
        Some(latest)
    }

    /// Loads persisted tasks after a restart: `queued` ones go back in the
    /// queue (in their original order), `running` ones become
    /// `interrupted`. Returns how many were requeued and interrupted.
    pub fn restore(&self) -> (usize, usize) {
        let (mut requeued, mut interrupted) = (0, 0);
        for mut record in self.inner.persist.load_all() {
            if self.tasks().contains_key(&record.id) {
                continue;
            }
            match record.status {
                TaskStatus::Queued => requeued += 1,
                TaskStatus::Running => {
                    record.status = TaskStatus::Interrupted;
                    record.ended_at = Some(now_ms());
                    record.error = Some("the daemon stopped while this task was running".into());
                    self.inner.persist.save(&record);
                    interrupted += 1;
                }
                _ => {}
            }
            self.spawn(record);
        }
        (requeued, interrupted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;

    /// Worker that blocks until released; counts peak concurrency.
    struct Gate {
        running: AtomicUsize,
        peak: AtomicUsize,
        release: tokio::sync::Notify,
        order: Mutex<Vec<String>>,
    }

    fn gated(gate: &Arc<Gate>) -> Worker {
        let gate = Arc::clone(gate);
        Arc::new(move |r: TaskRecord| {
            let gate = Arc::clone(&gate);
            Box::pin(async move {
                /// Decrements on drop: a canceled worker is dropped mid-await.
                struct Running<'a>(&'a AtomicUsize);
                impl Drop for Running<'_> {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                let now = gate.running.fetch_add(1, Ordering::SeqCst) + 1;
                let _running = Running(&gate.running);
                gate.peak.fetch_max(now, Ordering::SeqCst);
                gate.order.lock().unwrap().push(r.request["n"].to_string());
                gate.release.notified().await;
                if r.request["fail"] == json!(true) {
                    Err("provider said no".to_owned())
                } else {
                    Ok(json!({"name": format!("media/{}.png", r.request["n"])}))
                }
            })
        })
    }

    fn gate() -> Arc<Gate> {
        Arc::new(Gate {
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            release: tokio::sync::Notify::new(),
            order: Mutex::default(),
        })
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn runs_at_most_n_at_once_in_order() {
        let g = gate();
        let q = TaskQueue::new(2, Arc::new(MemoryPersist::default()), gated(&g));
        let ids: Vec<String> = (0..5)
            .map(|n| q.submit("p", json!({"n": n, "fail": n == 4})).id)
            .collect();
        settle().await;
        assert_eq!(g.running.load(Ordering::SeqCst), 2);
        assert_eq!(q.get(&ids[2]).unwrap().status, TaskStatus::Queued);
        for _ in 0..5 {
            g.release.notify_waiters();
            settle().await;
        }
        let last = q.wait(&ids[4], Duration::from_secs(2)).await.unwrap();
        assert_eq!(last.status, TaskStatus::Failed);
        assert_eq!(last.error.as_deref(), Some("provider said no"));
        let first = q.get(&ids[0]).unwrap();
        assert_eq!(first.status, TaskStatus::Done);
        assert_eq!(first.result, Some(json!({"name": "media/0.png"})));
        assert!(first.started_at.is_some() && first.ended_at.is_some());
        assert_eq!(g.peak.load(Ordering::SeqCst), 2);
        assert_eq!(*g.order.lock().unwrap(), ["0", "1", "2", "3", "4"]);
        assert_eq!(q.list(Some("p")).len(), 5);
        assert!(q.list(Some("other")).is_empty());
    }

    #[tokio::test]
    async fn cancel_queued_and_running() {
        let g = gate();
        let q = TaskQueue::new(1, Arc::new(MemoryPersist::default()), gated(&g));
        let running = q.submit("p", json!({"n": 0})).id;
        let queued = q.submit("p", json!({"n": 1})).id;
        let after = q.submit("p", json!({"n": 2})).id;
        settle().await;
        assert_eq!(q.cancel(&queued).unwrap().status, TaskStatus::Canceled);
        assert_eq!(q.cancel(&running).unwrap().status, TaskStatus::Canceled);
        settle().await;
        // The freed slot goes to the next live task; the canceled one never ran.
        assert_eq!(g.running.load(Ordering::SeqCst), 1);
        assert_eq!(*g.order.lock().unwrap(), ["0", "2"]);
        assert_eq!(q.get(&after).unwrap().status, TaskStatus::Running);
        g.release.notify_waiters();
        let done = q.wait(&after, Duration::from_secs(2)).await.unwrap();
        assert_eq!(done.status, TaskStatus::Done);
        // Canceling a finished task changes nothing; unknown ids are None.
        assert_eq!(q.cancel(&after).unwrap().status, TaskStatus::Done);
        assert_eq!(q.get(&running).unwrap().status, TaskStatus::Canceled);
        assert!(q.cancel("nope").is_none());
    }

    #[tokio::test]
    async fn restore_requeues_queued_and_interrupts_running() {
        let persist = Arc::new(MemoryPersist::default());
        let g = gate();
        let before = TaskQueue::new(1, Arc::clone(&persist) as Arc<dyn TaskPersist>, gated(&g));
        let a = before.submit("p", json!({"n": 0})).id;
        let b = before.submit("p", json!({"n": 1})).id;
        let c = before.submit("p", json!({"n": 2})).id;
        settle().await;
        assert_eq!(before.cancel(&c).unwrap().status, TaskStatus::Canceled);
        // "Restart": a new queue over the same records; the old one is gone.
        drop(before);
        let g2 = gate();
        let after = TaskQueue::new(1, Arc::clone(&persist) as Arc<dyn TaskPersist>, gated(&g2));
        assert_eq!(after.restore(), (1, 1));
        assert_eq!(after.restore(), (0, 0), "restore is idempotent");
        let a_rec = after.get(&a).unwrap();
        assert_eq!(a_rec.status, TaskStatus::Interrupted);
        assert!(a_rec.error.is_some());
        assert_eq!(after.get(&c).unwrap().status, TaskStatus::Canceled);
        settle().await;
        assert_eq!(*g2.order.lock().unwrap(), ["1"]);
        g2.release.notify_waiters();
        let b_rec = after.wait(&b, Duration::from_secs(2)).await.unwrap();
        assert_eq!(b_rec.status, TaskStatus::Done);
        let stored = persist.load_all();
        assert_eq!(
            stored.iter().map(|r| r.status).collect::<Vec<_>>(),
            [
                TaskStatus::Interrupted,
                TaskStatus::Done,
                TaskStatus::Canceled
            ]
        );
    }

    #[tokio::test]
    async fn a_cancel_racing_a_freed_slot_never_runs() {
        // The running task and the queued one are canceled back to back: the
        // freed slot and the cancel reach the queued task together.
        for _ in 0..30 {
            let g = gate();
            let q = TaskQueue::new(1, Arc::new(MemoryPersist::default()), gated(&g));
            let a = q.submit("p", json!({"n": 0})).id;
            let b = q.submit("p", json!({"n": 1})).id;
            settle().await;
            let _ = q.cancel(&a);
            let _ = q.cancel(&b);
            settle().await;
            assert_eq!(*g.order.lock().unwrap(), ["0"], "the canceled task ran");
            assert_eq!(q.get(&b).unwrap().status, TaskStatus::Canceled);
        }
    }

    /// Frees the slot while a cancel of another task is being persisted:
    /// the running task's token is canceled, so it leaves without waiting
    /// for the queue's lock and the queued task takes the slot before its
    /// own token is canceled.
    struct SlowCancel {
        inner: MemoryPersist,
        free_slot: std::sync::OnceLock<CancellationToken>,
    }

    impl TaskPersist for SlowCancel {
        fn save(&self, record: &TaskRecord) {
            if record.status == TaskStatus::Canceled {
                if let Some(token) = self.free_slot.get() {
                    token.cancel();
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
            self.inner.save(record);
        }

        fn load_all(&self) -> Vec<TaskRecord> {
            self.inner.load_all()
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_slot_freed_on_another_thread_mid_cancel_never_runs_the_task() {
        let g = gate();
        let persist = Arc::new(SlowCancel {
            inner: MemoryPersist::default(),
            free_slot: std::sync::OnceLock::new(),
        });
        // Counts calls, not polls: a worker built for a canceled task is a bug
        // even when `select!` happens to drop it unpolled.
        let calls = Arc::new(AtomicUsize::new(0));
        let (inner, counted) = (gated(&g), Arc::clone(&calls));
        let worker: Worker = Arc::new(move |r: TaskRecord| {
            counted.fetch_add(1, Ordering::SeqCst);
            inner(r)
        });
        let q = TaskQueue::new(1, Arc::clone(&persist) as Arc<dyn TaskPersist>, worker);
        let a = q.submit("p", json!({"n": 0})).id;
        // `b` only once `a` holds the slot: spawned together, either may get
        // it first on a multi-thread runtime.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while q.get(&a).unwrap().status != TaskStatus::Running
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(q.get(&a).unwrap().status, TaskStatus::Running);
        let b = q.submit("p", json!({"n": 1})).id;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(q.get(&b).unwrap().status, TaskStatus::Queued);
        let token = q.tasks().get(&a).unwrap().cancel.clone();
        persist.free_slot.set(token).unwrap();
        // Off the workers, so the woken tasks run while the cancel sleeps.
        let (q2, b2) = (q.clone(), b.clone());
        let canceled = tokio::task::spawn_blocking(move || q2.cancel(&b2))
            .await
            .unwrap();
        assert_eq!(canceled.unwrap().status, TaskStatus::Canceled);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(q.get(&b).unwrap().status, TaskStatus::Canceled);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the canceled task ran");
        assert_eq!(persist.load_all()[1].status, TaskStatus::Canceled);
    }

    #[tokio::test]
    async fn a_cancel_landing_as_work_finishes_wins() {
        // The worker itself cancels its task just before returning Ok: the
        // caller was told "canceled", so the result must not overwrite it.
        let cell: Arc<std::sync::OnceLock<TaskQueue>> = Arc::default();
        let c2 = Arc::clone(&cell);
        let worker: Worker = Arc::new(move |r: TaskRecord| {
            let c3 = Arc::clone(&c2);
            Box::pin(async move {
                let canceled = c3.get().unwrap().cancel(&r.id).unwrap();
                assert_eq!(canceled.status, TaskStatus::Canceled);
                Ok(json!({"name": "late.png"}))
            })
        });
        let q = TaskQueue::new(1, Arc::new(MemoryPersist::default()), worker);
        cell.set(q.clone()).unwrap();
        let id = q.submit("p", json!({})).id;
        settle().await;
        let r = q.get(&id).unwrap();
        assert_eq!(r.status, TaskStatus::Canceled);
        assert!(r.result.is_none());
    }

    #[tokio::test]
    async fn wait_times_out_with_the_live_record() {
        let g = gate();
        let q = TaskQueue::new(1, Arc::new(MemoryPersist::default()), gated(&g));
        let id = q.submit("p", json!({"n": 0})).id;
        settle().await;
        let r = q.wait(&id, Duration::from_millis(30)).await.unwrap();
        assert_eq!(r.status, TaskStatus::Running);
        assert!(q.wait("nope", Duration::from_millis(1)).await.is_none());
    }
}
