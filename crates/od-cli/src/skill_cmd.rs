//! `od skill`: install a local skill folder into the user skills. "Save as
//! skill" in the Studio has the agent run it on the folder it wrote. There
//! is no command that copies a skill out to an agent CLI: `od` is on every
//! agent's PATH, and that copy is the desktop app's (decision #9).

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
    }
}
