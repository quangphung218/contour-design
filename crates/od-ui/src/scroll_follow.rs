//! Stick-to-bottom scrolling for a chat transcript, the way chat apps do
//! it: new output keeps the list at its end only while the reader is
//! there. Scrolling up to read earlier turns stops the follow; scrolling
//! back down (or a new message being sent) starts it again. While the
//! follow is off, new output raises "Jump to latest".
//!
//! Pure state, so the decision is tested natively; the browser wiring is
//! `app::follow`.

/// How close to the end (px) still counts as "at the bottom".
pub const NEAR_BOTTOM_PX: f64 = 48.0;

/// The scroll box's measurements, as the DOM gives them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub scroll_top: f64,
    pub client_height: f64,
    pub scroll_height: f64,
}

impl Metrics {
    /// Whether the end of the content is at most [`NEAR_BOTTOM_PX`] below
    /// the visible part.
    #[must_use]
    pub fn near_bottom(self) -> bool {
        self.scroll_height - (self.scroll_top + self.client_height) <= NEAR_BOTTOM_PX
    }
}

/// Whether the list follows new output, and whether output arrived while
/// it did not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Follow {
    pinned: bool,
    unseen: bool,
    last_top: f64,
}

impl Default for Follow {
    fn default() -> Self {
        Self {
            pinned: true,
            unseen: false,
            last_top: 0.0,
        }
    }
}

impl Follow {
    /// A scroll event. Moving up hands the decision to where the reader
    /// stopped; moving down (or the browser clamping after our own scroll
    /// to the end, while more output already grew the list) never turns the
    /// follow off, only on once the end is near.
    pub fn scrolled(&mut self, m: Metrics) {
        let up = m.scroll_top < self.last_top - 0.5;
        self.last_top = m.scroll_top;
        self.pinned = if up {
            m.near_bottom()
        } else {
            self.pinned || m.near_bottom()
        };
        if self.pinned {
            self.unseen = false;
        }
    }

    /// The transcript changed; `new_item` when a message joined it (one was
    /// sent, or a conversation opened), which always goes to the end.
    /// Returns whether to scroll to the end now.
    pub fn changed(&mut self, new_item: bool) -> bool {
        if new_item {
            self.pinned = true;
        }
        self.unseen = !self.pinned;
        self.pinned
    }

    /// "Jump to latest" was pressed.
    pub fn jump(&mut self) {
        self.pinned = true;
        self.unseen = false;
    }

    #[must_use]
    pub fn pinned(&self) -> bool {
        self.pinned
    }

    /// Show "Jump to latest": away from the end with output not yet seen.
    #[must_use]
    pub fn show_jump(&self) -> bool {
        !self.pinned && self.unseen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(top: f64) -> Metrics {
        Metrics {
            scroll_top: top,
            client_height: 400.0,
            scroll_height: 2000.0,
        }
    }

    #[test]
    fn near_bottom_within_48px() {
        assert!(at(1600.0).near_bottom());
        assert!(at(1552.0).near_bottom());
        assert!(!at(1551.0).near_bottom());
        assert!(!at(0.0).near_bottom());
    }

    #[test]
    fn follows_output_at_the_bottom() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        assert!(f.changed(false));
        assert!(!f.show_jump());
    }

    #[test]
    fn scrolling_up_stops_the_follow_while_streaming() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        f.scrolled(at(900.0));
        assert!(
            !f.changed(false),
            "new tokens must not pull the reader down"
        );
        assert!(!f.changed(false));
        assert!(f.show_jump());
    }

    #[test]
    fn a_small_nudge_up_near_the_end_keeps_following() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        f.scrolled(at(1580.0));
        assert!(f.changed(false));
    }

    #[test]
    fn scrolling_back_to_the_end_resumes() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        f.scrolled(at(500.0));
        f.changed(false);
        f.scrolled(at(1000.0));
        assert!(!f.pinned(), "half way down is still reading");
        f.scrolled(at(1590.0));
        assert!(f.pinned());
        assert!(!f.show_jump());
        assert!(f.changed(false));
    }

    #[test]
    fn our_own_scroll_lagging_behind_growth_keeps_following() {
        // We scrolled to 1600, but 200 px more output landed before the
        // scroll event: the list is no longer near its end, yet the reader
        // did not move up.
        let mut f = Follow::default();
        f.scrolled(at(1400.0));
        f.scrolled(Metrics {
            scroll_top: 1600.0,
            client_height: 400.0,
            scroll_height: 2200.0,
        });
        assert!(f.pinned());
    }

    #[test]
    fn sending_goes_to_the_end() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        f.scrolled(at(200.0));
        assert!(f.changed(true));
        assert!(!f.show_jump());
    }

    #[test]
    fn jump_to_latest_resumes_and_hides_itself() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        f.scrolled(at(200.0));
        f.changed(false);
        assert!(f.show_jump());
        f.jump();
        assert!(!f.show_jump());
        assert!(f.changed(false));
    }

    #[test]
    fn no_button_without_new_output() {
        let mut f = Follow::default();
        f.scrolled(at(1600.0));
        f.scrolled(at(200.0));
        assert!(!f.show_jump());
    }
}
