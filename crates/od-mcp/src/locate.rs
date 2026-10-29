//! Which daemon an `mcp` stdio server talks to, for `od mcp` and
//! `<app> mcp` alike.
//!
//! The URL is `--daemon-url`, else `OD_DAEMON_URL`, else the one recorded
//! in `<data>/daemon.lock` (the app running now, whose port changes per
//! launch), else [`DEFAULT_DAEMON_URL`]. Beta ≤4 registered `--daemon-url`
//! with the port of the launch it was installed from, a port that is dead
//! after the next launch. So a flag or env URL with no daemon behind it
//! (health check, [`HEALTH_WITHIN`]) gives way to `daemon.lock`, and that
//! old registration keeps working.

use crate::{DEFAULT_DAEMON_URL, Daemon};
use std::time::Duration;

/// How long the health check of a `--daemon-url` / `OD_DAEMON_URL` waits.
pub const HEALTH_WITHIN: Duration = Duration::from_secs(1);

/// The URL to use and, when it is the flag's or the env's, the one in
/// `daemon.lock` to fall back on.
#[must_use]
pub fn daemon_urls(
    flag: Option<&str>,
    env: Option<&str>,
    lock: Option<&str>,
) -> (String, Option<String>) {
    fn set(u: Option<&str>) -> Option<&str> {
        u.map(str::trim).filter(|u| !u.is_empty())
    }
    let lock = set(lock);
    match set(flag).or_else(|| set(env)) {
        Some(url) => (
            url.to_owned(),
            lock.filter(|l| *l != url).map(str::to_owned),
        ),
        None => (lock.unwrap_or(DEFAULT_DAEMON_URL).to_owned(), None),
    }
}

/// `url`, or `fallback` (the running app's) when `url` has no daemon; the
/// switch is said on stderr (stdout carries the protocol).
///
/// # Errors
/// An unparsable `url`.
pub async fn reachable(url: String, fallback: Option<String>) -> anyhow::Result<String> {
    let Some(lock) = fallback else {
        return Ok(url);
    };
    if Daemon::new(&url)?.is_up(HEALTH_WITHIN).await {
        return Ok(url);
    }
    eprintln!("mcp: no daemon at {url}; using the running app's {lock} from daemon.lock");
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_then_the_env_then_the_running_app() {
        let lock = Some("http://127.0.0.1:5001\n");
        let flag = Some("http://127.0.0.1:4000");
        let env = Some("http://e");
        let url = |flag, env, lock| daemon_urls(flag, env, lock).0;
        assert_eq!(url(flag, env, lock), "http://127.0.0.1:4000");
        assert_eq!(url(None, env, lock), "http://e");
        assert_eq!(url(None, None, lock), "http://127.0.0.1:5001");
        assert_eq!(url(Some(""), None, lock), "http://127.0.0.1:5001");
        assert_eq!(url(None, None, None), DEFAULT_DAEMON_URL);
    }

    #[test]
    fn the_flag_and_the_env_fall_back_on_the_running_app() {
        let lock = Some("http://127.0.0.1:5001\n");
        let fallback = |flag, env, lock| daemon_urls(flag, env, lock).1;
        let running = Some("http://127.0.0.1:5001".to_owned());
        assert_eq!(fallback(Some("http://127.0.0.1:4000"), None, lock), running);
        assert_eq!(fallback(None, Some("http://e"), lock), running);
        assert_eq!(fallback(Some("http://127.0.0.1:5001"), None, lock), None);
        assert_eq!(fallback(None, None, lock), None);
        assert_eq!(fallback(Some("http://a"), None, None), None);
    }
}
