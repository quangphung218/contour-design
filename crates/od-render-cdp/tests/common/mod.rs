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
