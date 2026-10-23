//! SIGTERM (launchd, systemd, `kill`) stops the daemon as Ctrl-C does:
//! terminal tabs and their jobs end with it. Alone in its binary: it
//! signals itself.

#![cfg(unix)]

mod shutdown_common;

#[tokio::test]
async fn sigterm_kills_terminal_jobs_that_ignore_sighup() {
    shutdown_common::signal_kills_terminal_jobs(nix::sys::signal::Signal::SIGTERM).await;
}
