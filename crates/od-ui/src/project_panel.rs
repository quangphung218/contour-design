//! The new-project panel's choices and the create request they make
//! (the original's `NewProjectPanel` `buildMetadata`).

use crate::intent::{Chip, default_skill};
use od_contracts::{CreateProjectRequest, ProjectKind, ProjectMetadata, SkillSummary};
use serde_json::{Map, Value, json};

/// Panel tabs, in the original's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    Prototype,
    LiveArtifact,
    Deck,
    Template,
    Media,
    Other,
}

impl PanelTab {
    pub const ALL: [Self; 6] = [
        Self::Prototype,
        Self::LiveArtifact,
        Self::Deck,
        Self::Template,
        Self::Media,
        Self::Other,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Prototype => "prototype",
            Self::LiveArtifact => "live-artifact",
            Self::Deck => "deck",
            Self::Template => "template",
            Self::Media => "media",
            Self::Other => "other",
        }
    }

    /// Tabs that pick target platforms.
    #[must_use]
    pub fn has_platforms(self) -> bool {
        matches!(
            self,
            Self::Prototype | Self::LiveArtifact | Self::Template | Self::Other
        )
    }
}

/// Target platforms, `responsive` first (the default).
pub const PLATFORMS: [&str; 6] = [
    "responsive",
    "web-desktop",
    "mobile-ios",
    "mobile-android",
    "tablet",
    "desktop-app",
];

/// Platforms with OS widgets (home screen, lock screen).
const WIDGET_PLATFORMS: [&str; 3] = ["mobile-ios", "mobile-android", "tablet"];

pub const MEDIA_ASPECTS: [&str; 5] = ["1:1", "16:9", "9:16", "4:3", "3:4"];
pub const VIDEO_LENGTHS: [u32; 6] = [3, 5, 8, 10, 15, 30];
pub const AUDIO_DURATIONS: [u32; 6] = [5, 10, 15, 30, 60, 120];
/// Sound effects are capped at 30 s.
pub const SFX_MAX_SECONDS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSurface {
    Image,
    Video,
    Audio,
}

impl MediaSurface {
    pub const ALL: [Self; 3] = [Self::Image, Self::Video, Self::Audio];

    /// The catalogue's `surface` value.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    #[must_use]
    pub fn kind(self) -> ProjectKind {
        match self {
            Self::Image => ProjectKind::Image,
            Self::Video => ProjectKind::Video,
            Self::Audio => ProjectKind::Audio,
        }
    }
}

/// Everything the panel collected.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelChoice {
    pub tab: PanelTab,
    /// Typed name; empty generates "{tab label} · {date}".
    pub name: String,
    pub design_system_id: Option<String>,
    pub platforms: Vec<String>,
    pub include_landing_page: bool,
    pub include_os_widgets: bool,
    /// `wireframe` or `high-fidelity`.
    pub fidelity: String,
    pub speaker_notes: bool,
    pub animations: bool,
    pub template_id: Option<String>,
    pub template_label: Option<String>,
    pub surface: MediaSurface,
    /// A media model id from `GET /api/media/models`; none = default.
    pub media_model: Option<String>,
    pub aspect: String,
    pub video_length: u32,
    /// `speech` or `sfx`.
    pub audio_kind: String,
    pub audio_duration: u32,
}

impl Default for PanelChoice {
    fn default() -> Self {
        Self {
            tab: PanelTab::Prototype,
            name: String::new(),
            design_system_id: None,
            platforms: vec!["responsive".to_owned()],
            include_landing_page: false,
            include_os_widgets: false,
            fidelity: "high-fidelity".to_owned(),
            speaker_notes: false,
            animations: false,
            template_id: None,
            template_label: None,
            surface: MediaSurface::Image,
            media_model: None,
            aspect: "1:1".to_owned(),
            video_length: 5,
            audio_kind: "speech".to_owned(),
            audio_duration: 10,
        }
    }
}

impl PanelChoice {
    /// `POST /api/projects` body. `generated_name` is used when no name was
    /// typed; no pending prompt (the user writes the first turn).
    #[must_use]
    pub fn create_request(
        &self,
        skills: &[SkillSummary],
        generated_name: &str,
    ) -> CreateProjectRequest {
        let typed = self.name.trim();
        let mut extra = Map::new();
        extra.insert(
            "nameSource".to_owned(),
            json!(if typed.is_empty() {
                "generated"
            } else {
                "user"
            }),
        );
        let mut meta = ProjectMetadata::default();
        if self.tab.has_platforms() {
            let platforms: Vec<&str> = PLATFORMS
                .iter()
                .copied()
                .filter(|p| self.platforms.iter().any(|c| c == p))
                .collect();
            let platforms = if platforms.is_empty() {
                vec!["responsive"]
            } else {
                platforms
            };
            meta.platform = Some(platforms[0].to_owned());
            extra.insert("platformTargets".to_owned(), json!(platforms));
            if self.include_landing_page {
                extra.insert("includeLandingPage".to_owned(), Value::Bool(true));
            }
            let widgets = platforms.iter().any(|p| WIDGET_PLATFORMS.contains(p));
            if self.include_os_widgets && widgets {
                extra.insert("includeOsWidgets".to_owned(), Value::Bool(true));
            }
        }
        let skill = match self.tab {
            PanelTab::Prototype => {
                meta.kind = Some(ProjectKind::Prototype);
                meta.fidelity = Some(self.fidelity.clone());
                default_skill(Chip::Prototype, skills)
            }
            PanelTab::LiveArtifact => {
                meta.kind = Some(ProjectKind::Prototype);
                meta.intent = Some("live-artifact".to_owned());
                meta.fidelity = Some("high-fidelity".to_owned());
                skills
                    .iter()
                    .find(|s| s.info.id == "live-artifact")
                    .map(|s| s.info.id.clone())
            }
            PanelTab::Deck => {
                meta.kind = Some(ProjectKind::Deck);
                extra.insert("speakerNotes".to_owned(), Value::Bool(self.speaker_notes));
                default_skill(Chip::Deck, skills)
            }
            PanelTab::Template => {
                meta.kind = Some(ProjectKind::Template);
                extra.insert("animations".to_owned(), Value::Bool(self.animations));
                if let Some(id) = &self.template_id {
                    extra.insert("templateId".to_owned(), json!(id));
                }
                extra.insert(
                    "templateLabel".to_owned(),
                    json!(self.template_label.as_deref().unwrap_or("Saved template")),
                );
                self.template_id.clone()
            }
            PanelTab::Media => {
                meta.kind = Some(self.surface.kind());
                if let Some(model) = &self.media_model {
                    let key = match self.surface {
                        MediaSurface::Image => "imageModel",
                        MediaSurface::Video => "videoModel",
                        MediaSurface::Audio => "audioModel",
                    };
                    extra.insert(key.to_owned(), json!(model));
                }
                match self.surface {
                    MediaSurface::Image => {
                        extra.insert("imageAspect".to_owned(), json!(self.aspect));
                    }
                    MediaSurface::Video => {
                        extra.insert("videoAspect".to_owned(), json!(self.aspect));
                        extra.insert("videoLength".to_owned(), json!(self.video_length));
                    }
                    MediaSurface::Audio => {
                        let sfx = self.audio_kind == "sfx";
                        extra.insert(
                            "audioKind".to_owned(),
                            json!(if sfx { "sfx" } else { "speech" }),
                        );
                        let duration = if sfx {
                            self.audio_duration.min(SFX_MAX_SECONDS)
                        } else {
                            self.audio_duration
                        };
                        extra.insert("audioDuration".to_owned(), json!(duration));
                    }
                }
                None
            }
            PanelTab::Other => {
                meta.kind = Some(ProjectKind::Other);
                None
            }
        };
        meta.extra = extra;
        CreateProjectRequest {
            name: if typed.is_empty() {
                generated_name.to_owned()
            } else {
                typed.to_owned()
            },
            skill_id: skill,
            design_system_id: self.design_system_id.clone(),
            metadata: Some(meta),
            ..CreateProjectRequest::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(req: &CreateProjectRequest) -> Value {
        serde_json::to_value(req.metadata.as_ref()).unwrap_or(Value::Null)
    }

    #[test]
    fn prototype_keeps_platform_order_and_drops_widgets_without_devices() {
        let choice = PanelChoice {
            platforms: vec!["tablet".into(), "responsive".into()],
            include_os_widgets: true,
            include_landing_page: true,
            fidelity: "wireframe".into(),
            ..PanelChoice::default()
        };
        let req = choice.create_request(&[], "Prototype · Oct 23");
        let m = meta(&req);
        assert_eq!(req.name, "Prototype · Oct 23");
        assert_eq!(m["kind"], "prototype");
        assert_eq!(m["platform"], "responsive");
        assert_eq!(m["platformTargets"], json!(["responsive", "tablet"]));
        assert_eq!(m["includeOsWidgets"], true);
        assert_eq!(m["fidelity"], "wireframe");
        assert_eq!(m["nameSource"], "generated");
        let web = PanelChoice {
            include_os_widgets: true,
            ..PanelChoice::default()
        };
        assert_eq!(
            meta(&web.create_request(&[], "x")).get("includeOsWidgets"),
            None
        );
    }

    #[test]
    fn deck_template_media_and_live_artifact() {
        let deck = PanelChoice {
            tab: PanelTab::Deck,
            speaker_notes: true,
            name: " Pitch ".into(),
            ..PanelChoice::default()
        };
        let req = deck.create_request(&[], "x");
        assert_eq!(req.name, "Pitch");
        let m = meta(&req);
        assert_eq!(
            (m["kind"].clone(), m["speakerNotes"].clone()),
            (json!("deck"), json!(true))
        );
        assert!(m.get("platformTargets").is_none());
        let tpl = PanelChoice {
            tab: PanelTab::Template,
            animations: true,
            template_id: Some("kami-deck".into()),
            ..PanelChoice::default()
        };
        let req = tpl.create_request(&[], "x");
        assert_eq!(req.skill_id.as_deref(), Some("kami-deck"));
        assert_eq!(meta(&req)["templateLabel"], "Saved template");
        let sfx = PanelChoice {
            tab: PanelTab::Media,
            surface: MediaSurface::Audio,
            audio_kind: "sfx".into(),
            audio_duration: 120,
            media_model: Some("eleven-sfx".into()),
            ..PanelChoice::default()
        };
        let m = meta(&sfx.create_request(&[], "x"));
        assert_eq!(
            (m["kind"].clone(), m["audioDuration"].clone()),
            (json!("audio"), json!(30))
        );
        assert_eq!(m["audioModel"], "eleven-sfx");
        let live = PanelChoice {
            tab: PanelTab::LiveArtifact,
            ..PanelChoice::default()
        };
        let m = meta(&live.create_request(&[], "x"));
        assert_eq!(
            (m["intent"].clone(), m["fidelity"].clone()),
            (json!("live-artifact"), json!("high-fidelity"))
        );
    }
}
