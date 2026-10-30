//! Project events (spec 4.1 live reload): one file watcher per project,
//! shared by every subscriber and closed when the last one leaves. Writes
//! settle for 200 ms before a `file-changed` event goes out. When the
//! watcher drops events (FSEvents `MustScanSubDirs`, an inotify queue
//! overflow) a `reload-all` goes out instead of nothing.

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, PollWatcher, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, RecommendedCache, new_debouncer,
    new_debouncer_opt,
};
use od_store::paths::{is_ignored_dir_name, is_reserved_segment};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// Quiet period before a change is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(200);
/// Window and tick of notify's debouncer, which only merges raw events (and
/// renames) here; `Settle` waits out `DEBOUNCE` on top.
const TICK: Duration = Duration::from_millis(20);
const CHANNEL_CAPACITY: usize = 256;
/// `OD_WATCHER_USE_POLLING=1` forces the polling watcher.
pub const POLLING_ENV: &str = "OD_WATCHER_USE_POLLING";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Add,
    Change,
    Unlink,
}

/// Payload of `GET /api/projects/:id/events`; the SSE event name is
/// [`ProjectEvent::name`] and the data is this value as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProjectEvent {
    #[serde(rename_all = "camelCase")]
    Ready {
        project_id: String,
    },
    FileChanged {
        path: String,
        kind: ChangeKind,
    },
    /// The watcher lost events, so any file may have changed: reload the
    /// file list and the preview.
    ReloadAll,
    #[serde(rename_all = "camelCase")]
    ConversationCreated {
        project_id: String,
        conversation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        created_at: i64,
    },
}

impl ProjectEvent {
    /// SSE `event:` name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "ready",
            Self::FileChanged { .. } => "file-changed",
            Self::ReloadAll => "reload-all",
            Self::ConversationCreated { .. } => "conversation-created",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatchOptions {
    pub debounce: Duration,
    /// Skip native watching (also chosen when watch handles run out).
    pub force_polling: bool,
    pub poll_interval: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: DEBOUNCE,
            force_polling: std::env::var(POLLING_ENV).is_ok_and(|v| v == "1"),
            poll_interval: Duration::from_millis(500),
        }
    }
}

enum Watcher {
    Native(#[allow(dead_code)] Debouncer<RecommendedWatcher, RecommendedCache>),
    Polling(#[allow(dead_code)] Debouncer<PollWatcher, NoCache>),
}

struct Channel {
    tx: broadcast::Sender<ProjectEvent>,
    refs: usize,
    /// Kept alive while anyone listens; dropping it stops watching.
    _watcher: Option<Watcher>,
}

type Channels = Arc<Mutex<HashMap<String, Channel>>>;

fn lock(channels: &Channels) -> MutexGuard<'_, HashMap<String, Channel>> {
    // Every mutation is a single insert/remove/counter step.
    channels.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Per-project event hub.
pub struct ProjectEvents {
    opts: WatchOptions,
    channels: Channels,
}

impl Default for ProjectEvents {
    fn default() -> Self {
        Self::new(WatchOptions::default())
    }
}

impl ProjectEvents {
    #[must_use]
    pub fn new(opts: WatchOptions) -> Self {
        Self {
            opts,
            channels: Arc::default(),
        }
    }

    /// Starts listening to `project_id`, watching `root` if this is the
    /// first subscriber. Blocking: setting up a watcher touches the disk.
    ///
    /// # Errors
    /// The root cannot be resolved or watched.
    pub fn subscribe(&self, project_id: &str, root: &Path) -> io::Result<Subscription> {
        let mut channels = lock(&self.channels);
        let rx = if let Some(channel) = channels.get_mut(project_id) {
            channel.refs += 1;
            channel.tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
            let watcher = start_watcher(root, tx.clone(), self.opts)?;
            channels.insert(
                project_id.to_owned(),
                Channel {
                    tx,
                    refs: 1,
                    _watcher: Some(watcher),
                },
            );
            rx
        };
        Ok(Subscription {
            project_id: project_id.to_owned(),
            rx,
            channels: Arc::clone(&self.channels),
        })
    }

    /// Sends `event` to the project's current subscribers, if any.
    pub fn publish(&self, project_id: &str, event: ProjectEvent) {
        if let Some(channel) = lock(&self.channels).get(project_id) {
            let _ = channel.tx.send(event);
        }
    }

    /// Projects with a live watcher.
    #[must_use]
    pub fn watched_projects(&self) -> usize {
        lock(&self.channels).len()
    }
}

/// One listener. Dropping the last one for a project stops its watcher.
pub struct Subscription {
    project_id: String,
    rx: broadcast::Receiver<ProjectEvent>,
    channels: Channels,
}

impl Subscription {
    /// Next event; `None` once the hub is gone. A slow reader skips what it
    /// missed rather than failing (the client refreshes on any event).
    pub async fn recv(&mut self) -> Option<ProjectEvent> {
        loop {
            match self.rx.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::debug!(project = %self.project_id, skipped, "project events lagged");
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut channels = lock(&self.channels);
        let last = channels.get_mut(&self.project_id).is_some_and(|c| {
            c.refs -= 1;
            c.refs == 0
        });
        let closed = if last {
            channels.remove(&self.project_id)
        } else {
            None
        };
        drop(channels);
        // Stops the watcher thread outside the lock.
        drop(closed);
    }
}

fn out_of_watch_handles(err: &notify::Error) -> bool {
    const EMFILE: i32 = 24;
    const ENOSPC: i32 = 28;
    match &err.kind {
        notify::ErrorKind::MaxFilesWatch => true,
        notify::ErrorKind::Io(io) => matches!(io.raw_os_error(), Some(EMFILE | ENOSPC)),
        _ => false,
    }
}

fn start_watcher(
    root: &Path,
    tx: broadcast::Sender<ProjectEvent>,
    opts: WatchOptions,
) -> io::Result<Watcher> {
    // FSEvents reports real paths (`/private/var/...`); compare against one.
    let root = std::fs::canonicalize(root)?;
    let batches = spawn_settle(root.clone(), tx, opts.debounce)?;
    let handler = {
        let root = root.clone();
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                let _ = batches.send(events);
            }
            Err(errors) => tracing::warn!(?errors, root = %root.display(), "project watcher error"),
        }
    };
    if !opts.force_polling {
        let native = new_debouncer(TICK, Some(TICK), handler.clone()).and_then(|mut d| {
            d.watch(&root, RecursiveMode::Recursive)?;
            Ok(d)
        });
        match native {
            Ok(d) => return Ok(Watcher::Native(d)),
            Err(err) if out_of_watch_handles(&err) => {
                tracing::warn!(error = %err, "out of watch handles; polling instead");
            }
            Err(err) => return Err(io::Error::other(err)),
        }
    }
    let config = notify::Config::default().with_poll_interval(opts.poll_interval);
    let mut d =
        new_debouncer_opt::<_, PollWatcher, NoCache>(TICK, Some(TICK), handler, NoCache, config)
            .map_err(io::Error::other)?;
    d.watch(&root, RecursiveMode::Recursive)
        .map_err(io::Error::other)?;
    Ok(Watcher::Polling(d))
}

/// Editor droppings and our own atomic-write temp files.
fn is_junk_name(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini")
        || name.starts_with(".od-tmp-")
        || name.starts_with(".#")
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swx")
}

/// `a/b/c` for a path under `root`, or `None` for the root itself, paths
/// outside it, ignored trees and junk files.
fn project_path(root: &Path, full: &Path) -> Option<String> {
    let rel = full.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if is_reserved_segment(part) || is_ignored_dir_name(part) {
            return None;
        }
        parts.push(part);
    }
    let name = parts.last()?;
    if is_junk_name(name) {
        return None;
    }
    Some(parts.join("/"))
}

/// Holds each changed path until it has been quiet for the window, so a
/// burst of writes gives one event. notify's debouncer times every raw event
/// on its own: on Linux each write of a burst came out as its own change.
struct Settle {
    window: Duration,
    /// Relative path → (last raw event, created in the meantime).
    pending: HashMap<String, (Instant, bool)>,
    /// The last event telling us the watcher dropped some, which names no
    /// path: until it settles too.
    rescan: Option<Instant>,
}

impl Settle {
    fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
            rescan: None,
        }
    }

    fn add(&mut self, root: &Path, events: &[DebouncedEvent]) {
        for event in events {
            if event.need_rescan() {
                self.rescan = Some(self.rescan.map_or(event.time, |t| t.max(event.time)));
                continue;
            }
            // inotify reports every open and read-only close. Taking those
            // for changes made the preview's own read of a file reload it,
            // forever.
            if matches!(event.kind, EventKind::Access(kind) if kind != AccessKind::Close(AccessMode::Write))
            {
                continue;
            }
            let created = matches!(event.kind, EventKind::Create(_));
            for path in &event.paths {
                let Some(rel) = project_path(root, path) else {
                    continue;
                };
                let entry = self.pending.entry(rel).or_insert((event.time, false));
                entry.0 = entry.0.max(event.time);
                entry.1 |= created;
            }
        }
    }

    /// When the next path settles.
    fn next_due(&self) -> Option<Instant> {
        self.pending
            .values()
            .map(|(last, _)| *last)
            .chain(self.rescan)
            .map(|last| last + self.window)
            .min()
    }

    /// Whether a rescan is quiet for the window by `now`; it is then taken.
    fn take_rescan(&mut self, now: Instant) -> bool {
        let settled = self.rescan.is_some_and(|last| last + self.window <= now);
        if settled {
            self.rescan = None;
        }
        settled
    }

    /// Paths quiet for the window by `now`, in path order.
    fn take_due(&mut self, now: Instant) -> Vec<(String, bool)> {
        let mut due: Vec<(String, bool)> = Vec::new();
        self.pending.retain(|path, (last, created)| {
            let settled = *last + self.window <= now;
            if settled {
                due.push((path.clone(), *created));
            }
            !settled
        });
        due.sort();
        due
    }
}

/// Runs `Settle` on its own thread; it ends when the watcher (the sender)
/// is dropped.
fn spawn_settle(
    root: std::path::PathBuf,
    tx: broadcast::Sender<ProjectEvent>,
    window: Duration,
) -> io::Result<mpsc::Sender<Vec<DebouncedEvent>>> {
    let (batches, rx) = mpsc::channel::<Vec<DebouncedEvent>>();
    std::thread::Builder::new()
        .name("od-preview-settle".into())
        .spawn(move || {
            let mut settle = Settle::new(window);
            let mut known = HashSet::new();
            loop {
                let next = match settle.next_due() {
                    None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                    Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
                };
                match next {
                    Ok(events) => settle.add(&root, &events),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                let now = Instant::now();
                let due = settle.take_due(now);
                for event in classify(&root, due, &mut known) {
                    let _ = tx.send(event);
                }
                if settle.take_rescan(now) {
                    let _ = tx.send(ProjectEvent::ReloadAll);
                }
            }
        })?;
    Ok(batches)
}

/// Turns settled paths into `file-changed` events, kind decided by what is
/// on disk now. Directories and symlinks are skipped.
///
/// `known` holds files already reported as present: FSEvents keeps the
/// "created" flag on a fresh file's next writes, so a create counts as `add`
/// only the first time.
fn classify(
    root: &Path,
    settled: Vec<(String, bool)>,
    known: &mut HashSet<String>,
) -> Vec<ProjectEvent> {
    settled
        .into_iter()
        .filter_map(|(path, created)| {
            let kind = match std::fs::symlink_metadata(root.join(&path)) {
                Ok(meta) if meta.is_dir() || meta.file_type().is_symlink() => return None,
                Ok(_) if known.insert(path.clone()) && created => ChangeKind::Add,
                Ok(_) => ChangeKind::Change,
                Err(_) => {
                    known.remove(&path);
                    ChangeKind::Unlink
                }
            };
            Some(ProjectEvent::FileChanged { path, kind })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_json_shapes() {
        let changed = ProjectEvent::FileChanged {
            path: "a/index.html".into(),
            kind: ChangeKind::Unlink,
        };
        assert_eq!(changed.name(), "file-changed");
        assert_eq!(
            serde_json::to_value(&changed).unwrap(),
            json!({"type": "file-changed", "path": "a/index.html", "kind": "unlink"})
        );
        let ready = ProjectEvent::Ready {
            project_id: "p".into(),
        };
        assert_eq!(
            serde_json::to_value(&ready).unwrap(),
            json!({"type": "ready", "projectId": "p"})
        );
        let conv = ProjectEvent::ConversationCreated {
            project_id: "p".into(),
            conversation_id: "c".into(),
            title: None,
            created_at: 5,
        };
        assert_eq!(conv.name(), "conversation-created");
        assert_eq!(
            serde_json::to_value(&conv).unwrap(),
            json!({"type": "conversation-created", "projectId": "p", "conversationId": "c", "createdAt": 5})
        );
    }

    #[test]
    fn project_paths_skip_internal_and_junk() {
        let root = Path::new("/r");
        let rel = |p: &str| project_path(root, Path::new(p));
        assert_eq!(
            rel("/r/deck/index.html").as_deref(),
            Some("deck/index.html")
        );
        assert_eq!(rel("/r"), None);
        assert_eq!(rel("/elsewhere/x.html"), None);
        assert_eq!(rel("/r/.open-design/project.json"), None);
        assert_eq!(rel("/r/.file-versions/x/1.html"), None);
        assert_eq!(rel("/r/node_modules/a.js"), None);
        assert_eq!(rel("/r/.DS_Store"), None);
        assert_eq!(rel("/r/.od-tmp-abc"), None);
        assert_eq!(rel("/r/index.html~"), None);
    }

    fn raw(kind: EventKind, path: &str, time: Instant) -> DebouncedEvent {
        DebouncedEvent::new(
            notify::Event::new(kind).add_path(Path::new("/r").join(path)),
            time,
        )
    }

    #[test]
    fn reads_are_not_changes() {
        use notify::event::{DataChange, ModifyKind};
        let t0 = Instant::now();
        let mut settle = Settle::new(DEBOUNCE);
        // inotify reports every open, e.g. the daemon serving the preview.
        settle.add(
            Path::new("/r"),
            &[
                raw(
                    EventKind::Access(AccessKind::Open(AccessMode::Any)),
                    "a.html",
                    t0,
                ),
                raw(
                    EventKind::Access(AccessKind::Close(AccessMode::Read)),
                    "a.html",
                    t0,
                ),
            ],
        );
        assert_eq!(settle.next_due(), None);
        settle.add(
            Path::new("/r"),
            &[
                raw(
                    EventKind::Access(AccessKind::Open(AccessMode::Any)),
                    "b.html",
                    t0,
                ),
                raw(
                    EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                    "b.html",
                    t0,
                ),
                raw(
                    EventKind::Access(AccessKind::Close(AccessMode::Write)),
                    "b.html",
                    t0,
                ),
            ],
        );
        assert_eq!(
            settle.take_due(t0 + DEBOUNCE),
            [("b.html".to_owned(), false)]
        );
    }

    #[test]
    fn a_burst_settles_once_after_its_last_write() {
        use notify::event::{CreateKind, DataChange, ModifyKind};
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut settle = Settle::new(DEBOUNCE);
        let root = Path::new("/r");
        settle.add(
            root,
            &[raw(EventKind::Create(CreateKind::File), "a.css", t0)],
        );
        for i in 1..5 {
            let at = t0 + ms(30 * i);
            settle.add(
                root,
                &[raw(
                    EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                    "a.css",
                    at,
                )],
            );
        }
        settle.add(
            root,
            &[raw(
                EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                "b.css",
                t0,
            )],
        );
        assert_eq!(settle.next_due(), Some(t0 + DEBOUNCE));
        assert_eq!(
            settle.take_due(t0 + DEBOUNCE),
            [("b.css".to_owned(), false)]
        );
        // Each write restarts the window; the create is remembered.
        assert!(settle.take_due(t0 + ms(319)).is_empty());
        assert_eq!(settle.take_due(t0 + ms(320)), [("a.css".to_owned(), true)]);
        assert_eq!(settle.next_due(), None);
    }

    /// What FSEvents `MustScanSubDirs` and an inotify overflow come out as:
    /// no path, only the flag.
    fn rescan(time: Instant) -> DebouncedEvent {
        DebouncedEvent::new(
            notify::Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan),
            time,
        )
    }

    #[test]
    fn a_rescan_settles_like_a_path() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut settle = Settle::new(DEBOUNCE);
        settle.add(Path::new("/r"), &[rescan(t0), rescan(t0 + ms(50))]);
        assert_eq!(settle.next_due(), Some(t0 + ms(250)));
        assert!(!settle.take_rescan(t0 + ms(249)));
        assert!(settle.take_rescan(t0 + ms(250)));
        assert!(!settle.take_rescan(t0 + ms(500)), "taken once");
        assert_eq!(settle.next_due(), None);
    }

    #[tokio::test]
    async fn dropped_events_reach_subscribers_as_reload_all() {
        let root = tempfile::tempdir().unwrap();
        let (tx, mut rx) = broadcast::channel(8);
        let batches = spawn_settle(root.path().to_owned(), tx, DEBOUNCE).unwrap();
        batches.send(vec![rescan(Instant::now())]).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a reload-all within 5 s")
            .unwrap();
        assert_eq!(event, ProjectEvent::ReloadAll);
        assert_eq!(event.name(), "reload-all");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"type": "reload-all"})
        );
    }
}
