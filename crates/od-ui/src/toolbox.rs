//! Design toolbox (track B3-chat): follow-up actions that seed the composer
//! with a request and the bundled skill best suited to it, as the
//! original's `runtime/design-toolbox` catalogue. Pure, tested natively.

use crate::i18n::T;

/// One follow-up action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    pub id: &'static str,
    pub title: T,
    /// The request it puts in the composer.
    pub prompt: T,
    /// Skills to mention, first bundled one wins.
    pub skills: &'static [&'static str],
}

pub const ACTIONS: [Action; 8] = [
    Action {
        id: "auto-match",
        title: T::ToolMatch,
        prompt: T::ToolMatchPrompt,
        skills: &["creative-director", "frontend-design"],
    },
    Action {
        id: "icon-workflow",
        title: T::ToolIcons,
        prompt: T::ToolIconsPrompt,
        skills: &["imagegen-frontend-web", "imagegen", "frontend-design"],
    },
    Action {
        id: "motion",
        title: T::ToolMotion,
        prompt: T::ToolMotionPrompt,
        skills: &[
            "emilkowalski-motion",
            "gsap-react",
            "gsap-core",
            "motion-design",
        ],
    },
    Action {
        id: "anti-ai-polish",
        title: T::ToolTaste,
        prompt: T::ToolTastePrompt,
        skills: &["taste-skill", "gpt-tasteskill", "impeccable-design-polish"],
    },
    Action {
        id: "visual-polish",
        title: T::ToolPolish,
        prompt: T::ToolPolishPrompt,
        skills: &["impeccable-design-polish", "frontend-design"],
    },
    Action {
        id: "image-gen",
        title: T::ToolImages,
        prompt: T::ToolImagesPrompt,
        skills: &["imagegen-frontend-web", "imagegen"],
    },
    Action {
        id: "chart-gen",
        title: T::ToolCharts,
        prompt: T::ToolChartsPrompt,
        skills: &["d3-visualization", "frame-data-chart-nyt"],
    },
    Action {
        id: "threejs-scene",
        title: T::Tool3d,
        prompt: T::Tool3dPrompt,
        skills: &["threejs"],
    },
];

/// Shown on the next-step card of a finished turn (the original's
/// featured rows).
pub const FEATURED: [&str; 2] = ["motion", "visual-polish"];

/// The skill `action` mentions among the bundled `available` ones.
#[must_use]
pub fn skill_for<'a>(action: &Action, available: &'a [String]) -> Option<&'a str> {
    action
        .skills
        .iter()
        .find_map(|id| available.iter().find(|a| a == id).map(String::as_str))
}

#[must_use]
pub fn find(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_bundled_skill_wins() {
        let motion = find("motion").unwrap_or_else(|| panic!("motion"));
        let have = vec!["gsap-core".to_owned(), "emilkowalski-motion".to_owned()];
        assert_eq!(skill_for(motion, &have), Some("emilkowalski-motion"));
        assert_eq!(skill_for(motion, &[]), None);
        assert!(FEATURED.iter().all(|f| find(f).is_some()));
        let ids: std::collections::HashSet<_> = ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), ACTIONS.len());
    }
}
