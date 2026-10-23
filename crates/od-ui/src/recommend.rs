//! Home ▸ "Recommended for you" (track C2-chat-home, the original's
//! `onboarding/recommendation.ts`): starters picked from what the user
//! made so far. The original reads its "About you" answers; this app has
//! no such form, so the recent projects' kinds pick the bucket. Pure,
//! tested natively.

use crate::i18n::T;
use crate::intent::Chip;
use od_contracts::ProjectKind;

/// A starter: what the strip shows and what "Start creating" fills in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Starter {
    pub key: &'static str,
    pub title: T,
    pub body: T,
    pub prompt: T,
    pub chip: Chip,
}

const PROTOTYPE: Starter = Starter {
    key: "prototype",
    title: T::RecPrototype,
    body: T::RecPrototypeBody,
    prompt: T::RecPrototypePrompt,
    chip: Chip::Prototype,
};
const COMPONENT: Starter = Starter {
    key: "component",
    title: T::RecComponent,
    body: T::RecComponentBody,
    prompt: T::RecComponentPrompt,
    chip: Chip::Prototype,
};
const LOWFI: Starter = Starter {
    key: "lowfi",
    title: T::RecLowfi,
    body: T::RecLowfiBody,
    prompt: T::RecLowfiPrompt,
    chip: Chip::Wireframe,
};
const LANDING: Starter = Starter {
    key: "landing",
    title: T::RecLanding,
    body: T::RecLandingBody,
    prompt: T::RecLandingPrompt,
    chip: Chip::Prototype,
};
const MULTIVARIANT: Starter = Starter {
    key: "multivariant",
    title: T::RecVariants,
    body: T::RecVariantsBody,
    prompt: T::RecVariantsPrompt,
    chip: Chip::Prototype,
};
const DASHBOARD: Starter = Starter {
    key: "dashboard",
    title: T::RecDashboard,
    body: T::RecDashboardBody,
    prompt: T::RecDashboardPrompt,
    chip: Chip::Prototype,
};
const REPORT: Starter = Starter {
    key: "report",
    title: T::RecReport,
    body: T::RecReportBody,
    prompt: T::RecReportPrompt,
    chip: Chip::Document,
};
const GENERAL: Starter = Starter {
    key: "general",
    title: T::RecGeneral,
    body: T::RecGeneralBody,
    prompt: T::RecGeneralPrompt,
    chip: Chip::Prototype,
};

/// The original's buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    ProductUi,
    Marketing,
    InternalTool,
    General,
}

impl Bucket {
    #[must_use]
    pub fn starters(self) -> &'static [Starter] {
        match self {
            Self::ProductUi => &[PROTOTYPE, COMPONENT, LOWFI],
            Self::Marketing => &[LANDING, MULTIVARIANT],
            Self::InternalTool => &[DASHBOARD, REPORT],
            Self::General => &[GENERAL],
        }
    }
}

/// How many recent projects vote.
pub const RECENT: usize = 10;

/// The bucket the recent projects (newest first) point at; the newest
/// wins a tie. No projects: `General`.
#[must_use]
pub fn bucket(kinds: &[ProjectKind]) -> Bucket {
    let of = |k: ProjectKind| match k {
        ProjectKind::Prototype | ProjectKind::Template => Bucket::ProductUi,
        ProjectKind::Deck | ProjectKind::Image | ProjectKind::Video | ProjectKind::Audio => {
            Bucket::Marketing
        }
        ProjectKind::Other => Bucket::InternalTool,
    };
    let recent = &kinds[..kinds.len().min(RECENT)];
    let mut best: Option<(Bucket, usize)> = None;
    for k in recent {
        let b = of(*k);
        let n = recent.iter().filter(|x| of(**x) == b).count();
        if best.is_none_or(|(_, m)| n > m) {
            best = Some((b, n));
        }
    }
    best.map_or(Bucket::General, |(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_kinds_pick_the_bucket() {
        assert_eq!(bucket(&[]), Bucket::General);
        assert_eq!(
            bucket(&[
                ProjectKind::Deck,
                ProjectKind::Prototype,
                ProjectKind::Prototype
            ]),
            Bucket::ProductUi
        );
        // A tie goes to the newest.
        assert_eq!(
            bucket(&[ProjectKind::Other, ProjectKind::Deck]),
            Bucket::InternalTool
        );
        // Only the ten newest vote.
        let mut kinds = vec![ProjectKind::Image; 10];
        kinds.extend([ProjectKind::Prototype; 20]);
        assert_eq!(bucket(&kinds), Bucket::Marketing);
    }

    #[test]
    fn every_bucket_has_starters_with_distinct_keys() {
        for b in [
            Bucket::ProductUi,
            Bucket::Marketing,
            Bucket::InternalTool,
            Bucket::General,
        ] {
            let s = b.starters();
            assert!(!s.is_empty());
            let mut keys: Vec<&str> = s.iter().map(|x| x.key).collect();
            keys.dedup();
            assert_eq!(keys.len(), s.len());
        }
    }
}
