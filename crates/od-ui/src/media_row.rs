//! Media generation rows in the chat (track C2-chat-home, the original's
//! `ImageRow`): a run's media tasks, consecutive ones of one surface
//! merged into a row with a cell per generation. Pure, tested natively.

use od_contracts::{MediaTask, MediaTaskStatus};

/// One cell of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Pending,
    Done(String),
    Failed(String),
}

/// A row: one surface, its cells in start order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub surface: String,
    pub cells: Vec<Cell>,
    /// From the first start to the last end (or `now` while one runs).
    pub elapsed_ms: i64,
    /// Latest progress line of a running cell.
    pub progress: Option<String>,
}

impl Row {
    #[must_use]
    pub fn done(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| matches!(c, Cell::Done(_)))
            .count()
    }

    #[must_use]
    pub fn failed(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| matches!(c, Cell::Failed(_)))
            .count()
    }

    #[must_use]
    pub fn pending(&self) -> bool {
        self.cells.contains(&Cell::Pending)
    }

    /// All done, none failed: the row folds to one line.
    #[must_use]
    pub fn collapsed(&self) -> bool {
        !self.pending() && self.failed() == 0
    }

    /// 1-based positions of the failed cells, for the retry message.
    #[must_use]
    pub fn failed_positions(&self) -> Vec<usize> {
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Cell::Failed(_)))
            .map(|(i, _)| i + 1)
            .collect()
    }
}

/// The rows of a turn's tasks at time `now` (epoch ms).
#[must_use]
pub fn rows(tasks: &[MediaTask], now: i64) -> Vec<Row> {
    let mut out: Vec<(Row, i64, i64)> = Vec::new();
    for task in tasks {
        let cell = match task.status {
            MediaTaskStatus::Done => Cell::Done(task.file.clone().unwrap_or_default()),
            MediaTaskStatus::Failed => Cell::Failed(task.error.clone().unwrap_or_default()),
            MediaTaskStatus::Queued | MediaTaskStatus::Running => Cell::Pending,
        };
        let end = task.ended_at.unwrap_or(now);
        let progress = (!task.status.is_terminal())
            .then(|| task.progress.last().cloned())
            .flatten();
        match out.last_mut() {
            Some((row, start, last)) if row.surface == task.surface => {
                row.cells.push(cell);
                *start = (*start).min(task.started_at);
                *last = (*last).max(end);
                if progress.is_some() {
                    row.progress = progress;
                }
            }
            _ => out.push((
                Row {
                    surface: task.surface.clone(),
                    cells: vec![cell],
                    elapsed_ms: 0,
                    progress,
                },
                task.started_at,
                end,
            )),
        }
    }
    out.into_iter()
        .map(|(mut row, start, end)| {
            row.elapsed_ms = (end - start).max(0);
            row
        })
        .collect()
}

/// "12s", "1m 05s".
#[must_use]
pub fn elapsed(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(
        id: &str,
        surface: &str,
        status: MediaTaskStatus,
        start: i64,
        end: Option<i64>,
    ) -> MediaTask {
        MediaTask {
            task_id: id.into(),
            surface: surface.into(),
            model: "m".into(),
            status,
            progress: vec![format!("{id} working")],
            file: (status == MediaTaskStatus::Done).then(|| format!("{id}.png")),
            error: (status == MediaTaskStatus::Failed).then(|| "quota".into()),
            started_at: start,
            ended_at: end,
        }
    }

    #[test]
    fn consecutive_tasks_of_a_surface_share_a_row() {
        let tasks = [
            task("a", "image", MediaTaskStatus::Done, 1_000, Some(3_000)),
            task("b", "image", MediaTaskStatus::Running, 2_000, None),
            task("c", "audio", MediaTaskStatus::Failed, 4_000, Some(5_000)),
        ];
        let r = rows(&tasks, 9_000);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].cells, [Cell::Done("a.png".into()), Cell::Pending]);
        assert_eq!((r[0].done(), r[0].elapsed_ms), (1, 8_000));
        assert_eq!(r[0].progress.as_deref(), Some("b working"));
        assert!(r[0].pending() && !r[0].collapsed());
        assert_eq!(r[1].failed_positions(), [1]);
        assert_eq!(r[1].progress, None);
    }

    #[test]
    fn a_row_folds_once_everything_is_done() {
        let tasks = [
            task("a", "image", MediaTaskStatus::Done, 0, Some(1_500)),
            task("b", "image", MediaTaskStatus::Done, 0, Some(2_500)),
        ];
        let r = rows(&tasks, 10_000);
        assert!(r[0].collapsed());
        assert_eq!(elapsed(r[0].elapsed_ms), "2s");
        assert_eq!(elapsed(65_000), "1m 05s");
    }
}
