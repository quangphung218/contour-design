//! Project events (spec 4.1 live reload): one file watcher per project,
//! shared by every subscriber and closed when the last one leaves. Writes
//! settle for 200 ms before a `file-changed` event goes out.

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
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::broadcast;

/// Quiet period before a change is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(200);
/// Debouncer tick: bounds how late after the quiet period an event fires.
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
    let handler = {
        let root = root.clone();
        let mut known = HashSet::new();
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                for event in classify(&root, &events, &mut known) {
                    let _ = tx.send(event);
                }
            }
            Err(errors) => tracing::warn!(?errors, root = %root.display(), "project watcher error"),
        }
    };
    if !opts.force_polling {
        let native = new_debouncer(opts.debounce, Some(TICK), handler.clone()).and_then(|mut d| {
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
    let mut d = new_debouncer_opt::<_, PollWatcher, NoCache>(
        opts.debounce,
        Some(TICK),
        handler,
        NoCache,
        config,
    )
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

/// Turns one debounced batch into `file-changed` events: one per path, kind
/// decided by what is on disk now. Directories and symlinks are skipped.
///
/// `known` holds files already reported as present: FSEvents keeps the
/// "created" flag on a fresh file's next writes, so a create counts as `add`
/// only the first time.
fn classify(
    root: &Path,
    events: &[DebouncedEvent],
    known: &mut HashSet<String>,
) -> Vec<ProjectEvent> {
    let mut seen: Vec<(String, bool)> = Vec::new();
    for event in events {
        let created = matches!(event.kind, EventKind::Create(_));
        for path in &event.paths {
            let Some(rel) = project_path(root, path) else {
                continue;
            };
            match seen.iter_mut().find(|(p, _)| *p == rel) {
                Some(entry) => entry.1 |= created,
                None => seen.push((rel, created)),
            }
        }
    }
    seen.into_iter()
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
}
