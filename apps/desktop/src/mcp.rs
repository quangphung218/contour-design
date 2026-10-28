//! `<app> mcp [--daemon-url URL]`: the MCP stdio server `od mcp` runs, for
//! the command Integrations ▸ Install registers in an agent. No window and
//! no daemon of its own: it talks to the app's daemon over HTTP.
//!
//! The URL is `--daemon-url`, else `OD_DAEMON_URL`, as `od` takes them, else
//! the one recorded in `<data>/daemon.lock`: the app running now, whose port
//! changes per launch (so Install registers no `--daemon-url`), else the CLI
//! default.
//! Beta ≤4 registered `--daemon-url` with the port of the launch it was
//! installed from, a port that is dead after the next launch. So a
//! `--daemon-url` or `OD_DAEMON_URL` with no daemon behind it (health check,
//! 1 s) gives way to `daemon.lock`, and that old registration keeps working.
//! On Windows the app is a GUI-subsystem binary with no console, but the
//! agent starts it with pipes for stdin and stdout, which it inherits.

use anyhow::{Context, bail};
use std::ffi::OsString;
use std::time::Duration;

/// How long the health check of a `--daemon-url` / `OD_DAEMON_URL` waits.
const HEALTH_WITHIN: Duration = Duration::from_secs(1);

/// Runs the server until the agent closes stdin (or it idles out).
///
/// # Errors
/// An unknown argument, or the server failing.
pub fn run(args: &[OsString]) -> anyhow::Result<()> {
    let flag = parse_args(args)?;
    let data = od_store::DataDir::from_env();
    let lock = std::fs::read_to_string(data.root().join(od_server::lock::LOCK_FILE)).ok();
    let (url, fallback) = daemon_urls(
        flag.as_deref(),
        std::env::var("OD_DAEMON_URL").ok().as_deref(),
        lock.as_deref(),
    );
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the async runtime")?
        .block_on(async {
            let url = reachable(url, fallback).await?;
            od_mcp::serve_stdio(&url).await
        })
}

/// `url`, or `fallback` (the running app's) when `url` has no daemon.
async fn reachable(url: String, fallback: Option<String>) -> anyhow::Result<String> {
    let Some(lock) = fallback else {
        return Ok(url);
    };
    if od_mcp::Daemon::new(&url)?.is_up(HEALTH_WITHIN).await {
        return Ok(url);
    }
    eprintln!("mcp: no daemon at {url}; using the running app's {lock} from daemon.lock");
    Ok(lock)
}

/// `--daemon-url URL` or `--daemon-url=URL`, as `od` takes it.
fn parse_args(args: &[OsString]) -> anyhow::Result<Option<String>> {
    let mut url = None;
    let mut it = args.iter().map(|a| a.to_string_lossy().into_owned());
    while let Some(arg) = it.next() {
        if arg == "--daemon-url" {
            url = Some(it.next().context("--daemon-url needs a URL")?);
        } else if let Some(v) = arg.strip_prefix("--daemon-url=") {
            url = Some(v.to_owned());
        } else {
            bail!("mcp: unexpected argument {arg} (usage: mcp [--daemon-url URL])");
        }
    }
    Ok(url)
}

/// The URL to use and, when it is the flag's or the env's, the one in
/// `daemon.lock` to fall back on.
fn daemon_urls(
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
        None => (lock.unwrap_or(od_mcp::DEFAULT_DAEMON_URL).to_owned(), None),
    }
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
        assert_eq!(url(None, None, None), od_mcp::DEFAULT_DAEMON_URL);
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

    #[test]
    fn takes_the_daemon_url_like_od() {
        let args = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(parse_args(&args(&[])).unwrap(), None);
        assert_eq!(
            parse_args(&args(&["--daemon-url", "http://a"]))
                .unwrap()
                .as_deref(),
            Some("http://a")
        );
        assert_eq!(
            parse_args(&args(&["--daemon-url=http://b"]))
                .unwrap()
                .as_deref(),
            Some("http://b")
        );
        assert!(parse_args(&args(&["--daemon-url"])).is_err());
        assert!(parse_args(&args(&["live-artifacts"])).is_err());
    }
}
