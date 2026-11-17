//! Shared by the real-browser tests.

use od_render_cdp::{CdpRenderer, chrome_sandbox_works, find_chrome};
use std::path::Path;
use std::sync::OnceLock;

/// A renderer for `exe` that works where the tests run: inside another
/// sandbox (the maintainer's pre-merge check runs `make check` under `sandbox-exec`) Chrome
/// cannot start its own, so the tests opt out of it there, as
/// `OD_CHROME_NO_SANDBOX=1` would. The product never decides this itself.
#[allow(dead_code)] // not every test binary uses it
pub fn renderer(exe: &Path) -> CdpRenderer {
    static SANDBOX_OK: OnceLock<bool> = OnceLock::new();
    let ok = *SANDBOX_OK.get_or_init(|| find_chrome().is_ok_and(|c| chrome_sandbox_works(&c)));
    let renderer = CdpRenderer::with_executable(exe);
    if ok {
        renderer
    } else {
        renderer.without_chrome_sandbox()
    }
}

/// Removes Gatekeeper's quarantine from `path` and everything inside it,
/// and checks none is left. A file this test process writes can carry it
/// (when what runs the tests quarantines what it creates), and LaunchServices
/// would then show "… Not Opened" on the screen of whoever runs the tests
/// for an unsigned bundle. A test must never show a dialog.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // not every test binary uses it
pub fn unquarantine(path: &Path) {
    const QUARANTINE: &str = "com.apple.quarantine";
    let _ = xattr::remove(path, QUARANTINE);
    let left = xattr::get(path, QUARANTINE).ok().flatten();
    assert!(left.is_none(), "{} is still quarantined", path.display());
    let is_dir = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    if is_dir {
        for entry in std::fs::read_dir(path).unwrap() {
            unquarantine(&entry.unwrap().path());
        }
    }
}
