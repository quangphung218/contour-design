//! Files a turn changed outside the file tools (spec L4).
//!
//! An agent that writes with `python`, `ffmpeg` or `cp` reports no `Write`,
//! so "Files from this turn" would miss the result. The engine snapshots
//! the project tree when the run starts and again just before it ends;
//! every file that is new or changed (mtime or size) and that no write tool
//! already reported joins the turn as a `Write` (new) or `Edit` (changed).
//!
//! The walk skips what the file listing skips (hidden entries, `.git`,
//! `node_modules`, build and cache folders, the app's own folders), stops
//! at [`MAX_DEPTH`], and gives up past [`MAX_FILES`]: a truncated snapshot
//! cannot tell a new file from an unseen one, so it reports nothing.

use od_contracts::AgentEvent;
use od_runs::{Run, RunEvent};
use od_store::paths::is_ignored_dir_name;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

/// Files past this many make the snapshot incomplete.
pub const MAX_FILES: usize = 20_000;
/// Folder levels below the project root that are walked.
pub const MAX_DEPTH: usize = 12;

/// Sidecar the store writes next to an artifact; never a turn file.
const ARTIFACT_SUFFIX: &str = ".artifact.json";

/// Tool names whose `file_path` already lists the file (as the chat reads them).
const WRITE_TOOLS: [&str; 5] = ["Write", "Edit", "MultiEdit", "write_file", "edit_file"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    mtime: Option<SystemTime>,
    len: u64,
}

/// The project's files by relative path (`a/b.mp4`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    files: HashMap<String, Stamp>,
    complete: bool,
}

impl Snapshot {
    /// Walks `root`. A missing root is an empty, complete snapshot.
    #[must_use]
    pub fn take(root: &Path) -> Self {
        let mut snap = Self {
            files: HashMap::new(),
            complete: true,
        };
        snap.complete = walk(root, "", 0, &mut snap.files);
        snap
    }

    /// Files seen.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether the walk saw every file (it stopped at [`MAX_FILES`] otherwise).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

/// Returns false once [`MAX_FILES`] is passed.
fn walk(dir: &Path, rel_dir: &str, depth: usize, out: &mut HashMap<String, Stamp>) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return true;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let rel = if rel_dir.is_empty() {
            name.clone()
        } else {
            format!("{rel_dir}/{name}")
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if depth < MAX_DEPTH
                && !is_ignored_dir_name(&name)
                && !walk(&entry.path(), &rel, depth + 1, out)
            {
                return false;
            }
            continue;
        }
        if !file_type.is_file() || name.ends_with(ARTIFACT_SUFFIX) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if out.len() >= MAX_FILES {
            return false;
        }
        out.insert(
            rel,
            Stamp {
                mtime: meta.modified().ok(),
                len: meta.len(),
            },
        );
    }
    true
}

/// How the turn touched a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Modified,
}

/// New and changed files, sorted by path. Empty when either snapshot is
/// incomplete.
#[must_use]
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<(String, Change)> {
    if !before.complete || !after.complete {
        return Vec::new();
    }
    let mut out: Vec<(String, Change)> = after
        .files
        .iter()
        .filter_map(|(path, stamp)| match before.files.get(path) {
            None => Some((path.clone(), Change::Added)),
            Some(old) if old != stamp => Some((path.clone(), Change::Modified)),
            Some(_) => None,
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `path` relative to `cwd` (or to its macOS `/private` twin), or as
/// given when it is already relative.
fn relative_to(path: &str, cwd: &Path) -> Option<String> {
    let path = path.trim();
    if !path.starts_with('/') {
        let rel = path.trim_start_matches("./");
        return (!rel.is_empty()).then(|| rel.to_owned());
    }
    let cwd = cwd.to_string_lossy();
    let cwd = cwd.trim_end_matches('/');
    let twin = cwd
        .strip_prefix("/private")
        .map_or_else(|| format!("/private{cwd}"), str::to_owned);
    [cwd, twin.as_str()]
        .iter()
        .find_map(|root| path.strip_prefix(root)?.strip_prefix('/'))
        .filter(|rel| !rel.is_empty())
        .map(str::to_owned)
}

/// Project paths a write tool already reported in `events`.
fn tool_written(events: &[Arc<RunEvent>], cwd: &Path) -> HashSet<String> {
    events
        .iter()
        .filter(|e| e.event == od_contracts::sse::AGENT)
        .filter(|e| e.data.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter(|e| {
            e.data
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| WRITE_TOOLS.contains(&n))
        })
        .filter_map(|e| {
            let input = e.data.get("input")?;
            let path = input
                .get("file_path")
                .or_else(|| input.get("path"))?
                .as_str()?;
            relative_to(path, cwd)
        })
        .collect()
}

/// The run's events so far (it has not ended, so this does not wait).
async fn history(run: &Arc<Run>) -> Vec<Arc<RunEvent>> {
    let last = run.info().last_event_id;
    let mut sub = run.subscribe(0);
    let mut out = Vec::new();
    while sub.last_id() < last {
        match sub.next().await {
            Some(ev) => out.push(ev),
            None => break,
        }
    }
    out
}

/// Snapshots `cwd` again and adds each file the turn changed outside the
/// write tools as a `Write`/`Edit` with its result. Call before the run
/// ends: an ended run takes no events. Returns how many were added.
pub async fn report(run: &Arc<Run>, cwd: &Path, before: &Snapshot) -> usize {
    let started = std::time::Instant::now();
    let root = cwd.to_path_buf();
    let after = match tokio::task::spawn_blocking(move || Snapshot::take(&root)).await {
        Ok(snap) => snap,
        Err(err) => {
            tracing::warn!(run = %run.id(), error = %err, "turn files: snapshot failed");
            return 0;
        }
    };
    if !before.is_complete() || !after.is_complete() {
        tracing::info!(
            run = %run.id(),
            limit = MAX_FILES,
            "turn files: project too large to compare, shell-written files not listed"
        );
        return 0;
    }
    let changes = diff(before, &after);
    let known = if changes.is_empty() {
        HashSet::new()
    } else {
        tool_written(&history(run).await, cwd)
    };
    let mut added = 0usize;
    for (rel, change) in changes.into_iter().filter(|(p, _)| !known.contains(p)) {
        added += 1;
        let id = format!("turn-file-{added}");
        let abs = cwd.join(&rel).to_string_lossy().into_owned();
        let (name, verb) = match change {
            Change::Added => ("Write", "created"),
            Change::Modified => ("Edit", "changed"),
        };
        run.emit_json(
            od_contracts::sse::AGENT,
            &AgentEvent::ToolUse {
                id: id.clone(),
                name: name.to_owned(),
                input: json!({ "file_path": abs, "description": "written outside the file tools" }),
            },
        );
        run.emit_json(
            od_contracts::sse::AGENT,
            &AgentEvent::ToolResult {
                tool_use_id: id,
                content: format!("{rel} {verb} during the turn"),
                is_error: false,
            },
        );
    }
    tracing::info!(
        run = %run.id(),
        files = after.len(),
        added,
        scan_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "turn files compared"
    );
    added
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn diff_lists_new_and_changed_files_and_skips_ignored_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("keep.txt"), "a").unwrap();
        fs::write(root.join("grow.txt"), "a").unwrap();
        let before = Snapshot::take(root);
        fs::write(root.join("grow.txt"), "abc").unwrap();
        fs::create_dir_all(root.join("renders")).unwrap();
        fs::write(root.join("renders/clip.mp4"), "mp4").unwrap();
        for skipped in [".git/x", "node_modules/y", ".open-design/z", ".hidden"] {
            let p = root.join(skipped);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "x").unwrap();
        }
        fs::write(root.join("a.html.artifact.json"), "{}").unwrap();
        let after = Snapshot::take(root);
        assert_eq!(
            diff(&before, &after),
            [
                ("grow.txt".to_owned(), Change::Modified),
                ("renders/clip.mp4".to_owned(), Change::Added)
            ]
        );
    }

    #[test]
    fn a_truncated_snapshot_reports_nothing() {
        let before = Snapshot::default();
        let mut after = Snapshot {
            complete: true,
            ..Snapshot::default()
        };
        after.files.insert(
            "new.txt".into(),
            Stamp {
                mtime: None,
                len: 1,
            },
        );
        assert!(diff(&before, &after).is_empty(), "before is incomplete");
        let before = Snapshot {
            complete: true,
            ..Snapshot::default()
        };
        assert_eq!(diff(&before, &after).len(), 1);
    }

    /// `cargo test -p od-runtime --release --lib -- --ignored --nocapture scan_time`
    #[test]
    #[ignore = "timing, run by hand"]
    fn scan_time_on_5000_files() {
        let tmp = tempfile::tempdir().unwrap();
        for d in 0..50 {
            let dir = tmp.path().join(format!("d{d}/sub"));
            fs::create_dir_all(&dir).unwrap();
            for f in 0..100 {
                fs::write(dir.join(format!("f{f}.txt")), "x").unwrap();
            }
        }
        let mut worst = std::time::Duration::ZERO;
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let before = Snapshot::take(tmp.path());
            let after = Snapshot::take(tmp.path());
            assert!(diff(&before, &after).is_empty());
            worst = worst.max(started.elapsed());
            assert_eq!(after.len(), 5000);
        }
        println!("two snapshots + diff of 5000 files, worst of 5: {worst:?}");
    }

    #[test]
    fn relative_paths_follow_the_macos_private_twin() {
        let cwd = Path::new("/var/p");
        assert_eq!(
            relative_to("/private/var/p/a.mp4", cwd).as_deref(),
            Some("a.mp4")
        );
        assert_eq!(
            relative_to("/var/p/b/c.png", cwd).as_deref(),
            Some("b/c.png")
        );
        assert_eq!(relative_to("./d.txt", cwd).as_deref(), Some("d.txt"));
        assert_eq!(relative_to("/elsewhere/e", cwd), None);
    }
}
