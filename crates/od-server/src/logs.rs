//! The daemon log: `tracing` to stderr and, when asked, to
//! `<data>/logs/daemon.log`, rolled over by size ([`MAX_FILE_BYTES`],
//! [`KEEP_FILES`] files). The desktop app always writes the file (a
//! Finder launch drops stdout and stderr); the `od` daemon does with
//! `od daemon start --log-file` or `OD_LOG_FILE=1`. With the file on, a
//! panic writes its message and stack there before the process goes on to
//! unwind or abort.
//!
//! Nothing leaves the machine: Report Issue and Export diagnostics read the
//! tail of the file ([`tail_lines`]).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use tracing_subscriber::Layer as _;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Folder of the log files, under the data root.
pub const LOGS_DIR: &str = "logs";
/// The file being written; older ones are `daemon.1.log` (newest) to
/// `daemon.4.log`.
pub const LOG_FILE: &str = "daemon.log";
/// Size at which the file rolls over.
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Files kept, the current one included.
pub const KEEP_FILES: usize = 5;
/// Environment variable that turns the file on for the `od` daemon.
pub const LOG_FILE_ENV: &str = "OD_LOG_FILE";

/// `<data>/logs`.
#[must_use]
pub fn dir(data_root: &Path) -> PathBuf {
    data_root.join(LOGS_DIR)
}

/// `daemon.log` for `n == 0`, else `daemon.<n>.log`.
fn file_name(n: usize) -> String {
    if n == 0 {
        LOG_FILE.to_owned()
    } else {
        format!("daemon.{n}.log")
    }
}

/// A log file that rolls over once it would pass `max_bytes`, keeping
/// `keep` files in all.
#[derive(Debug)]
pub struct RollingFile {
    dir: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: File,
    size: u64,
}

impl RollingFile {
    /// Opens (appends to) `<dir>/daemon.log`, creating `dir`.
    ///
    /// # Errors
    /// When the folder or the file cannot be created.
    pub fn open(dir: &Path, max_bytes: u64, keep: usize) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let file = append(&dir.join(LOG_FILE))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            max_bytes,
            keep: keep.max(1),
            file,
            size,
        })
    }

    fn roll_over(&mut self) -> io::Result<()> {
        let path = |n| self.dir.join(file_name(n));
        let last = path(self.keep - 1);
        if last.exists() {
            std::fs::remove_file(&last)?;
        }
        for n in (0..self.keep - 1).rev() {
            if path(n).exists() {
                std::fs::rename(path(n), path(n + 1))?;
            }
        }
        self.file = append(&path(0))?;
        self.size = 0;
        Ok(())
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

impl Write for RollingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.size > 0 && self.size.saturating_add(len) > self.max_bytes {
            self.roll_over()?;
        }
        let n = self.file.write(buf)?;
        self.size = self
            .size
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// The shared log file: the `tracing` writer and the panic hook.
#[derive(Debug)]
pub struct LogFile {
    path: PathBuf,
    file: Mutex<RollingFile>,
}

impl LogFile {
    #[must_use]
    pub fn new(file: RollingFile) -> Self {
        Self {
            path: file.dir.join(LOG_FILE),
            file: Mutex::new(file),
        }
    }

    /// Writes `text` from the panic hook. A panic while this thread holds
    /// the lock (or another thread does) appends to the file directly
    /// instead of waiting on it.
    fn write_panic(&self, text: &str) {
        let written = match self.file.try_lock() {
            Ok(mut f) => f.write_all(text.as_bytes()),
            Err(TryLockError::Poisoned(p)) => p.into_inner().write_all(text.as_bytes()),
            Err(TryLockError::WouldBlock) => Err(io::ErrorKind::WouldBlock.into()),
        };
        if written.is_err()
            && let Ok(mut f) = append(&self.path)
        {
            let _ = f.write_all(text.as_bytes());
        }
    }
}

impl Write for &LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flush()
    }
}

/// What the panic hook writes: the time, the thread, the message and the
/// stack.
#[must_use]
pub fn panic_report(info: &std::panic::PanicHookInfo<'_>) -> String {
    let thread = std::thread::current();
    format!(
        "{} PANIC thread '{}' {info}\nstack backtrace:\n{}\n",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        thread.name().unwrap_or("<unnamed>"),
        std::backtrace::Backtrace::force_capture(),
    )
}

/// Writes each panic to `file`, then runs the hook that was set before
/// (the default one prints to stderr).
pub fn install_panic_hook(file: Arc<LogFile>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        file.write_panic(&panic_report(info));
        previous(info);
    }));
}

/// Whether `OD_LOG_FILE` asks for the log file (`1`, `true`, `yes`, `on`).
#[must_use]
pub fn file_from_env() -> bool {
    std::env::var(LOG_FILE_ENV).is_ok_and(|v| is_on(&v))
}

fn is_on(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Runs `f` with a console-only subscriber, for what happens before
/// [`init`] can know the data folder (resolving it may move the folder of
/// an older version, and logs that).
pub fn with_console<T>(f: impl FnOnce() -> T) -> T {
    let console = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(io::stderr)
            .with_filter(env_filter()),
    );
    tracing::subscriber::with_default(console, f)
}

fn env_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

/// Sets the global `tracing` subscriber: stderr, filtered by
/// `RUST_LOG` (default `info`), plus `<data_root>/logs/daemon.log` and the
/// panic hook when `data_root` is given. A file that cannot be opened is
/// logged to the console and skipped. Call once, at startup.
pub fn init(data_root: Option<&Path>) {
    let opened = data_root.map(|root| {
        let logs = dir(root);
        RollingFile::open(&logs, MAX_FILE_BYTES, KEEP_FILES)
            .map(|f| Arc::new(LogFile::new(f)))
            .map_err(|e| (logs, e))
    });
    let file = match &opened {
        Some(Ok(file)) => Some(Arc::clone(file)),
        _ => None,
    };
    let file_layer = file.clone().map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(f)
            .with_filter(env_filter())
    });
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(io::stderr)
                .with_filter(env_filter()),
        )
        .with(file_layer)
        .try_init();
    match opened {
        Some(Ok(_)) => {}
        Some(Err((logs, e))) => {
            tracing::warn!(error = %e, dir = %logs.display(), "no log file");
        }
        None => {}
    }
    if let Some(file) = file {
        install_panic_hook(file);
    }
}

/// The last `n` lines of the log, oldest first, reading into the previous
/// file when the current one is shorter. Empty when there is no log.
#[must_use]
pub fn tail_lines(data_root: &Path, n: usize) -> Vec<String> {
    let logs = dir(data_root);
    let mut lines: Vec<String> = Vec::new();
    for k in 0..KEEP_FILES {
        if lines.len() >= n {
            break;
        }
        let Ok(bytes) = std::fs::read(logs.join(file_name(k))) else {
            break;
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut older: Vec<String> = text
            .lines()
            .rev()
            .take(n - lines.len())
            .map(str::to_owned)
            .collect();
        older.reverse();
        older.append(&mut lines);
        lines = older;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_rolls_over_and_keeps_a_few() {
        let tmp = tempfile::tempdir().unwrap();
        let mut f = RollingFile::open(tmp.path(), 100, 3).unwrap();
        let line = |i: usize| format!("{i:0>59}\n"); // 60 bytes
        for i in 0..10 {
            f.write_all(line(i).as_bytes()).unwrap();
        }
        let mut names: Vec<String> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["daemon.1.log", "daemon.2.log", "daemon.log"]);
        for name in &names {
            let len = std::fs::metadata(tmp.path().join(name)).unwrap().len();
            assert!(len <= 100, "{name}: {len}");
        }
        let read = |name: &str| std::fs::read_to_string(tmp.path().join(name)).unwrap();
        assert_eq!(read(LOG_FILE), line(9));
        assert_eq!(read("daemon.2.log"), line(7));
    }

    fn log_file(dir: &Path) -> LogFile {
        LogFile::new(RollingFile::open(dir, MAX_FILE_BYTES, KEEP_FILES).unwrap())
    }

    #[test]
    fn a_panic_while_the_log_is_locked_still_writes_and_returns() {
        let tmp = tempfile::tempdir().unwrap();
        let file = Arc::new(log_file(tmp.path()));
        let held = file.file.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let writer = Arc::clone(&file);
        let worker = std::thread::spawn(move || {
            writer.write_panic("PANIC while locked\n");
            let _ = tx.send(());
        });
        let returned = rx.recv_timeout(std::time::Duration::from_secs(5));
        drop(held);
        worker.join().unwrap();
        assert!(returned.is_ok(), "write_panic waited on the lock");
        let text = std::fs::read_to_string(tmp.path().join(LOG_FILE)).unwrap();
        assert_eq!(text, "PANIC while locked\n");
    }

    #[test]
    fn a_poisoned_log_still_takes_the_panic_through_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = Arc::new(log_file(tmp.path()));
        let poison = Arc::clone(&file);
        let _ = std::thread::spawn(move || {
            let _g = poison.file.lock().unwrap();
            panic!("poison the log lock");
        })
        .join();
        assert!(file.file.is_poisoned());
        file.write_panic("PANIC after poison\n");
        let text = std::fs::read_to_string(tmp.path().join(LOG_FILE)).unwrap();
        assert_eq!(text, "PANIC after poison\n");
        // Written through the rolling file, so its size counts it.
        let size = file
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .size;
        assert_eq!(size, 19);
    }

    #[test]
    fn od_log_file_takes_the_usual_yes_words() {
        for v in ["1", "true", "YES", " on "] {
            assert!(is_on(v), "{v}");
        }
        for v in ["", "0", "false", "off", "no", "2"] {
            assert!(!is_on(v), "{v}");
        }
    }

    #[test]
    fn a_reopened_file_counts_what_it_holds() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(LOG_FILE), "x".repeat(90)).unwrap();
        let mut f = RollingFile::open(tmp.path(), 100, 2).unwrap();
        f.write_all(&[b'y'; 20]).unwrap();
        assert!(tmp.path().join("daemon.1.log").exists());
    }

    #[test]
    fn the_tail_reads_back_into_the_previous_file() {
        let tmp = tempfile::tempdir().unwrap();
        let logs = dir(tmp.path());
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("daemon.1.log"), "a\nb\nc\n").unwrap();
        std::fs::write(logs.join(LOG_FILE), "d\ne\n").unwrap();
        assert_eq!(tail_lines(tmp.path(), 3), ["c", "d", "e"]);
        assert_eq!(tail_lines(tmp.path(), 1), ["e"]);
        assert_eq!(tail_lines(tmp.path(), 10).len(), 5);
        assert!(tail_lines(&tmp.path().join("none"), 3).is_empty());
    }
}
