//! Automations (spec 1.5 "Automations, routines"): a routine runs a prompt
//! in a project on a local cron schedule, or on demand.
//!
//! - [`cron`] parses schedules and finds the next local fire time.
//! - [`store`] keeps routines and their run history.
//! - [`Scheduler`] fires due routines through a [`Launcher`] (the daemon
//!   starts a normal agent run) and never lets one routine overlap itself:
//!   a scheduled fire while the previous run still goes is recorded as
//!   `skipped`, a manual one is refused.
//!
//! Missed fire times while the daemon was down are not caught up: the next
//! run is computed from the current time.

pub mod cron;
pub mod store;

use chrono::{DateTime, Local, TimeZone};
use futures_util::future::BoxFuture;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use cron::Schedule;
pub use store::{
    AutomationError, NewRoutine, Routine, RoutinePatch, RoutineRun, RoutineStore, RunState, Trigger,
};

/// How a launched run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEnd {
    pub status: RunState,
    pub error: Option<String>,
}

/// A run the launcher started.
pub struct Launched {
    pub run_id: String,
    pub conversation_id: String,
    /// Resolves when the run ends.
    pub done: BoxFuture<'static, RunEnd>,
}

/// Starts the agent run for a routine.
pub trait Launcher: Send + Sync + 'static {
    fn launch(&self, routine: Routine) -> BoxFuture<'static, Result<Launched, String>>;
}

/// Next fire time after `now`, as epoch milliseconds.
#[must_use]
pub fn next_fire_ms(schedule: &Schedule, now: DateTime<Local>) -> Option<i64> {
    schedule.next_after(now).map(|t| t.timestamp_millis())
}

fn local(ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Local::now)
}

/// Fires routines. Cheap to share behind an `Arc`.
pub struct Scheduler {
    store: Arc<RoutineStore>,
    launcher: Arc<dyn Launcher>,
    /// Routines with a run in flight.
    running: Mutex<HashSet<String>>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler").finish_non_exhaustive()
    }
}

impl Scheduler {
    #[must_use]
    pub fn new(store: Arc<RoutineStore>, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            store,
            launcher,
            running: Mutex::default(),
        }
    }

    #[must_use]
    pub fn store(&self) -> &Arc<RoutineStore> {
        &self.store
    }

    fn running(&self) -> MutexGuard<'_, HashSet<String>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether a run of `routine_id` is in flight.
    #[must_use]
    pub fn is_running(&self, routine_id: &str) -> bool {
        self.running().contains(routine_id)
    }

    /// Fires every routine due at `now_ms` and moves each to its next time.
    /// Returns the history rows written (started or skipped).
    ///
    /// # Errors
    /// Database errors.
    pub async fn tick(self: &Arc<Self>, now_ms: i64) -> Result<Vec<RoutineRun>, AutomationError> {
        let due = self.store.due(now_ms)?;
        let mut fired = Vec::with_capacity(due.len());
        for routine in due {
            // Advance first: a slow launch must not fire it twice.
            let next = store::parse_schedule(&routine.schedule)
                .ok()
                .and_then(|s| next_fire_ms(&s, local(now_ms)));
            self.store.set_next_run(&routine.id, next)?;
            match self.fire(routine, Trigger::Schedule).await {
                Ok(run) => fired.push(run),
                Err(AutomationError::Busy) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(fired)
    }

    /// Runs a routine now, outside its schedule.
    ///
    /// # Errors
    /// [`AutomationError::NotFound`]; [`AutomationError::Busy`] while its
    /// previous run is still going.
    pub async fn run_now(
        self: &Arc<Self>,
        routine_id: &str,
    ) -> Result<RoutineRun, AutomationError> {
        let routine = self.store.get(routine_id)?;
        self.fire(routine, Trigger::Manual).await
    }

    async fn fire(
        self: &Arc<Self>,
        routine: Routine,
        trigger: Trigger,
    ) -> Result<RoutineRun, AutomationError> {
        if !self.running().insert(routine.id.clone()) {
            if trigger == Trigger::Schedule {
                tracing::info!(routine = %routine.id, "previous run still going; skipping this one");
                return self
                    .store
                    .begin_run(&routine.id, trigger, RunState::Skipped);
            }
            return Err(AutomationError::Busy);
        }
        let mut row = match self
            .store
            .begin_run(&routine.id, trigger, RunState::Running)
        {
            Ok(row) => row,
            Err(e) => {
                self.running().remove(&routine.id);
                return Err(e);
            }
        };
        let routine_id = routine.id.clone();
        match self.launcher.launch(routine).await {
            Ok(launched) => {
                let _ = self
                    .store
                    .attach_run(&row.id, &launched.run_id, &launched.conversation_id);
                row.run_id = Some(launched.run_id);
                row.conversation_id = Some(launched.conversation_id);
                let this = Arc::clone(self);
                let history_id = row.id.clone();
                tokio::spawn(async move {
                    let end = launched.done.await;
                    if let Err(e) =
                        this.store
                            .end_run(&history_id, end.status, end.error.as_deref())
                    {
                        tracing::error!(error = %e, "recording a routine run");
                    }
                    this.running().remove(&routine_id);
                });
            }
            Err(message) => {
                tracing::warn!(routine = %routine_id, %message, "routine run did not start");
                self.store
                    .end_run(&row.id, RunState::Failed, Some(&message))?;
                row.status = RunState::Failed;
                row.error = Some(message);
                self.running().remove(&routine_id);
            }
        }
        Ok(row)
    }

    /// Recomputes every enabled routine's next time from `now_ms`, so fire
    /// times missed while the daemon was down are not caught up.
    ///
    /// # Errors
    /// Database errors.
    pub fn resync(&self, now_ms: i64) -> Result<(), AutomationError> {
        for r in self.store.list()?.into_iter().filter(|r| r.enabled) {
            let next = store::parse_schedule(&r.schedule)
                .ok()
                .and_then(|s| next_fire_ms(&s, local(now_ms)));
            self.store.set_next_run(&r.id, next)?;
        }
        Ok(())
    }

    /// Checks for due routines every `every` until the task is dropped.
    pub fn spawn_loop(self: Arc<Self>, every: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                if let Err(e) = self.tick(Local::now().timestamp_millis()).await {
                    tracing::error!(error = %e, "automation tick failed");
                }
            }
        })
    }
}
