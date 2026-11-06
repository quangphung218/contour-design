//! The Comments tab's model (spec 1.2): which comments of the shown file
//! it lists, their numbers (shared with the pins), and whether each one's
//! anchor still holds in the preview.

use crate::preview_bridge::Located;
use od_contracts::{CommentStatus, PreviewComment};
use std::collections::{HashMap, HashSet};

/// Where a comment's element stands in the page now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorState {
    /// The selector finds the element and it reads as when commented.
    Anchored,
    /// The selector finds an element whose text changed: the anchor may
    /// have moved to another element, or the element was rewritten.
    Stale,
    /// The selector finds nothing.
    Lost,
}

impl AnchorState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anchored => "anchored",
            Self::Stale => "stale",
            Self::Lost => "lost",
        }
    }
}

/// Longest text the bridge prints (`plainText`); the comment keeps more.
const PRINT_CHARS: usize = 160;

/// `s` as compared: whitespace collapsed, capped as the bridge caps it,
/// and trimmed (the store trims what it keeps; a cap may end on a space).
fn print(s: &str) -> String {
    let capped: String = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(PRINT_CHARS)
        .collect();
    capped.trim_end().to_owned()
}

/// The anchor state from where the frame located the comment's element.
/// A comment saved without text (older ones) is anchored while found.
#[must_use]
pub fn anchor_state(comment_text: &str, located: &Located) -> AnchorState {
    if located.rect.is_none() {
        return AnchorState::Lost;
    }
    let was = print(comment_text);
    match &located.text {
        Some(now) if !was.is_empty() && print(now) != was => AnchorState::Stale,
        _ => AnchorState::Anchored,
    }
}

/// The comments of `file`, oldest first: resolved ones only when
/// `resolved`, the others otherwise.
#[must_use]
pub fn board<'a>(
    list: &'a [PreviewComment],
    file: &str,
    resolved: bool,
) -> Vec<&'a PreviewComment> {
    list.iter()
        .filter(|c| c.file_path == file && (c.status == CommentStatus::Resolved) == resolved)
        .collect()
}

/// A comment's number on its file (1-based, by creation), the same in the
/// list and on the pin.
#[must_use]
pub fn number(list: &[PreviewComment], comment: &PreviewComment) -> usize {
    list.iter()
        .filter(|c| c.file_path == comment.file_path)
        .position(|c| c.id == comment.id)
        .map_or(0, |i| i + 1)
}

/// Which list a comment load was asked for: the project, the conversation
/// and the load's number (newer loads have higher numbers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadTicket {
    pub project: String,
    pub conversation: String,
    pub seq: u32,
}

/// Whether the answer to `asked` may replace the shown list: only while
/// the Studio still shows that project and conversation and no newer load
/// was asked for since. A late answer for another project, another
/// conversation or an older load is dropped.
#[must_use]
pub fn is_current(asked: &LoadTicket, now: Option<&LoadTicket>) -> bool {
    now == Some(asked)
}

/// What the Comments tab knows beyond the list itself: why the last load
/// failed, and which comments came in since the conversation was loaded
/// and were not shown yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Feed {
    /// The last load failed (daemon away, request error): the list shown
    /// is the one loaded before, and the tab offers a retry.
    pub error: Option<String>,
    /// Ids of comments not shown yet in the tab.
    pub unread: HashSet<String>,
    /// The conversation whose comments were loaded once already; its first
    /// load sets what counts as seen.
    pub loaded: Option<String>,
}

impl Feed {
    /// A conversation is opened: nothing loaded, nothing failed.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Folds the outcome of a load of `conversation` into `list`. A failure
    /// keeps the list as it is and records why; a success replaces it, and
    /// what it brings that the list lacked is unread (unless it is the
    /// conversation's first load).
    pub fn apply(
        &mut self,
        list: &mut Vec<PreviewComment>,
        conversation: &str,
        outcome: Result<Vec<PreviewComment>, String>,
    ) {
        let fresh = match outcome {
            Ok(fresh) => fresh,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        self.error = None;
        if self.loaded.as_deref() == Some(conversation) {
            self.unread.extend(
                fresh
                    .iter()
                    .filter(|c| !list.iter().any(|old| old.id == c.id))
                    .map(|c| c.id.clone()),
            );
            self.unread.retain(|id| fresh.iter().any(|c| &c.id == id));
        } else {
            self.unread.clear();
            self.loaded = Some(conversation.to_owned());
        }
        *list = fresh;
    }

    /// The comments the tab shows now (`shown`) were seen: those, and only
    /// those, are no longer unread. Returns whether anything changed.
    pub fn mark_read<'a>(&mut self, shown: impl IntoIterator<Item = &'a str>) -> bool {
        let before = self.unread.len();
        for id in shown {
            self.unread.remove(id);
        }
        self.unread.len() != before
    }

    /// Whether the tab of `file` lists a comment not shown yet.
    #[must_use]
    pub fn has_unread(&self, list: &[PreviewComment], file: &str) -> bool {
        board(list, file, false)
            .iter()
            .any(|c| self.unread.contains(&c.id))
    }
}

/// What the preview outlines for the comment picked in the tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    /// Nothing picked, or the pick is no longer listed.
    Off,
    /// The picked comment's element, by its selector.
    Outline(String),
    /// The picked comment's element is gone from the page: outline
    /// nothing and say so.
    Missing,
}

impl Marker {
    /// The selector to outline, if any.
    #[must_use]
    pub fn selector(&self) -> Option<&str> {
        match self {
            Self::Outline(s) => Some(s),
            Self::Off | Self::Missing => None,
        }
    }
}

/// The marker for the comment `selected` among the `shown` ones, from the
/// latest anchor states.
#[must_use]
pub fn marker(
    shown: &[&PreviewComment],
    selected: Option<&str>,
    anchors: &HashMap<String, AnchorState>,
) -> Marker {
    let Some(c) = selected.and_then(|id| shown.iter().find(|c| c.id == id)) else {
        return Marker::Off;
    };
    match anchors.get(&c.id) {
        Some(AnchorState::Lost) => Marker::Missing,
        _ => Marker::Outline(c.selector.clone()),
    }
}

/// Project paths of the images of the attached comments, each once, for
/// the next message's attachments.
#[must_use]
pub fn attached_images(list: &[PreviewComment]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in list.iter().filter(|c| c.status == CommentStatus::Attached) {
        for a in &c.attachments {
            if !out.contains(&a.path) {
                out.push(a.path.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_contracts::{CommentImage, CommentRect};

    fn comment(id: &str, file: &str, status: CommentStatus, images: &[&str]) -> PreviewComment {
        PreviewComment {
            id: id.into(),
            project_id: "p".into(),
            conversation_id: "c".into(),
            file_path: file.into(),
            element_id: String::new(),
            selector: "h1".into(),
            label: "h1".into(),
            text: "Hello  world".into(),
            position: CommentRect::default(),
            html_hint: String::new(),
            slide_index: None,
            note: "n".into(),
            status,
            attachments: images
                .iter()
                .map(|p| CommentImage {
                    path: (*p).into(),
                    name: (*p).into(),
                })
                .collect(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn at(text: Option<&str>, found: bool) -> Located {
        Located {
            id: "c1".into(),
            rect: found.then(CommentRect::default),
            text: text.map(str::to_owned),
        }
    }

    #[test]
    fn anchors_hold_go_stale_or_get_lost() {
        assert_eq!(
            anchor_state("Hello world", &at(Some("Hello\n  world"), true)),
            AnchorState::Anchored
        );
        assert_eq!(
            anchor_state("Hello world", &at(Some("Goodbye"), true)),
            AnchorState::Stale
        );
        assert_eq!(
            anchor_state("Hello world", &at(Some("Hello world"), false)),
            AnchorState::Lost
        );
        // No text then, or no text reported: found is enough.
        assert_eq!(
            anchor_state("", &at(Some("Anything"), true)),
            AnchorState::Anchored
        );
        assert_eq!(anchor_state("Hi", &at(None, true)), AnchorState::Anchored);
        // A long text compares as the bridge prints it (capped).
        let long = "word ".repeat(80);
        let printed: String = long.chars().take(160).collect();
        assert_eq!(
            anchor_state(&long, &at(Some(&printed), true)),
            AnchorState::Anchored
        );
    }

    #[test]
    fn board_numbers_and_images() {
        let list = vec![
            comment("a", "index.html", CommentStatus::Open, &["s/1.png"]),
            comment("b", "other.html", CommentStatus::Attached, &[]),
            comment("c", "index.html", CommentStatus::Resolved, &[]),
            comment(
                "d",
                "index.html",
                CommentStatus::Attached,
                &["s/1.png", "s/2.png"],
            ),
        ];
        let open: Vec<_> = board(&list, "index.html", false)
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(open, ["a", "d"]);
        let done: Vec<_> = board(&list, "index.html", true)
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(done, ["c"]);
        assert_eq!(number(&list, &list[3]), 3);
        assert_eq!(number(&list, &list[1]), 1);
        // Only attached comments send their images, each once.
        assert_eq!(attached_images(&list), ["s/1.png", "s/2.png"]);
    }

    fn ids(list: &[PreviewComment]) -> Vec<&str> {
        list.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn a_failed_refresh_keeps_the_list_and_says_so() {
        let mut feed = Feed::default();
        let mut list = Vec::new();
        let first = vec![
            comment("a", "index.html", CommentStatus::Open, &[]),
            comment("b", "index.html", CommentStatus::Attached, &[]),
        ];
        feed.apply(&mut list, "c", Ok(first));
        feed.apply(&mut list, "c", Err("daemon away".into()));
        assert_eq!(ids(&list), ["a", "b"], "the shown comments stay");
        assert_eq!(feed.error.as_deref(), Some("daemon away"));
        // A retry that works clears the error.
        feed.apply(
            &mut list,
            "c",
            Ok(vec![comment("a", "index.html", CommentStatus::Open, &[])]),
        );
        assert_eq!(ids(&list), ["a"]);
        assert_eq!(feed.error, None);
    }

    #[test]
    fn read_state_counts_only_comments_shown() {
        let mut feed = Feed::default();
        let mut list = Vec::new();
        feed.apply(
            &mut list,
            "c",
            Ok(vec![comment("a", "index.html", CommentStatus::Open, &[])]),
        );
        // The first load is the baseline: nothing is new.
        assert!(feed.unread.is_empty());
        assert!(!feed.has_unread(&list, "index.html"));
        // A refresh brings one comment here and one on another file.
        feed.apply(
            &mut list,
            "c",
            Ok(vec![
                comment("a", "index.html", CommentStatus::Open, &[]),
                comment("b", "index.html", CommentStatus::Open, &[]),
                comment("x", "other.html", CommentStatus::Open, &[]),
            ]),
        );
        assert!(feed.has_unread(&list, "index.html"));
        assert!(feed.has_unread(&list, "other.html"));
        // The tab shows index.html: only its comments are read.
        let shown: Vec<_> = board(&list, "index.html", false)
            .iter()
            .map(|c| c.id.clone())
            .collect();
        // One more comes in after the tab drew its list.
        feed.apply(
            &mut list,
            "c",
            Ok(vec![
                comment("a", "index.html", CommentStatus::Open, &[]),
                comment("b", "index.html", CommentStatus::Open, &[]),
                comment("x", "other.html", CommentStatus::Open, &[]),
                comment("late", "index.html", CommentStatus::Open, &[]),
            ]),
        );
        feed.mark_read(shown.iter().map(String::as_str));
        assert!(!feed.unread.contains("b"));
        assert!(feed.unread.contains("x"), "other file stays unread");
        assert!(feed.unread.contains("late"), "not shown yet stays unread");
        assert!(feed.has_unread(&list, "index.html"));
        // A comment removed elsewhere is no longer counted.
        feed.apply(
            &mut list,
            "c",
            Ok(vec![
                comment("a", "index.html", CommentStatus::Open, &[]),
                comment("late", "index.html", CommentStatus::Open, &[]),
            ]),
        );
        assert!(!feed.unread.contains("x"));
        assert!(feed.unread.contains("late"));
        // The first load of another conversation is its baseline too.
        feed.apply(
            &mut list,
            "d",
            Ok(vec![
                comment("late", "index.html", CommentStatus::Open, &[]),
                comment("y", "index.html", CommentStatus::Open, &[]),
            ]),
        );
        assert!(feed.unread.is_empty());
        // Opening another conversation starts over.
        feed.reset();
        assert_eq!(feed.loaded, None);
        feed.apply(
            &mut list,
            "d",
            Ok(vec![comment("z", "index.html", CommentStatus::Open, &[])]),
        );
        assert!(feed.unread.is_empty());
    }

    #[test]
    fn a_late_answer_for_another_project_or_load_is_dropped() {
        let ticket = |project: &str, conversation: &str, seq| LoadTicket {
            project: project.into(),
            conversation: conversation.into(),
            seq,
        };
        let asked = ticket("p1", "c", 3);
        assert!(is_current(&asked, Some(&ticket("p1", "c", 3))));
        // Same conversation id, but the Studio shows another project now.
        assert!(!is_current(&asked, Some(&ticket("p2", "c", 3))));
        assert!(!is_current(&asked, Some(&ticket("p1", "d", 3))));
        // A newer load of the same list was asked for since.
        assert!(!is_current(&asked, Some(&ticket("p1", "c", 4))));
        // The Studio is gone.
        assert!(!is_current(&asked, None));
    }

    #[test]
    fn the_marker_follows_the_picked_comment() {
        let list = [
            comment("a", "index.html", CommentStatus::Open, &[]),
            comment("b", "index.html", CommentStatus::Open, &[]),
        ];
        let mut b = list[1].clone();
        b.selector = "#cta".into();
        let shown = [&list[0], &b];
        let mut anchors = HashMap::new();
        assert_eq!(marker(&shown, None, &anchors), Marker::Off);
        assert_eq!(
            marker(&shown, Some("b"), &anchors),
            Marker::Outline("#cta".into())
        );
        anchors.insert("b".to_owned(), AnchorState::Stale);
        assert_eq!(marker(&shown, Some("b"), &anchors).selector(), Some("#cta"));
        // Its element was removed from the page: no outline, a fallback.
        anchors.insert("b".to_owned(), AnchorState::Lost);
        assert_eq!(marker(&shown, Some("b"), &anchors), Marker::Missing);
        assert_eq!(Marker::Missing.selector(), None);
        // Another comment's lost anchor does not matter.
        assert_eq!(
            marker(&shown, Some("a"), &anchors),
            Marker::Outline("h1".into())
        );
        // The picked comment is no longer listed (removed, other file).
        assert_eq!(marker(&shown[..1], Some("b"), &anchors), Marker::Off);
    }
}
