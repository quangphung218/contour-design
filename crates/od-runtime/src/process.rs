//! Spawning agents as process-group leaders and stopping the whole tree
//! (spec 2.3, 5.4). POSIX only for now: `process_group(0)` + `killpg`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

/// How long each escalation step waits (spec 2.3: 3 s).
pub const KILL_GRACE: Duration = Duration::from_secs(3);

const POLL: Duration = Duration::from_millis(25);

/// Everything needed to start one attempt.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The complete environment (the parent's is not inherited).
    pub env: Vec<(OsString, OsString)>,
}

/// Starts the agent as the leader of a new process group with piped stdio.
///
/// # Errors
/// The spawn error (missing binary, permission, bad cwd).
pub fn spawn(spec: &SpawnSpec) -> std::io::Result<Child> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    #[cfg(unix)]
    cmd.process_group(0);
    cmd.spawn()
}

#[cfg(unix)]
mod imp {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    pub fn signal_group(pgid: i32, signal: Option<Signal>) -> bool {
        pgid > 1 && killpg(Pid::from_raw(pgid), signal).is_ok()
    }

    pub const TERM: Option<Signal> = Some(Signal::SIGTERM);
    pub const KILL: Option<Signal> = Some(Signal::SIGKILL);
}

/// Whether any process of the group still exists.
#[cfg(unix)]
#[must_use]
pub fn group_alive(pgid: i32) -> bool {
    imp::signal_group(pgid, None)
}

#[cfg(not(unix))]
#[must_use]
pub fn group_alive(_pgid: i32) -> bool {
    false
}

/// Sends SIGTERM to the group, waits up to `grace` for it to disappear,
/// then SIGKILLs whatever is left and waits up to `grace` again. Returns
/// true when SIGKILL was needed.
#[cfg(unix)]
pub async fn terminate_group(pgid: i32, grace: Duration) -> bool {
    if !imp::signal_group(pgid, imp::TERM) {
        return false;
    }
    if wait_group_gone(pgid, grace).await {
        return false;
    }
    imp::signal_group(pgid, imp::KILL);
    wait_group_gone(pgid, grace).await;
    true
}

#[cfg(not(unix))]
pub async fn terminate_group(_pgid: i32, _grace: Duration) -> bool {
    false
}

/// Polls until the group has no processes or `timeout` passes.
pub async fn wait_group_gone(pgid: i32, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !group_alive(pgid) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Kills a process group a crashed daemon left behind, but only when its
/// leader still runs the recorded binary: after a reboot the id may belong
/// to an unrelated program.
pub fn reap_orphan(pgid: i32, bin: Option<&str>) -> bool {
    #[cfg(unix)]
    {
        let Some(bin) = bin.filter(|b| !b.is_empty()) else {
            return false;
        };
        if !group_alive(pgid) {
            return false;
        }
        let Ok(out) = std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &pgid.to_string()])
            .output()
        else {
            return false;
        };
        let command = String::from_utf8_lossy(&out.stdout);
        let name = std::path::Path::new(bin)
            .file_name()
            .map_or_else(|| bin.to_owned(), |n| n.to_string_lossy().into_owned());
        if !command.contains(&name) {
            return false;
        }
        imp::signal_group(pgid, imp::KILL)
    }
    #[cfg(not(unix))]
    {
        let _ = (pgid, bin);
        false
    }
}
