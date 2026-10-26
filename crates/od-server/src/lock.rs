//! One daemon per data dir.
//!
//! `<data>/daemon.lock` holds an exclusive `flock` for the daemon's
//! lifetime and records its URL. A second daemon on the same folder refuses
//! to start instead of treating the first one's live runs as orphans.

use std::fs::{File, OpenOptions};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

/// File name of the lock inside the data dir.
pub const LOCK_FILE: &str = "daemon.lock";

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

/// The held lock. Dropping it releases the data dir.
#[derive(Debug)]
pub struct DaemonLock {
    #[cfg(unix)]
    file: nix::fcntl::Flock<File>,
    #[cfg(not(unix))]
    file: File,
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
            use std::io::Read;
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(file) => Ok(Self { file }),
                Err((mut file, nix::errno::Errno::EWOULDBLOCK)) => {
                    let mut url = String::new();
                    let _ = file.read_to_string(&mut url);
                    let url = url.trim();
                    Err(LockError::Held {
                        dir: data_root.to_path_buf(),
                        url: (!url.is_empty()).then(|| url.to_owned()),
                    })
                }
                Err((_, errno)) => Err(io(errno.into())),
            }
        }
        // No flock off unix: the desktop app and CLI there are single-user
        // and orphan cleanup is a no-op (`reap_orphan`).
        #[cfg(not(unix))]
        {
            Ok(Self { file })
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
