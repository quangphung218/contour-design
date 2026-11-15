//! Copies the active skills into `<cwd>/.od-skills/<alias>/` before a run
//! (spec 2.3). Real copies, never symlinks: the agent may only be allowed
//! to read inside its working directory.

use std::io;
use std::path::{Path, PathBuf};

/// Folder under the project that holds staged skills.
pub const STAGED_SKILLS_DIR: &str = ".od-skills";

/// One skill to stage.
#[derive(Debug, Clone)]
pub struct SkillToStage<'a> {
    pub source_dir: &'a Path,
    /// `<folder>-<hash>`, the same alias the prompt preamble names.
    pub alias: &'a str,
    /// `SKILL.md` as the agent should read it (with the preamble).
    pub skill_md: &'a str,
}

/// Stages one skill, replacing an older copy. Symlinks in the source are
/// skipped. Returns the staged folder.
///
/// # Errors
/// I/O errors, or `InvalidInput` for an alias that is not one segment.
pub fn stage_skill(cwd: &Path, skill: &SkillToStage<'_>) -> io::Result<PathBuf> {
    if skill.alias.is_empty() || skill.alias.starts_with('.') || skill.alias.contains(['/', '\\']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bad skill alias",
        ));
    }
    let staged = cwd.join(STAGED_SKILLS_DIR);
    let dest = staged.join(skill.alias);
    // The agent can make the staging folder a link, maybe to a folder
    // outside the project: the link goes, never the folder it points to,
    // and a real folder takes its place. (A skill's folder that is a link
    // goes too: `remove_dir_all` removes a link, not what it points to.)
    remove_link(&staged)?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    copy_tree(skill.source_dir, &dest)?;
    std::fs::write(dest.join("SKILL.md"), skill.skill_md)?;
    Ok(dest)
}

/// Removes `path` when it is a link (the link only).
fn remove_link(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        // A link to a folder is a directory entry on Windows.
        std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent made the staging folder (or a skill's folder in it) a link
    /// to a folder outside the project: staging replaces the link with a
    /// folder of its own and never deletes or writes in the linked one.
    #[cfg(unix)]
    #[test]
    fn a_staging_folder_that_is_a_link_is_replaced_not_written_through() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(src.join("SKILL.md"), "orig").unwrap_or_else(|e| panic!("{e}"));
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(outside.join("deck-abc")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(outside.join("deck-abc/mine.txt"), "mine").unwrap_or_else(|e| panic!("{e}"));
        let cwd = tmp.path().join("proj");
        std::fs::create_dir_all(&cwd).unwrap_or_else(|e| panic!("{e}"));
        let skill = SkillToStage {
            source_dir: &src,
            alias: "deck-abc",
            skill_md: "body",
        };
        std::os::unix::fs::symlink(&outside, cwd.join(STAGED_SKILLS_DIR))
            .unwrap_or_else(|e| panic!("{e}"));
        let dest = stage_skill(&cwd, &skill).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            outside.join("deck-abc/mine.txt").is_file(),
            "deleted outside"
        );
        assert!(
            !outside.join("deck-abc/SKILL.md").exists(),
            "written outside"
        );
        assert!(
            !std::fs::symlink_metadata(cwd.join(STAGED_SKILLS_DIR))
                .unwrap_or_else(|e| panic!("{e}"))
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("SKILL.md")).unwrap_or_default(),
            "body"
        );
        // A skill's own folder a link.
        std::fs::remove_dir_all(&dest).unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(outside.join("deck-abc"), &dest)
            .unwrap_or_else(|e| panic!("{e}"));
        stage_skill(&cwd, &skill).unwrap_or_else(|e| panic!("{e}"));
        assert!(outside.join("deck-abc/mine.txt").is_file());
        assert!(!outside.join("deck-abc/SKILL.md").exists());
        assert!(dest.join("SKILL.md").is_file());
    }

    #[test]
    fn stages_a_real_copy_with_the_given_body() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("assets")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(src.join("SKILL.md"), "orig").unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(src.join("assets/template.html"), "<html>")
            .unwrap_or_else(|e| panic!("{e}"));
        let cwd = tmp.path().join("proj");
        let dest = stage_skill(
            &cwd,
            &SkillToStage {
                source_dir: &src,
                alias: "deck-abc",
                skill_md: "preamble\n\norig",
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(dest, cwd.join(".od-skills/deck-abc"));
        assert_eq!(
            std::fs::read_to_string(dest.join("SKILL.md")).unwrap_or_default(),
            "preamble\n\norig"
        );
        assert!(!dest.join("assets").is_symlink());
        assert!(dest.join("assets/template.html").is_file());
        assert!(
            stage_skill(
                &cwd,
                &SkillToStage {
                    source_dir: &src,
                    alias: "../x",
                    skill_md: ""
                }
            )
            .is_err()
        );
    }
}
