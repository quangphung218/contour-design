//! Workspace state that is pure data: open file tabs, recently opened files
//! and the queue of messages typed while a run is going (spec 1.1, 1.2).

/// Most recent files kept for the quick switcher.
const RECENT_MAX: usize = 20;

/// Open file tabs and the active one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tabs {
    pub open: Vec<String>,
    pub active: Option<String>,
}

impl Tabs {
    /// Activates `path`, opening a tab for it when needed.
    pub fn open(&mut self, path: &str) {
        if !self.open.iter().any(|p| p == path) {
            let at = self
                .active
                .as_ref()
                .and_then(|a| self.open.iter().position(|p| p == a))
                .map_or(self.open.len(), |i| i + 1);
            self.open.insert(at, path.to_owned());
        }
        self.active = Some(path.to_owned());
    }

    /// Closes `path`; the right neighbour (else the left one) becomes active.
    pub fn close(&mut self, path: &str) {
        let Some(i) = self.open.iter().position(|p| p == path) else {
            return;
        };
        self.open.remove(i);
        if self.active.as_deref() == Some(path) {
            self.active = self
                .open
                .get(i)
                .or_else(|| i.checked_sub(1).and_then(|j| self.open.get(j)))
                .cloned();
        }
    }

    /// Cmd/Ctrl+1–8 pick that tab; 9 picks the last one, like browsers.
    pub fn select_nth(&mut self, n: usize) {
        let pick = if n >= 9 {
            self.open.last()
        } else {
            n.checked_sub(1).and_then(|i| self.open.get(i))
        };
        if let Some(p) = pick.cloned() {
            self.active = Some(p);
        }
    }

    /// Ctrl+Tab (`forward`) and Ctrl+Shift+Tab, wrapping around.
    pub fn cycle(&mut self, forward: bool) {
        let n = self.open.len();
        if n == 0 {
            return;
        }
        let cur = self
            .active
            .as_ref()
            .and_then(|a| self.open.iter().position(|p| p == a))
            .unwrap_or(0);
        let next = if forward {
            (cur + 1) % n
        } else {
            (cur + n - 1) % n
        };
        self.active = Some(self.open[next].clone());
    }

    /// Drops tabs whose file no longer exists.
    pub fn retain_existing(&mut self, exists: impl Fn(&str) -> bool) {
        let active = self.active.clone();
        self.open.retain(|p| exists(p));
        if let Some(a) = active.filter(|a| !exists(a)) {
            self.open.retain(|p| *p != a);
            self.active = self.open.first().cloned();
        }
    }
}

/// Moves `path` to the front of the recent list.
pub fn touch_recent(recent: &mut Vec<String>, path: &str) {
    recent.retain(|p| p != path);
    recent.insert(0, path.to_owned());
    recent.truncate(RECENT_MAX);
}

/// Adds the `files` not already in `list`, keeping its order (a retry puts
/// back the attachments of the message it resends).
pub fn add_missing(list: &mut Vec<String>, files: Vec<String>) {
    for f in files {
        if !list.contains(&f) {
            list.push(f);
        }
    }
}

/// A message waiting for the running turn to end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub id: String,
    pub text: String,
}

/// Moves the item at `from` to `to` (both clamped), for drag and drop and
/// the up/down buttons.
pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from >= items.len() {
        return;
    }
    let item = items.remove(from);
    let to = to.min(items.len());
    items.insert(to, item);
}

/// Narrowest chat pane, px (the original's split layout).
pub const CHAT_MIN: f64 = 345.0;
/// Room the workspace keeps next to the chat, px.
const WORKSPACE_MIN: f64 = 400.0;

/// The chat pane width for a drag to `want` px in a split `total` px wide:
/// at least [`CHAT_MIN`], at most 720 or half the split (whichever is
/// larger), and never squeezing the workspace under 400 px.
#[must_use]
pub fn clamp_chat_width(want: f64, total: f64) -> f64 {
    let max = 720f64.max(total / 2.0).min(total - WORKSPACE_MIN);
    want.min(max).max(CHAT_MIN)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_retry_adds_back_only_the_missing_attachments() {
        let mut list = vec!["a.png".to_owned()];
        add_missing(&mut list, vec!["b.pdf".to_owned(), "a.png".to_owned()]);
        assert_eq!(list, ["a.png", "b.pdf"]);
    }

    use super::*;

    fn tabs(open: &[&str], active: &str) -> Tabs {
        Tabs {
            open: open.iter().map(|s| (*s).to_owned()).collect(),
            active: Some(active.to_owned()),
        }
    }

    #[test]
    fn open_inserts_after_active_and_activates() {
        let mut t = tabs(&["a", "b"], "a");
        t.open("c");
        assert_eq!(t, tabs(&["a", "c", "b"], "c"));
        t.open("b");
        assert_eq!(t, tabs(&["a", "c", "b"], "b"), "existing tab is reused");
    }

    #[test]
    fn close_picks_a_neighbour() {
        let mut t = tabs(&["a", "b", "c"], "b");
        t.close("b");
        assert_eq!(t, tabs(&["a", "c"], "c"));
        t.close("c");
        assert_eq!(t, tabs(&["a"], "a"));
        t.close("a");
        assert_eq!(t, Tabs::default());
        let mut t = tabs(&["a", "b"], "a");
        t.close("b");
        assert_eq!(
            t,
            tabs(&["a"], "a"),
            "closing another tab keeps the active one"
        );
    }

    #[test]
    fn nth_and_cycle() {
        let mut t = tabs(&["a", "b", "c"], "a");
        t.select_nth(2);
        assert_eq!(t.active.as_deref(), Some("b"));
        t.select_nth(9);
        assert_eq!(t.active.as_deref(), Some("c"));
        t.select_nth(7);
        assert_eq!(t.active.as_deref(), Some("c"), "missing tab: no change");
        t.cycle(true);
        assert_eq!(t.active.as_deref(), Some("a"));
        t.cycle(false);
        assert_eq!(t.active.as_deref(), Some("c"));
    }

    #[test]
    fn retain_existing_moves_off_deleted_active() {
        let mut t = tabs(&["a", "b", "c"], "b");
        t.retain_existing(|p| p != "b");
        assert_eq!(t, tabs(&["a", "c"], "a"));
    }

    #[test]
    fn recent_and_queue_moves() {
        let mut r = vec!["a".to_owned(), "b".to_owned()];
        touch_recent(&mut r, "b");
        assert_eq!(r, ["b", "a"]);
        let mut q = vec![1, 2, 3];
        move_item(&mut q, 0, 2);
        assert_eq!(q, [2, 3, 1]);
        move_item(&mut q, 2, 0);
        assert_eq!(q, [1, 2, 3]);
        move_item(&mut q, 5, 0);
        assert_eq!(q, [1, 2, 3]);
    }

    #[test]
    fn chat_width_stays_in_bounds() {
        assert_eq!(clamp_chat_width(100.0, 1400.0), CHAT_MIN);
        assert_eq!(clamp_chat_width(500.0, 1400.0), 500.0);
        assert_eq!(clamp_chat_width(900.0, 1400.0), 720.0);
        assert_eq!(clamp_chat_width(1500.0, 2400.0), 1200.0);
        assert_eq!(
            clamp_chat_width(900.0, 1000.0),
            600.0,
            "workspace keeps 400"
        );
        assert_eq!(
            clamp_chat_width(900.0, 600.0),
            CHAT_MIN,
            "never under the minimum"
        );
    }
}
