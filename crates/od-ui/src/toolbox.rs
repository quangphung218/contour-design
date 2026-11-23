//! Design toolbox (track B3-chat): follow-up actions that seed the composer
//! with a request and the bundled skill best suited to it, as the
//! original's `runtime/design-toolbox` catalogue. Pure, tested natively.

use crate::i18n::{Locale, T};

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

/// Where a "Save as skill" turn writes the skill, relative to the project.
pub const SKILL_DIR: &str = "generated-skill";

/// The turn "Save as skill" sends: the agent reads this project's chat and
/// files, writes a reusable skill under [`SKILL_DIR`] and installs it
/// through the daemon (`od skill install`, else `POST /api/skills/install`),
/// as `authoring_prompt` in `od-server` does for plugins.
#[must_use]
pub fn skill_authoring_prompt(project_name: &str, locale: Locale) -> String {
    let name: String = project_name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let name = name.trim();
    let language = locale.native_name();
    let tag = locale.tag();
    format!(
        "# Save this project as a skill\n\n\
         Turn the work in this project (\"{name}\") into a reusable Contour skill, so a \
         later project gets the same result from one sentence.\n\n\
         Read the conversation so far and the project's files. Keep what can be reused: \
         the design rules (colours, fonts, spacing, layout, tone of voice) and the steps \
         that worked. Leave out this project's own content (names, copy, data).\n\n\
         ## What to write\n\n\
         1. `{SKILL_DIR}/<id>/SKILL.md`, where `<id>` is a short kebab-case id \
         (letters, digits and `-`). Front matter: `name` (the id), `description` (one or \
         two sentences: what it makes and when to use it) and `triggers` (3 to 6 short \
         phrases a user would type). Then the body with the sections \"When to use\", \
         \"Design rules\" and \"Process\".\n\
         2. Optional: up to 3 small files the skill needs again (such as `tokens.css` \
         or a logo), each under 1 MB, in `{SKILL_DIR}/<id>/assets/`, named in SKILL.md \
         by their relative path.\n\n\
         Write the description and the body in {language} ({tag}).\n\n\
         ## Install it\n\n\
         Install the folder (absolute path) with `od skill install \"$PWD/{SKILL_DIR}/<id>\"`, \
         or if `od` is not on PATH: `curl -sS -X POST \"$OD_DAEMON_URL/api/skills/install\" \
         -H 'content-type: application/json' -d \"{{\\\"path\\\": \\\"$PWD/{SKILL_DIR}/<id>\\\"}}\"`. \
         If the answer is 409 (a built-in skill has that id), pick another id and install \
         again. Report the skill id when done."
    )
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

    #[test]
    fn the_skill_prompt_names_the_folder_the_install_and_the_language() {
        let p = skill_authoring_prompt("Tea house\nlanding", Locale::Vi);
        assert!(p.contains("\"Tea house landing\""), "{p}");
        assert!(p.contains("generated-skill/<id>/SKILL.md"));
        assert!(p.contains("generated-skill/<id>/assets/"));
        assert!(p.contains("od skill install \"$PWD/generated-skill/<id>\""));
        assert!(
            p.contains("-d \"{\\\"path\\\": \\\"$PWD/generated-skill/<id>\\\"}\""),
            "{p}"
        );
        assert!(p.contains("$OD_DAEMON_URL/api/skills/install"));
        assert!(p.contains("Tiếng Việt (vi)"));
    }
}
