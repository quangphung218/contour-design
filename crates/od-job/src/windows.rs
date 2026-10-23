//! Windows Job Objects (spec 5.4): the Windows counterpart of a POSIX
//! process group. Each agent or terminal shell is assigned to its own job
//! with "kill on job close", so closing the job ends the process and every
//! descendant it started, and a daemon that dies takes them with it.
//!
//! Jobs are tracked by the leader's process id, the same number POSIX code
//! records as the process-group id.

use std::collections::HashMap;
use std::io;
use std::os::windows::io::RawHandle;
use std::sync::{LazyLock, Mutex, PoisonError};
use win32job::{ExtendedLimitInfo, Job, JobError};

static JOBS: LazyLock<Mutex<HashMap<u32, Job>>> = LazyLock::new(Mutex::default);

fn io_err(e: JobError) -> io::Error {
    io::Error::other(e.to_string())
}

fn jobs() -> std::sync::MutexGuard<'static, HashMap<u32, Job>> {
    JOBS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts the process behind `handle` (pid `pid`) into a fresh kill-on-close
/// job. Children it starts afterwards join the job automatically.
///
/// # Errors
/// When the job cannot be created or the process cannot be assigned (it
/// already exited, or sits in a job that forbids nesting).
pub fn track(pid: u32, handle: RawHandle) -> io::Result<()> {
    let mut info = ExtendedLimitInfo::new();
    info.limit_kill_on_job_close();
    let job = Job::create_with_limit_info(&info).map_err(io_err)?;
    // The handle is only read by the call; the owner keeps it open.
    job.assign_process(handle as isize).map_err(io_err)?;
    jobs().insert(pid, job);
    Ok(())
}

/// Whether any process of `pid`'s job still runs.
#[must_use]
pub fn alive(pid: u32) -> bool {
    jobs()
        .get(&pid)
        .and_then(|job| job.query_process_id_list().ok())
        .is_some_and(|ids| !ids.is_empty())
}

/// Kills every process in `pid`'s job by closing it, and forgets the job.
/// Returns false when no job is tracked for `pid`.
pub fn kill(pid: u32) -> bool {
    // Dropping the last handle triggers JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.
    jobs().remove(&pid).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn killing_the_job_ends_the_child_tree() {
        // `cmd` starts a grandchild `ping` that outlives a plain kill of cmd.
        let mut child = Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .spawn()
            .expect("spawn cmd");
        let pid = child.id();
        track(pid, child.as_raw_handle()).expect("track");
        std::thread::sleep(Duration::from_millis(300));
        assert!(alive(pid));
        assert!(kill(pid));
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().expect("wait").is_none() {
            assert!(Instant::now() < deadline, "child survived the job");
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(!alive(pid));
        assert!(!kill(pid), "second kill finds nothing");
    }
}
