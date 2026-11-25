//! `od skill`: install a local skill folder into the user skills ("Save as
//! skill" in the Studio has the agent run it on the folder it wrote), or
//! copy a Personal skill out to an agent CLI (`export --to claude`).

use crate::client::Client;
use anyhow::Context;
use clap::Subcommand;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Install a folder with a `SKILL.md` (and side files such as
    /// `assets/`) into the user skills. A user skill with the same id is
    /// kept (error 409) unless `--replace` is given.
    Install {
        dir: PathBuf,
        /// Replace the user skill with the same id.
        #[arg(long)]
        replace: bool,
    },
    /// Copy a Personal skill (its `SKILL.md` and side files) into
    /// `~/.claude/skills/<id>` for Claude Code. A folder of that name there
    /// is never written over (error 409).
    Export {
        id: String,
        /// The agent CLI to copy it to.
        #[arg(long, value_parser = ["claude"])]
        to: String,
    },
}

pub fn run(client: &Client, cmd: SkillCommand, json_out: bool) -> anyhow::Result<()> {
    match cmd {
        SkillCommand::Install { dir, replace } => {
            let dir = std::fs::canonicalize(&dir)
                .with_context(|| format!("{} does not exist", dir.display()))?;
            let res: Value = client.send_json(
                "POST",
                &["api", "skills", "install"],
                &json!({ "path": dir.to_string_lossy(), "replace": replace }),
            )?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&res)?);
                return Ok(());
            }
            println!("installed {}", res["id"].as_str().unwrap_or_default());
            Ok(())
        }
        SkillCommand::Export { id, to } => {
            let res: Value = client.send_json(
                "POST",
                &["api", "skills", &id, "export"],
                &json!({ "target": to }),
            )?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&res)?);
                return Ok(());
            }
            if res["contourTools"] == true {
                eprintln!(
                    "warning: This skill calls Contour tools; it may not work outside Contour"
                );
            }
            println!("copied to {}", res["path"].as_str().unwrap_or_default());
            Ok(())
        }
    }
}
