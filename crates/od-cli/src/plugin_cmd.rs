//! `od plugin` (spec 2.6): list, info, apply and validate plugins. `apply`
//! enforces the headless capability gate (plugins-spec §9.1): a plugin
//! that is not first-party and needs capabilities beyond `prompt:inject`
//! that were not passed with `--grant-caps` exits with code 66 and a JSON
//! error on stderr.

use crate::client::Client;
use crate::exit::Exit;
use anyhow::Context;
use clap::Subcommand;
use od_content::plugin_validate::{missing_capabilities, validate_plugin_dir};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// Exit code for missing capabilities.
pub const EXIT_CAPABILITIES: u8 = 66;

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List plugins (hidden ones with --all).
    List {
        /// scenario, atom, skill or bundle.
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Show one plugin, its inputs and pipeline.
    Info { id: String },
    /// Fill a plugin's inputs and print the brief and prompt block a run gets.
    Apply {
        id: String,
        /// `name=value`, repeatable.
        #[arg(long = "input", short = 'i')]
        inputs: Vec<String>,
        #[arg(long)]
        locale: Option<String>,
        /// Capabilities granted for this apply, comma-separated.
        #[arg(long, value_delimiter = ',')]
        grant_caps: Vec<String>,
    },
    /// Check a plugin folder before installing it.
    Validate { dir: PathBuf },
    /// The first-party atom catalogue.
    Atoms,
}

fn pretty(v: &Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn parse_inputs(raw: &[String]) -> anyhow::Result<Map<String, Value>> {
    raw.iter()
        .map(|kv| {
            let (k, v) = kv
                .split_once('=')
                .with_context(|| format!("--input {kv:?} must be name=value"))?;
            Ok((k.trim().to_owned(), Value::String(v.to_owned())))
        })
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub fn run(client: &Client, cmd: PluginCommand, json_out: bool) -> anyhow::Result<()> {
    match cmd {
        PluginCommand::List { kind, all } => {
            let mut query: Vec<(&str, &str)> = Vec::new();
            if let Some(k) = kind.as_deref() {
                query.push(("kind", k));
            }
            if all {
                query.push(("includeHidden", "1"));
            }
            let res: Value = client.get_json_query(&["api", "plugins"], &query)?;
            if json_out {
                return pretty(&res);
            }
            for p in res["plugins"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}\t{}",
                    p["id"].as_str().unwrap_or(""),
                    p["kind"].as_str().unwrap_or(""),
                    p["version"].as_str().unwrap_or(""),
                    p["title"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        PluginCommand::Info { id } => {
            let res: Value = client.get_json(&["api", "plugins", &id])?;
            let pipeline: Value = client
                .get_json(&["api", "plugins", &id, "pipeline"])
                .unwrap_or(Value::Null);
            if json_out {
                let mut out = res;
                out["pipeline"] = pipeline["pipeline"].clone();
                return pretty(&out);
            }
            let p = &res["plugin"];
            println!("id:           {}", p["id"].as_str().unwrap_or(""));
            println!("title:        {}", p["title"].as_str().unwrap_or(""));
            println!("version:      {}", p["version"].as_str().unwrap_or(""));
            println!("kind:         {}", p["kind"].as_str().unwrap_or(""));
            println!("source:       {}", p["source"].as_str().unwrap_or(""));
            println!("capabilities: {}", strings(&p["capabilities"]).join(", "));
            for input in p["inputs"].as_array().into_iter().flatten() {
                let required = if input["required"] == true { " (required)" } else { "" };
                println!("input:        {}{required}", input["name"].as_str().unwrap_or(""));
            }
            for (i, s) in pipeline["pipeline"]["stages"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let until = s["until"]
                    .as_str()
                    .map(|u| format!(" until {u}"))
                    .unwrap_or_default();
                println!(
                    "stage {}:      {} [{}]{until}",
                    i + 1,
                    s["id"].as_str().unwrap_or(""),
                    strings(&s["atoms"]).join(", ")
                );
            }
            Ok(())
        }
        PluginCommand::Apply {
            id,
            inputs,
            locale,
            grant_caps,
        } => {
            let info: Value = client.get_json(&["api", "plugins", &id])?;
            let plugin = &info["plugin"];
            let trusted = plugin["source"] == "bundled";
            let required = strings(&plugin["capabilities"]);
            let granted: Vec<String> = grant_caps
                .into_iter()
                .map(|c| c.trim().to_owned())
                .filter(|c| !c.is_empty())
                .collect();
            let missing = missing_capabilities(&required, &granted, trusted);
            if !missing.is_empty() {
                let message = format!("Plugin {id} requires capabilities not yet granted.");
                let err = json!({ "error": {
                    "code": "capabilities-required",
                    "message": message,
                    "data": { "pluginId": id, "required": required, "missing": missing, "granted": granted,
                              "hint": format!("od plugin apply {id} --grant-caps {}", missing.join(",")) },
                }});
                eprintln!("{err}");
                return Err(Exit::printed(EXIT_CAPABILITIES, message).into());
            }
            let body = json!({ "inputs": parse_inputs(&inputs)?, "locale": locale });
            let res: Value = client.send_json("POST", &["api", "plugins", &id, "apply"], &body)?;
            if json_out {
                return pretty(&res);
            }
            if let Some(q) = res["query"].as_str() {
                println!("{q}");
            }
            if let Some(block) = res["appliedPlugin"]["promptBlock"].as_str() {
                println!("{}", block.trim_start());
            }
            Ok(())
        }
        PluginCommand::Validate { dir } => {
            let report = validate_plugin_dir(&dir);
            if json_out {
                pretty(&json!({ "ok": report.ok(), "id": report.id, "errors": report.errors, "warnings": report.warnings }))?;
            } else {
                for w in &report.warnings {
                    println!("warning: {w}");
                }
                for e in &report.errors {
                    println!("error: {e}");
                }
                if report.ok() {
                    println!("ok {}", report.id.as_deref().unwrap_or(""));
                }
            }
            if report.ok() {
                Ok(())
            } else {
                Err(Exit::printed(
                    1,
                    format!("{} problem(s) in {}", report.errors.len(), dir.display()),
                )
                .into())
            }
        }
        PluginCommand::Atoms => {
            let res: Value = client.get_json(&["api", "atoms"])?;
            if json_out {
                return pretty(&res);
            }
            for a in res["atoms"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}",
                    a["id"].as_str().unwrap_or(""),
                    a["status"].as_str().unwrap_or(""),
                    a["description"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
    }
}
