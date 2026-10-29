//! `<app> mcp [--daemon-url URL]`: the MCP stdio server `od mcp` runs, for
//! the command Integrations ▸ Install registers in an agent. No window and
//! no daemon of its own: it talks to the app's daemon over HTTP.
//!
//! The URL is picked as `od mcp` picks it ([`od_mcp::locate`]): the flag,
//! the env, the running app's `daemon.lock` (so Install registers no
//! `--daemon-url`), the CLI default; a flag or env URL with no daemon
//! behind it gives way to `daemon.lock`.
//! On Windows the app is a GUI-subsystem binary with no console, but the
//! agent starts it with pipes for stdin and stdout, which it inherits.

use anyhow::{Context, bail};
use od_mcp::locate::{daemon_urls, reachable};
use std::ffi::OsString;

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

#[cfg(test)]
mod tests {
    use super::*;

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
