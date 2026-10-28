//! `<app> mcp [--daemon-url URL]`: the MCP stdio server `od mcp` runs, for
//! the command Integrations ▸ Install registers in an agent. No window and
//! no daemon of its own: it talks to the app's daemon over HTTP.
//!
//! The app's daemon listens on a fresh port per launch, so the URL recorded
//! in `<data>/daemon.lock` (the app running now) goes before `--daemon-url`
//! (the port at install time); then `OD_DAEMON_URL`, then the CLI default.
//! On Windows the app is a GUI-subsystem binary with no console, but the
//! agent starts it with pipes for stdin and stdout, which it inherits.

use anyhow::{Context, bail};
use std::ffi::OsString;

/// Runs the server until the agent closes stdin (or it idles out).
///
/// # Errors
/// An unknown argument, or the server failing.
pub fn run(args: &[OsString]) -> anyhow::Result<()> {
    let flag = parse_args(args)?;
    let data = od_store::DataDir::from_env();
    let lock = std::fs::read_to_string(data.root().join(od_server::lock::LOCK_FILE)).ok();
    let url = daemon_url(
        lock.as_deref(),
        flag.as_deref(),
        std::env::var("OD_DAEMON_URL").ok().as_deref(),
    );
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the async runtime")?
        .block_on(od_mcp::serve_stdio(&url))
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

fn daemon_url(lock: Option<&str>, flag: Option<&str>, env: Option<&str>) -> String {
    [lock, flag, env]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|u| !u.is_empty())
        .unwrap_or(od_mcp::DEFAULT_DAEMON_URL)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_app_goes_before_the_port_at_install_time() {
        let now = Some("http://127.0.0.1:5001\n");
        let then = Some("http://127.0.0.1:4000");
        assert_eq!(daemon_url(now, then, None), "http://127.0.0.1:5001");
        assert_eq!(
            daemon_url(None, then, Some("http://e")),
            "http://127.0.0.1:4000"
        );
        assert_eq!(daemon_url(Some(""), None, Some("http://e")), "http://e");
        assert_eq!(daemon_url(None, None, None), od_mcp::DEFAULT_DAEMON_URL);
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
