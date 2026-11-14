//! The images of work not saved yet (a comment being written, a Manual
//! edit image not applied) and the one place they are let go.
//!
//! Each Studio page has a draft session: a random id sent with every image
//! uploaded for a draft. The server records those images apart from the
//! project's own files (they do not ship) until a comment takes them, and
//! deletes them when the session is released, an upload landing after it
//! included. The page releases its session whichever way it goes: the
//! Studio left in the app ([`release`], from its cleanup), the page
//! reloaded or closed ([`release_unloading`], on `pagehide`); the desktop
//! app releases every session as it quits, and the server sweeps what a
//! crash left after a day.
//!
//! A comment being sent when its Studio goes holds the release back
//! ([`Sends`]): its images are its own once it is saved.

use super::api;
use super::tasks;
use std::collections::HashMap;

/// A fresh draft session id: 24 hex digits, random.
pub fn new_session() -> String {
    let part = || (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
    format!("{:08x}{:08x}{:08x}", part(), part(), part())
}

/// `POST …/drafts/<session>/release` of the project at `base`.
pub fn release_url(base: &str, session: &str) -> String {
    format!("{base}/drafts/{}/release", api::seg(session))
}

/// Releases `session` of the project at `base`: work with no view, it runs
/// to its end whatever goes meanwhile.
pub fn release(base: &str, session: &str) {
    let url = release_url(base, session);
    tasks::detached(async move {
        let _ = api::post::<_, od_contracts::OkResponse>(&url, &serde_json::json!({})).await;
    });
}

/// Releases `session` as the page unloads: a `keepalive` request, which
/// the unload does not cut (as the autosaves do, QA-090).
pub fn release_unloading(base: &str, session: &str) {
    api::post_unloading(&release_url(base, session));
}

/// Comments being sent, by draft session, and the sessions whose page
/// went meanwhile: their release waits for the last send to end, else it
/// would delete images the comment is saving with.
#[derive(Debug, Default)]
pub struct Sends(HashMap<String, Pending>);

#[derive(Debug, Default)]
struct Pending {
    sending: u32,
    left: bool,
}

impl Sends {
    /// A comment of `session` is being sent.
    pub fn begin(&mut self, session: &str) {
        self.0.entry(session.to_owned()).or_default().sending += 1;
    }

    /// A comment of `session` was sent (or failed). True when its page
    /// went meanwhile and this was the last one: release it now.
    pub fn end(&mut self, session: &str) -> bool {
        let Some(p) = self.0.get_mut(session) else {
            return false;
        };
        p.sending = p.sending.saturating_sub(1);
        if p.sending > 0 {
            return false;
        }
        let left = p.left;
        self.0.remove(session);
        left
    }

    /// The page of `session` went. True when nothing is being sent:
    /// release it now; else the last send does.
    pub fn leave(&mut self, session: &str) -> bool {
        match self.0.get_mut(session) {
            Some(p) if p.sending > 0 => {
                p.left = true;
                false
            }
            _ => true,
        }
    }

    /// Whether a comment of `session` is being sent.
    pub fn busy(&self, session: &str) -> bool {
        self.0.get(session).is_some_and(|p| p.sending > 0)
    }
}

/// A file or a folder being renamed or moved, from where to where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub from: String,
    pub to: String,
    /// A folder: every path under `from/` moves under `to/`.
    pub folder: bool,
}

impl Move {
    /// Where `path` is after this move, if it moves.
    pub fn apply(&self, path: &str) -> Option<String> {
        if !self.folder {
            return (path == self.from).then(|| self.to.clone());
        }
        let rest = path.strip_prefix(&self.from)?.strip_prefix('/')?;
        Some(format!("{}/{rest}", self.to))
    }
}

/// Where `path` is after `moves`, done in order.
pub fn moved(moves: &[Move], path: &str) -> Option<String> {
    let mut at: Option<String> = None;
    for m in moves {
        if let Some(next) = m.apply(at.as_deref().unwrap_or(path)) {
            at = Some(next);
        }
    }
    at
}

/// Whether `path` is (in) one of `moving`, the files and folders (`dir/`)
/// whose rename or move is on its way: gone from the list a moment, not
/// deleted.
pub fn is_moving(moving: &[String], path: &str) -> bool {
    moving
        .iter()
        .any(|m| path == m || (m.ends_with('/') && path.starts_with(m.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_waits_for_the_comments_being_sent() {
        let mut s = Sends::default();
        assert!(s.leave("a"), "nothing sent: release at once");
        s.begin("b");
        s.begin("b");
        assert!(s.busy("b"));
        assert!(!s.leave("b"), "held for the sends");
        assert!(!s.end("b"), "one still on its way");
        assert!(s.end("b"), "the last one releases it");
        assert!(!s.busy("b"));
        assert!(!s.end("b"), "released once");
        // Sent while the page stays: no release.
        s.begin("c");
        assert!(!s.end("c"));
        assert!(s.leave("c"));
    }

    #[test]
    fn a_move_takes_the_file_or_what_is_in_the_folder() {
        let file = Move {
            from: "a.html".into(),
            to: "c.html".into(),
            folder: false,
        };
        assert_eq!(file.apply("a.html").as_deref(), Some("c.html"));
        assert_eq!(file.apply("a.html.bak"), None);
        let dir = Move {
            from: "pages".into(),
            to: "site/pages".into(),
            folder: true,
        };
        assert_eq!(
            dir.apply("pages/a.html").as_deref(),
            Some("site/pages/a.html")
        );
        assert_eq!(dir.apply("pages2/a.html"), None, "another folder");
        assert_eq!(dir.apply("pages"), None);
        assert_eq!(
            moved(&[file, dir], "a.html").as_deref(),
            Some("c.html"),
            "in order"
        );
    }

    #[test]
    fn a_path_being_moved_is_not_gone() {
        let moving = ["a.html".to_owned(), "pages/".to_owned()];
        assert!(is_moving(&moving, "a.html"));
        assert!(is_moving(&moving, "pages/b.html"));
        assert!(!is_moving(&moving, "pages2/b.html"));
        assert!(!is_moving(&moving, "b.html"));
    }
}
