//! The main window's shell state in one place: the View ▸ Reload question
//! (QA-068, QA-072), the restarts that end the web content process
//! (QA-067, QA-070), the crash loop (QA-071) and the page last loaded
//! (QA-073).
//!
//! [`ShellState::step`] takes one [`ShellEvent`] and says what to do as
//! [`Action`]s. It calls neither Tauri nor WebKit: `main.rs` feeds it the
//! events its callbacks see and carries out the actions, off WebKit's
//! call. Each store used to be updated by hand where an event came in,
//! and a store left out (the process ending kept `listening`, R1) was a
//! bug; here every event meets the whole state.

// Off macOS only the Reload question runs (no restart, no crash hook):
// the rest is kept for macOS and the tests.
#![cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]

use std::time::{Duration, Instant};
use tauri::Url;

/// How long the page gets to answer View ▸ Reload (`reload_ack`) before
/// the shell asks the user whether to restart it.
pub(crate) const RELOAD_ACK_WAIT: Duration = Duration::from_secs(1);

/// After Wait, how long until the shell asks again about a page that
/// still has not answered.
pub(crate) const RELOAD_ASK_AGAIN: Duration = Duration::from_secs(5);

/// How long WebKit gets to say the web content process ended before the
/// shell loads a restart's page itself ([`ShellEvent::GraceOver`]).
pub(crate) const RESTART_GRACE: Duration = Duration::from_millis(1500);

/// After the shell loaded a restart's page itself, WebKit silent for
/// [`RESTART_GRACE`], how long WebKit's late word that the old web content
/// process ended is still taken for that restart rather than a crash.
pub(crate) const LATE_END_WINDOW: Duration = Duration::from_secs(5);

/// Web content crashes within this long of each other make a loop: a
/// design that crashes the process as it loads would reload for ever.
pub(crate) const CRASH_LOOP_WINDOW: Duration = Duration::from_secs(10);

/// The query the Studio reads on load: the shell stopped the design's
/// preview (od-ui `desktop::preview_was_stopped`).
pub(crate) const PREVIEW_STOPPED: (&str, &str) = ("preview", "stopped");

/// The query key that says why ([`StopReason`]; od-ui
/// `desktop::stop_reason`).
pub(crate) const STOP_REASON_KEY: &str = "why";

/// Why the shell stopped the design's preview, so the Studio says the
/// right thing (QA-071).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    /// The design hung the window in a dialog loop (`js_dialogs`).
    Dialogs,
    /// The page did not answer View ▸ Reload and the user chose Reload.
    Hung,
    /// The web content process crashed twice in a row.
    Crashed,
}

impl StopReason {
    /// Its value in the query.
    pub(crate) fn as_query(self) -> &'static str {
        match self {
            Self::Dialogs => "dialogs",
            Self::Hung => "hung",
            Self::Crashed => "crashed",
        }
    }
}

/// Where the app opens again after a design hung or crashed it (QA-067):
/// the same page with the preview stopped and why
/// (`?preview=stopped&why=…`). Present mode goes back to that design's
/// Studio, which says why and can run it again; it would only loop again.
pub(crate) fn stopped_preview_url(url: &Url, why: StopReason) -> Url {
    let mut url = url.clone();
    let presented = url
        .path()
        .strip_prefix("/present/")
        .and_then(|rest| rest.split('/').next())
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    if let Some(id) = presented {
        url.set_path(&format!("/projects/{id}"));
    }
    let (key, value) = PREVIEW_STOPPED;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != key && k != STOP_REASON_KEY)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair(key, value)
        .append_pair(STOP_REASON_KEY, why.as_query());
    url
}

/// The user's answer to "This page isn't responding"
/// (`web_process::ask_not_responding`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotRespondingChoice {
    /// Wait (the first button, Return).
    Wait,
    /// Reload (the second button).
    Reload,
    /// Nobody answered: the page answered the Reload question first, so
    /// the shell closed the alert itself (QA-072).
    Closed,
}

/// What the shell does about a View ▸ Reload the page has not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotAnswered {
    /// End the web content process; open the page with the design's
    /// preview stopped (the design is what hangs it).
    Restart,
    /// Ask again after [`RELOAD_ASK_AGAIN`].
    AskAgainLater,
    /// The page answered after all, or a newer Reload took over.
    Nothing,
}

/// The step after asking the user (`choice`; `None` when no question
/// could show, a dialog being up), the question still `open` or not. A
/// page that is only busy may answer while the user decides: then nothing
/// happens, whatever the choice (the alert closes itself then, QA-072,
/// and a Reload pressed just before is a no-op); Reload restarts only a
/// page still silent.
pub(crate) fn after_not_answering(open: bool, choice: Option<NotRespondingChoice>) -> NotAnswered {
    match (open, choice) {
        (false, _) => NotAnswered::Nothing,
        (true, Some(NotRespondingChoice::Reload)) => NotAnswered::Restart,
        (true, Some(NotRespondingChoice::Wait | NotRespondingChoice::Closed) | None) => {
            NotAnswered::AskAgainLater
        }
    }
}

/// What a crash of the web content process does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AfterCrash {
    /// Reload the page shown.
    Reload,
    /// The second crash in a row: open the page with the design's preview
    /// stopped ([`stopped_preview_url`]).
    StopPreview,
    /// Crashing even so: leave the window as it is (⌘R, ⌘Q still work).
    GiveUp,
}

/// What happened to the main window, as its callbacks, commands and
/// timers say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellEvent {
    /// A page started loading (`on_page_load` Started): its own reload, a
    /// navigation, a restart's page.
    PageStarted(Url),
    /// A page finished loading (`on_page_load` Finished).
    PageFinished(Url),
    /// The page's shell shows and takes `od:menu` (`set_menu_labels`).
    /// `origin`: where the call came from (its `Origin` header), when
    /// known; a call from another origin than the page started last is a
    /// page gone (R4).
    MenuListens { origin: Option<String> },
    /// View ▸ Reload (⌘R).
    ReloadAsked,
    /// The page took View ▸ Reload (`reload_ack`).
    ReloadAck,
    /// [`Action::CheckLater`]'s time came for question `token`.
    ReloadCheck(u64),
    /// The user answered "This page isn't responding" about `token`
    /// (`None`: no question could show).
    UserChose {
        token: u64,
        choice: Option<NotRespondingChoice>,
    },
    /// `js_dialogs`'s gate saw the page hang the window in a dialog loop.
    DialogHung,
    /// A restart ([`Action::Restart`]) of the page `shown`, the address
    /// the window showed then.
    Restart { shown: Url, why: StopReason },
    /// [`RESTART_GRACE`] after [`Action::Kill`] is over; `killed`: the
    /// process was asked to end, so WebKit may still say so, late.
    GraceOver { killed: bool },
    /// WebKit says the web content process ended.
    ProcessEnded,
    /// QA's crash-loop test hook asks the web content process to end, as
    /// a design that crashes it would (`main.rs` `qa_crash_web`).
    QaCrash,
}

/// What the shell does about an event. Only `main.rs` carries them out,
/// off WebKit's call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// Send the page `od:menu` `reload`: question `token` is open.
    AskPage(u64),
    /// Feed [`ShellEvent::ReloadCheck`] after `after` (macOS only asks).
    CheckLater { token: u64, after: Duration },
    /// The page shown takes no `od:menu`: load it again at once
    /// (`reload_unheard`).
    ReloadUnheard,
    /// Ask the user "This page isn't responding" about `token`, then feed
    /// [`ShellEvent::UserChose`].
    AskUser(u64),
    /// Read the address shown and feed [`ShellEvent::Restart`].
    Restart(StopReason),
    /// End the web content process, then feed [`ShellEvent::GraceOver`]
    /// after [`RESTART_GRACE`] (at once when it could not be ended).
    Kill,
    /// Load `url`.
    Navigate(Url),
    /// Reload the page shown.
    Reload,
    /// End the web content process, nothing more: what comes next is a
    /// crash's ([`ShellEvent::QaCrash`]).
    EndProcess,
}

/// `url`'s origin, written the way an `Origin` header is. Not
/// [`Url::origin`]: a `tauri://` page's origin is opaque, never equal to
/// itself.
fn origin_of(url: &Url) -> String {
    let mut origin = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        origin.push_str(&format!(":{port}"));
    }
    origin
}

/// The main window's shell state. See the module.
#[derive(Debug, Default)]
pub(crate) struct ShellState {
    /// The page shown, counted: one more at each page start and restart.
    page: u64,
    /// The origin of the page started last.
    page_origin: Option<String>,
    /// The page that takes `od:menu` (its shell said so); the splash, and
    /// the app before its shell shows, do not. Only the page shown counts.
    listening: Option<u64>,
    /// The last Reload question's token.
    last_token: u64,
    /// The Reload question the page has not answered yet.
    open: Option<u64>,
    /// The last page the window started or finished loading: once its web
    /// content process ended, WebKit has no address to give (QA-073).
    loaded: Option<Url>,
    /// The page a restart loads once the process is gone.
    next: Option<Url>,
    /// When the grace timer loaded the page of a restart whose process end
    /// WebKit had not reported yet, and that page.
    loaded_early: Option<(Instant, Url)>,
    /// That page's own start came ([`Self::loaded_early`]): the grace
    /// timer's load, not a new page.
    early_started: bool,
    /// Another page started since [`Self::loaded_early`] (or that page a
    /// second time): a process end now is that page's own (R2).
    started_since_early: bool,
    /// The last crash and how many came in a row, [`CRASH_LOOP_WINDOW`]
    /// apart at most.
    last_crash: Option<Instant>,
    crashes_in_a_row: u32,
}

impl ShellState {
    /// Whether the page shown takes `od:menu`.
    pub(crate) fn listens(&self) -> bool {
        self.listening == Some(self.page)
    }

    /// Whether Reload question `token` is still open: the page has not
    /// answered it, no newer question replaced it, and the page is the
    /// one asked.
    pub(crate) fn is_open(&self, token: u64) -> bool {
        self.open == Some(token)
    }

    /// The last page the window started or finished loading.
    pub(crate) fn loaded(&self) -> Option<&Url> {
        self.loaded.as_ref()
    }

    /// Takes `event` at `now`: the state after, and what to do.
    pub(crate) fn step(&mut self, event: ShellEvent, now: Instant) -> Vec<Action> {
        match event {
            ShellEvent::PageStarted(url) => {
                self.new_page();
                self.page_origin = Some(origin_of(&url));
                if let Some((_, early)) = &self.loaded_early {
                    // The grace timer's own load starts its page once: the
                    // old process's late word may still come after it.
                    if !self.early_started && *early == url {
                        self.early_started = true;
                    } else {
                        self.started_since_early = true;
                    }
                }
                self.loaded = Some(url);
                Vec::new()
            }
            ShellEvent::PageFinished(url) => {
                self.loaded = Some(url);
                Vec::new()
            }
            ShellEvent::MenuListens { origin } => {
                let gone = origin
                    .zip(self.page_origin.as_ref())
                    .is_some_and(|(from, shown)| from != *shown);
                if gone {
                    tracing::info!("a page gone said it takes the menu: ignored");
                } else {
                    self.listening = Some(self.page);
                }
                Vec::new()
            }
            ShellEvent::ReloadAsked => {
                if !self.listens() {
                    return vec![Action::ReloadUnheard];
                }
                self.last_token = self.last_token.wrapping_add(1);
                self.open = Some(self.last_token);
                vec![
                    Action::AskPage(self.last_token),
                    Action::CheckLater {
                        token: self.last_token,
                        after: RELOAD_ACK_WAIT,
                    },
                ]
            }
            ShellEvent::ReloadAck => {
                self.open = None;
                Vec::new()
            }
            ShellEvent::ReloadCheck(token) => {
                if self.is_open(token) {
                    vec![Action::AskUser(token)]
                } else {
                    Vec::new()
                }
            }
            ShellEvent::UserChose { token, choice } => {
                match after_not_answering(self.is_open(token), choice) {
                    NotAnswered::Nothing => {
                        if choice == Some(NotRespondingChoice::Reload) {
                            tracing::info!("the page answered Reload first: not restarting it");
                        }
                        Vec::new()
                    }
                    NotAnswered::AskAgainLater => vec![Action::CheckLater {
                        token,
                        after: RELOAD_ASK_AGAIN,
                    }],
                    NotAnswered::Restart => {
                        self.open = None;
                        tracing::warn!("the page did not answer Reload: restarting it");
                        vec![Action::Restart(StopReason::Hung)]
                    }
                }
            }
            ShellEvent::DialogHung => vec![Action::Restart(StopReason::Dialogs)],
            ShellEvent::Restart { shown, why } => {
                // The page is replaced: no Reload question is left about it.
                self.new_page();
                self.next = Some(stopped_preview_url(&shown, why));
                self.loaded_early = None;
                vec![Action::Kill]
            }
            ShellEvent::GraceOver { killed } => {
                let Some(next) = self.next.take() else {
                    return Vec::new();
                };
                if killed {
                    self.loaded_early = Some((now, next.clone()));
                    self.early_started = false;
                    self.started_since_early = false;
                }
                vec![Action::Navigate(next)]
            }
            ShellEvent::ProcessEnded => self.process_ended(now),
            ShellEvent::QaCrash => vec![Action::EndProcess],
        }
    }

    /// A new page replaces the one shown: it takes no `od:menu` until its
    /// shell says so (`listening` names the page before), and no Reload
    /// question is left about the page gone.
    fn new_page(&mut self) {
        self.page = self.page.wrapping_add(1);
        self.open = None;
    }

    /// The web content process ended at `now`: a restart's page loads; a
    /// restart's late word does nothing; anything else is a crash.
    fn process_ended(&mut self, now: Instant) -> Vec<Action> {
        // The page is gone with its process, whatever comes next (R1).
        self.open = None;
        self.listening = None;
        if let Some(url) = self.next.take() {
            self.loaded_early = None;
            return vec![Action::Navigate(url)];
        }
        let since_new_page = std::mem::take(&mut self.started_since_early);
        match self.loaded_early.take() {
            Some((at, _))
                if !since_new_page && now.saturating_duration_since(at) < LATE_END_WINDOW =>
            {
                tracing::info!("the web content process ended after its restart");
                return Vec::new();
            }
            _ => {}
        }
        let after = self.crashed(now);
        tracing::warn!(?after, "the web content process ended by itself");
        match after {
            AfterCrash::Reload => vec![Action::Reload],
            AfterCrash::StopPreview => match &self.loaded {
                Some(url) => vec![Action::Navigate(stopped_preview_url(
                    url,
                    StopReason::Crashed,
                ))],
                None => {
                    tracing::warn!("cannot tell which page crashed");
                    Vec::new()
                }
            },
            AfterCrash::GiveUp => Vec::new(),
        }
    }

    /// The web content process crashed at `now`.
    fn crashed(&mut self, now: Instant) -> AfterCrash {
        let again = self
            .last_crash
            .is_some_and(|last| now.saturating_duration_since(last) < CRASH_LOOP_WINDOW);
        self.crashes_in_a_row = if again {
            self.crashes_in_a_row.saturating_add(1)
        } else {
            1
        };
        self.last_crash = Some(now);
        match self.crashes_in_a_row {
            1 => AfterCrash::Reload,
            2 => AfterCrash::StopPreview,
            _ => AfterCrash::GiveUp,
        }
    }

    /// How many crashes came in a row.
    #[cfg(test)]
    pub(crate) fn crashes_in_a_row(&self) -> u32 {
        self.crashes_in_a_row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    const APP: &str = "http://127.0.0.1:5173/projects/p1";

    /// A state showing [`APP`] at `t0`.
    fn shown(t0: Instant) -> ShellState {
        let mut state = ShellState::default();
        state.step(ShellEvent::PageStarted(url(APP)), t0);
        state
    }

    /// [`shown`], its shell taking `od:menu`.
    fn listening(t0: Instant) -> ShellState {
        let mut state = shown(t0);
        state.step(ShellEvent::MenuListens { origin: None }, t0);
        state
    }

    /// [`listening`], Reload question 1 open.
    fn asked(t0: Instant) -> ShellState {
        let mut state = listening(t0);
        state.step(ShellEvent::ReloadAsked, t0);
        assert!(state.is_open(1));
        state
    }

    /// [`listening`], a restart from [`APP`] begun (its process asked to
    /// end).
    fn restarting(t0: Instant) -> ShellState {
        let mut state = listening(t0);
        let why = StopReason::Hung;
        state.step(
            ShellEvent::Restart {
                shown: url(APP),
                why,
            },
            t0,
        );
        state
    }

    /// [`restarting`], the grace timer loaded its page, WebKit silent.
    fn loaded_early(t0: Instant) -> ShellState {
        let mut state = restarting(t0);
        state.step(ShellEvent::GraceOver { killed: true }, t0);
        state
    }

    fn stopped(why: StopReason) -> Url {
        stopped_preview_url(&url(APP), why)
    }

    /// [`loaded_early`], the restart's page started (WebKit's own
    /// `PageStarted` for the grace timer's load).
    fn early_page_started(t0: Instant) -> ShellState {
        let mut state = loaded_early(t0);
        state.step(ShellEvent::PageStarted(stopped(StopReason::Hung)), t0);
        state
    }

    /// [`early_page_started`], then another page started.
    fn early_then_another(t0: Instant) -> ShellState {
        let mut state = early_page_started(t0);
        state.step(ShellEvent::PageStarted(url(APP)), t0);
        state
    }

    /// [`early_page_started`], then the same page started again (a reload).
    fn early_started_twice(t0: Instant) -> ShellState {
        let mut state = early_page_started(t0);
        state.step(ShellEvent::PageStarted(stopped(StopReason::Hung)), t0);
        state
    }

    /// One row: a state, an event `after` its start, the actions, and a
    /// check of the state after.
    struct Row {
        name: &'static str,
        state: fn(Instant) -> ShellState,
        event: ShellEvent,
        after: Duration,
        actions: Vec<Action>,
        then: fn(&ShellState) -> bool,
    }

    fn row(
        name: &'static str,
        state: fn(Instant) -> ShellState,
        event: ShellEvent,
        actions: Vec<Action>,
        then: fn(&ShellState) -> bool,
    ) -> Row {
        Row {
            name,
            state,
            event,
            after: Duration::ZERO,
            actions,
            then,
        }
    }

    #[test]
    fn each_state_takes_each_event_as_the_table_says() {
        use Action::*;
        use ShellEvent as E;
        let soon = Duration::from_millis(1);
        let rows = [
            // The Reload question.
            row(
                "deaf page: reload unheard",
                shown,
                E::ReloadAsked,
                vec![ReloadUnheard],
                |s| s.open.is_none(),
            ),
            row(
                "listening page: asked",
                listening,
                E::ReloadAsked,
                vec![
                    AskPage(1),
                    CheckLater {
                        token: 1,
                        after: RELOAD_ACK_WAIT,
                    },
                ],
                |s| s.is_open(1),
            ),
            row(
                "a newer Reload takes over",
                asked,
                E::ReloadAsked,
                vec![
                    AskPage(2),
                    CheckLater {
                        token: 2,
                        after: RELOAD_ACK_WAIT,
                    },
                ],
                |s| !s.is_open(1) && s.is_open(2),
            ),
            row(
                "ack closes the question",
                asked,
                E::ReloadAck,
                vec![],
                |s| !s.is_open(1) && s.listens(),
            ),
            row(
                "check of an open question asks the user",
                asked,
                E::ReloadCheck(1),
                vec![AskUser(1)],
                |s| s.is_open(1),
            ),
            row(
                "check of an answered question",
                listening,
                E::ReloadCheck(1),
                vec![],
                |_| true,
            ),
            row(
                "user waits: ask again later",
                asked,
                E::UserChose {
                    token: 1,
                    choice: Some(NotRespondingChoice::Wait),
                },
                vec![CheckLater {
                    token: 1,
                    after: RELOAD_ASK_AGAIN,
                }],
                |s| s.is_open(1),
            ),
            row(
                "no question could show: ask again later",
                asked,
                E::UserChose {
                    token: 1,
                    choice: None,
                },
                vec![CheckLater {
                    token: 1,
                    after: RELOAD_ASK_AGAIN,
                }],
                |s| s.is_open(1),
            ),
            row(
                "user reloads a silent page: restart",
                asked,
                E::UserChose {
                    token: 1,
                    choice: Some(NotRespondingChoice::Reload),
                },
                vec![Restart(StopReason::Hung)],
                |s| !s.is_open(1),
            ),
            row(
                "user reloads a page that answered: nothing",
                listening,
                E::UserChose {
                    token: 1,
                    choice: Some(NotRespondingChoice::Reload),
                },
                vec![],
                |_| true,
            ),
            // Pages.
            row(
                "page start: deaf, question closed",
                asked,
                E::PageStarted(url(APP)),
                vec![],
                |s| !s.listens() && !s.is_open(1),
            ),
            row(
                "page finish: kept as shown",
                listening,
                E::PageFinished(url("http://127.0.0.1:5173/present/p2")),
                vec![],
                |s| s.listens() && s.loaded() == Some(&url("http://127.0.0.1:5173/present/p2")),
            ),
            row(
                "menu word from the page's origin",
                shown,
                E::MenuListens {
                    origin: Some("http://127.0.0.1:5173".to_owned()),
                },
                vec![],
                ShellState::listens,
            ),
            row(
                "menu word from another origin: a page gone (R4)",
                shown,
                E::MenuListens {
                    origin: Some("tauri://localhost".to_owned()),
                },
                vec![],
                |s| !s.listens(),
            ),
            // Restarts.
            row(
                "dialog loop: restart",
                listening,
                E::DialogHung,
                vec![Restart(StopReason::Dialogs)],
                |s| s.listens(),
            ),
            row(
                "restart: kill, page replaced",
                asked,
                E::Restart {
                    shown: url(APP),
                    why: StopReason::Dialogs,
                },
                vec![Kill],
                |s| !s.listens() && !s.is_open(1) && s.next == Some(stopped(StopReason::Dialogs)),
            ),
            row(
                "WebKit says it ended: the restart's page",
                restarting,
                E::ProcessEnded,
                vec![Navigate(stopped(StopReason::Hung))],
                |s| s.next.is_none() && s.crashes_in_a_row == 0,
            ),
            row(
                "grace over, WebKit silent: the restart's page",
                restarting,
                E::GraceOver { killed: true },
                vec![Navigate(stopped(StopReason::Hung))],
                |s| s.loaded_early.is_some(),
            ),
            row(
                "grace over, not killed: the page, no late word",
                restarting,
                E::GraceOver { killed: false },
                vec![Navigate(stopped(StopReason::Hung))],
                |s| s.loaded_early.is_none(),
            ),
            row(
                "grace over after WebKit's word: nothing",
                shown,
                E::GraceOver { killed: true },
                vec![],
                |s| s.loaded_early.is_none(),
            ),
            row(
                "late word before the restart's page started: no crash",
                loaded_early,
                E::ProcessEnded,
                vec![],
                |s| s.crashes_in_a_row == 0 && s.loaded_early.is_none(),
            ),
            row(
                "late word after the restart's page started: no crash",
                early_page_started,
                E::ProcessEnded,
                vec![],
                |s| s.crashes_in_a_row == 0 && s.loaded_early.is_none(),
            ),
            row(
                "an end after another page started: a crash (R2)",
                early_then_another,
                E::ProcessEnded,
                vec![Reload],
                |s| s.crashes_in_a_row == 1,
            ),
            row(
                "an end after the restart's page started twice: a crash (R2)",
                early_started_twice,
                E::ProcessEnded,
                vec![Reload],
                |s| s.crashes_in_a_row == 1,
            ),
            Row {
                after: LATE_END_WINDOW,
                ..row(
                    "late word too late: a crash",
                    loaded_early,
                    E::ProcessEnded,
                    vec![Reload],
                    |s| s.crashes_in_a_row == 1,
                )
            },
            // Crashes.
            row(
                "a crash reloads, and the page is deaf (R1)",
                listening,
                E::ProcessEnded,
                vec![Reload],
                |s| !s.listens() && s.crashes_in_a_row == 1,
            ),
            row(
                "QA's crash hook ends the process, no more",
                listening,
                E::QaCrash,
                vec![EndProcess],
                |s| s.listens() && s.crashes_in_a_row == 0,
            ),
            row(
                "a crash closes the question (R1)",
                asked,
                E::ProcessEnded,
                vec![Reload],
                |s| !s.is_open(1),
            ),
            Row {
                after: soon,
                ..row(
                    "a second crash soon after stops the preview",
                    |t0| {
                        let mut s = listening(t0);
                        s.step(ShellEvent::ProcessEnded, t0);
                        s
                    },
                    E::ProcessEnded,
                    vec![Navigate(stopped(StopReason::Crashed))],
                    |s| s.crashes_in_a_row == 2,
                )
            },
        ];
        for row in rows {
            let t0 = Instant::now();
            let mut state = (row.state)(t0);
            let actions = state.step(row.event.clone(), t0 + row.after);
            assert_eq!(actions, row.actions, "{}: {:?}", row.name, row.event);
            assert!((row.then)(&state), "{}: state after {state:?}", row.name);
        }
    }

    #[test]
    fn a_lone_crash_reloads() {
        let t0 = Instant::now();
        let mut state = ShellState::default();
        assert_eq!(state.crashed(t0), AfterCrash::Reload);
        // Long after: a crash of its own, not a loop.
        assert_eq!(state.crashed(t0 + CRASH_LOOP_WINDOW), AfterCrash::Reload);
    }

    #[test]
    fn a_second_crash_soon_after_stops_the_preview() {
        // A design that crashes the process as it loads reloaded for ever.
        let t0 = Instant::now();
        let mut state = ShellState::default();
        assert_eq!(state.crashed(t0), AfterCrash::Reload);
        let soon = t0 + CRASH_LOOP_WINDOW - Duration::from_millis(1);
        assert_eq!(state.crashed(soon), AfterCrash::StopPreview);
        // Crashing even with the preview stopped: no more loads.
        assert_eq!(state.crashed(soon), AfterCrash::GiveUp);
        assert_eq!(state.crashed(soon), AfterCrash::GiveUp);
        // Quiet for a while: a new crash reloads again.
        let later = soon + CRASH_LOOP_WINDOW;
        assert_eq!(state.crashed(later), AfterCrash::Reload);
    }

    #[test]
    fn a_crash_loop_given_up_leaves_view_reload_working() {
        // R1: the window is blank after the third crash; ⌘R must load it
        // again, not ask the dead page and say it is not responding.
        let t0 = Instant::now();
        let mut state = listening(t0);
        for _ in 0..3 {
            state.step(ShellEvent::ProcessEnded, t0);
        }
        assert_eq!(state.crashes_in_a_row, 3);
        assert_eq!(
            state.step(ShellEvent::ReloadAsked, t0),
            [Action::ReloadUnheard]
        );
    }

    #[test]
    fn webkit_late_word_after_a_restart_is_not_a_crash() {
        // WebKit said nothing within RESTART_GRACE, so the grace timer
        // loaded the page; WebKit saying the old process ended after that
        // is the same restart: no second load, no crash counted.
        let t0 = Instant::now();
        let mut state = loaded_early(t0);
        let late = t0 + LATE_END_WINDOW - Duration::from_millis(1);
        assert_eq!(state.step(ShellEvent::ProcessEnded, late), []);
        // Once: the next end is the new page's own.
        assert_eq!(state.step(ShellEvent::ProcessEnded, late), [Action::Reload]);
    }

    #[test]
    fn webkit_late_word_after_the_restarts_page_started_is_not_a_crash() {
        // The app's order: the grace timer navigates, WebKit starts that
        // page (`PageStarted`), then says the old process ended. The start
        // is the restart's own load, not a new page.
        let t0 = Instant::now();
        let mut state = loaded_early(t0);
        let page = stopped(StopReason::Hung);
        state.step(ShellEvent::PageStarted(page.clone()), t0);
        state.step(ShellEvent::PageFinished(page), t0);
        assert_eq!(state.step(ShellEvent::ProcessEnded, t0), []);
        assert_eq!(state.crashes_in_a_row, 0);
    }

    #[test]
    fn a_crash_after_a_new_page_since_the_restart_is_a_crash() {
        // R2: a page started since the restart's own, so the process that
        // ended is that page's, however soon: a crash, reloaded, not a
        // blank window.
        let t0 = Instant::now();
        let mut state = early_then_another(t0);
        assert_eq!(state.step(ShellEvent::ProcessEnded, t0), [Action::Reload]);
        assert_eq!(state.crashes_in_a_row, 1);
    }

    #[test]
    fn a_process_end_long_after_a_restart_is_a_crash() {
        let t0 = Instant::now();
        let mut state = loaded_early(t0);
        let ended = state.step(ShellEvent::ProcessEnded, t0 + LATE_END_WINDOW);
        assert_eq!(ended, [Action::Reload]);
        // The process was never asked to end (this WebKit cannot): any
        // end after is a crash.
        let mut state = restarting(t0);
        state.step(ShellEvent::GraceOver { killed: false }, t0);
        assert_eq!(state.step(ShellEvent::ProcessEnded, t0), [Action::Reload]);
    }

    #[test]
    fn webkit_word_in_time_loads_the_restarts_page_once() {
        let t0 = Instant::now();
        let mut state = restarting(t0);
        let ended = state.step(ShellEvent::ProcessEnded, t0);
        assert_eq!(ended, [Action::Navigate(stopped(StopReason::Hung))]);
        // The grace timer finds nothing left to load.
        assert_eq!(state.step(ShellEvent::GraceOver { killed: true }, t0), []);
        assert_eq!(state.step(ShellEvent::ProcessEnded, t0), [Action::Reload]);
    }

    #[test]
    fn a_reload_question_stays_open_until_answered_or_replaced() {
        let t0 = Instant::now();
        let mut state = asked(t0);
        assert!(state.is_open(1), "looking does not close it");
        state.step(ShellEvent::ReloadAsked, t0);
        assert!(!state.is_open(1), "a newer Reload takes over");
        assert!(state.is_open(2));
        // An old question's timer does not act on a newer one.
        let old = ShellEvent::UserChose {
            token: 1,
            choice: Some(NotRespondingChoice::Reload),
        };
        assert_eq!(state.step(old, t0), []);
        state.step(ShellEvent::ReloadAck, t0);
        assert!(!state.is_open(2));
    }

    #[test]
    fn a_page_silent_after_reload_is_restarted_only_on_the_users_word() {
        use NotAnswered::{AskAgainLater, Nothing, Restart};
        use NotRespondingChoice::{Closed, Reload, Wait};
        // A busy page is not killed behind the user's back: Wait, or no
        // question shown (a dialog is up), asks again later.
        assert_eq!(after_not_answering(true, Some(Wait)), AskAgainLater);
        assert_eq!(after_not_answering(true, None), AskAgainLater);
        assert_eq!(after_not_answering(true, Some(Reload)), Restart);
        // The page answered while the user decided: its leave guard has
        // the reload now, whatever the choice.
        assert_eq!(after_not_answering(false, Some(Reload)), Nothing);
        assert_eq!(after_not_answering(false, Some(Wait)), Nothing);
        assert_eq!(after_not_answering(false, None), Nothing);
        // The page answered, so the shell closed the alert (QA-072): a
        // Reload on the stale question, or the alert closing, does nothing.
        assert_eq!(after_not_answering(false, Some(Closed)), Nothing);
        // Closed with the question still open (no path does it): never a
        // restart without the user's word.
        assert_eq!(after_not_answering(true, Some(Closed)), AskAgainLater);
    }

    #[test]
    fn an_origin_is_written_as_its_header() {
        assert_eq!(
            origin_of(&url("http://127.0.0.1:5173/a?b")),
            "http://127.0.0.1:5173"
        );
        assert_eq!(
            origin_of(&url("tauri://localhost/index.html")),
            "tauri://localhost"
        );
        assert_eq!(
            origin_of(&url("http://tauri.localhost/")),
            "http://tauri.localhost"
        );
    }

    #[test]
    fn a_hung_design_reopens_its_studio_with_the_preview_stopped() {
        let base = "http://127.0.0.1:5173";
        let dialogs = StopReason::Dialogs;
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/projects/p1")), dialogs).as_str(),
            format!("{base}/projects/p1?preview=stopped&why=dialogs")
        );
        // Other query kept, the mark not doubled, the fragment kept.
        assert_eq!(
            stopped_preview_url(
                &url(&format!(
                    "{base}/projects/p1?a=1&preview=stopped&why=hung#x"
                )),
                dialogs
            )
            .as_str(),
            format!("{base}/projects/p1?a=1&preview=stopped&why=dialogs#x")
        );
        // Present mode would loop again: back to the design's Studio.
        assert_eq!(
            stopped_preview_url(&url(&format!("{base}/present/p1?slide=3")), dialogs).as_str(),
            format!("{base}/projects/p1?slide=3&preview=stopped&why=dialogs")
        );
    }

    #[test]
    fn the_address_says_why_the_preview_stopped() {
        // QA-071: a busy loop with no dialog is not "kept opening dialogs".
        let page = url(APP);
        let why = |reason| {
            stopped_preview_url(&page, reason)
                .query_pairs()
                .find(|(k, _)| k == STOP_REASON_KEY)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(why(StopReason::Dialogs).as_deref(), Some("dialogs"));
        assert_eq!(why(StopReason::Hung).as_deref(), Some("hung"));
        assert_eq!(why(StopReason::Crashed).as_deref(), Some("crashed"));
    }

    /// A small seeded random source (xorshift64*): the same runs each time,
    /// no crate.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
        }
    }

    /// The pages the runs load, with their origin as the model knows it.
    const PAGES: [&str; 4] = [
        APP,
        "http://127.0.0.1:5173/",
        "http://127.0.0.1:5173/present/p1",
        "tauri://localhost/index.html",
    ];

    /// The origins of [`PAGES`].
    const ORIGINS: [&str; 2] = ["http://127.0.0.1:5173", "tauri://localhost"];

    /// The model's origin of a page of [`PAGES`] or a stopped preview of
    /// one: by its prefix, not by the shell's `origin_of`.
    fn model_origin(page: &Url) -> &'static str {
        ORIGINS
            .into_iter()
            .find(|origin| page.as_str().starts_with(origin))
            .unwrap_or("?")
    }

    /// A restart the shell began: its page, whether its process was asked
    /// to end (WebKit may still say so), whether WebKit said so, whether
    /// the grace timer ran, how many times the shell loaded its page.
    struct Restarting {
        page: Url,
        killed: bool,
        said: bool,
        grace: bool,
        loads: usize,
    }

    /// The grace timer loaded a restart's page `at`, WebKit's word owed:
    /// whether that page's own start came.
    struct Early {
        at: Instant,
        page: Url,
        own_started: bool,
    }

    /// The world around the shell, for [`random_runs_keep_the_invariants`]:
    /// what WebKit and the timers still owe it, and a model of the shell
    /// kept on its own (never the shell's fields or predicates).
    #[derive(Default)]
    struct World {
        restart: Option<Restarting>,
        /// Questions the shell set a timer or asked the user about.
        checks: Vec<u64>,
        /// Pages the shell loaded whose start WebKit owes, in order.
        owed_starts: Vec<Url>,
        /// The page last started or finished.
        shown: Option<Url>,
        /// The origin of the page last started.
        origin: Option<&'static str>,
        /// The page shown said it takes `od:menu`, from its own origin.
        listens: bool,
        /// The Reload question the page has not answered.
        open: Option<u64>,
        /// A restart's page the grace timer loaded; `None` once a process
        /// end used it, or another page started.
        early: Option<Early>,
    }

    impl World {
        /// A new page replaces the one shown.
        fn new_page(&mut self) {
            self.listens = false;
            self.open = None;
        }
    }

    /// The events the shell can see next, `world` allowing.
    fn next_event(rng: &mut Rng, world: &mut World) -> ShellEvent {
        let page = |rng: &mut Rng| url(PAGES[usize::try_from(rng.below(4)).unwrap_or(0)]);
        loop {
            let pick = rng.below(14);
            let event = match pick {
                0 => ShellEvent::PageStarted(page(rng)),
                1 => ShellEvent::PageFinished(page(rng)),
                // No shell speaks before a restart's page loads.
                2 if world.restart.as_ref().is_none_or(|r| r.loads == 1) => {
                    ShellEvent::MenuListens {
                        origin: match rng.below(3) {
                            0 => None,
                            n => Some(ORIGINS[usize::try_from(n - 1).unwrap_or(0)].to_owned()),
                        },
                    }
                }
                3 => ShellEvent::ReloadAsked,
                4 => ShellEvent::ReloadAck,
                5 | 6 if !world.checks.is_empty() => {
                    let at = usize::try_from(rng.below(world.checks.len() as u64)).unwrap_or(0);
                    let token = world.checks.swap_remove(at);
                    if pick == 5 {
                        ShellEvent::ReloadCheck(token)
                    } else {
                        let choice = match rng.below(4) {
                            0 => None,
                            1 => Some(NotRespondingChoice::Wait),
                            2 => Some(NotRespondingChoice::Reload),
                            _ => Some(NotRespondingChoice::Closed),
                        };
                        ShellEvent::UserChose { token, choice }
                    }
                }
                // Restarts come from a live page: none while one runs.
                7 if world.restart.is_none() => ShellEvent::DialogHung,
                8 if world.restart.is_none() => ShellEvent::Restart {
                    shown: page(rng),
                    why: StopReason::Dialogs,
                },
                9 => match &mut world.restart {
                    Some(restart) if !restart.grace => {
                        restart.grace = true;
                        ShellEvent::GraceOver {
                            killed: restart.killed,
                        }
                    }
                    _ => continue,
                },
                10 => match &mut world.restart {
                    // WebKit's word of the restart's process, once.
                    Some(restart) if restart.killed && !restart.said => {
                        restart.said = true;
                        ShellEvent::ProcessEnded
                    }
                    _ => continue,
                },
                // A crash, any time.
                11 => ShellEvent::ProcessEnded,
                12 => ShellEvent::QaCrash,
                // WebKit starts a page the shell loaded, as the app's
                // `on_page_load` says.
                _ if !world.owed_starts.is_empty() => {
                    ShellEvent::PageStarted(world.owed_starts.remove(0))
                }
                _ => ShellEvent::PageStarted(page(rng)),
            };
            return event;
        }
    }

    /// What the model expects of a process end at `now`.
    #[derive(Debug, PartialEq)]
    enum EndIs {
        /// A restart's: its page loads.
        Restart(Url),
        /// The restart's late word: nothing.
        Late,
        /// A crash.
        Crash,
    }

    #[test]
    fn random_runs_keep_the_invariants() {
        for seed in 1..=300_u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut world = World::default();
            let mut state = ShellState::default();
            let mut now = Instant::now();
            for _ in 0..200 {
                now += Duration::from_millis(rng.below(3000));
                let event = next_event(&mut rng, &mut world);
                // The model's word, before the step.
                let listens = world.listens;
                let open = world.open;
                let end_is = match (&world.restart, &world.early) {
                    (Some(restart), _) if restart.loads == 0 => {
                        EndIs::Restart(restart.page.clone())
                    }
                    (_, Some(early))
                        if now.saturating_duration_since(early.at) < LATE_END_WINDOW =>
                    {
                        EndIs::Late
                    }
                    _ => EndIs::Crash,
                };
                let mut actions = state.step(event.clone(), now);
                let trace = |actions: &[Action]| format!("seed {seed}: {event:?} → {actions:?}");
                // The model takes the event.
                match &event {
                    ShellEvent::PageStarted(page) => {
                        world.new_page();
                        world.origin = Some(model_origin(page));
                        world.shown = Some(page.clone());
                        if let Some(early) = &mut world.early {
                            if !early.own_started && early.page == *page {
                                early.own_started = true;
                            } else {
                                world.early = None;
                            }
                        }
                    }
                    ShellEvent::PageFinished(page) => world.shown = Some(page.clone()),
                    ShellEvent::MenuListens { origin } => {
                        let from = origin.as_deref();
                        if from.is_none() || world.origin.is_none() || from == world.origin {
                            world.listens = true;
                        }
                    }
                    ShellEvent::ReloadAsked => {
                        if listens {
                            let Some(Action::AskPage(token)) = actions.first() else {
                                panic!("a listening page not asked: {}", trace(&actions));
                            };
                            world.open = Some(*token);
                        } else {
                            assert_eq!(actions, [Action::ReloadUnheard], "{}", trace(&actions));
                        }
                    }
                    ShellEvent::ReloadAck => world.open = None,
                    ShellEvent::ReloadCheck(token) => {
                        let asks = actions == [Action::AskUser(*token)];
                        assert_eq!(asks, open == Some(*token), "{}", trace(&actions));
                    }
                    ShellEvent::ProcessEnded => {
                        match &end_is {
                            EndIs::Restart(page) => {
                                assert_eq!(
                                    actions,
                                    [Action::Navigate(page.clone())],
                                    "{}",
                                    trace(&actions)
                                );
                            }
                            EndIs::Late => assert_eq!(actions, [], "{}", trace(&actions)),
                            // A real crash, after a restart's early load too,
                            // counts.
                            EndIs::Crash => assert_eq!(
                                state.last_crash,
                                Some(now),
                                "crash not counted: {}",
                                trace(&actions)
                            ),
                        }
                        // The page is gone with its process.
                        world.new_page();
                        world.early = None;
                    }
                    ShellEvent::QaCrash => {
                        assert_eq!(actions, [Action::EndProcess], "{}", trace(&actions));
                    }
                    _ => {}
                }
                // The shell's actions, carried out.
                let mut restarted = None;
                for action in &actions {
                    match action {
                        // Never ask a page that cannot answer.
                        Action::AskPage(_) => {
                            assert!(listens, "asked a deaf page: {}", trace(&actions))
                        }
                        Action::CheckLater { token, .. } | Action::AskUser(token) => {
                            world.checks.push(*token);
                        }
                        Action::Restart(why) => restarted = Some(*why),
                        Action::Navigate(page) => world.owed_starts.push(page.clone()),
                        Action::Reload | Action::ReloadUnheard => {
                            world
                                .owed_starts
                                .push(world.shown.clone().unwrap_or_else(|| url(APP)));
                        }
                        _ => {}
                    }
                }
                let begun = match &event {
                    ShellEvent::Restart { shown, why } => Some((shown.clone(), *why)),
                    _ => {
                        restarted.map(|why| (world.shown.clone().unwrap_or_else(|| url(APP)), why))
                    }
                };
                if let Some((shown, why)) = begun {
                    if !matches!(event, ShellEvent::Restart { .. }) {
                        // The app reads the address shown and feeds it.
                        let more = state.step(
                            ShellEvent::Restart {
                                shown: shown.clone(),
                                why,
                            },
                            now,
                        );
                        actions.extend(more.iter().cloned());
                        assert_eq!(more, [Action::Kill], "{}", trace(&actions));
                    }
                    assert!(actions.contains(&Action::Kill), "{}", trace(&actions));
                    world.new_page();
                    world.early = None;
                    world.restart = Some(Restarting {
                        page: stopped_preview_url(&shown, why),
                        killed: rng.below(4) != 0,
                        said: false,
                        grace: false,
                        loads: 0,
                    });
                }
                // Each kill loads its page once, from WebKit's word or the
                // grace timer, whichever comes first.
                if let Some(restart) = &mut world.restart {
                    let loads = actions
                        .iter()
                        .filter(|a| **a == Action::Navigate(restart.page.clone()))
                        .count();
                    restart.loads += loads;
                    assert!(
                        restart.loads <= 1,
                        "a restart loaded twice: {}",
                        trace(&actions)
                    );
                    if event
                        == (ShellEvent::GraceOver {
                            killed: restart.killed,
                        })
                    {
                        assert_eq!(
                            restart.loads,
                            1,
                            "a restart never loaded: {}",
                            trace(&actions)
                        );
                        if loads == 1 && restart.killed {
                            world.early = Some(Early {
                                at: now,
                                page: restart.page.clone(),
                                own_started: false,
                            });
                        }
                    }
                    // Done once the timer ran and WebKit's word, if owed, came:
                    // a later load of its page would be a second.
                    if restart.grace && (restart.said || !restart.killed) {
                        world.restart = None;
                    }
                }
            }
        }
    }
}
