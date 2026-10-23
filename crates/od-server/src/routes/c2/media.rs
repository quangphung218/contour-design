//! Media generation progress in the chat: when the agent generates an
//! image, video or audio through the tool endpoint during a run, the
//! daemon emits `agent` events of type `media` on that run (queued →
//! running → done | failed, with progress lines), which the chat shows as
//! a generation row and the stored events replay after a reload.

use crate::state::AppState;
use od_contracts::{AgentEvent, MediaTask, MediaTaskStatus, sse};
use od_runs::Run;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn next_task_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("media-{}-{n}", od_store::now_ms())
}

/// Reports one generation on its run; a no-op without a run.
pub struct Progress {
    run: Option<Arc<Run>>,
    task: MediaTask,
}

impl Progress {
    /// Starts reporting `surface`/`model` on `run_id` (running at once:
    /// the tool endpoint generates synchronously).
    #[must_use]
    pub fn start(state: &AppState, run_id: Option<&str>, surface: &str, model: &str) -> Self {
        let run = run_id.and_then(|id| state.runs.registry.get(id));
        let mut p = Self {
            run,
            task: MediaTask {
                task_id: next_task_id(),
                surface: surface.to_owned(),
                model: model.to_owned(),
                status: MediaTaskStatus::Running,
                progress: vec![format!("Generating with {model}")],
                file: None,
                error: None,
                started_at: od_store::now_ms(),
                ended_at: None,
            },
        };
        p.emit();
        p
    }

    pub fn line(&mut self, text: impl Into<String>) {
        self.task.progress.push(text.into());
        self.emit();
    }

    pub fn done(mut self, file: &str) {
        self.task.status = MediaTaskStatus::Done;
        self.task.progress.push(format!("Saved {file}"));
        self.task.file = Some(file.to_owned());
        self.task.ended_at = Some(od_store::now_ms());
        self.emit();
    }

    pub fn failed(mut self, error: &str) {
        self.task.status = MediaTaskStatus::Failed;
        self.task.error = Some(error.to_owned());
        self.task.ended_at = Some(od_store::now_ms());
        self.emit();
    }

    fn emit(&mut self) {
        let Some(run) = &self.run else {
            return;
        };
        if let Ok(v) = serde_json::to_value(AgentEvent::Media {
            task: self.task.clone(),
        }) {
            run.emit(sse::AGENT, v);
        }
    }
}
