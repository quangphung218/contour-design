//! Preview comments (spec 1.2, table `preview_comments` in 3.3): notes pinned
//! to elements of a previewed file, sent with a later turn as scoped context.

use serde::{Deserialize, Serialize};

/// Where a comment stands in its life cycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentStatus {
    /// Pinned, not queued for a turn.
    Open,
    /// Goes with the next turn.
    #[default]
    Attached,
    /// Sent with a turn.
    Applying,
    Resolved,
}

impl CommentStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Attached => "attached",
            Self::Applying => "applying",
            Self::Resolved => "resolved",
        }
    }

    /// Unknown values read back as [`Self::Open`].
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "attached" => Self::Attached,
            "applying" => Self::Applying,
            "resolved" => Self::Resolved,
            _ => Self::Open,
        }
    }
}

/// Element box in preview viewport pixels when the comment was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CommentRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewComment {
    pub id: String,
    pub project_id: String,
    pub conversation_id: String,
    pub file_path: String,
    /// The element's `data-od-id`, or empty.
    pub element_id: String,
    /// Stable selector from the bridge (`[data-od-id=…] > p:nth-of-type(2)`).
    pub selector: String,
    /// Tag name shown in the UI.
    pub label: String,
    /// The element's text when picked (trimmed).
    pub text: String,
    pub position: CommentRect,
    pub html_hint: String,
    #[serde(default)]
    pub slide_index: Option<u32>,
    pub note: String,
    pub status: CommentStatus,
    /// Images attached to the note (project files), in order.
    #[serde(default)]
    pub attachments: Vec<CommentImage>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `POST /api/projects/:id/conversations/:cid/comments`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentRequest {
    pub file_path: String,
    #[serde(default)]
    pub element_id: String,
    pub selector: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub position: CommentRect,
    #[serde(default)]
    pub html_hint: String,
    #[serde(default)]
    pub slide_index: Option<u32>,
    pub note: String,
    #[serde(default)]
    pub status: Option<CommentStatus>,
    /// Project paths of images to attach (at most
    /// [`MAX_COMMENT_IMAGES`]); each must be an image file in the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
}

/// Most images one comment carries.
pub const MAX_COMMENT_IMAGES: usize = 4;

/// An image attached to a comment: a project file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentImage {
    pub path: String,
    /// File name shown in the UI.
    pub name: String,
}

/// `PATCH …/comments/:commentId`. Absent fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommentRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<CommentStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommentResponse {
    pub comment: PreviewComment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommentsResponse {
    pub comments: Vec<PreviewComment>,
}

/// One comment as carried by `ChatRequest.commentAttachments`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAttachment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 1-based order shown to the agent.
    #[serde(default)]
    pub order: u32,
    pub file_path: String,
    #[serde(default)]
    pub element_id: String,
    pub selector: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub current_text: String,
    #[serde(default)]
    pub page_position: CommentRect,
    #[serde(default)]
    pub html_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide_index: Option<u32>,
    pub comment: String,
    /// Project paths of the images attached to the comment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl CommentAttachment {
    /// The attachment for `comment`, numbered `order` (1-based).
    #[must_use]
    pub fn from_comment(comment: &PreviewComment, order: u32) -> Self {
        Self {
            id: Some(comment.id.clone()),
            order,
            file_path: comment.file_path.clone(),
            element_id: comment.element_id.clone(),
            selector: comment.selector.clone(),
            label: comment.label.clone(),
            current_text: comment.text.clone(),
            page_position: comment.position,
            html_hint: comment.html_hint.clone(),
            slide_index: comment.slide_index,
            comment: comment.note.clone(),
            images: comment.attachments.iter().map(|a| a.path.clone()).collect(),
        }
    }
}
