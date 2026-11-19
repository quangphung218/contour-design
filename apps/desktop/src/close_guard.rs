//! Closing the main window (Windows, Linux) or quitting the app (macOS:
//! ⌘Q, Quit in the menu or the Dock) while the page holds work not saved
//! (a comment not sent, a Manual edit not applied, in any file) or agents
//! are running (in any project, from the chat, `od` or MCP alike): the
//! page asks first, in one dialog, the leave guard's own (od-ui
//! `leave_guard`). Quitting stops the agents (QA-114).
//!
//! The page tells the shell whenever it starts or stops holding such work
//! (`set_unsaved`); the shell counts the running agents itself (the daemon
//! runs in its process). So a close with nothing to lose goes as before,
//! with no round trip. Otherwise the shell holds the close, sends the page
//! `od:menu` `close` or `quit` (`close:N` / `quit:N` with N agents
//! running), and goes on once the page says `go` (`close_answer`). A page
//! that says nothing within [`CLOSE_REPLY_WAIT`] (hung) does not keep the
//! app open: the close goes on. With no page to ask (gone, not loaded
//! again yet) running agents do not hold the close.
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

    /// The `od:menu` detail of the ask, with `running` agents: `quit`, or
    /// `quit:2` while two run.
    pub(crate) fn menu_detail(self, running: usize) -> String {
        match running {
            0 => self.as_menu().to_owned(),
            n => format!("{}:{n}", self.as_menu()),
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
    /// The user asked to close or quit, with `running` agents running.
    Asked { way: Way, running: usize },
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
    /// Send the page `od:menu` with [`Way::menu_detail`].
    AskPage { way: Way, running: usize },
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
    /// The page shown has said anything (`set_unsaved`, sent once it
    /// loads): there is a page to ask.
    heard: bool,
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
                self.heard = false;
                self.held = None;
                Vec::new()
            }
            CloseEvent::Unsaved(unsaved) => {
                self.unsaved = unsaved;
                self.heard = true;
                Vec::new()
            }
            CloseEvent::Asked { way, running } => {
                // Running agents hold it only when there is a page to ask.
                if !self.unsaved && (running == 0 || !self.heard) {
                    self.held = None;
                    return vec![CloseAction::Let];
                }
                // Asked again while the page asks (a second ⌘Q): the page
                // is asked again, keeps its one dialog and says `asking`
                // again; a page hung since then quits on the new timer.
                self.last_token = self.last_token.wrapping_add(1);
                let token = self.last_token;
                self.held = Some(Held {
                    way,
                    token,
                    asking: false,
                });
                vec![
                    CloseAction::AskPage { way, running },
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

    /// Takes the user's ask to close or quit, with `running` agents
    /// running: whether the shell holds it (the caller then prevents it),
    /// and the actions to carry out after.
    pub(crate) fn ask(&mut self, way: Way, running: usize) -> (bool, Vec<CloseAction>) {
        let actions = self.step(CloseEvent::Asked { way, running });
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
        let (held, _) = guard.ask(way, 0);
        assert!(held);
        guard
    }

    #[test]
    fn nothing_unsaved_closes_as_before() {
        let mut guard = CloseGuard::default();
        assert_eq!(guard.ask(Way::Close, 0), (false, vec![Let]));
        assert_eq!(guard.ask(Way::Quit, 0), (false, vec![Let]));
        // Work the page dropped again no longer holds a close.
        let mut guard = with_unsaved();
        guard.step(E::Unsaved(false));
        assert_eq!(guard.ask(Way::Close, 0), (false, vec![Let]));
    }

    #[test]
    fn unsaved_work_holds_the_close_and_asks_the_page() {
        let mut guard = with_unsaved();
        assert_eq!(
            guard.ask(Way::Quit, 0),
            (
                true,
                vec![
                    AskPage {
                        way: Way::Quit,
                        running: 0
                    },
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
        assert!(guard.ask(Way::Quit, 0).0);
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
        let (held, actions) = guard.ask(Way::Quit, 0);
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
        assert_eq!(guard.ask(Way::Close, 0), (false, vec![Let]));
    }

    /// A page loaded with nothing unsaved (it says so once it loads).
    fn page_loaded() -> CloseGuard {
        let mut guard = CloseGuard::default();
        guard.step(E::Unsaved(false));
        guard
    }

    #[test]
    fn running_agents_hold_the_close_and_ask_the_page_with_their_count() {
        // QA-114: ⌘Q mid-run quit at once and cut the agent off.
        for way in [Way::Quit, Way::Close] {
            let mut guard = page_loaded();
            assert_eq!(
                guard.ask(way, 2),
                (
                    true,
                    vec![
                        AskPage { way, running: 2 },
                        CheckLater {
                            token: 1,
                            after: CLOSE_REPLY_WAIT,
                        },
                    ]
                )
            );
            // Cancel keeps everything running; the next ask asks again.
            guard.step(E::Reply(Reply::Asking));
            assert_eq!(guard.step(E::Reply(Reply::Stay)), vec![]);
            assert!(guard.ask(way, 2).0);
            // Quit and stop agents.
            guard.step(E::Reply(Reply::Asking));
            assert_eq!(guard.step(E::Reply(Reply::Go)), vec![Go(way)]);
        }
        // Nothing running, nothing unsaved: as before.
        assert_eq!(page_loaded().ask(Way::Quit, 0), (false, vec![Let]));
    }

    #[test]
    fn one_dialog_covers_work_not_saved_and_running_agents() {
        let mut guard = with_unsaved();
        let (held, actions) = guard.ask(Way::Quit, 1);
        assert!(held);
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a, AskPage { .. }))
                .count(),
            1
        );
        assert!(actions.contains(&AskPage {
            way: Way::Quit,
            running: 1,
        }));
    }

    #[test]
    fn running_agents_with_no_page_to_ask_do_not_hold_the_close() {
        // Before the page loads, or once it is gone: nobody could answer.
        assert_eq!(CloseGuard::default().ask(Way::Quit, 1), (false, vec![Let]));
        let mut guard = page_loaded();
        guard.step(E::PageGone);
        assert_eq!(guard.ask(Way::Quit, 1), (false, vec![Let]));
        // The page loaded again: it is asked.
        guard.step(E::Unsaved(false));
        assert!(guard.ask(Way::Quit, 1).0);
    }

    #[test]
    fn a_second_quit_while_the_page_asks_asks_it_again_and_never_quits_by_itself() {
        let mut guard = page_loaded();
        guard.ask(Way::Quit, 1);
        guard.step(E::Reply(Reply::Asking));
        // ⌘Q again: no Go, the same page is asked (its one dialog stays).
        let (held, actions) = guard.ask(Way::Quit, 1);
        assert!(held);
        assert!(!actions.iter().any(|a| matches!(a, Go(_))), "{actions:?}");
        assert!(actions.contains(&AskPage {
            way: Way::Quit,
            running: 1,
        }));
        // The page says it still asks: neither timer quits.
        guard.step(E::Reply(Reply::Asking));
        assert_eq!(guard.step(E::Check(1)), vec![]);
        assert_eq!(guard.step(E::Check(2)), vec![]);
        assert_eq!(guard.step(E::Reply(Reply::Go)), vec![Go(Way::Quit)]);
    }

    #[test]
    fn the_page_takes_each_way_by_its_word() {
        assert_eq!(Way::Close.as_menu(), "close");
        assert_eq!(Way::Quit.as_menu(), "quit");
        assert_eq!(Way::Quit.menu_detail(0), "quit");
        assert_eq!(Way::Quit.menu_detail(2), "quit:2");
        assert_eq!(Way::Close.menu_detail(1), "close:1");
        assert_eq!(Reply::parse("asking"), Some(Reply::Asking));
        assert_eq!(Reply::parse("go"), Some(Reply::Go));
        assert_eq!(Reply::parse("stay"), Some(Reply::Stay));
        assert_eq!(Reply::parse("quit"), None);
    }
}
