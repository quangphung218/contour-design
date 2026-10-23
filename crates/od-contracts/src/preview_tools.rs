//! Preview tools (spec 1.2): artifact lint, edits made in the preview and
//! written back to the source, and the deliverable check after a run.

use crate::{FileKind, FileVersion, ProjectFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// P0 must be fixed, P1 should be, P2 is nice to have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LintSeverity {
    P0,
    P1,
    P2,
}

/// One anti-slop finding in an HTML artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintFinding {
    pub severity: LintSeverity,
    /// Stable rule id (`purple-gradient`, `emoji-icon`…).
    pub id: String,
    pub message: String,
    pub fix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// `POST /api/artifacts/lint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintRequest {
    pub html: String,
}

/// Lint results, plus the same findings as a block for the agent's next
/// turn (empty when there are none).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintResponse {
    pub findings: Vec<LintFinding>,
    pub agent_message: String,
}

/// One change made in the preview, addressed by the bridge's selector.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEdit {
    pub selector: String,
    /// Inline style properties to set (`Some`) or remove (`None`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub style: BTreeMap<String, Option<String>>,
    /// New text of a text-only element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The element's text as the preview showed it before the edit, without
    /// the code of nested scripts and styles. The source element must still
    /// read the same, or the edit is refused. A style edit may send only the
    /// start of a long text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_text: Option<String>,
    /// The element's tag as the preview showed it; the source element must
    /// have it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_tag: Option<String>,
    /// Attributes to set (`Some`) or remove (`None`), as manual edit's
    /// Attributes tab, link and image fields write them. `data-od-id`
    /// cannot change (edits and comments find elements by it).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Option<String>>,
    /// Replacement HTML for the whole element, with exactly one root
    /// element (manual edit's Html tab). Alone in its edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_html: Option<String>,
    /// Removes the element. Alone in its edit; the page's last visible
    /// element and `html`/`head`/`body` stay.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove: bool,
}

/// `POST /api/projects/:id/source-edits`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEditRequest {
    pub file: String,
    pub edits: Vec<SourceEdit>,
    /// Version label; defaults to "Edited in preview".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The file after the edits, and the manual version that records them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEditResponse {
    pub file: ProjectFile,
    pub version: FileVersion,
    /// The version that holds the source from before the edits (undo
    /// restores it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version_id: Option<String>,
}

/// `POST /api/projects/:id/source-element`: one element as the source
/// has it, for manual edit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceElementRequest {
    pub file: String,
    pub selector: String,
    /// As in [`SourceEdit`]: the read is refused when the source element
    /// is not the one the preview showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_text: Option<String>,
}

/// One attribute, in source order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAttribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceElementResponse {
    pub tag: String,
    pub attributes: Vec<SourceAttribute>,
    /// The element's source bytes, start tag to end tag.
    pub outer_html: String,
    /// Its text (entities decoded, code left out).
    pub text: String,
    /// No child elements: its text can be edited in place.
    pub text_only: bool,
}

/// Outcome of the deliverable check (spec 1.2, P1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliverableValidation {
    Valid,
    /// The run did not succeed (yet).
    NotSucceeded,
    /// The run wrote no file.
    NoArtifact,
    ProjectMissing,
    /// No entry file could be told apart.
    EntryMissing,
    /// The entry exists but this run did not write it.
    EntryNotTouched,
    EntryUnreadable,
    /// The entry is not a kind this project delivers.
    TypeMismatch,
}

/// `GET /api/runs/:id/deliverable`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverableResponse {
    pub valid: bool,
    pub validation: DeliverableValidation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<FileKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shapes() {
        let edit: SourceEdit = serde_json::from_str(
            r#"{"selector":"h1","style":{"color":"red","margin":null},"expectText":"Hi"}"#,
        )
        .unwrap();
        assert_eq!(edit.style["color"].as_deref(), Some("red"));
        assert_eq!(edit.style["margin"], None);
        assert_eq!(edit.expect_text.as_deref(), Some("Hi"));
        let lint = LintResponse {
            findings: vec![LintFinding {
                severity: LintSeverity::P0,
                id: "x".into(),
                message: "m".into(),
                fix: "f".into(),
                snippet: None,
            }],
            agent_message: String::new(),
        };
        let v = serde_json::to_value(&lint).unwrap();
        assert_eq!(v["findings"][0]["severity"], "P0");
        assert!(v.get("agentMessage").is_some());
        assert_eq!(
            serde_json::to_value(DeliverableValidation::EntryNotTouched).unwrap(),
            "entry_not_touched"
        );
    }
}
