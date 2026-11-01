use crate::{FLUSH_INTERVAL, RING_CAPACITY, Result, now_ms};
use od_contracts::{RunEndPayload, RunErrorPayload, RunInfo, RunStatus, sse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::sync::broadcast;

/// Live fan-out buffer. A subscriber that falls further behind re-reads the
/// gap from the ring or disk (see [`Subscription::next`]), so this only
/// bounds memory, not correctness.
const BROADCAST_CAPACITY: usize = 1024;

/// `events.jsonl` write buffer. Anything older than the ring is already on
/// disk because the ring holds more bytes than this buffer can: a replay
/// that reads the gap from the file never needs to flush first.
const WRITER_CAPACITY: usize = 8 * 1024;
/// Shortest possible line: `{"id":1,"event":"x","data":null}`.
const MIN_LINE_BYTES: usize = 33;
const _: () = assert!(RING_CAPACITY * MIN_LINE_BYTES > WRITER_CAPACITY);

pub(crate) const STATE_FILE: &str = "state.json";
pub(crate) const EVENTS_FILE: &str = "events.jsonl";

/// The process group a crashed daemon left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub pgid: i32,
    pub bin: Option<String>,
}

/// One SSE event: `id: N` / `event: <name>` / `data: <json>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    pub id: u64,
    pub event: String,
    pub data: Value,
}

/// Fixed facts about a run, recorded at creation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMeta {
    pub run_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub assistant_message_id: Option<String>,
    #[serde(default)]
    pub client_request_id: Option<String>,
    /// Fingerprint of the logical request, to reject a reused
    /// `clientRequestId` carrying a different request.
    #[serde(default)]
    pub request_hash: Option<String>,
    pub created_at: i64,
    /// The first line of the user's message the run answers (chat runs),
    /// for the run list ([`crate::prompt_title`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// `runs/<id>/state.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    #[serde(flatten)]
    pub meta: RunMeta,
    pub status: RunStatus,
    #[serde(default)]
    pub ended_at: Option<i64>,
    pub last_event_id: u64,
    #[serde(default)]
    pub end: Option<RunEndPayload>,
    /// Process group of the live attempt, for orphan cleanup after a crash.
    #[serde(default)]
    pub pgid: Option<i32>,
    /// Executable of the live attempt (orphan cleanup checks it).
    #[serde(default)]
    pub bin: Option<String>,
}

#[derive(Debug)]
struct Inner {
    ring: VecDeque<Arc<RunEvent>>,
    next_id: u64,
    writer: Option<BufWriter<File>>,
    dirty: bool,
    last_flush: Instant,
    status: RunStatus,
    ended_at: Option<i64>,
    end: Option<RunEndPayload>,
    pgid: Option<i32>,
    bin: Option<String>,
}

/// One run's event log. Thread-safe; emits never block on subscribers.
#[derive(Debug)]
pub struct Run {
    meta: RunMeta,
    dir: PathBuf,
    inner: Mutex<Inner>,
    tx: broadcast::Sender<Arc<RunEvent>>,
}

impl Run {
    /// Creates `dir`, writes the initial `state.json` and opens the log.
    pub(crate) fn create(dir: PathBuf, meta: RunMeta) -> Result<Self> {
        fs::create_dir_all(&dir)?;
        let run = Self::with_state(dir, meta, 1, RunStatus::Running, None, None)?;
        run.write_state(&run.lock())?;
        Ok(run)
    }

    /// Runs written before runs kept only a title ([`crate::prompt_title`])
    /// carry the whole message; it is cut here, on create and on load.
    fn with_state(
        dir: PathBuf,
        mut meta: RunMeta,
        next_id: u64,
        status: RunStatus,
        ended_at: Option<i64>,
        end: Option<RunEndPayload>,
    ) -> Result<Self> {
        let writer = if status.is_terminal() {
            None
        } else {
            Some(BufWriter::with_capacity(
                WRITER_CAPACITY,
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(EVENTS_FILE))?,
            ))
        };
        meta.prompt = meta.prompt.as_deref().and_then(crate::prompt_title);
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(Self {
            meta,
            dir,
            inner: Mutex::new(Inner {
                ring: VecDeque::new(),
                next_id,
                writer,
                dirty: false,
                last_flush: Instant::now(),
                status,
                ended_at,
                end,
                pgid: None,
                bin: None,
            }),
            tx,
        })
    }

    /// Restores a run from its folder. A run that was still running when the
    /// daemon died gets `error` + `end{failed}` appended (spec 2.2).
    ///
    /// Returns the run and, when this call ended it, `Some(orphan)` with the
    /// process group and binary of its last attempt, if recorded.
    pub(crate) fn load(dir: PathBuf) -> Result<(Self, Option<Option<Orphan>>)> {
        let state: RunState = serde_json::from_slice(&fs::read(dir.join(STATE_FILE))?)?;
        let last_on_disk = repair_and_scan(&dir.join(EVENTS_FILE))?;
        let next_id = last_on_disk.max(state.last_event_id) + 1;
        if state.status.is_terminal() {
            let run = Self::with_state(
                dir,
                state.meta,
                next_id,
                state.status,
                state.ended_at,
                state.end,
            )?;
            return Ok((run, None));
        }
        let orphan = state.pgid.map(|pgid| Orphan {
            pgid,
            bin: state.bin.clone(),
        });
        let run = Self::with_state(dir, state.meta, next_id, RunStatus::Running, None, None)?;
        run.emit_error(
            "the daemon restarted while this run was in progress",
            Some(od_contracts::DAEMON_RESTART_CODE),
        );
        run.finish(RunEndPayload {
            status: RunStatus::Failed,
            code: None,
            signal: None,
            terminal_at: now_ms(),
            artifact_paths: Vec::new(),
            resumable: false,
            retryable: true,
            failure_category: Some(od_contracts::FailureCategory::DaemonRestart),
            error: Some("daemon restarted".to_owned()),
        });
        Ok((run, Some(orphan)))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Every mutation below leaves `Inner` consistent before any call that
        // could panic, so a poisoned guard is still usable.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.meta.run_id
    }

    #[must_use]
    pub fn meta(&self) -> &RunMeta {
        &self.meta
    }

    #[must_use]
    pub fn status(&self) -> RunStatus {
        self.lock().status
    }

    #[must_use]
    pub fn end_payload(&self) -> Option<RunEndPayload> {
        self.lock().end.clone()
    }

    #[must_use]
    pub fn info(&self) -> RunInfo {
        let inner = self.lock();
        RunInfo {
            id: self.meta.run_id.clone(),
            status: inner.status,
            agent_id: self.meta.agent_id.clone(),
            project_id: self.meta.project_id.clone(),
            conversation_id: self.meta.conversation_id.clone(),
            assistant_message_id: self.meta.assistant_message_id.clone(),
            client_request_id: self.meta.client_request_id.clone(),
            created_at: self.meta.created_at,
            ended_at: inner.ended_at,
            last_event_id: inner.next_id - 1,
            prompt: self.meta.prompt.clone(),
        }
    }

    /// Appends one event and fans it out. Returns its id, or `None` once the
    /// run has ended (nothing follows `end`).
    pub fn emit(&self, event: &str, data: Value) -> Option<u64> {
        let mut inner = self.lock();
        if inner.status.is_terminal() {
            return None;
        }
        let id = inner.next_id;
        inner.next_id += 1;
        let ev = Arc::new(RunEvent {
            id,
            event: event.to_owned(),
            data,
        });
        if inner.ring.len() == RING_CAPACITY {
            inner.ring.pop_front();
        }
        inner.ring.push_back(Arc::clone(&ev));
        if let Some(writer) = inner.writer.as_mut() {
            let line = serde_json::to_string(&*ev).unwrap_or_default();
            if let Err(err) = writeln!(writer, "{line}") {
                tracing::warn!(run = %self.meta.run_id, error = %err, "events.jsonl append failed");
            }
            inner.dirty = true;
        }
        let lifecycle = matches!(event, sse::START | sse::ERROR | sse::END);
        if event == sse::END {
            let end: Option<RunEndPayload> = serde_json::from_value(ev.data.clone()).ok();
            inner.status = end.as_ref().map_or(RunStatus::Failed, |e| e.status);
            inner.ended_at = Some(end.as_ref().map_or_else(now_ms, |e| e.terminal_at));
            inner.end = end;
        }
        if lifecycle || inner.last_flush.elapsed() >= FLUSH_INTERVAL {
            flush(&mut inner, &self.meta.run_id);
        }
        if lifecycle {
            if let Err(err) = self.write_state(&inner) {
                tracing::warn!(run = %self.meta.run_id, error = %err, "state.json write failed");
            }
        }
        if event == sse::END {
            inner.writer = None;
        }
        // Sent under the lock: a subscriber that cut its backlog under this
        // same lock sees each event exactly once.
        let _ = self.tx.send(ev);
        Some(id)
    }

    /// Emits a JSON-serializable payload.
    pub fn emit_json<T: Serialize>(&self, event: &str, data: &T) -> Option<u64> {
        self.emit(event, serde_json::to_value(data).unwrap_or(Value::Null))
    }

    pub fn emit_error(&self, message: &str, code: Option<&str>) -> Option<u64> {
        self.emit_json(
            sse::ERROR,
            &RunErrorPayload {
                message: message.to_owned(),
                code: code.map(str::to_owned),
            },
        )
    }

    /// Emits `end`. Later calls are ignored, so the first terminal outcome
    /// wins (a cancel racing a natural exit, for example).
    pub fn finish(&self, end: RunEndPayload) -> Option<u64> {
        self.emit_json(sse::END, &end)
    }

    /// Records the live attempt's process group in `state.json`.
    pub fn set_process(&self, pgid: Option<i32>, bin: Option<String>) {
        let mut inner = self.lock();
        inner.pgid = pgid;
        inner.bin = bin;
        if let Err(err) = self.write_state(&inner) {
            tracing::warn!(run = %self.meta.run_id, error = %err, "state.json write failed");
        }
    }

    /// Flushes buffered `events.jsonl` writes older than the interval.
    pub fn flush_if_due(&self) {
        let mut inner = self.lock();
        if inner.dirty && inner.last_flush.elapsed() >= FLUSH_INTERVAL {
            flush(&mut inner, &self.meta.run_id);
        }
    }

    fn write_state(&self, inner: &Inner) -> Result<()> {
        let state = RunState {
            meta: self.meta.clone(),
            status: inner.status,
            ended_at: inner.ended_at,
            last_event_id: inner.next_id - 1,
            end: inner.end.clone(),
            pgid: inner.pgid,
            bin: inner.bin.clone(),
        };
        let tmp = self.dir.join(format!("{STATE_FILE}.tmp"));
        fs::write(&tmp, serde_json::to_vec_pretty(&state)?)?;
        fs::rename(tmp, self.dir.join(STATE_FILE))?;
        Ok(())
    }

    /// Starts a subscription at the first event with id greater than `after`.
    pub fn subscribe(self: &Arc<Self>, after: u64) -> Subscription {
        let (ring_tail, ring_first, rx) = {
            let inner = self.lock();
            let ring_first = inner.ring.front().map_or(inner.next_id, |e| e.id);
            let tail: Vec<Arc<RunEvent>> = inner
                .ring
                .iter()
                .filter(|e| e.id > after)
                .cloned()
                .collect();
            let rx = (!inner.status.is_terminal()).then(|| self.tx.subscribe());
            (tail, ring_first, rx)
        };
        let mut backlog = VecDeque::new();
        if after + 1 < ring_first {
            match read_events(&self.dir.join(EVENTS_FILE), after, ring_first) {
                Ok(events) => backlog.extend(events.into_iter().map(Arc::new)),
                Err(err) => {
                    tracing::warn!(run = %self.meta.run_id, error = %err, "events.jsonl replay failed");
                }
            }
        }
        backlog.extend(ring_tail);
        Subscription {
            run: Arc::clone(self),
            backlog,
            rx,
            last_id: after,
            done: false,
        }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn flush(inner: &mut Inner, run_id: &str) {
    if let Some(writer) = inner.writer.as_mut() {
        if let Err(err) = writer.flush() {
            tracing::warn!(run = %run_id, error = %err, "events.jsonl flush failed");
        }
    }
    inner.dirty = false;
    inner.last_flush = Instant::now();
}

/// Events with `after < id < before`, in file order. A torn last line (a
/// crash mid-write) is skipped.
fn read_events(path: &Path, after: u64, before: u64) -> Result<Vec<RunEvent>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(ev) = serde_json::from_str::<RunEvent>(&line) else {
            continue;
        };
        if ev.id >= before {
            break;
        }
        if ev.id > after {
            out.push(ev);
        }
    }
    Ok(out)
}

/// Returns the highest event id in `events.jsonl` and terminates a torn last
/// line so later appends start on a fresh line.
fn repair_and_scan(path: &Path) -> Result<u64> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };
    if bytes.last().is_some_and(|b| *b != b'\n') {
        OpenOptions::new()
            .append(true)
            .open(path)?
            .write_all(b"\n")?;
    }
    let max = bytes
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<RunEvent>(line).ok())
        .map(|ev| ev.id)
        .max()
        .unwrap_or(0);
    Ok(max)
}

/// A gap-free, duplicate-free view of a run's events after some id.
#[derive(Debug)]
pub struct Subscription {
    run: Arc<Run>,
    backlog: VecDeque<Arc<RunEvent>>,
    rx: Option<broadcast::Receiver<Arc<RunEvent>>>,
    last_id: u64,
    done: bool,
}

impl Subscription {
    /// The next event, or `None` after `end` (or when the run is finished
    /// and fully replayed).
    pub async fn next(&mut self) -> Option<Arc<RunEvent>> {
        loop {
            if self.done {
                return None;
            }
            // The backlog and the live receiver were cut at one point under
            // the run's lock, so they never overlap.
            if let Some(ev) = self.backlog.pop_front() {
                return Some(self.deliver(ev));
            }
            let rx = self.rx.as_mut()?;
            match rx.recv().await {
                Ok(ev) => return Some(self.deliver(ev)),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Too slow for the live buffer: re-read from the log.
                    let fresh = self.run.subscribe(self.last_id);
                    self.backlog = fresh.backlog;
                    self.rx = fresh.rx;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    fn deliver(&mut self, ev: Arc<RunEvent>) -> Arc<RunEvent> {
        self.last_id = ev.id;
        if ev.event == sse::END {
            self.done = true;
        }
        ev
    }

    /// Id of the last delivered event.
    #[must_use]
    pub fn last_id(&self) -> u64 {
        self.last_id
    }
}
