//! Shared pieces of the management screens: run status of a project
//! (`GET /api/project-status`), the type tag, sorting and filters of the
//! project lists, and small `localStorage` helpers.

use super::api;
use super::icons::Glyph;
use crate::i18n::{T, t};
use od_contracts::{Project, ProjectKind};
use serde::Deserialize;
use std::collections::BTreeMap;

/// A project's run state, as the original's `project.status.value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RunState {
    NotStarted,
    Running,
    AwaitingInput,
    Incomplete,
    Succeeded,
    Failed,
    Canceled,
}

impl RunState {
    /// Board column order.
    pub const ALL: [Self; 7] = [
        Self::NotStarted,
        Self::Running,
        Self::AwaitingInput,
        Self::Incomplete,
        Self::Succeeded,
        Self::Failed,
        Self::Canceled,
    ];

    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "running" | "queued" => Self::Running,
            "awaiting_input" => Self::AwaitingInput,
            "incomplete" => Self::Incomplete,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "canceled" => Self::Canceled,
            _ => Self::NotStarted,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Running => "running",
            Self::AwaitingInput => "awaiting_input",
            Self::Incomplete => "incomplete",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        t(match self {
            Self::NotStarted => T::StatusNotStarted,
            Self::Running => T::StatusRunning,
            Self::AwaitingInput => T::StatusAwaitingInput,
            Self::Incomplete => T::StatusIncomplete,
            Self::Succeeded => T::StatusSucceeded,
            Self::Failed => T::StatusFailed,
            Self::Canceled => T::StatusCanceled,
        })
    }

    /// Badge variant: state colours only for real states.
    #[must_use]
    pub fn badge_class(self) -> &'static str {
        match self {
            Self::Running => "badge accent",
            Self::AwaitingInput | Self::Incomplete => "badge warning",
            Self::Succeeded => "badge success",
            Self::Failed => "badge danger",
            Self::NotStarted | Self::Canceled => "badge outline",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusEntry {
    value: String,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    statuses: BTreeMap<String, StatusEntry>,
}

/// Every project's state; empty when the daemon does not answer.
pub async fn fetch_statuses() -> BTreeMap<String, RunState> {
    api::get::<StatusResponse>("/api/project-status")
        .await
        .map(|r| {
            r.statuses
                .into_iter()
                .map(|(id, s)| (id, RunState::parse(&s.value)))
                .collect()
        })
        .unwrap_or_default()
}

/// The type tag of a card, first match wins (the original's order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    LiveArtifact,
    WebClone,
    Slide,
    Media,
    Prototype,
    Other,
}

impl Tag {
    pub const ALL: [Self; 6] = [
        Self::Prototype,
        Self::Slide,
        Self::LiveArtifact,
        Self::WebClone,
        Self::Media,
        Self::Other,
    ];

    #[must_use]
    pub fn of(project: &Project) -> Self {
        let meta = project.metadata.as_ref();
        let intent = meta.and_then(|m| m.intent.as_deref());
        if intent == Some("live-artifact") || project.skill_id.as_deref() == Some("live-artifact") {
            return Self::LiveArtifact;
        }
        if intent == Some("web-clone") {
            return Self::WebClone;
        }
        match meta.and_then(|m| m.kind) {
            Some(ProjectKind::Deck) => Self::Slide,
            Some(ProjectKind::Image | ProjectKind::Video | ProjectKind::Audio) => Self::Media,
            Some(ProjectKind::Other) => Self::Other,
            _ => Self::Prototype,
        }
    }

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::LiveArtifact => "live-artifact",
            Self::WebClone => "web-clone",
            Self::Slide => "slide",
            Self::Media => "media",
            Self::Prototype => "prototype",
            Self::Other => "other",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == s)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        t(match self {
            Self::LiveArtifact => T::TagLiveArtifact,
            Self::WebClone => T::TagWebClone,
            Self::Slide => T::TagSlide,
            Self::Media => T::TagMedia,
            Self::Prototype => T::TagPrototype,
            Self::Other => T::TagOther,
        })
    }

    #[must_use]
    pub fn glyph(self) -> Glyph {
        match self {
            Self::LiveArtifact => Glyph::LiveArtifact,
            Self::WebClone => Glyph::Globe,
            Self::Slide => Glyph::Deck,
            Self::Media => Glyph::FileImage,
            Self::Prototype | Self::Other => Glyph::Prototype,
        }
    }
}

/// Project list order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Newest,
    Oldest,
    Name,
}

impl Sort {
    pub const ALL: [Self; 3] = [Self::Newest, Self::Oldest, Self::Name];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Newest => "updated-desc",
            Self::Oldest => "updated-asc",
            Self::Name => "name-asc",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.id() == s)
            .unwrap_or(Self::Newest)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        t(match self {
            Self::Newest => T::SortNewest,
            Self::Oldest => T::SortOldest,
            Self::Name => T::SortName,
        })
    }

    pub fn apply(self, list: &mut [Project]) {
        match self {
            Self::Newest => list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
            Self::Oldest => list.sort_by(|a, b| a.updated_at.cmp(&b.updated_at)),
            Self::Name => list.sort_by_key(|p| p.name.to_lowercase()),
        }
    }
}

/// `list` narrowed to `tag` (all with `None`) and to names containing
/// `query` (case-insensitive), then sorted.
#[must_use]
pub fn filtered(list: &[Project], tag: Option<Tag>, query: &str, sort: Sort) -> Vec<Project> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<Project> = list
        .iter()
        .filter(|p| tag.is_none_or(|t| Tag::of(p) == t))
        .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
        .cloned()
        .collect();
    sort.apply(&mut out);
    out
}

pub fn stored(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(key)
        .ok()?
}

pub fn store(key: &str, value: &str) {
    if let Some(Ok(Some(s))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = s.set_item(key, value);
    }
}

/// Local date and time of epoch milliseconds, in the UI language.
#[must_use]
pub fn local_time(ms: i64) -> String {
    #[allow(clippy::cast_precision_loss)] // epoch ms fit f64 exactly
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    date.to_locale_string(
        crate::i18n::locale().tag(),
        &wasm_bindgen::JsValue::UNDEFINED,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(name: &str, updated: i64, meta: serde_json::Value) -> Project {
        serde_json::from_value(json!({
            "id": name, "name": name, "skillId": null, "designSystemId": null,
            "metadata": meta, "createdAt": 1, "updatedAt": updated
        }))
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn tags_follow_the_original_precedence() {
        assert_eq!(
            Tag::of(&project("a", 1, json!({"kind": "deck"}))),
            Tag::Slide
        );
        assert_eq!(
            Tag::of(&project(
                "a",
                1,
                json!({"kind": "deck", "intent": "live-artifact"})
            )),
            Tag::LiveArtifact
        );
        assert_eq!(
            Tag::of(&project("a", 1, json!({"kind": "video"}))),
            Tag::Media
        );
        assert_eq!(
            Tag::of(&project("a", 1, json!({"intent": "web-clone"}))),
            Tag::WebClone
        );
        assert_eq!(Tag::of(&project("a", 1, json!(null))), Tag::Prototype);
    }

    #[test]
    fn filter_and_sort() {
        let list = vec![
            project("Tea shop", 3, json!({"kind": "prototype"})),
            project("pitch", 1, json!({"kind": "deck"})),
            project("Bakery", 2, json!({"kind": "prototype"})),
        ];
        let names = |v: Vec<Project>| v.into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(
            names(filtered(&list, None, "", Sort::Newest)),
            ["Tea shop", "Bakery", "pitch"]
        );
        assert_eq!(
            names(filtered(&list, None, "", Sort::Oldest)),
            ["pitch", "Bakery", "Tea shop"]
        );
        assert_eq!(
            names(filtered(&list, None, "", Sort::Name)),
            ["Bakery", "pitch", "Tea shop"]
        );
        assert_eq!(
            names(filtered(&list, Some(Tag::Slide), "", Sort::Name)),
            ["pitch"]
        );
        assert_eq!(
            names(filtered(&list, None, "TEA", Sort::Name)),
            ["Tea shop"]
        );
        assert_eq!(RunState::parse("queued"), RunState::Running);
        assert_eq!(RunState::parse("?"), RunState::NotStarted);
    }
}
