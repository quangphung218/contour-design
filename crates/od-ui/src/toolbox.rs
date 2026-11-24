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

/// Whether a turn wrote files only inside [`SKILL_DIR`]: a "Save as
/// skill" turn, which is not meant to touch the design's entry page.
#[must_use]
pub fn wrote_only_a_skill(written: &[String]) -> bool {
    !written.is_empty()
        && written.iter().all(|f| {
            f.strip_prefix(SKILL_DIR)
                .is_some_and(|rest| rest.starts_with('/'))
        })
}

/// The ids of the skills a turn wrote, in the order it wrote them: `<id>`
/// of each `SKILL_DIR/<id>/SKILL.md`. A Save as skill turn that found its
/// id taken writes again under a new one (`-2`), so the last one is the
/// skill it installed.
#[must_use]
pub fn written_skill_ids(written: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in written {
        let Some(id) = f
            .strip_prefix(SKILL_DIR)
            .and_then(|rest| rest.strip_prefix('/'))
            .and_then(|rest| rest.strip_suffix("/SKILL.md"))
            .filter(|id| !id.is_empty() && !id.contains('/'))
        else {
            continue;
        };
        out.retain(|x| x != id);
        out.push(id.to_owned());
    }
    out
}

/// The first line of [`skill_authoring_prompt`], whatever the language.
const SKILL_PROMPT_HEAD: &str = "# Save this project as a skill\n";

/// Whether `request` (a prepared turn's, `Message.prepared_message`) is
/// the one "Save as skill" sends.
#[must_use]
pub fn is_skill_request(request: &str) -> bool {
    request.starts_with(SKILL_PROMPT_HEAD)
}

/// Whether the next-step card offers "Save as skill" (a chip, besides the
/// "+" menu item): after a turn that wrote a page, once the conversation
/// has `succeeded` two turns or more, until a Save as skill turn has
/// succeeded in it (`saved`) or the user dismissed the chip.
#[must_use]
pub fn offer_skill_save(wrote_page: bool, succeeded: usize, saved: bool, dismissed: bool) -> bool {
    wrote_page && succeeded >= 2 && !saved && !dismissed
}

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
        "{SKILL_PROMPT_HEAD}\n\
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
         If the answer is 409, a built-in skill or another of the user's skills has that \
         id: never replace it. List the ids taken with `curl -sS \"$OD_DAEMON_URL/api/skills\"`, \
         pick one not in the list by adding `-2`, `-3`… to yours, rename the folder and \
         `name` to match, and install again. Only to install again the skill this turn \
         installed itself, add `--replace` to `od skill install`, or `\\\"replace\\\": true` \
         to the curl body. Report the skill id when done."
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
    fn the_skill_ids_written_come_in_the_order_written() {
        let v = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            written_skill_ids(&v(&[
                "generated-skill/tea/SKILL.md",
                "generated-skill/tea/assets/tokens.css",
                "generated-skill/tea-2/SKILL.md",
                "index.html",
            ])),
            v(&["tea", "tea-2"])
        );
        // Written again last: it is the last one.
        assert_eq!(
            written_skill_ids(&v(&[
                "generated-skill/a/SKILL.md",
                "generated-skill/b/SKILL.md",
                "generated-skill/a/SKILL.md",
            ])),
            v(&["b", "a"])
        );
        for none in [
            "generated-skill/SKILL.md",
            "generated-skill//SKILL.md",
            "generated-skill/a/b/SKILL.md",
            "generated-skill-old/a/SKILL.md",
            "x/generated-skill/a/SKILL.md",
        ] {
            assert!(written_skill_ids(&v(&[none])).is_empty(), "{none}");
        }
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
        // A taken id gets a new one; replacing is only for this turn's own.
        assert!(p.contains("curl -sS \"$OD_DAEMON_URL/api/skills\""), "{p}");
        assert!(p.contains("`-2`, `-3`"));
        assert!(p.contains("never replace it"));
        assert!(
            p.contains("--replace") && p.contains("\\\"replace\\\": true"),
            "{p}"
        );
    }

    #[test]
    fn the_skill_request_is_known_by_its_head_in_any_language() {
        for locale in [Locale::En, Locale::Vi, Locale::Ja] {
            assert!(is_skill_request(&skill_authoring_prompt("Tea", locale)));
        }
        assert!(!is_skill_request("Save as skill"));
        assert!(!is_skill_request("make a tea house landing"));
        assert!(!is_skill_request(""));
    }

    #[test]
    fn the_chip_shows_from_the_second_page_turn_until_saved_or_dismissed() {
        assert!(offer_skill_save(true, 2, false, false));
        assert!(offer_skill_save(true, 5, false, false));
        assert!(!offer_skill_save(true, 1, false, false), "one turn");
        assert!(!offer_skill_save(false, 2, false, false), "no page");
        assert!(!offer_skill_save(true, 2, true, false), "saved once");
        assert!(!offer_skill_save(true, 2, false, true), "dismissed");
    }

    #[test]
    fn only_a_turn_that_wrote_inside_the_skill_folder_is_a_skill_turn() {
        let v = |fs: &[&str]| fs.iter().map(|f| (*f).to_owned()).collect::<Vec<_>>();
        assert!(wrote_only_a_skill(&v(&[
            "generated-skill/tea/SKILL.md",
            "generated-skill/tea/assets/tokens.css"
        ])));
        assert!(!wrote_only_a_skill(&v(&[])));
        assert!(!wrote_only_a_skill(&v(&[
            "generated-skill/tea/SKILL.md",
            "index.html"
        ])));
        assert!(!wrote_only_a_skill(&v(&["generated-skill-old/x.md"])));
        assert!(!wrote_only_a_skill(&v(&["generated-skill"])));
    }
}
