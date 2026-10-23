//! POSIX process trees from one `ps` listing of parent links.

use std::collections::BTreeSet;
use std::process::Command;

/// Process groups of every descendant of `root` (not `root` itself), as
/// `ps` lists them now. Empty when `ps` cannot run.
#[must_use]
pub fn descendant_groups(root: u32) -> Vec<i32> {
    let Ok(out) = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pgid="])
        .output()
    else {
        return Vec::new();
    };
    groups_below(root, &String::from_utf8_lossy(&out.stdout))
}

/// [`descendant_groups`] over a `pid ppid pgid` listing.
fn groups_below(root: u32, listing: &str) -> Vec<i32> {
    let procs: Vec<(u32, u32, i32)> = listing
        .lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            Some((
                f.next()?.parse().ok()?,
                f.next()?.parse().ok()?,
                f.next()?.parse().ok()?,
            ))
        })
        .collect();
    let mut tree = BTreeSet::from([root]);
    let mut groups = Vec::new();
    // Parents may be listed after their children: repeat until stable.
    loop {
        let before = tree.len();
        for &(pid, ppid, pgid) in &procs {
            if tree.contains(&ppid) && tree.insert(pid) {
                groups.push(pgid);
            }
        }
        if tree.len() == before {
            return groups;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_children_listed_in_any_order() {
        let listing = "  1     0     1\n 30    20    30\n 20    10    10\n 10     1    10\n 40     1    40\nbad\n";
        let mut groups = groups_below(10, listing);
        groups.sort_unstable();
        assert_eq!(groups, [10, 30]);
        assert!(groups_below(40, listing).is_empty());
    }

    #[test]
    fn finds_a_live_child() {
        let mut child = Command::new("sleep").arg("5").spawn().expect("spawn sleep");
        let groups = descendant_groups(std::process::id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(!groups.is_empty());
    }
}
