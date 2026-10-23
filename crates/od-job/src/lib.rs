//! Ending a whole process tree (spec 5.4).
//!
//! - Windows: Job Objects (`track`, `alive`, `kill`), the counterpart of a
//!   POSIX process group.
//! - POSIX: [`descendant_groups`], the process groups of everything a
//!   process started, including jobs that left its group or session with
//!   `setsid` (and so escape a `killpg` of the leader's group).

#[cfg(unix)]
mod tree;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use tree::descendant_groups;
#[cfg(windows)]
pub use windows::{alive, kill, track};
