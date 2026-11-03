//! A test daemon started with `--port 0` binds a port of its own and then
//! writes its URL to `<data>/daemon.lock`. Reading the URL from there means
//! the test talks to its own daemon: picking a free port first and passing
//! it in leaves a gap in which another test's daemon can take that port.

use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

/// The URL the daemon recorded in `<data>/daemon.lock`, once it is there;
/// `None` when the daemon exits first or nothing appears within 20 s.
pub fn wait(data: &Path, child: &mut Child) -> Option<String> {
    let lock = data.join("daemon.lock");
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(url) = std::fs::read_to_string(&lock) {
            let url = url.trim();
            if url.starts_with("http://") {
                return Some(url.to_owned());
            }
        }
        if child.try_wait().unwrap().is_some() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}
