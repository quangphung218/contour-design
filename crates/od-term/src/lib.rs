//! Terminal tabs (spec 1.5 Terminal, 1.2 workspace tabs): an interactive
//! shell on a PTY whose working directory is the project folder.
//!
//! - Sessions live in memory only; a daemon restart ends them.
//! - Output fans out to every attached client. A bounded scrollback lets a
//!   reconnecting client (page reload) replay what it missed.
//! - Closing a session kills the shell's whole tree: on POSIX the process
//!   groups of the shell, the terminal's foreground job, every other job
//!   on the tab's TTY and every descendant of the shell (a job that left
//!   the session with `setsid`) get SIGHUP, then SIGKILL after a grace
//!   period. Descendants are also noted while the tab runs, so one whose
//!   parent exited (it now belongs to init) is still found. On Windows the
//!   shell runs in its own kill-on-close Job Object (`od-job`).
//!
//! The HTTP side (loopback-only WebSocket behind the daemon's guards) is
//! in `od-server`.

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

/// Environment variables of the daemon a shell must not inherit.
const SECRET_ENV: [&str; 2] = ["OD_API_TOKEN", "OD_TOOL_TOKEN"];
/// Largest accepted terminal dimension (cells).
pub const MAX_DIMENSION: u16 = 1000;
/// How long the exit event waits for the last output to drain.
const DRAIN_WAIT: Duration = Duration::from_millis(300);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error("too many terminals open (limit {0})")]
    Limit(usize),
    #[error("the working directory {0} does not exist")]
    BadCwd(PathBuf),
    #[error("could not start the shell: {0}")]
    Spawn(String),
}

/// Settings for [`Terminals`].
#[derive(Debug, Clone)]
pub struct TermConfig {
    /// Shell to run; `None` picks `$SHELL` (POSIX) or `%ComSpec%` (Windows).
    pub shell: Option<PathBuf>,
    /// Running sessions allowed at once.
    pub max_sessions: usize,
    /// Output bytes kept for replay per session.
    pub scrollback_bytes: usize,
    /// Wait between the polite hang-up and the hard kill.
    pub kill_grace: Duration,
    /// How often a running tab notes the process groups of the shell's
    /// descendants (POSIX).
    pub track_every: Duration,
}

impl Default for TermConfig {
    fn default() -> Self {
        Self {
            shell: None,
            max_sessions: 16,
            scrollback_bytes: 256 * 1024,
            kill_grace: Duration::from_secs(2),
            track_every: Duration::from_secs(2),
        }
    }
}

/// The shell a new terminal runs.
#[must_use]
pub fn default_shell() -> PathBuf {
    #[cfg(windows)]
    let shell = std::env::var_os("ComSpec").unwrap_or_else(|| "powershell.exe".into());
    #[cfg(not(windows))]
    let shell = std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    PathBuf::from(shell)
}

/// Clamps a requested size to `1..=MAX_DIMENSION`, defaulting to 80×24.
#[must_use]
pub fn clamp_size(cols: Option<u16>, rows: Option<u16>) -> (u16, u16) {
    let fit = |v: Option<u16>, d: u16| v.unwrap_or(d).clamp(1, MAX_DIMENSION);
    (fit(cols, 80), fit(rows, 24))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TermStatus {
    Running,
    Exited,
}

/// How the shell ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermExit {
    pub code: u32,
    pub signal: Option<String>,
}

/// What clients see of a session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub id: String,
    pub project_id: String,
    pub cwd: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub status: TermStatus,
    pub exit: Option<TermExit>,
    pub created_at: i64,
}

/// One frame for attached clients.
#[derive(Debug, Clone)]
pub enum Output {
    Data(Arc<[u8]>),
    Exit(TermExit),
}

/// Bounded byte history.
#[derive(Debug, Default)]
struct Scrollback {
    bytes: VecDeque<u8>,
    cap: usize,
}

impl Scrollback {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend(chunk);
        let over = self.bytes.len().saturating_sub(self.cap);
        self.bytes.drain(..over);
    }
}

/// A live (or finished) shell.
pub struct Session {
    id: String,
    project_id: String,
    cwd: PathBuf,
    shell: PathBuf,
    created_at: i64,
    pid: Option<u32>,
    size: Mutex<(u16, u16)>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    /// Scrollback and the sender share this lock, so an attach sees every
    /// byte exactly once: either in the snapshot or on the channel.
    out: Mutex<Scrollback>,
    tx: broadcast::Sender<Output>,
    exit: watch::Sender<Option<TermExit>>,
    /// Process groups of the shell's descendants seen while it ran, still
    /// alive at the last look (POSIX).
    tracked: Mutex<BTreeSet<i32>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

/// What a newly attached client gets.
#[derive(Debug)]
pub struct Attached {
    pub scrollback: Vec<u8>,
    pub live: broadcast::Receiver<Output>,
    /// Already exited: no more output will come.
    pub exit: Option<TermExit>,
}

impl Session {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// The shell's process id (its process-group and session id on POSIX).
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    #[must_use]
    pub fn exit_status(&self) -> Option<TermExit> {
        self.exit.borrow().clone()
    }

    #[must_use]
    pub fn info(&self) -> TerminalInfo {
        let (cols, rows) = *lock(&self.size);
        let exit = self.exit_status();
        TerminalInfo {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            cwd: self.cwd.to_string_lossy().into_owned(),
            shell: self.shell.to_string_lossy().into_owned(),
            cols,
            rows,
            status: if exit.is_some() {
                TermStatus::Exited
            } else {
                TermStatus::Running
            },
            exit,
            created_at: self.created_at,
        }
    }

    /// Scrollback so far plus a subscription to what follows.
    #[must_use]
    pub fn attach(&self) -> Attached {
        let out = lock(&self.out);
        let live = self.tx.subscribe();
        let scrollback = out.bytes.iter().copied().collect();
        drop(out);
        Attached {
            scrollback,
            live,
            exit: self.exit_status(),
        }
    }

    /// Types `bytes` into the shell. Blocks while the PTY buffer is full,
    /// so async callers should use `spawn_blocking`.
    ///
    /// # Errors
    /// When the shell has gone away.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = lock(&self.writer);
        let w = writer
            .as_mut()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        w.write_all(bytes)?;
        w.flush()
    }

    /// Tells the shell the window changed size (clamped).
    pub fn resize(&self, cols: u16, rows: u16) {
        let (cols, rows) = clamp_size(Some(cols), Some(rows));
        let size = PtySize {
            rows,
            cols,
            ..PtySize::default()
        };
        if lock(&self.master).resize(size).is_ok() {
            *lock(&self.size) = (cols, rows);
        }
    }

    /// Waits until the shell exits or `timeout` passes.
    async fn wait_exit(&self, timeout: Duration) -> bool {
        let mut rx = self.exit.subscribe();
        tokio::time::timeout(timeout, rx.wait_for(Option::is_some))
            .await
            .is_ok_and(|r| r.is_ok())
    }

    /// Ends the shell and everything it started.
    async fn kill(&self, grace: Duration) {
        let Some(pid) = self.pid else {
            lock(&self.writer).take();
            return;
        };
        // Read before the shell can exit: its jobs lose the TTY with it.
        #[cfg(unix)]
        let groups = self.process_groups(pid).await;
        // EOF on stdin first: a shell at its prompt exits by itself.
        lock(&self.writer).take();
        #[cfg(unix)]
        {
            signal_groups(&groups, nix::sys::signal::Signal::SIGHUP);
            if !self.wait_exit(grace).await || groups_alive(&groups) {
                signal_groups(&groups, nix::sys::signal::Signal::SIGKILL);
                self.wait_exit(grace).await;
            }
        }
        #[cfg(windows)]
        {
            od_job::kill(pid);
            self.wait_exit(grace).await;
        }
        #[cfg(not(any(unix, windows)))]
        let _ = (pid, grace);
    }

    /// The shell's own group, the terminal's foreground job, the other jobs
    /// on the tab's TTY (job control puts each in a group of its own), the
    /// groups of the shell's descendants now and those noted earlier.
    #[cfg(unix)]
    async fn process_groups(&self, pid: u32) -> Vec<i32> {
        let mut groups: Vec<i32> = i32::try_from(pid).into_iter().collect();
        let (fg, tty) = {
            let master = lock(&self.master);
            (master.process_group_leader(), master.tty_name())
        };
        if let Some(tty) = tty {
            groups.extend(tty_groups(&tty).await);
        }
        groups.extend(fg);
        groups.extend(descendant_groups(pid).await);
        groups.extend(self.tracked_groups());
        let own = nix::unistd::getpgrp().as_raw();
        groups.retain(|&g| g > 1 && g != own);
        groups.sort_unstable();
        groups.dedup();
        groups
    }

    /// Notes the groups of the shell's descendants and forgets the ones
    /// that ended (so a reused group id is not killed later).
    #[cfg(unix)]
    async fn track(&self, pid: u32) {
        let now = descendant_groups(pid).await;
        let mut tracked = lock(&self.tracked);
        tracked.extend(now);
        tracked.retain(|&g| groups_alive(&[g]));
    }

    #[cfg(unix)]
    fn tracked_groups(&self) -> Vec<i32> {
        lock(&self.tracked).iter().copied().collect()
    }
}

/// While the shell runs, notes its descendants' process groups every
/// `every`: a job that left the session and lost its parent is found by
/// nothing else.
#[cfg(unix)]
fn spawn_tracker(session: &Arc<Session>, every: Duration) {
    let Some(pid) = session.pid else {
        return;
    };
    let weak = Arc::downgrade(session);
    let mut exit = session.exit.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = tokio::time::sleep(every) => {}
                _ = exit.wait_for(Option::is_some) => return,
            }
            let Some(session) = weak.upgrade() else {
                return;
            };
            session.track(pid).await;
        }
    });
}

/// [`od_job::descendant_groups`] off the async threads.
#[cfg(unix)]
async fn descendant_groups(root: u32) -> Vec<i32> {
    tokio::task::spawn_blocking(move || od_job::descendant_groups(root))
        .await
        .unwrap_or_default()
}

/// Process groups of everything whose controlling terminal is `tty` (a
/// job a script put in the background keeps it; one that left the session
/// with `setsid` does not).
#[cfg(unix)]
async fn tty_groups(tty: &Path) -> Vec<i32> {
    let name = tty.strip_prefix("/dev").unwrap_or(tty);
    let Ok(out) = tokio::process::Command::new("ps")
        .arg("-o")
        .arg("pgid=")
        .arg("-t")
        .arg(name)
        .output()
        .await
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|g| g.parse().ok())
        .collect()
}

#[cfg(unix)]
fn signal_groups(groups: &[i32], signal: nix::sys::signal::Signal) {
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;
    for &g in groups.iter().filter(|&&g| g > 1) {
        let _ = killpg(Pid::from_raw(g), signal);
    }
}

#[cfg(unix)]
fn groups_alive(groups: &[i32]) -> bool {
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;
    groups
        .iter()
        .filter(|&&g| g > 1)
        .any(|&g| killpg(Pid::from_raw(g), None).is_ok())
}

/// Every terminal of this daemon.
#[derive(Debug)]
pub struct Terminals {
    cfg: TermConfig,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl Default for Terminals {
    fn default() -> Self {
        Self::new(TermConfig::default())
    }
}

impl Terminals {
    #[must_use]
    pub fn new(cfg: TermConfig) -> Self {
        Self {
            cfg,
            sessions: Mutex::default(),
        }
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).get(id).cloned()
    }

    /// A project's sessions, oldest first.
    #[must_use]
    pub fn list(&self, project_id: &str) -> Vec<TerminalInfo> {
        let mut out: Vec<TerminalInfo> = lock(&self.sessions)
            .values()
            .filter(|s| s.project_id == project_id)
            .map(|s| s.info())
            .collect();
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        out
    }

    /// Starts a shell in `cwd`.
    ///
    /// # Errors
    /// [`TermError::Limit`] when too many shells run, [`TermError::BadCwd`],
    /// or the PTY/spawn failure.
    pub fn create(
        &self,
        project_id: &str,
        cwd: &Path,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Arc<Session>, TermError> {
        if !cwd.is_dir() {
            return Err(TermError::BadCwd(cwd.to_path_buf()));
        }
        {
            let mut sessions = lock(&self.sessions);
            // Finished tabs nobody closed do not count against the limit.
            sessions.retain(|_, s| s.exit_status().is_none());
            if sessions.len() >= self.cfg.max_sessions {
                return Err(TermError::Limit(self.cfg.max_sessions));
            }
        }
        let session = spawn_session(&self.cfg, project_id, cwd, clamp_size(cols, rows))?;
        let mut sessions = lock(&self.sessions);
        if sessions.len() >= self.cfg.max_sessions {
            drop(sessions);
            // Lost a race with another create: undo.
            let grace = self.cfg.kill_grace;
            let s = Arc::clone(&session);
            tokio::spawn(async move { s.kill(grace).await });
            return Err(TermError::Limit(self.cfg.max_sessions));
        }
        sessions.insert(session.id.clone(), Arc::clone(&session));
        Ok(session)
    }

    /// Removes a session and kills its shell tree. False when unknown.
    pub async fn close(&self, id: &str) -> bool {
        let Some(session) = lock(&self.sessions).remove(id) else {
            return false;
        };
        session.kill(self.cfg.kill_grace).await;
        true
    }

    /// Closes every session (daemon shutdown).
    pub async fn close_all(&self) {
        let all: Vec<Arc<Session>> = lock(&self.sessions).drain().map(|(_, s)| s).collect();
        for s in all {
            s.kill(self.cfg.kill_grace).await;
        }
    }

    /// Closes the sessions of one project (it was deleted).
    pub async fn close_project(&self, project_id: &str) {
        let gone: Vec<Arc<Session>> = {
            let mut sessions = lock(&self.sessions);
            let ids: Vec<String> = sessions
                .values()
                .filter(|s| s.project_id == project_id)
                .map(|s| s.id.clone())
                .collect();
            ids.iter().filter_map(|id| sessions.remove(id)).collect()
        };
        for s in gone {
            s.kill(self.cfg.kill_grace).await;
        }
    }
}

fn spawn_session(
    cfg: &TermConfig,
    project_id: &str,
    cwd: &Path,
    (cols, rows): (u16, u16),
) -> Result<Arc<Session>, TermError> {
    let spawn_err = |e: &dyn std::fmt::Display| TermError::Spawn(e.to_string());
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            ..PtySize::default()
        })
        .map_err(|e| spawn_err(&e))?;
    let shell = cfg.shell.clone().unwrap_or_else(default_shell);
    let mut cmd = CommandBuilder::new(&shell);
    cmd.cwd(cwd);
    for key in SECRET_ENV {
        cmd.env_remove(key);
    }
    cmd.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| spawn_err(&e))?;
    // Only the child keeps the slave open, so its exit ends our reads.
    drop(pair.slave);
    let pid = child.process_id();
    #[cfg(windows)]
    if let (Some(pid), Some(handle)) = (pid, child.as_raw_handle()) {
        if let Err(e) = od_job::track(pid, handle) {
            let _ = child.kill();
            return Err(spawn_err(&e));
        }
    }
    let reader = pair.master.try_clone_reader().map_err(|e| spawn_err(&e))?;
    let writer = pair.master.take_writer().map_err(|e| spawn_err(&e))?;
    let (tx, _) = broadcast::channel(1024);
    let (exit, _) = watch::channel(None);
    let session = Arc::new(Session {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.to_owned(),
        cwd: cwd.to_path_buf(),
        shell,
        created_at: now_ms(),
        pid,
        size: Mutex::new((cols, rows)),
        master: Mutex::new(pair.master),
        writer: Mutex::new(Some(writer)),
        out: Mutex::new(Scrollback {
            bytes: VecDeque::new(),
            cap: cfg.scrollback_bytes,
        }),
        tx,
        exit,
        tracked: Mutex::default(),
    });
    #[cfg(unix)]
    spawn_tracker(&session, cfg.track_every);
    let drained = Arc::new((Mutex::new(false), Condvar::new()));
    {
        let session = Arc::clone(&session);
        let drained = Arc::clone(&drained);
        std::thread::Builder::new()
            .name("od-term-read".into())
            .spawn(move || read_loop(&session, reader, &drained))
            .map_err(|e| spawn_err(&e))?;
    }
    {
        let session = Arc::clone(&session);
        std::thread::Builder::new()
            .name("od-term-wait".into())
            .spawn(move || {
                let status = child.wait();
                // Let the reader pass on the last bytes before `exit`.
                let (done, cv) = &*drained;
                let guard = lock(done);
                let _ = cv.wait_timeout_while(guard, DRAIN_WAIT, |d| !*d);
                let exit = match status {
                    Ok(s) => TermExit {
                        code: s.exit_code(),
                        signal: s.signal().map(str::to_owned),
                    },
                    Err(_) => TermExit {
                        code: 1,
                        signal: None,
                    },
                };
                #[cfg(windows)]
                if let Some(pid) = session.pid {
                    od_job::kill(pid);
                }
                tracing::debug!(terminal = %session.id, code = exit.code, "shell exited");
                lock(&session.writer).take();
                let _out = lock(&session.out);
                session.exit.send_replace(Some(exit.clone()));
                let _ = session.tx.send(Output::Exit(exit));
            })
            .map_err(|e| spawn_err(&e))?;
    }
    Ok(session)
}

fn read_loop(
    session: &Session,
    mut reader: Box<dyn Read + Send>,
    drained: &(Mutex<bool>, Condvar),
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk: Arc<[u8]> = Arc::from(&buf[..n]);
                let mut out = lock(&session.out);
                out.push(&chunk);
                let _ = session.tx.send(Output::Data(chunk));
            }
        }
    }
    let (done, cv) = drained;
    *lock(done) = true;
    cv.notify_all();
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_keeps_the_tail() {
        let mut s = Scrollback {
            bytes: VecDeque::new(),
            cap: 4,
        };
        s.push(b"abc");
        s.push(b"def");
        assert_eq!(s.bytes.iter().copied().collect::<Vec<_>>(), b"cdef");
    }

    #[test]
    fn sizes_are_clamped() {
        assert_eq!(clamp_size(None, None), (80, 24));
        assert_eq!(clamp_size(Some(0), Some(5000)), (1, MAX_DIMENSION));
    }

    /// A noted group is forgotten once it ends: its id may be reused by a
    /// stranger by the time the tab closes.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_tracker_forgets_groups_that_ended() {
        let dir = std::env::temp_dir();
        let terms = Terminals::new(TermConfig {
            shell: Some("/bin/sh".into()),
            track_every: Duration::from_millis(100),
            ..TermConfig::default()
        });
        let s = terms.create("p", &dir, None, None).unwrap();
        s.write(b"sleep 1 &\r").unwrap();
        let until = |want: bool| {
            let s = Arc::clone(&s);
            async move {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                while s.tracked_groups().is_empty() == want {
                    assert!(tokio::time::Instant::now() < deadline, "tracked: {want}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };
        until(true).await;
        until(false).await;
        terms.close_all().await;
    }
}
