//! Stopping the daemon (Ctrl-C) ends what its terminal tabs started, even
//! a job that ignores SIGHUP. Alone in its binary: it signals itself.

#![cfg(unix)]

mod shutdown_common;

#[tokio::test]
async fn ctrl_c_kills_terminal_jobs_that_ignore_sighup() {
    shutdown_common::signal_kills_terminal_jobs(nix::sys::signal::Signal::SIGINT).await;
}
