//! Home composer logic: chip → project kind and default skill, project
//! naming, and the create request (spec 1.1).

use crate::i18n::T;
use od_contracts::{CreateProjectRequest, ProjectKind, ProjectMetadata, SkillMode, SkillSummary};
use serde_json::{Map, Value, json};

/// Longest generated project name, in characters.
const NAME_CHARS: usize = 60;

/// Output-type chips under the brief box (the original's `home-hero/chips`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chip {
    Prototype,
    WebClone,
    Wireframe,
    MobileApp,
    Deck,
    Document,
    Image,
    Video,
    Audio,
    Motion,
    HyperFrames,
    WebGl,
    LiveArtifact,
}

/// What the Home media picker asks for a chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaSpec {
    pub aspect: bool,
    pub resolution: bool,
    /// Duration choices in seconds; empty: no duration.
    pub durations: &'static [u32],
    /// Speech, sound effect.
    pub audio_kind: bool,
    /// Media model of this surface (`/api/media/models`).
    pub model: Option<&'static str>,
    /// Free style notes.
    pub notes: bool,
}

pub const ASPECTS: [&str; 5] = ["1:1", "16:9", "9:16", "4:3", "3:4"];
pub const RESOLUTIONS: [&str; 2] = ["2k", "4k"];
pub const AUDIO_KINDS: [&str; 2] = ["speech", "sfx"];
const VIDEO_SECONDS: &[u32] = &[3, 5, 8, 10, 15, 30];
const AUDIO_SECONDS: &[u32] = &[5, 10, 15, 30, 60, 120];
/// Sound effects stay short (the original caps them at 30 s).
const SFX_MAX_SECONDS: u32 = 30;

impl Chip {
    pub const ALL: [Self; 13] = [
        Self::Prototype,
        Self::Deck,
        Self::Motion,
        Self::WebClone,
        Self::Wireframe,
        Self::MobileApp,
        Self::Document,
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::HyperFrames,
        Self::WebGl,
        Self::LiveArtifact,
    ];
    /// Shown in the row; the rest sit under "More".
    pub const PRIMARY: [Self; 3] = [Self::Prototype, Self::Deck, Self::Motion];

    #[must_use]
    pub fn mode(self) -> SkillMode {
        match self {
            Self::Prototype
            | Self::WebClone
            | Self::Wireframe
            | Self::MobileApp
            | Self::Document
            | Self::WebGl
            | Self::LiveArtifact => SkillMode::Prototype,
            Self::Deck => SkillMode::Deck,
            Self::Image => SkillMode::Image,
            Self::Video | Self::Motion | Self::HyperFrames => SkillMode::Video,
            Self::Audio => SkillMode::Audio,
        }
    }

    #[must_use]
    pub fn kind(self) -> ProjectKind {
        match self.mode() {
            SkillMode::Deck => ProjectKind::Deck,
            SkillMode::Image => ProjectKind::Image,
            SkillMode::Video => ProjectKind::Video,
            SkillMode::Audio => ProjectKind::Audio,
            _ if self == Self::Document => ProjectKind::Other,
            _ => ProjectKind::Prototype,
        }
    }

    /// Stable id: `default_for` value in skills, `data-chip` in the UI.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Prototype => "prototype",
            Self::WebClone => "web-clone",
            Self::Wireframe => "wireframe",
            Self::MobileApp => "mobile-app",
            Self::Deck => "deck",
            Self::Document => "document",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Motion => "motion-design",
            Self::HyperFrames => "hyperframes",
            Self::WebGl => "webgl",
            Self::LiveArtifact => "live-artifact",
        }
    }

    /// Bundled skills or templates the chip runs, first found wins.
    fn preferred(self) -> &'static [&'static str] {
        match self {
            Self::Prototype | Self::Deck | Self::Video => &[],
            Self::WebClone => &["web-clone"],
            Self::Wireframe => &["wireframe-sketch", "wireframe-greybox"],
            Self::MobileApp => &["mobile-app", "wireframe-mobile-flow"],
            Self::Document => &["doc", "docs-page"],
            Self::Image => &["imagegen", "imagen"],
            Self::Audio => &["speech", "venice-audio-speech"],
            Self::Motion => &["motion-design", "video-hyperframes"],
            Self::HyperFrames => &["video-hyperframes"],
            Self::WebGl => &["webgl-experience"],
            Self::LiveArtifact => &["live-artifact", "live-dashboard"],
        }
    }

    /// `metadata.intent`, as the original sets it.
    #[must_use]
    pub fn intent(self) -> Option<&'static str> {
        match self {
            Self::WebClone => Some("web-clone"),
            Self::Wireframe => Some("wireframe"),
            Self::MobileApp => Some("mobile-app"),
            Self::Document => Some("document"),
            Self::Motion => Some("motion-design"),
            Self::WebGl => Some("webgl-experience"),
            Self::LiveArtifact => Some("live-artifact"),
            _ => None,
        }
    }

    fn fidelity(self) -> Option<&'static str> {
        match self {
            Self::WebClone => Some("high-fidelity"),
            Self::Wireframe => Some("wireframe"),
            _ => None,
        }
    }

    fn platform(self) -> Option<&'static str> {
        (self == Self::MobileApp).then_some("mobile")
    }

    /// The media picker's fields; `None` for non-media chips.
    #[must_use]
    pub fn media(self) -> Option<MediaSpec> {
        let none = MediaSpec {
            aspect: false,
            resolution: false,
            durations: &[],
            audio_kind: false,
            model: None,
            notes: false,
        };
        match self {
            Self::Image => Some(MediaSpec {
                aspect: true,
                resolution: true,
                model: Some("image"),
                notes: true,
                ..none
            }),
            Self::Video => Some(MediaSpec {
                aspect: true,
                resolution: true,
                durations: VIDEO_SECONDS,
                model: Some("video"),
                ..none
            }),
            Self::Motion | Self::HyperFrames => Some(MediaSpec {
                aspect: true,
                durations: VIDEO_SECONDS,
                ..none
            }),
            Self::Audio => Some(MediaSpec {
                durations: AUDIO_SECONDS,
                audio_kind: true,
                model: Some("audio"),
                ..none
            }),
            _ => None,
        }
    }
}

/// The media picker's choices; `None` fields were not asked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaOptions {
    pub aspect: Option<String>,
    pub resolution: Option<String>,
    pub duration: Option<u32>,
    pub audio_kind: Option<String>,
    /// Media model id; `None` lets the agent pick.
    pub model: Option<String>,
    pub notes: Option<String>,
}

impl MediaOptions {
    /// The original's defaults for `chip`.
    #[must_use]
    pub fn defaults(chip: Chip) -> Self {
        let Some(spec) = chip.media() else {
            return Self::default();
        };
        let duration = match chip {
            Chip::Video => Some(5),
            Chip::Audio | Chip::Motion | Chip::HyperFrames => Some(10),
            _ => None,
        };
        Self {
            aspect: spec.aspect.then(|| "16:9".to_owned()),
            resolution: spec.resolution.then(|| "2k".to_owned()),
            duration: duration.filter(|_| !spec.durations.is_empty()),
            audio_kind: spec.audio_kind.then(|| "speech".to_owned()),
            model: None,
            notes: None,
        }
    }

    /// Durations offered for `chip` with these options (sound effects are
    /// capped at 30 s).
    #[must_use]
    pub fn durations(&self, chip: Chip) -> Vec<u32> {
        let all = chip.media().map_or(&[][..], |m| m.durations);
        let sfx = self.audio_kind.as_deref() == Some("sfx");
        all.iter()
            .copied()
            .filter(|&d| !sfx || d <= SFX_MAX_SECONDS)
            .collect()
    }

    /// Switches the audio kind, pulling the duration under the sound
    /// effect cap.
    pub fn set_audio_kind(&mut self, kind: &str) {
        self.audio_kind = Some(kind.to_owned());
        if kind == "sfx" {
            self.duration = self.duration.map(|d| d.min(SFX_MAX_SECONDS));
        }
    }

    /// Writes the choices into `metadata.extra` under the original's keys
    /// (`imageAspect`, `videoLength`, `audioDuration`…).
    fn write(&self, chip: Chip, extra: &mut Map<String, Value>) {
        let (aspect, resolution, length, model) = match chip.kind() {
            ProjectKind::Image => ("imageAspect", "imageResolution", None, "imageModel"),
            ProjectKind::Video => (
                "videoAspect",
                "videoResolution",
                Some("videoLength"),
                "videoModel",
            ),
            ProjectKind::Audio => ("", "", Some("audioDuration"), "audioModel"),
            _ => return,
        };
        let text = |v: &Option<String>| {
            v.as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        if let Some(m) = text(&self.model) {
            extra.insert(model.to_owned(), json!(m));
        }
        if let Some(n) = text(&self.notes).filter(|_| chip.kind() == ProjectKind::Image) {
            extra.insert("imageStyle".to_owned(), json!(n));
        }
        if let Some(a) = self.aspect.as_ref().filter(|_| !aspect.is_empty()) {
            extra.insert(aspect.to_owned(), json!(a));
        }
        if let Some(r) = self.resolution.as_ref().filter(|_| !resolution.is_empty()) {
            extra.insert(resolution.to_owned(), json!(r));
        }
        if let (Some(key), Some(d)) = (length, self.duration) {
            extra.insert(key.to_owned(), json!(d));
        }
        if let Some(k) = &self.audio_kind {
            extra.insert("audioKind".to_owned(), json!(k));
        }
        if matches!(chip, Chip::Motion | Chip::HyperFrames) {
            extra.insert("videoModel".to_owned(), json!("hyperframes-html"));
        }
    }
}

/// Placeholder carousel: example briefs and the chip each belongs to (the
/// original's `placeholderScenarios`).
pub const SCENARIOS: [(T, Chip); 13] = [
    (T::ScnOnePageBrief, Chip::Document),
    (T::ScnNotesToDeck, Chip::Deck),
    (T::ScnSignupFlow, Chip::Prototype),
    (T::ScnImproveBrief, Chip::Document),
    (T::ScnLoadingAnimation, Chip::HyperFrames),
    (T::ScnTeamUpdateSlides, Chip::Deck),
    (T::ScnOrdersDashboard, Chip::Prototype),
    (T::ScnProductDetail, Chip::Wireframe),
    (T::ScnCaseStudy, Chip::Document),
    (T::ScnLandingIntro, Chip::Prototype),
    (T::ScnPitchDeck, Chip::Deck),
    (T::ScnAppIdea, Chip::MobileApp),
    (T::ScnLandingLayout, Chip::Wireframe),
];

/// Scenarios the carousel cycles for `chip`: its own, else its example
/// prompts, else all of them.
#[must_use]
pub fn scenarios_for(chip: Option<Chip>) -> Vec<T> {
    let Some(chip) = chip else {
        return SCENARIOS.iter().map(|(k, _)| *k).collect();
    };
    let own: Vec<T> = SCENARIOS
        .iter()
        .filter(|(_, c)| *c == chip)
        .map(|(k, _)| *k)
        .collect();
    if !own.is_empty() {
        return own;
    }
    if !chip.examples().is_empty() {
        return chip.examples().to_vec();
    }
    SCENARIOS.iter().map(|(k, _)| *k).collect()
}

impl Chip {
    /// Example prompts under the composer (the original's
    /// `HOME_PROMPT_EXAMPLES`).
    #[must_use]
    pub fn examples(self) -> &'static [T] {
        match self {
            Self::Prototype => &[T::ExPrototype1, T::ExPrototype2, T::ExPrototype3],
            Self::WebClone => &[T::ExWebClone1],
            Self::Wireframe => &[T::ExWireframe1, T::ExWireframe2, T::ExWireframe3],
            Self::MobileApp => &[T::ExMobile1, T::ExMobile2, T::ExMobile3],
            Self::Deck => &[T::ExDeck1, T::ExDeck2, T::ExDeck3],
            Self::Document => &[T::ExDocument1, T::ExDocument2, T::ExDocument3],
            Self::Image => &[T::ExImage1, T::ExImage2, T::ExImage3],
            Self::Video => &[T::ExVideo1, T::ExVideo2, T::ExVideo3],
            Self::Audio => &[T::ExAudio1, T::ExAudio2, T::ExAudio3],
            Self::Motion => &[T::ExMotion1, T::ExMotion2],
            Self::HyperFrames => &[T::ExHyperFrames1, T::ExHyperFrames2],
            Self::WebGl => &[T::ExWebGl1, T::ExWebGl2],
            Self::LiveArtifact => &[T::ExLive1, T::ExLive2],
        }
    }

    /// The chip's name in the UI.
    #[must_use]
    pub fn label_key(self) -> T {
        match self {
            Self::Prototype => T::ChipPrototype,
            Self::WebClone => T::ChipWebClone,
            Self::Wireframe => T::ChipWireframe,
            Self::MobileApp => T::ChipMobileApp,
            Self::Deck => T::ChipDeck,
            Self::Document => T::ChipDocument,
            Self::Image => T::ChipImage,
            Self::Video => T::ChipVideo,
            Self::Audio => T::ChipAudio,
            Self::Motion => T::ChipMotion,
            Self::HyperFrames => T::ChipHyperFrames,
            Self::WebGl => T::ChipWebGl,
            Self::LiveArtifact => T::ChipLiveArtifact,
        }
    }
}

/// Recent working folders kept in settings (`recentLinkedDirs`).
pub const RECENT_DIRS: usize = 5;

/// Puts `dir` first in `list`, without duplicates, at most [`RECENT_DIRS`].
pub fn push_recent_dir(list: &mut Vec<String>, dir: &str) {
    list.retain(|d| d != dir);
    list.insert(0, dir.to_owned());
    list.truncate(RECENT_DIRS);
}

/// The next character count of a typewriter placeholder: types `target` in,
/// holds, then deletes it. `step` is the frame number since the start;
/// returns `None` once the cycle is over (next scenario).
#[must_use]
pub fn typewriter(target: &str, step: usize) -> Option<usize> {
    const HOLD: usize = 45;
    let n = target.chars().count();
    match step {
        s if s <= n => Some(s),
        s if s <= n + HOLD => Some(n),
        s if s <= 2 * n + HOLD => Some(2 * n + HOLD - s),
        _ => None,
    }
}

/// The skill a chip runs when no template is picked: the chip's own
/// bundled skill or template, else one declaring `default_for` the chip,
/// else the most featured skill of that mode, else
/// the first by id.
#[must_use]
pub fn default_skill(chip: Chip, skills: &[SkillSummary]) -> Option<String> {
    let of_mode = || skills.iter().filter(|s| s.info.mode == chip.mode());
    if let Some(id) = chip
        .preferred()
        .iter()
        .find(|id| skills.iter().any(|s| s.info.id == **id))
    {
        return Some((*id).to_owned());
    }
    if let Some(s) = skills
        .iter()
        .find(|s| s.info.default_for.iter().any(|d| d == chip.key()))
    {
        return Some(s.info.id.clone());
    }
    of_mode()
        .filter(|s| s.info.featured.is_some())
        .min_by(|a, b| {
            let (fa, fb) = (
                a.info.featured.unwrap_or(0.0),
                b.info.featured.unwrap_or(0.0),
            );
            fa.total_cmp(&fb).then_with(|| a.info.id.cmp(&b.info.id))
        })
        .or_else(|| of_mode().min_by(|a, b| a.info.id.cmp(&b.info.id)))
        .map(|s| s.info.id.clone())
}

/// Templates offered for a chip (same mode), featured first, then by name.
#[must_use]
pub fn templates_for(chip: Chip, templates: &[SkillSummary]) -> Vec<SkillSummary> {
    let mut out: Vec<SkillSummary> = templates
        .iter()
        .filter(|t| t.info.mode == chip.mode())
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        let fa = a.info.featured.unwrap_or(f64::MAX);
        let fb = b.info.featured.unwrap_or(f64::MAX);
        fa.total_cmp(&fb)
            .then_with(|| a.info.name.cmp(&b.info.name))
    });
    out
}

/// Project name from the brief's first non-empty line.
#[must_use]
pub fn project_name(brief: &str) -> String {
    let line = brief
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Untitled");
    if line.chars().count() <= NAME_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(NAME_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Everything the Home composer collected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeIntent {
    pub brief: String,
    pub chip: Chip,
    pub design_system_id: Option<String>,
    /// A picked template replaces the chip's default skill.
    pub template_id: Option<String>,
    pub media: MediaOptions,
    /// Folders the agent may read (`metadata.linkedDirs`), already checked.
    pub linked_dirs: Vec<String>,
}

impl HomeIntent {
    /// `POST /api/projects` body. The brief rides along as `pendingPrompt`;
    /// the Studio sends it as the first turn.
    #[must_use]
    pub fn create_request(&self, skills: &[SkillSummary]) -> CreateProjectRequest {
        CreateProjectRequest {
            name: project_name(&self.brief),
            skill_id: self
                .template_id
                .clone()
                .or_else(|| default_skill(self.chip, skills)),
            design_system_id: self.design_system_id.clone(),
            pending_prompt: Some(self.brief.trim().to_owned()).filter(|p| !p.is_empty()),
            metadata: Some(self.metadata()),
            ..CreateProjectRequest::default()
        }
    }

    fn metadata(&self) -> ProjectMetadata {
        let mut extra = Map::new();
        self.media.write(self.chip, &mut extra);
        ProjectMetadata {
            kind: Some(self.chip.kind()),
            intent: self.chip.intent().map(str::to_owned),
            fidelity: self.chip.fidelity().map(str::to_owned),
            platform: self.chip.platform().map(str::to_owned),
            linked_dirs: self.linked_dirs.clone(),
            extra,
            ..ProjectMetadata::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn skill(id: &str, mode: &str, featured: Option<f64>, default_for: &[&str]) -> SkillSummary {
        serde_json::from_value(json!({
            "id": id, "name": id, "description": "", "triggers": [], "mode": mode,
            "surface": "web", "source": "built-in", "craftRequires": [], "platform": null,
            "scenario": "", "category": null, "previewType": "html",
            "designSystemRequired": false, "defaultFor": default_for, "upstream": null,
            "featured": featured, "fidelity": null, "speakerNotes": null, "animations": null,
            "examplePrompt": "", "aggregatesExamples": false, "critiquePolicy": null,
            "hasBody": true
        }))
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn default_skill_prefers_default_for_then_featured() {
        let skills = vec![
            skill("b-proto", "prototype", None, &[]),
            skill("a-proto", "prototype", None, &[]),
            skill("feat-proto", "prototype", Some(2.0), &[]),
            skill("top-proto", "prototype", Some(1.0), &[]),
            skill("my-deck", "deck", None, &["deck"]),
        ];
        assert_eq!(
            default_skill(Chip::Prototype, &skills).as_deref(),
            Some("top-proto")
        );
        assert_eq!(
            default_skill(Chip::Deck, &skills).as_deref(),
            Some("my-deck")
        );
        let plain = vec![
            skill("b", "prototype", None, &[]),
            skill("a", "prototype", None, &[]),
        ];
        assert_eq!(default_skill(Chip::Prototype, &plain).as_deref(), Some("a"));
        assert_eq!(default_skill(Chip::Deck, &plain), None);
    }

    #[test]
    fn names_from_brief() {
        assert_eq!(
            project_name("\n  A landing page for tea\nmore"),
            "A landing page for tea"
        );
        assert_eq!(project_name("   "), "Untitled");
        let long = "word ".repeat(30);
        let name = project_name(&long);
        assert_eq!(name.chars().count(), NAME_CHARS);
        assert!(name.ends_with('…'));
    }

    #[test]
    fn template_replaces_default_skill() {
        let skills = vec![skill("proto", "prototype", None, &[])];
        let mut intent = HomeIntent {
            brief: "Tea shop".into(),
            chip: Chip::Prototype,
            design_system_id: Some("ds1".into()),
            template_id: None,
            media: MediaOptions::default(),
            linked_dirs: Vec::new(),
        };
        let req = intent.create_request(&skills);
        assert_eq!(req.skill_id.as_deref(), Some("proto"));
        assert_eq!(req.pending_prompt.as_deref(), Some("Tea shop"));
        assert_eq!(
            req.metadata.and_then(|m| m.kind),
            Some(ProjectKind::Prototype)
        );
        intent.template_id = Some("tpl".into());
        assert_eq!(
            intent.create_request(&skills).skill_id.as_deref(),
            Some("tpl")
        );
    }

    #[test]
    fn chips_pick_their_own_skill_and_metadata() {
        let skills = vec![
            skill("proto", "prototype", None, &[]),
            skill("wireframe-sketch", "prototype", None, &[]),
            skill("video-hyperframes", "video", None, &[]),
        ];
        assert_eq!(
            default_skill(Chip::Wireframe, &skills).as_deref(),
            Some("wireframe-sketch")
        );
        // Missing preferred skill: the mode's default.
        assert_eq!(
            default_skill(Chip::WebGl, &skills).as_deref(),
            Some("proto")
        );
        let intent = HomeIntent {
            brief: "Logo sting".into(),
            chip: Chip::Motion,
            design_system_id: None,
            template_id: None,
            media: MediaOptions::defaults(Chip::Motion),
            linked_dirs: vec!["/work/site".into()],
        };
        let req = intent.create_request(&skills);
        assert_eq!(req.skill_id.as_deref(), Some("video-hyperframes"));
        let meta = req.metadata.unwrap_or_default();
        assert_eq!(meta.kind, Some(ProjectKind::Video));
        assert_eq!(meta.intent.as_deref(), Some("motion-design"));
        assert_eq!(meta.linked_dirs, ["/work/site"]);
        assert_eq!(
            Value::Object(meta.extra),
            json!({ "videoAspect": "16:9", "videoLength": 10, "videoModel": "hyperframes-html" })
        );
        let doc = HomeIntent {
            chip: Chip::Document,
            media: MediaOptions::defaults(Chip::Document),
            ..intent
        };
        let meta = doc.create_request(&skills).metadata.unwrap_or_default();
        assert_eq!(meta.kind, Some(ProjectKind::Other));
        assert!(meta.extra.is_empty());
    }

    #[test]
    fn media_defaults_and_sound_effect_cap() {
        assert_eq!(
            MediaOptions::defaults(Chip::Prototype),
            MediaOptions::default()
        );
        let image = MediaOptions::defaults(Chip::Image);
        assert_eq!(image.aspect.as_deref(), Some("16:9"));
        assert_eq!(image.resolution.as_deref(), Some("2k"));
        assert_eq!(image.duration, None);
        assert_eq!(MediaOptions::defaults(Chip::Video).duration, Some(5));
        let mut audio = MediaOptions::defaults(Chip::Audio);
        audio.duration = Some(120);
        assert!(audio.durations(Chip::Audio).contains(&120));
        audio.set_audio_kind("sfx");
        assert_eq!(audio.duration, Some(30));
        assert_eq!(audio.durations(Chip::Audio), [5, 10, 15, 30]);
        let mut extra = Map::new();
        audio.write(Chip::Audio, &mut extra);
        let mut image = MediaOptions::defaults(Chip::Image);
        image.model = Some("gpt-image-2".into());
        image.notes = Some("  soft light ".into());
        let mut img = Map::new();
        image.write(Chip::Image, &mut img);
        assert_eq!(
            Value::Object(img),
            json!({ "imageAspect": "16:9", "imageResolution": "2k", "imageModel": "gpt-image-2", "imageStyle": "soft light" })
        );
        assert_eq!(
            Value::Object(extra),
            json!({ "audioDuration": 30, "audioKind": "sfx" })
        );
    }

    #[test]
    fn every_chip_has_a_unique_key_and_primary_chips_come_first() {
        let keys: std::collections::HashSet<_> = Chip::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(keys.len(), Chip::ALL.len());
        assert_eq!(Chip::ALL[..3], Chip::PRIMARY);
    }

    #[test]
    fn carousel_follows_the_chip() {
        assert_eq!(scenarios_for(None).len(), SCENARIOS.len());
        assert_eq!(
            scenarios_for(Some(Chip::Deck)),
            [T::ScnNotesToDeck, T::ScnTeamUpdateSlides, T::ScnPitchDeck]
        );
        assert_eq!(scenarios_for(Some(Chip::Audio)), Chip::Audio.examples());
        assert!(Chip::ALL.iter().all(|c| !c.examples().is_empty()));
        assert_eq!(typewriter("abc", 0), Some(0));
        assert_eq!(typewriter("abc", 3), Some(3));
        assert_eq!(typewriter("abc", 40), Some(3));
        assert_eq!(typewriter("abc", 50), Some(1));
        assert_eq!(typewriter("abc", 52), None);
    }

    #[test]
    fn recent_dirs_dedupe_and_cap() {
        let mut list: Vec<String> = (0..5).map(|i| format!("/d{i}")).collect();
        push_recent_dir(&mut list, "/d3");
        assert_eq!(list, ["/d3", "/d0", "/d1", "/d2", "/d4"]);
        push_recent_dir(&mut list, "/new");
        assert_eq!(list.len(), RECENT_DIRS);
        assert_eq!(list[0], "/new");
        assert!(!list.contains(&"/d4".to_owned()));
    }

    #[test]
    fn templates_filtered_and_sorted() {
        let t = vec![
            skill("z", "deck", None, &[]),
            skill("y", "prototype", None, &[]),
            skill("x", "deck", Some(1.0), &[]),
        ];
        let ids: Vec<_> = templates_for(Chip::Deck, &t)
            .into_iter()
            .map(|s| s.info.id)
            .collect();
        assert_eq!(ids, vec!["x", "z"]);
    }
}
