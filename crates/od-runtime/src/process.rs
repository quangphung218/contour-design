//! Spawning agents as process-group leaders and stopping the whole tree
//! (spec 2.3, 5.4). POSIX: `process_group(0)` + `killpg`. Windows: each
//! agent gets its own kill-on-close Job Object (`od-job`), tracked by the
//! leader's pid, which stands in for the group id.

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
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    let child = cmd.spawn()?;
    #[cfg(windows)]
    track_job(&child)?;
    Ok(child)
}

/// `CREATE_NEW_PROCESS_GROUP`: the agent does not get the daemon's Ctrl+C.
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Puts a freshly spawned child into its own Job Object. On failure the
/// child is killed: an agent whose tree cannot be stopped must not run.
#[cfg(windows)]
fn track_job(child: &Child) -> std::io::Result<()> {
    let (Some(pid), Some(handle)) = (child.id(), child.raw_handle()) else {
        return Ok(()); // already exited and reaped
    };
    od_job::track(pid, handle).inspect_err(|_| {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .status();
    })
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

#[cfg(windows)]
#[must_use]
pub fn group_alive(pgid: i32) -> bool {
    u32::try_from(pgid).is_ok_and(od_job::alive)
}

#[cfg(not(any(unix, windows)))]
#[must_use]
pub fn group_alive(_pgid: i32) -> bool {
    false
}

/// Drops the bookkeeping for a finished agent. Windows closes its job,
/// which also ends anything still left in it; POSIX has nothing to free.
pub fn release_group(pgid: i32) {
    #[cfg(windows)]
    if let Ok(pid) = u32::try_from(pgid) {
        od_job::kill(pid);
    }
    #[cfg(not(windows))]
    let _ = pgid;
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

/// Windows has no SIGTERM for a console tree: closing the job kills it.
/// Returns true when a live job was killed.
#[cfg(windows)]
pub async fn terminate_group(pgid: i32, grace: Duration) -> bool {
    let Ok(pid) = u32::try_from(pgid) else {
        return false;
    };
    let was_alive = od_job::alive(pid);
    od_job::kill(pid);
    wait_group_gone(pgid, grace).await;
    was_alive
}

#[cfg(not(any(unix, windows)))]
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
