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
    let dest = cwd.join(STAGED_SKILLS_DIR).join(skill.alias);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    copy_tree(skill.source_dir, &dest)?;
    std::fs::write(dest.join("SKILL.md"), skill.skill_md)?;
    Ok(dest)
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
