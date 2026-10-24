//! `od research search`: shallow Tavily research through the daemon
//! (`POST /api/research/search`), for agents that need sources before
//! they design. The key is the one in Settings ▸ Media.

use crate::client::Client;
use clap::Subcommand;
use serde_json::{Value, json};

#[derive(Debug, Subcommand)]
pub enum ResearchCommand {
    /// Search the web; prints the summary and numbered sources.
    Search {
        #[arg(long)]
        query: String,
        /// 1 to 20 sources.
        #[arg(long, default_value_t = 5)]
        max_sources: u32,
    },
}

pub fn run(client: &Client, cmd: ResearchCommand, json_out: bool) -> anyhow::Result<()> {
    match cmd {
        ResearchCommand::Search { query, max_sources } => {
            let res: Value = client.send_json(
                "POST",
                &["api", "research", "search"],
                &json!({ "query": query, "maxSources": max_sources }),
            )?;
            let findings = &res["findings"];
            if json_out {
                println!("{}", serde_json::to_string_pretty(findings)?);
                return Ok(());
            }
            println!("{}", findings["summary"].as_str().unwrap_or_default());
            for (i, s) in findings["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                println!(
                    "[{}] {} {}",
                    i + 1,
                    s["title"].as_str().unwrap_or_default(),
                    s["url"].as_str().unwrap_or_default()
                );
            }
            Ok(())
        }
    }
}
