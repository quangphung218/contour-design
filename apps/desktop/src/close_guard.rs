//! Closing the main window (Windows, Linux) or quitting the app (macOS:
//! ⌘Q, Quit in the menu or the Dock) while the page holds work not saved
//! (a comment not sent, a Manual edit not applied, in any file): the page
//! asks first, with the leave guard's own dialog (od-ui `leave_guard`).
//!
//! The page tells the shell whenever it starts or stops holding such work
//! (`set_unsaved`), so a close with nothing to lose goes as before, with no
//! round trip. Otherwise the shell holds the close, sends the page
//! `od:menu` `close` or `quit`, and goes on once the page says `go`
//! (`close_answer`). A page that says nothing within [`CLOSE_REPLY_WAIT`]
//! (hung) does not keep the app open: the close goes on.
//!
//! [`CloseGuard::step`] calls neither Tauri nor AppKit: `main.rs` feeds it
//! and carries out its [`CloseAction`]s.

use std::time::Duration;

/// How long the page gets to answer a close before it goes on anyway.
pub(crate) const CLOSE_REPLY_WAIT: Duration = Duration::from_secs(3);

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    /// Close the main window, which quits the app (Windows, Linux).
    Close,
    /// Quit the app.
    Quit,
}

impl Way {
    /// The `od:menu` detail the page takes it as.
    pub(crate) fn as_menu(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Quit => "quit",
        }
    }
}

/// The page's answer to a close (`close_answer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reply {
    /// The user is being asked: no time limit any more.
    Asking,
    /// Close: nothing to lose, or the user chose Leave.
    Go,
    /// The user chose to stay.
    Stay,
}

impl Reply {
    /// The answer as the page sends it.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "asking" => Some(Self::Asking),
            "go" => Some(Self::Go),
            "stay" => Some(Self::Stay),
            _ => None,
        }
    }
}

/// What happened, as the shell's callbacks, commands and timers say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseEvent {
    /// The page shown is gone (another page started, its process ended):
    /// it holds nothing until the next page says so.
    PageGone,
    /// The page holds work not saved, or no longer (`set_unsaved`).
    Unsaved(bool),
    /// The user asked to close or quit.
    Asked(Way),
    /// The page answered (`close_answer`).
    Reply(Reply),
    /// [`CloseAction::CheckLater`]'s time came for close `token`.
    Check(u64),
}

/// What the shell does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAction {
    /// Let the close or quit go as it would (nothing held).
    Let,
    /// Send the page `od:menu` with the way's detail.
    AskPage(Way),
    /// Feed [`CloseEvent::Check`] after `after`.
    CheckLater { token: u64, after: Duration },
    /// Close or quit now, for real.
    Go(Way),
}

/// A close the shell holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    way: Way,
    token: u64,
    /// The page said it asks the user: no time limit.
    asking: bool,
}

/// See the module.
#[derive(Debug, Default)]
pub(crate) struct CloseGuard {
    /// The page shown said it holds work not saved.
    unsaved: bool,
    held: Option<Held>,
    last_token: u64,
}

impl CloseGuard {
    /// Takes `event`: what to do.
    pub(crate) fn step(&mut self, event: CloseEvent) -> Vec<CloseAction> {
        match event {
            CloseEvent::PageGone => {
                // A close held for it is dropped, never carried out: the
                // user closes again, and with nothing held it goes.
                self.unsaved = false;
                self.held = None;
                Vec::new()
            }
            CloseEvent::Unsaved(unsaved) => {
                self.unsaved = unsaved;
                Vec::new()
            }
            CloseEvent::Asked(way) => {
                if !self.unsaved {
                    self.held = None;
                    return vec![CloseAction::Let];
                }
                self.last_token = self.last_token.wrapping_add(1);
                let token = self.last_token;
                self.held = Some(Held {
                    way,
                    token,
                    asking: false,
                });
                vec![
                    CloseAction::AskPage(way),
                    CloseAction::CheckLater {
                        token,
                        after: CLOSE_REPLY_WAIT,
                    },
                ]
            }
            CloseEvent::Reply(Reply::Asking) => {
                if let Some(held) = &mut self.held {
                    held.asking = true;
                }
                Vec::new()
            }
            // Only a close the shell holds: a `go` out of the blue never
            // quits the app.
            CloseEvent::Reply(Reply::Go) => self
                .held
                .take()
                .map(|held| vec![CloseAction::Go(held.way)])
                .unwrap_or_default(),
            CloseEvent::Reply(Reply::Stay) => {
                self.held = None;
                Vec::new()
            }
            CloseEvent::Check(token) => match self.held {
                Some(held) if held.token == token && !held.asking => {
                    self.held = None;
                    tracing::warn!("the page did not answer the close: closing");
                    vec![CloseAction::Go(held.way)]
                }
                _ => Vec::new(),
            },
        }
    }

    /// Takes the user's ask to close or quit: whether the shell holds it
    /// (the caller then prevents it), and the actions to carry out after.
    pub(crate) fn ask(&mut self, way: Way) -> (bool, Vec<CloseAction>) {
        let actions = self.step(CloseEvent::Asked(way));
        let held = actions != [CloseAction::Let];
        (held, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CloseAction::*;
    use CloseEvent as E;

    fn with_unsaved() -> CloseGuard {
        let mut guard = CloseGuard::default();
        guard.step(E::Unsaved(true));
        guard
    }

    fn asked(way: Way) -> CloseGuard {
        let mut guard = with_unsaved();
        let (held, _) = guard.ask(way);
        assert!(held);
        guard
    }

    #[test]
    fn nothing_unsaved_closes_as_before() {
        let mut guard = CloseGuard::default();
        assert_eq!(guard.ask(Way::Close), (false, vec![Let]));
        assert_eq!(guard.ask(Way::Quit), (false, vec![Let]));
        // Work the page dropped again no longer holds a close.
        let mut guard = with_unsaved();
        guard.step(E::Unsaved(false));
        assert_eq!(guard.ask(Way::Close), (false, vec![Let]));
    }

    #[test]
    fn unsaved_work_holds_the_close_and_asks_the_page() {
        let mut guard = with_unsaved();
        assert_eq!(
            guard.ask(Way::Quit),
            (
                true,
                vec![
                    AskPage(Way::Quit),
                    CheckLater {
                        token: 1,
                        after: CLOSE_REPLY_WAIT,
                    },
                ]
            )
        );
    }

    #[test]
    fn the_pages_go_closes_for_real() {
        let mut guard = asked(Way::Close);
        assert_eq!(guard.step(E::Reply(Reply::Go)), vec![Go(Way::Close)]);
        // Once: its timer finds nothing held.
        assert_eq!(guard.step(E::Check(1)), vec![]);
        // The user asked, chose Leave.
        let mut guard = asked(Way::Quit);
        guard.step(E::Reply(Reply::Asking));
        assert_eq!(guard.step(E::Reply(Reply::Go)), vec![Go(Way::Quit)]);
    }

    #[test]
    fn staying_keeps_the_app_open() {
        let mut guard = asked(Way::Quit);
        guard.step(E::Reply(Reply::Asking));
        assert_eq!(guard.step(E::Reply(Reply::Stay)), vec![]);
        assert_eq!(guard.step(E::Check(1)), vec![]);
        // A `go` with nothing held does not quit.
        assert_eq!(guard.step(E::Reply(Reply::Go)), vec![]);
        // The work is still there: the next close asks again.
        assert!(guard.ask(Way::Quit).0);
    }

    #[test]
    fn a_page_that_does_not_answer_does_not_keep_the_app_open() {
        let mut guard = asked(Way::Close);
        assert_eq!(guard.step(E::Check(1)), vec![Go(Way::Close)]);
    }

    #[test]
    fn a_page_asking_the_user_has_no_time_limit() {
        let mut guard = asked(Way::Quit);
        guard.step(E::Reply(Reply::Asking));
        assert_eq!(guard.step(E::Check(1)), vec![]);
        // Still held: the user's Leave closes.
        assert_eq!(guard.step(E::Reply(Reply::Go)), vec![Go(Way::Quit)]);
    }

    #[test]
    fn a_newer_ask_takes_over_the_timer() {
        let mut guard = asked(Way::Quit);
        guard.step(E::Reply(Reply::Asking));
        // ⌘Q again, the page now hung: the new ask's timer quits.
        let (held, actions) = guard.ask(Way::Quit);
        assert!(held);
        assert!(actions.contains(&CheckLater {
            token: 2,
            after: CLOSE_REPLY_WAIT,
        }));
        assert_eq!(guard.step(E::Check(1)), vec![]);
        assert_eq!(guard.step(E::Check(2)), vec![Go(Way::Quit)]);
    }

    #[test]
    fn a_page_gone_holds_nothing() {
        let mut guard = asked(Way::Close);
        guard.step(E::PageGone);
        // Its close is dropped, never carried out late.
        assert_eq!(guard.step(E::Check(1)), vec![]);
        assert_eq!(guard.step(E::Reply(Reply::Go)), vec![]);
        // The next page has said nothing: a close goes.
        assert_eq!(guard.ask(Way::Close), (false, vec![Let]));
    }

    #[test]
    fn the_page_takes_each_way_by_its_word() {
        assert_eq!(Way::Close.as_menu(), "close");
        assert_eq!(Way::Quit.as_menu(), "quit");
        assert_eq!(Reply::parse("asking"), Some(Reply::Asking));
        assert_eq!(Reply::parse("go"), Some(Reply::Go));
        assert_eq!(Reply::parse("stay"), Some(Reply::Stay));
        assert_eq!(Reply::parse("quit"), None);
    }
}
