//! A skill's Save (`user_content::save_skill`) in each hazard an agent can
//! leave in `<data>/skills` (`od_fs::hostile`): it never waits on a FIFO,
//! never changes a byte outside the root, never leaves a temp file, never
//! writes through a link at `SKILL.md` and never into a name alike in
//! another case on a disk that keeps case.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::user_content::{SkillDraft, WriteError, save_skill};
use od_fs::hostile::{Hazard, Hostile, finishes};
use std::path::PathBuf;

fn draft() -> SkillDraft {
    SkillDraft {
        name: "tea".to_owned(),
        description: "Brews tea.".to_owned(),
        triggers: Vec::new(),
        body: "Saved body.".to_owned(),
    }
}

#[test]
fn a_skill_save_holds_up_in_every_hazard() {
    for hazard in Hazard::ALL {
        let h = Hostile::new();
        h.plant(hazard);
        let outside = Hostile::snapshot(&h.outside());
        let own = Hostile::snapshot(&h.own());
        let twin = Hostile::snapshot(&h.path().join("Tea"));
        let (root, tea) = (h.path().to_path_buf(), h.path().join("tea"));
        let Some(saved) = finishes(move || save_skill(&root, "tea", &tea, true, &draft(), None))
        else {
            h.release_fifo();
            panic!("{hazard:?}: still running after 3 s");
        };
        let at = format!("{hazard:?}: {saved:?}");
        assert_eq!(
            Hostile::snapshot(&h.outside()),
            outside,
            "{at}: wrote outside the root"
        );
        assert_eq!(
            h.temp_files(),
            Vec::<PathBuf>::new(),
            "{at}: left a temp file"
        );
        if hazard != Hazard::LinkedEntry {
            // Only the user's own linked entry is the skill saved.
            assert_eq!(
                Hostile::snapshot(&h.own()),
                own,
                "{at}: wrote the user's own folder"
            );
        }
        if hazard == Hazard::CaseTwin && !h.folds_case() {
            assert_eq!(
                Hostile::snapshot(&h.path().join("Tea")),
                twin,
                "{at}: reached the twin"
            );
        }
        let skill_md = h.path().join("tea/SKILL.md");
        match hazard {
            // A link or FIFO at the file is replaced, never written through.
            Hazard::LinkAtFile | Hazard::Fifo => {
                assert!(saved.is_ok(), "{at}");
                assert!(
                    std::fs::symlink_metadata(&skill_md).unwrap().is_file(),
                    "{at}"
                );
                assert!(
                    std::fs::read_to_string(&skill_md)
                        .unwrap()
                        .contains("Saved body."),
                    "{at}"
                );
            }
            // A read-only folder keeps its file, whole.
            Hazard::ReadOnly => {
                assert!(matches!(saved, Err(WriteError::Io(_))), "{at}");
                assert_eq!(std::fs::read(&skill_md).unwrap(), b"tea", "{at}");
            }
            // The user's own linked skill is saved where it is.
            Hazard::LinkedEntry => {
                assert!(saved.is_ok(), "{at}");
                let text = std::fs::read_to_string(h.own().join("SKILL.md")).unwrap();
                assert!(text.contains("Saved body."), "{at}");
            }
            _ => {}
        }
    }
}
