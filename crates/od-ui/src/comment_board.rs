//! The Comments tab's model (spec 1.2): which comments of the shown file
//! it lists, their numbers (shared with the pins), and whether each one's
//! anchor still holds in the preview.

use crate::preview_bridge::Located;
use od_contracts::{CommentStatus, PreviewComment};

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
}
