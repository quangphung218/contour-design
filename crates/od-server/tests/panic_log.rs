//! The log file and the panic hook ([`od_server::logs`]), in a child
//! process: the global subscriber and the panic hook are per process.

#![allow(clippy::unwrap_used)] // tests fail loudly by design

use std::path::Path;
use std::process::Command;

/// Set in the child: the data root it logs under.
const CHILD_ENV: &str = "OD_PANIC_LOG_TEST_ROOT";
const TEST: &str = "a_panic_leaves_its_stack_in_the_log_file";

#[inline(never)]
fn panics_on_purpose() {
    panic!("broken on purpose");
}

#[test]
fn a_panic_leaves_its_stack_in_the_log_file() {
    if let Some(root) = std::env::var_os(CHILD_ENV) {
        od_server::logs::init(Some(Path::new(&root)));
        tracing::info!(api_key = "sk-child-secret", "about to fail");
        panics_on_purpose();
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([TEST, "--exact", "--test-threads=1"])
        .env(CHILD_ENV, tmp.path())
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert!(!out.status.success(), "the child panics");
    let log = std::fs::read_to_string(tmp.path().join("logs/daemon.log")).unwrap();
    assert!(log.contains("about to fail"), "{log}");
    assert!(
        log.contains("PANIC thread") && log.contains("broken on purpose"),
        "{log}"
    );
    assert!(log.contains("stack backtrace:"), "{log}");
    assert!(
        log.contains("panics_on_purpose"),
        "the stack names the frame: {log}"
    );
    // Report Issue and Export diagnostics read it back without the key.
    let tail = od_server::routes::manage::diagnostics::log_tail(tmp.path());
    assert!(tail.contains("about to fail") && tail.contains("broken on purpose"));
    assert!(!tail.contains("sk-child-secret"), "{tail}");
    assert!(tail.contains("api_key=[redacted]"), "{tail}");
}
