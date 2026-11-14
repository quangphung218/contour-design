//! The data folder's lock, as the window takes it (spec 5.1).
//!
//! One daemon per data folder ([`DaemonLock`]). The app's own copy may
//! still hold it for a few seconds: one that is quitting stops its agents
//! first (up to `STOP_ALL_LIMIT`, then the terminals), and Update now opens
//! the next copy as the last one leaves. The app records its URL in
//! [`APP_URL_FILE`] beside the lock, so a launch that finds the lock held
//! can tell that copy (the URL in `daemon.lock` is the app's) from an
//! `od daemon start` (its own URL there): it waits for the first, up to
//! [`APP_WAIT`], and says at once what to do about the second.
//!
//! A second launch while the app runs never gets here: the single-instance
//! plugin shows the running window and quits it first ([`single_instance`]).

use od_server::lock::LOCK_FILE;
use od_server::{DaemonLock, LockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Beside `daemon.lock`: the URL the desktop app last recorded there.
/// Only the app writes it; clients read `daemon.lock`.
pub const APP_URL_FILE: &str = "daemon.lock.app";

/// How long a launch waits for the app's own last copy to let go.
pub const APP_WAIT: Duration = Duration::from_secs(12);

/// How often it tries again meanwhile.
const POLL: Duration = Duration::from_millis(250);

/// Whether a launch keeps a single instance: only on the app's own data
/// folder. A copy given its own `OD_DATA_DIR` (QA, a debug build beside
/// the installed app) runs alongside, as before; the plugin goes by the
/// bundle identifier, not the folder.
pub fn single_instance(data_dir_env: Option<&std::ffi::OsStr>) -> bool {
    data_dir_env.is_none_or(std::ffi::OsStr::is_empty)
}

/// Who holds the lock, as far as the files tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// Another copy of the app: quitting, restarting, or open on a folder
    /// of its own `OD_DATA_DIR` too.
    App,
    /// Anything else: `od daemon start`, `<app> daemon start`.
    Other,
}

/// The holder whose URL is `lock_url` (from `daemon.lock`), given the URL
/// the app last recorded (`app_url`, from [`APP_URL_FILE`]).
fn holder(lock_url: Option<&str>, app_url: Option<&str>) -> Holder {
    match (lock_url.map(str::trim), app_url.map(str::trim)) {
        (Some(lock), Some(app)) if !lock.is_empty() && lock == app => Holder::App,
        _ => Holder::Other,
    }
}

/// What to do about a held lock, `waited` after the first try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Retry,
    GiveUp(Holder),
}

fn step(holder: Holder, waited: Duration, limit: Duration) -> Step {
    if holder == Holder::App && waited < limit {
        Step::Retry
    } else {
        Step::GiveUp(holder)
    }
}

/// Records `url` as the app's, after [`DaemonLock::record_url`].
///
/// # Errors
/// When the file cannot be written.
pub fn record_app_url(data_root: &Path, url: &str) -> std::io::Result<()> {
    std::fs::write(data_root.join(APP_URL_FILE), url)
}

/// Records `url` in `daemon.lock`, for clients, then as the app's
/// ([`record_app_url`]), so the next launch waits for this copy to quit.
/// Without the second the next launch does not wait: only logged.
///
/// # Errors
/// When `daemon.lock` cannot be written.
pub fn record(lock: &mut DaemonLock, data_root: &Path, url: &str) -> std::io::Result<()> {
    lock.record_url(url)?;
    if let Err(e) = record_app_url(data_root, url) {
        tracing::warn!(error = %e, "cannot record the app's url beside the lock");
    }
    Ok(())
}

/// The lock could not be taken.
#[derive(Debug)]
pub struct Refused {
    /// Who holds it; `None` when the lock file could not be opened.
    pub holder: Option<Holder>,
    pub error: LockError,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for Refused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Takes the lock on `data_root`, waiting up to `limit` while the app's
/// own last copy holds it. Blocks: run it off the async runtime.
///
/// # Errors
/// [`Refused`]: held by something else (at once), by the app past
/// `limit`, or the file cannot be opened.
pub fn acquire_waiting(data_root: &Path, limit: Duration) -> Result<DaemonLock, Refused> {
    let started = Instant::now();
    loop {
        let error = match DaemonLock::acquire(data_root) {
            Ok(lock) => return Ok(lock),
            Err(error @ LockError::Io { .. }) => {
                return Err(Refused {
                    holder: None,
                    error,
                });
            }
            Err(error) => error,
        };
        let LockError::Held { url, .. } = &error else {
            unreachable!("Io returned above");
        };
        let app_url = std::fs::read_to_string(data_root.join(APP_URL_FILE)).ok();
        let holder = holder(url.as_deref(), app_url.as_deref());
        match step(holder, started.elapsed(), limit) {
            Step::Retry => std::thread::sleep(POLL),
            Step::GiveUp(holder) => {
                return Err(Refused {
                    holder: Some(holder),
                    error,
                });
            }
        }
    }
}

/// The start-up dialog's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Vi,
}

/// The UI's language as the app last saved it (`locale` in
/// `app-config.json`), else the system's, as the UI follows it then.
/// Vietnamese or English: the dialog shows before the UI and its
/// language files.
pub fn lang(setting: Option<&str>, system: Option<&str>) -> Lang {
    let tag = setting.filter(|s| !s.trim().is_empty()).or(system);
    match tag {
        Some(tag) if tag.trim().to_ascii_lowercase().starts_with("vi") => Lang::Vi,
        _ => Lang::En,
    }
}

/// [`lang`] for `data_root`'s settings and this system.
pub fn lang_of(data_root: &Path) -> Lang {
    let setting = od_store::DataDir::new(data_root.to_path_buf())
        .app_setting("locale")
        .and_then(|v| v.as_str().map(str::to_owned));
    lang(setting.as_deref(), sys_locale::get_locale().as_deref())
}

/// The words of the dialog for a held lock: what is going on and what to
/// do. No URL or folder: those go to the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text {
    pub title: &'static str,
    pub message: &'static str,
    pub button: &'static str,
}

pub fn held_text(holder: Holder, lang: Lang) -> Text {
    match (holder, lang) {
        (Holder::Other, Lang::En) => Text {
            title: "Contour can't open yet",
            message: "Another Contour (an older version, perhaps hidden) or \
                      \"od daemon start\" in a terminal is using Contour's \
                      data. Quit that Contour, or stop the command (Ctrl+C in \
                      that terminal), then open Contour again.",
            button: "Quit",
        },
        (Holder::Other, Lang::Vi) => Text {
            title: "Chưa mở được Contour",
            message: "Một Contour khác (bản cũ, có thể đang ẩn) hoặc lệnh \
                      \"od daemon start\" trong terminal đang dùng dữ liệu \
                      của Contour. Thoát Contour đó hoặc dừng lệnh (Ctrl+C \
                      trong terminal) rồi mở lại Contour.",
            button: "Thoát",
        },
        (Holder::App, Lang::En) => Text {
            title: "Contour is already open",
            message: "Another Contour window is open, or the last one is \
                      still closing. Switch to it, or wait a few seconds and \
                      open Contour again.",
            button: "Quit",
        },
        (Holder::App, Lang::Vi) => Text {
            title: "Contour đang mở",
            message: "Một cửa sổ Contour khác đang mở, hoặc bản trước chưa \
                      đóng xong. Chuyển sang cửa sổ đó, hoặc đợi vài giây rồi \
                      mở lại Contour.",
            button: "Thoát",
        },
    }
}

/// `daemon.lock`'s path, for the log.
pub fn lock_path(data_root: &Path) -> PathBuf {
    data_root.join(LOCK_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "http://127.0.0.1:5001";
    const CLI: &str = "http://127.0.0.1:7456";

    /// A lock on `data` held as a daemon at `url` holds it.
    fn hold(data: &Path, url: &str) -> DaemonLock {
        let mut lock = DaemonLock::acquire(data).expect("first lock");
        lock.record_url(url).expect("record");
        lock
    }

    #[test]
    fn only_the_apps_own_url_counts_as_the_app() {
        assert_eq!(holder(Some(APP), Some(APP)), Holder::App);
        assert_eq!(holder(Some(&format!("{APP}\n")), Some(APP)), Holder::App);
        // An `od daemon start` after the app quit: its URL, the app's old one.
        assert_eq!(holder(Some(CLI), Some(APP)), Holder::Other);
        // No app ever ran on this folder.
        assert_eq!(holder(Some(CLI), None), Holder::Other);
        // A holder that has not recorded its URL, and an empty marker.
        assert_eq!(holder(None, Some(APP)), Holder::Other);
        assert_eq!(holder(Some(""), Some("")), Holder::Other);
    }

    #[test]
    fn waits_for_the_app_only_and_only_so_long() {
        let limit = Duration::from_secs(12);
        assert_eq!(step(Holder::App, Duration::ZERO, limit), Step::Retry);
        assert_eq!(
            step(Holder::App, Duration::from_millis(11_999), limit),
            Step::Retry
        );
        assert_eq!(step(Holder::App, limit, limit), Step::GiveUp(Holder::App));
        assert_eq!(
            step(Holder::Other, Duration::ZERO, limit),
            Step::GiveUp(Holder::Other)
        );
    }

    /// Quitting or Update now: the last copy lets go 400 ms in, and the
    /// launch takes the lock then instead of failing.
    #[test]
    fn takes_the_lock_once_the_quitting_app_lets_go() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("data");
        let held = hold(&data, APP);
        record_app_url(&data, APP).expect("marker");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(held);
        });
        let started = Instant::now();
        let lock = acquire_waiting(&data, Duration::from_secs(10));
        release.join().expect("release");
        assert!(lock.is_ok(), "{:?}", lock.err());
        assert!(started.elapsed() >= Duration::from_millis(400));
    }

    /// What the app records once it listens: the next launch knows the
    /// holder for this app, and clients find its URL.
    #[test]
    fn the_recorded_url_marks_the_holder_as_the_app() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("data");
        let mut lock = DaemonLock::acquire(&data).expect("first lock");
        record(&mut lock, &data, APP).expect("record");
        let read = |file: &str| std::fs::read_to_string(data.join(file)).expect(file);
        assert_eq!(read(LOCK_FILE), APP);
        assert_eq!(read(APP_URL_FILE), APP);
        let refused = acquire_waiting(&data, Duration::from_millis(300)).expect_err("held");
        assert_eq!(refused.holder, Some(Holder::App));
        // Again on a later launch: both files take the new URL.
        drop(lock);
        let mut lock = DaemonLock::acquire(&data).expect("lock again");
        record(&mut lock, &data, "http://127.0.0.1:5002").expect("record");
        assert_eq!(read(LOCK_FILE), "http://127.0.0.1:5002");
        assert_eq!(read(APP_URL_FILE), "http://127.0.0.1:5002");
    }

    /// `od daemon start` on the folder: no wait, it says who.
    #[test]
    fn a_cli_daemon_is_refused_at_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("data");
        // The app ran here before; the CLI holds the folder now.
        let _held = hold(&data, CLI);
        record_app_url(&data, APP).expect("marker");
        let started = Instant::now();
        let refused = acquire_waiting(&data, Duration::from_secs(10)).expect_err("held");
        assert_eq!(refused.holder, Some(Holder::Other));
        assert!(matches!(refused.error, LockError::Held { .. }));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// The app's copy never lets go: the launch gives up after the limit.
    #[test]
    fn gives_up_on_the_app_after_the_limit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("data");
        let _held = hold(&data, APP);
        record_app_url(&data, APP).expect("marker");
        let limit = Duration::from_millis(600);
        let started = Instant::now();
        let refused = acquire_waiting(&data, limit).expect_err("held");
        assert_eq!(refused.holder, Some(Holder::App));
        assert!(started.elapsed() >= limit);
        assert!(started.elapsed() < limit + Duration::from_secs(2));
    }

    #[test]
    fn the_saved_language_first_then_the_systems() {
        assert_eq!(lang(Some("vi"), Some("en-US")), Lang::Vi);
        assert_eq!(lang(Some("en"), Some("vi-VN")), Lang::En);
        // No setting: the UI follows the system, so does the dialog.
        assert_eq!(lang(None, Some("vi-VN")), Lang::Vi);
        assert_eq!(lang(Some(""), Some("vi")), Lang::Vi);
        // The other UI languages have no dialog words: English.
        assert_eq!(lang(Some("de"), Some("vi-VN")), Lang::En);
        assert_eq!(lang(None, None), Lang::En);
    }

    #[test]
    fn the_saved_language_is_read_from_app_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("app-config.json"), r#"{"locale":"vi"}"#).expect("config");
        assert_eq!(lang_of(tmp.path()), Lang::Vi);
    }

    /// Each dialog says what to do and leaks no URL or folder.
    #[test]
    fn the_dialog_names_an_action_and_no_internals() {
        for holder in [Holder::App, Holder::Other] {
            for lang in [Lang::En, Lang::Vi] {
                let text = held_text(holder, lang);
                for word in [text.title, text.message, text.button] {
                    assert!(!word.is_empty());
                    assert!(!word.contains("http"), "{word}");
                    assert!(!word.contains('/') && !word.contains('\\'), "{word}");
                    assert!(!word.contains("OD_DATA_DIR"), "{word}");
                }
                let message = text.message;
                let acts = match lang {
                    Lang::En => ["open Contour again"],
                    Lang::Vi => ["mở lại Contour"],
                };
                assert!(acts.iter().all(|a| message.contains(a)), "{message}");
            }
        }
        // An older Contour (no app URL file) is `Other` too: the words name
        // both, not only the CLI.
        for (lang, app) in [
            (Lang::En, "Quit that Contour"),
            (Lang::Vi, "Thoát Contour đó"),
        ] {
            let message = held_text(Holder::Other, lang).message;
            assert!(message.contains("od daemon start"), "{message}");
            assert!(message.contains(app), "{message}");
        }
        assert_ne!(
            held_text(Holder::App, Lang::Vi),
            held_text(Holder::App, Lang::En)
        );
    }

    #[test]
    fn single_instance_only_on_the_apps_own_folder() {
        assert!(single_instance(None));
        assert!(single_instance(Some(std::ffi::OsStr::new(""))));
        assert!(!single_instance(Some(std::ffi::OsStr::new("/tmp/qa"))));
    }
}
