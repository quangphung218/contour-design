//! `od skill`: install a local skill folder into the user skills. "Save as
//! skill" in the Studio has the agent run it on the folder it wrote.

use crate::client::Client;
use anyhow::Context;
use clap::Subcommand;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Install a folder with a `SKILL.md` (and side files such as
    /// `assets/`) into the user skills; replaces a user skill with the
    /// same id.
    Install { dir: PathBuf },
}

pub fn run(client: &Client, cmd: SkillCommand, json_out: bool) -> anyhow::Result<()> {
    match cmd {
        SkillCommand::Install { dir } => {
            let dir = std::fs::canonicalize(&dir)
                .with_context(|| format!("{} does not exist", dir.display()))?;
            let res: Value = client.send_json(
                "POST",
                &["api", "skills", "install"],
                &json!({ "path": dir.to_string_lossy() }),
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
