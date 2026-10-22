use crate::run::STATE_FILE;
use crate::{Orphan, Result, Run, RunMeta, RunsError, is_safe_run_id};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// What [`Runs::load`] found on disk.
#[derive(Debug, Default)]
pub struct LoadReport {
    pub loaded: usize,
    /// Runs that were in flight when the daemon died, now ended as failed,
    /// with the process group and binary of their last attempt.
    pub interrupted: Vec<(String, Option<Orphan>)>,
}

#[derive(Debug, Default)]
struct Index {
    runs: HashMap<String, Arc<Run>>,
    by_client_request: HashMap<String, String>,
}

/// All runs of this daemon, keyed by id, backed by `<data>/runs/`.
#[derive(Debug)]
pub struct Runs {
    dir: PathBuf,
    index: RwLock<Index>,
}

impl Runs {
    /// An empty registry over `runs_dir` (created on first run).
    #[must_use]
    pub fn new(runs_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: runs_dir.into(),
            index: RwLock::new(Index::default()),
        }
    }

    /// Opens the registry and restores every run found on disk. Folders
    /// without a readable `state.json` are skipped with a warning.
    ///
    /// # Errors
    /// Only when `runs_dir` exists but cannot be listed.
    pub fn load(runs_dir: impl Into<PathBuf>) -> Result<(Self, LoadReport)> {
        let runs = Self::new(runs_dir);
        let mut report = LoadReport::default();
        let entries = match std::fs::read_dir(&runs.dir) {
            Ok(e) => e,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((runs, report)),
            Err(err) => return Err(err.into()),
        };
        let mut index = runs.write();
        for entry in entries.filter_map(std::result::Result::ok) {
            let dir = entry.path();
            if !dir.join(STATE_FILE).is_file() {
                continue;
            }
            match Run::load(dir.clone()) {
                Ok((run, interrupted)) => {
                    let id = run.id().to_owned();
                    if let Some(orphan) = interrupted {
                        report.interrupted.push((id.clone(), orphan));
                    }
                    if let Some(key) = run.meta().client_request_id.clone() {
                        index.by_client_request.insert(key, id.clone());
                    }
                    index.runs.insert(id, Arc::new(run));
                    report.loaded += 1;
                }
                Err(err) => {
                    tracing::warn!(dir = %dir.display(), error = %err, "skipping unreadable run");
                }
            }
        }
        drop(index);
        Ok((runs, report))
    }

    fn read(&self) -> RwLockReadGuard<'_, Index> {
        self.index
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Index> {
        self.index
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Creates a run, or returns the one already bound to the same
    /// `clientRequestId` (`reused = true`).
    ///
    /// # Errors
    /// [`RunsError::IdempotencyConflict`] when that id was used for a
    /// different `request_hash`; I/O errors creating the folder.
    pub fn create_or_reuse(&self, meta: RunMeta) -> Result<(Arc<Run>, bool)> {
        if !is_safe_run_id(&meta.run_id) {
            return Err(RunsError::InvalidId);
        }
        let mut index = self.write();
        if let Some(key) = meta.client_request_id.as_deref() {
            if let Some(existing) = index
                .by_client_request
                .get(key)
                .and_then(|id| index.runs.get(id))
            {
                if existing.meta().request_hash != meta.request_hash {
                    return Err(RunsError::IdempotencyConflict);
                }
                return Ok((Arc::clone(existing), true));
            }
        }
        let run = Arc::new(Run::create(self.dir.join(&meta.run_id), meta)?);
        if let Some(key) = run.meta().client_request_id.clone() {
            index.by_client_request.insert(key, run.id().to_owned());
        }
        index.runs.insert(run.id().to_owned(), Arc::clone(&run));
        Ok((run, false))
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<Run>> {
        self.read().runs.get(id).cloned()
    }

    /// Newest first.
    #[must_use]
    pub fn list(&self) -> Vec<Arc<Run>> {
        let mut runs: Vec<Arc<Run>> = self.read().runs.values().cloned().collect();
        runs.sort_by(|a, b| {
            b.meta()
                .created_at
                .cmp(&a.meta().created_at)
                .then_with(|| a.id().cmp(b.id()))
        });
        runs
    }

    /// Flushes every run's buffered log writes that are due.
    pub fn flush_due(&self) {
        for run in self.read().runs.values() {
            run.flush_if_due();
        }
    }

    /// Flushes buffered writes every [`crate::FLUSH_INTERVAL`] until the
    /// registry is dropped. Needs a tokio runtime.
    pub fn spawn_flusher(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(crate::FLUSH_INTERVAL);
            loop {
                tick.tick().await;
                let Some(runs) = weak.upgrade() else { break };
                runs.flush_due();
            }
        })
    }
}
