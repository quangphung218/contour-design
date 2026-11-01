//! Files a turn changed outside the file tools (spec L4).
//!
//! An agent that writes with `python`, `ffmpeg` or `cp` reports no `Write`,
//! so "Files from this turn" would miss the result. The engine snapshots
//! the project tree when the run starts and again just before it ends;
//! every file that is new or changed (mtime or size) and that no write tool
//! already reported joins the turn as a `Write` (new) or `Edit` (changed).
//!
//! More than [`GROUP_OVER`] such files of a folder's most common extension
//! (frames from `ffmpeg`) become one row, `frames/ (N files)`, while a
//! film, page or file of another kind there keeps its own row. A turn adds at most
//! [`MAX_ROWS`] rows plus one `+N more`: thousands of frames must not flood
//! the chat or the stored message.
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Files past this many make the snapshot incomplete.
pub const MAX_FILES: usize = 20_000;
/// Folder levels below the project root that are walked.
pub const MAX_DEPTH: usize = 12;
/// A folder with more new or changed files than this is one row.
pub const GROUP_OVER: usize = 20;
/// Rows a turn adds at most; the rest are counted in one `+N more`.
pub const MAX_ROWS: usize = 50;
/// Tool name of the `+N more` row (not a write tool: it names no file).
pub const MORE_TOOL: &str = "TurnFilesMore";

/// How far a snapshot walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_files: usize,
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: MAX_FILES,
            max_depth: MAX_DEPTH,
        }
    }
}

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
    /// Walks `root` within the default [`Limits`]. A missing root is an
    /// empty, complete snapshot.
    #[must_use]
    pub fn take(root: &Path) -> Self {
        Self::take_within(root, Limits::default())
    }

    /// [`Self::take`] within `limits`.
    #[must_use]
    pub fn take_within(root: &Path, limits: Limits) -> Self {
        let mut files = HashMap::new();
        let complete = walk(root, "", 0, limits, &mut files);
        Self { files, complete }
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

    /// Whether the walk saw every file (it stopped at `max_files` otherwise).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

/// Returns false once `limits.max_files` is passed.
fn walk(
    dir: &Path,
    rel_dir: &str,
    depth: usize,
    limits: Limits,
    out: &mut HashMap<String, Stamp>,
) -> bool {
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
            if depth < limits.max_depth
                && !is_ignored_dir_name(&name)
                && !walk(&entry.path(), &rel, depth + 1, limits, out)
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
        if out.len() >= limits.max_files {
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

/// `path` relative to `cwd`, with `/` separators (`a/b.html`), or as
/// given when it is already relative. Windows paths (`C:\p\a\b.html`)
/// and the macOS `/private` twin of `cwd` match too.
fn relative_to(path: &str, cwd: &Path) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let cwd = cwd.to_string_lossy().replace('\\', "/");
    let cwd = Path::new(cwd.trim_end_matches('/'));
    let absolute = path.starts_with('/') || is_drive_path(&path);
    let rel = if absolute {
        let twin = cwd.to_string_lossy().strip_prefix("/private").map_or_else(
            || PathBuf::from(format!("/private{}", cwd.display())),
            PathBuf::from,
        );
        [cwd, twin.as_path()]
            .iter()
            .find_map(|root| Path::new(&path).strip_prefix(root).ok())?
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        path.trim_start_matches("./").to_owned()
    };
    (!rel.is_empty()).then_some(rel)
}

/// `C:/…` (separators already turned to `/`).
fn is_drive_path(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

/// `rel` (`a/b.mp4`) under `cwd`, with the platform's separators.
fn absolute(cwd: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(cwd.to_path_buf(), |p, part| p.join(part))
}

/// One row the turn gains: a file, or a folder standing for `files` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// `a/b.mp4`, or `frames/` for a folder.
    pub path: String,
    pub change: Change,
    /// Files a folder row stands for; `None` for a file.
    pub files: Option<usize>,
}

/// Files people open first (renders, pages), listed before the rest.
fn is_result_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".mp4", ".mov", ".webm", ".gif", ".png", ".jpg", ".jpeg", ".webp", ".svg", ".html", ".pdf",
        ".mp3", ".wav",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

/// Files that keep a row of their own even inside a grouped folder: a
/// film, a page or a document is what people open.
fn keeps_its_row(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".mp4", ".mov", ".webm", ".gif", ".html", ".pdf"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// Lower-case extension of the file name (`""` without one).
fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Turns the changed files into rows. In each folder, the files sharing
/// its most common extension (frames) become one row once there are more
/// than [`GROUP_OVER`] of them; films, pages, documents and files of any
/// other extension keep their own row. Result files come first, then
/// folders, then the rest, each by path. At most [`MAX_ROWS`] rows; the
/// second value counts the files left out (a folder row counts its files).
#[must_use]
pub fn plan_rows(changes: &[(String, Change)]) -> (Vec<Row>, usize) {
    let parent = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_owned());
    let mut per_dir: HashMap<String, Vec<(&str, Change)>> = HashMap::new();
    for (path, change) in changes {
        if let Some(dir) = parent(path) {
            per_dir.entry(dir).or_default().push((path, *change));
        }
    }
    let mut grouped: HashSet<&str> = HashSet::new();
    let mut folders: Vec<Row> = Vec::new();
    for (dir, entries) in &per_dir {
        let mut by_ext: HashMap<String, usize> = HashMap::new();
        for (path, _) in entries {
            *by_ext.entry(extension(path)).or_default() += 1;
        }
        // Ties go to the alphabetically first extension, so runs agree.
        let Some(major) = by_ext
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .map(|(e, _)| e)
        else {
            continue;
        };
        let members: Vec<&(&str, Change)> = entries
            .iter()
            .filter(|(p, _)| extension(p) == major && !keeps_its_row(p))
            .collect();
        if members.len() <= GROUP_OVER {
            continue;
        }
        grouped.extend(members.iter().map(|(p, _)| *p));
        let all_added = members.iter().all(|(_, c)| *c == Change::Added);
        folders.push(Row {
            path: format!("{dir}/"),
            change: if all_added {
                Change::Added
            } else {
                Change::Modified
            },
            files: Some(members.len()),
        });
    }
    let mut files: Vec<Row> = changes
        .iter()
        .filter(|(p, _)| !grouped.contains(p.as_str()))
        .map(|(path, change)| Row {
            path: path.clone(),
            change: *change,
            files: None,
        })
        .collect();
    folders.sort_by(|a, b| a.path.cmp(&b.path));
    files.sort_by(|a, b| {
        is_result_file(&b.path)
            .cmp(&is_result_file(&a.path))
            .then_with(|| a.path.cmp(&b.path))
    });
    let split = files.iter().take_while(|r| is_result_file(&r.path)).count();
    let rest = files.split_off(split);
    let mut rows: Vec<Row> = files.into_iter().chain(folders).chain(rest).collect();
    let omitted = rows
        .iter()
        .skip(MAX_ROWS)
        .map(|r| r.files.unwrap_or(1))
        .sum();
    rows.truncate(MAX_ROWS);
    (rows, omitted)
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

/// One tool call and its result.
fn emit_row(run: &Run, id: String, name: &str, input: Value, content: String) {
    run.emit_json(
        od_contracts::sse::AGENT,
        &AgentEvent::ToolUse {
            id: id.clone(),
            name: name.to_owned(),
            input,
        },
    );
    run.emit_json(
        od_contracts::sse::AGENT,
        &AgentEvent::ToolResult {
            tool_use_id: id,
            content,
            is_error: false,
        },
    );
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
/// ends: an ended run takes no events. Returns how many rows were added.
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
    let changes: Vec<(String, Change)> = changes
        .into_iter()
        .filter(|(p, _)| !known.contains(p))
        .collect();
    let (rows, omitted) = plan_rows(&changes);
    for (i, row) in rows.iter().enumerate() {
        let id = format!("turn-file-{}", i + 1);
        let abs = absolute(cwd, &row.path).to_string_lossy().into_owned();
        let (name, verb) = match row.change {
            Change::Added => ("Write", "created"),
            Change::Modified => ("Edit", "changed"),
        };
        let (input, content) = match row.files {
            None => (
                json!({ "file_path": abs, "description": "written outside the file tools" }),
                format!("{} {verb} during the turn", row.path),
            ),
            Some(n) => (
                json!({ "file_path": abs, "files": n, "description": "written outside the file tools" }),
                format!("{} ({n} files) {verb} during the turn", row.path),
            ),
        };
        emit_row(run, id, name, input, content);
    }
    if omitted > 0 {
        emit_row(
            run,
            "turn-file-more".to_owned(),
            MORE_TOOL,
            json!({ "files": omitted, "description": format!("+{omitted} more files changed outside the file tools") }),
            format!("+{omitted} more files changed during the turn"),
        );
    }
    let added = rows.len();
    tracing::info!(
        run = %run.id(),
        files = after.len(),
        added,
        omitted,
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
    fn windows_paths_match_the_snapshot_keys() {
        let cwd = Path::new(r"C:\p");
        assert_eq!(
            relative_to(r"C:\p\a\b.html", cwd).as_deref(),
            Some("a/b.html")
        );
        assert_eq!(
            relative_to("C:/p/index.html", cwd).as_deref(),
            Some("index.html")
        );
        assert_eq!(relative_to(r"a\b.html", cwd).as_deref(), Some("a/b.html"));
        assert_eq!(relative_to(r"D:\other\b.html", cwd), None);
    }

    #[test]
    fn the_walk_stops_past_max_files_and_below_max_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for i in 0..5 {
            fs::write(root.join(format!("f{i}.txt")), "x").unwrap();
        }
        let tight = Limits {
            max_files: 4,
            max_depth: MAX_DEPTH,
        };
        assert!(!Snapshot::take_within(root, tight).is_complete());
        let roomy = Limits {
            max_files: 5,
            max_depth: MAX_DEPTH,
        };
        assert!(Snapshot::take_within(root, roomy).is_complete());

        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/one.txt"), "x").unwrap();
        fs::write(root.join("a/b/two.txt"), "x").unwrap();
        let shallow = Snapshot::take_within(
            root,
            Limits {
                max_files: MAX_FILES,
                max_depth: 1,
            },
        );
        assert!(shallow.files.contains_key("a/one.txt"));
        assert!(!shallow.files.contains_key("a/b/two.txt"));
        assert!(Snapshot::take(root).files.contains_key("a/b/two.txt"));
    }

    fn added(paths: impl IntoIterator<Item = String>) -> Vec<(String, Change)> {
        paths.into_iter().map(|p| (p, Change::Added)).collect()
    }

    #[test]
    fn a_folder_of_frames_is_one_row() {
        let mut changes = added((1..=1000).map(|i| format!("frames/{i:04}.png")));
        changes.push(("clip.mp4".to_owned(), Change::Added));
        changes.push(("notes.txt".to_owned(), Change::Modified));
        // 20 files in a folder stay files; 21 become a row.
        changes.extend(added((0..20).map(|i| format!("small/{i}.txt"))));
        let (rows, omitted) = plan_rows(&changes);
        assert_eq!(omitted, 0);
        assert_eq!(rows.len(), 23);
        assert_eq!(
            rows[0],
            Row {
                path: "clip.mp4".into(),
                change: Change::Added,
                files: None
            }
        );
        assert_eq!(
            rows[1],
            Row {
                path: "frames/".into(),
                change: Change::Added,
                files: Some(1000)
            }
        );
        assert_eq!(rows[2].path, "notes.txt");
        let edited = plan_rows(
            &[
                added((0..21).map(|i| format!("f/{i}.png"))),
                vec![("f/old.png".to_owned(), Change::Modified)],
            ]
            .concat(),
        )
        .0;
        assert_eq!(edited[0].change, Change::Modified, "a folder with an edit");
    }

    #[test]
    fn a_film_among_frames_keeps_a_row_that_opens() {
        let mut changes = added((1..=1000).map(|i| format!("frames/{i:04}.png")));
        changes.push(("frames/film.mp4".to_owned(), Change::Added));
        changes.push(("frames/index.html".to_owned(), Change::Added));
        changes.push(("frames/log.txt".to_owned(), Change::Added));
        let (rows, omitted) = plan_rows(&changes);
        assert_eq!(omitted, 0);
        let shown: Vec<(&str, Option<usize>)> =
            rows.iter().map(|r| (r.path.as_str(), r.files)).collect();
        assert_eq!(
            shown,
            [
                ("frames/film.mp4", None),
                ("frames/index.html", None),
                ("frames/", Some(1000)),
                ("frames/log.txt", None),
            ]
        );
    }

    #[test]
    fn films_keep_their_rows_even_when_they_are_the_folder_majority() {
        let changes = added((0..30).map(|i| format!("clips/{i:02}.mp4")));
        let (rows, _) = plan_rows(&changes);
        assert_eq!(rows.len(), 30);
        assert!(rows.iter().all(|r| r.files.is_none()));
    }

    #[test]
    fn a_folder_past_the_row_limit_counts_all_its_files() {
        // 60 root images fill the 50 rows; `frames/` (1000) falls past them.
        let mut changes = added((0..60).map(|i| format!("img{i:02}.png")));
        changes.extend(added((0..1000).map(|i| format!("frames/{i:04}.png"))));
        let (rows, omitted) = plan_rows(&changes);
        assert_eq!(rows.len(), MAX_ROWS);
        assert!(rows.iter().all(|r| r.files.is_none()));
        assert_eq!(omitted, 10 + 1000);
    }

    #[test]
    fn a_turn_adds_at_most_max_rows() {
        let changes = added((0..120).map(|i| format!("page{i:03}.txt")));
        let (rows, omitted) = plan_rows(&changes);
        assert_eq!(rows.len(), MAX_ROWS);
        assert_eq!(omitted, 120 - MAX_ROWS);
        assert_eq!(rows[0].path, "page000.txt");
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
