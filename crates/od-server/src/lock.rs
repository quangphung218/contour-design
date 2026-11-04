//! One daemon per data dir.
//!
//! `<data>/daemon.lock` records the daemon's URL, and an exclusive OS lock
//! is held for the daemon's lifetime: a `flock` on that file on unix, a
//! `LockFileEx` (through `fs4`) on `<data>/daemon.lock.held` on Windows. A
//! second daemon on the same folder refuses to start instead of treating
//! the first one's live runs as orphans.
//!
//! Windows locks are mandatory: a locked `daemon.lock` could not be read
//! by the clients that look up the running app's URL in it (`od mcp`, the
//! client commands), so the lock is on a file nobody reads.

use std::fs::{File, OpenOptions};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

/// File name of the lock inside the data dir.
pub const LOCK_FILE: &str = "daemon.lock";
/// The file the Windows OS lock is on, beside [`LOCK_FILE`].
pub const HELD_FILE: &str = "daemon.lock.held";

/// Why the lock could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("{}", held_message(.dir, .url.as_deref()))]
    Held { dir: PathBuf, url: Option<String> },
    #[error("opening {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn held_message(dir: &Path, url: Option<&str>) -> String {
    let dir = dir.display();
    match url {
        Some(url) => format!(
            "an od daemon is already running at {url} on {dir}; use it \
             (od --daemon-url {url} …) or set OD_DATA_DIR to another folder"
        ),
        None => format!(
            "an od daemon is already running on {dir}; use it or set OD_DATA_DIR \
             to another folder"
        ),
    }
}

/// [`LockError::Held`], with the URL the holder recorded in `file`.
fn held(data_root: &Path, file: &mut File) -> LockError {
    use std::io::Read;
    let mut url = String::new();
    let _ = file.read_to_string(&mut url);
    let url = url.trim();
    LockError::Held {
        dir: data_root.to_path_buf(),
        url: (!url.is_empty()).then(|| url.to_owned()),
    }
}

/// The held lock. Dropping it releases the data dir.
#[derive(Debug)]
pub struct DaemonLock {
    #[cfg(unix)]
    file: nix::fcntl::Flock<File>,
    #[cfg(not(unix))]
    file: File,
    /// [`HELD_FILE`], locked.
    #[cfg(not(unix))]
    _held: File,
}

impl DaemonLock {
    /// Takes the lock on `data_root`, creating the folder if needed.
    ///
    /// # Errors
    /// [`LockError::Held`] (with the holder's URL, when recorded) if another
    /// daemon has it; [`LockError::Io`] when the file cannot be opened.
    pub fn acquire(data_root: &Path) -> Result<Self, LockError> {
        let path = data_root.join(LOCK_FILE);
        let io = |source| LockError::Io {
            path: path.clone(),
            source,
        };
        std::fs::create_dir_all(data_root).map_err(io)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io)?;
        #[cfg(unix)]
        {
            use nix::fcntl::{Flock, FlockArg};
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(file) => Ok(Self { file }),
                Err((mut file, nix::errno::Errno::EWOULDBLOCK)) => Err(held(data_root, &mut file)),
                Err((_, errno)) => Err(io(errno.into())),
            }
        }
        #[cfg(not(unix))]
        {
            let held_path = data_root.join(HELD_FILE);
            let held_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&held_path)
                .map_err(|source| LockError::Io {
                    path: held_path.clone(),
                    source,
                })?;
            // Through fs4: `File::try_lock` needs Rust 1.89, newer than ours.
            match fs4::FileExt::try_lock(&held_file) {
                Ok(()) => Ok(Self {
                    file,
                    _held: held_file,
                }),
                Err(fs4::TryLockError::WouldBlock) => {
                    let mut file = file;
                    Err(held(data_root, &mut file))
                }
                Err(fs4::TryLockError::Error(source)) => Err(LockError::Io {
                    path: held_path,
                    source,
                }),
            }
        }
    }

    /// Records the daemon's URL for a second daemon's error message.
    ///
    /// # Errors
    /// When the lock file cannot be rewritten.
    pub fn record_url(&mut self, url: &str) -> std::io::Result<()> {
        let file: &mut File = &mut self.file;
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(url.as_bytes())?;
        file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On every platform (Windows took no lock before beta.19): a second
    /// lock on the folder is refused with the holder's URL, the URL stays
    /// readable by clients while the lock is held, and the folder is free
    /// once the holder drops it.
    #[test]
    fn one_holder_per_data_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("data");
        let mut first = DaemonLock::acquire(&data).expect("first lock");
        first.record_url("http://127.0.0.1:5001").expect("record");

        let url = std::fs::read_to_string(data.join(LOCK_FILE)).expect("clients read the url");
        assert_eq!(url, "http://127.0.0.1:5001");
        match DaemonLock::acquire(&data) {
            Err(LockError::Held { dir, url }) => {
                assert_eq!(dir, data);
                assert_eq!(url.as_deref(), Some("http://127.0.0.1:5001"));
            }
            other => panic!("a second holder: {other:?}"),
        }

        drop(first);
        DaemonLock::acquire(&data).expect("free again");
    }
}
