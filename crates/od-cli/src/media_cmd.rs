//! `od media`: queued media generation, its tasks, models and prompt
//! templates, through the daemon's `/api/media/*` routes.

use crate::client::Client;
use crate::exit::Exit;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    /// Queue a generation; waits for it unless --no-wait.
    Generate(GenerateArgs),
    /// Wait for a task to end; prints it. Fails unless it is done.
    Wait {
        task: String,
        /// Give up after this many seconds.
        #[arg(long, default_value_t = 900)]
        timeout: u64,
    },
    /// List media tasks, oldest first.
    Tasks {
        #[arg(long)]
        project: Option<String>,
    },
    /// Cancel a queued or running task.
    Cancel { task: String },
    /// Model catalogue and which providers have a key.
    Models,
    /// Image and video prompt templates.
    Templates {
        #[arg(long, value_parser = ["image", "video"])]
        surface: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct GenerateArgs {
    #[arg(long)]
    project: String,
    #[arg(long, value_parser = ["image", "video", "audio"])]
    surface: String,
    #[arg(long)]
    model: String,
    #[arg(long, default_value = "")]
    prompt: String,
    /// Project path; the extension follows what the provider returns.
    #[arg(long)]
    output: Option<String>,
    #[arg(long)]
    aspect: Option<String>,
    /// Video length or audio duration, in seconds.
    #[arg(long)]
    duration: Option<f64>,
    #[arg(long)]
    voice: Option<String>,
    /// Print the queued task and return at once.
    #[arg(long)]
    no_wait: bool,
    #[arg(long, default_value_t = 900)]
    timeout: u64,
}

fn print_task(task: &Value, json_out: bool) -> anyhow::Result<()> {
    if json_out {
        println!("{}", serde_json::to_string_pretty(task)?);
        return Ok(());
    }
    let id = task["id"].as_str().unwrap_or("");
    let status = task["status"].as_str().unwrap_or("");
    match (
        status,
        task["result"]["name"].as_str(),
        task["error"].as_str(),
    ) {
        ("done", Some(name), _) => println!("{id}\t{status}\t{name}"),
        (_, _, Some(err)) => println!("{id}\t{status}\t{err}"),
        _ => println!("{id}\t{status}"),
    }
    Ok(())
}

/// Polls with server-side waits until the task ends or `timeout` passes.
fn wait(client: &Client, id: &str, timeout: Duration) -> anyhow::Result<Value> {
    let started = Instant::now();
    loop {
        let left = timeout.saturating_sub(started.elapsed());
        let step = left
            .min(Duration::from_secs(25))
            .as_millis()
            .max(1)
            .to_string();
        let res: Value =
            client.get_json_query(&["api", "media", "tasks", id], &[("waitMs", step.as_str())])?;
        let task = res["task"].clone();
        let terminal = !matches!(task["status"].as_str(), Some("queued" | "running"));
        if terminal || started.elapsed() >= timeout {
            return Ok(task);
        }
    }
}

/// Exit status for a finished wait: 0 only for `done`.
fn finish(task: &Value, json_out: bool) -> anyhow::Result<()> {
    print_task(task, json_out)?;
    match task["status"].as_str() {
        Some("done") => Ok(()),
        Some("queued" | "running") => Err(Exit::new(1, "timed out waiting for the task").into()),
        Some(other) => Err(Exit::new(1, format!("task {other}")).into()),
        None => Err(Exit::new(1, "no task in the response").into()),
    }
}

pub fn run(client: &Client, cmd: MediaCommand, json_out: bool) -> anyhow::Result<()> {
    match cmd {
        MediaCommand::Generate(a) => {
            let mut body = json!({ "surface": a.surface, "model": a.model, "prompt": a.prompt });
            for (k, v) in [
                ("output", a.output.map(Value::from)),
                ("aspect", a.aspect.map(Value::from)),
                ("duration", a.duration.map(Value::from)),
                ("length", a.duration.map(Value::from)),
                ("voice", a.voice.map(Value::from)),
            ] {
                if let Some(v) = v {
                    body[k] = v;
                }
            }
            let res: Value = client.send_json(
                "POST",
                &["api", "projects", &a.project, "media", "tasks"],
                &body,
            )?;
            let task = &res["task"];
            if a.no_wait {
                return print_task(task, json_out);
            }
            let id = task["id"].as_str().unwrap_or_default().to_owned();
            let task = wait(client, &id, Duration::from_secs(a.timeout))?;
            finish(&task, json_out)
        }
        MediaCommand::Wait { task, timeout } => {
            let task = wait(client, &task, Duration::from_secs(timeout))?;
            finish(&task, json_out)
        }
        MediaCommand::Tasks { project } => {
            let query: Vec<(&str, &str)> = project
                .as_deref()
                .map(|p| ("projectId", p))
                .into_iter()
                .collect();
            let res: Value = client.get_json_query(&["api", "media", "tasks"], &query)?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&res)?);
                return Ok(());
            }
            for t in res["tasks"].as_array().into_iter().flatten() {
                print_task(t, false)?;
            }
            Ok(())
        }
        MediaCommand::Cancel { task } => {
            let res: Value =
                client.send_empty("POST", &["api", "media", "tasks", &task, "cancel"])?;
            print_task(&res["task"], json_out)
        }
        MediaCommand::Models => {
            let res: Value = client.get_json(&["api", "media", "models"])?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&res)?);
                return Ok(());
            }
            for p in res["providers"].as_array().into_iter().flatten() {
                let state = if p["configured"] == true {
                    "key set"
                } else {
                    "no key"
                };
                println!("provider\t{}\t{state}", p["id"].as_str().unwrap_or(""));
            }
            for m in res["models"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}",
                    m["surface"].as_str().unwrap_or(""),
                    m["id"].as_str().unwrap_or(""),
                    m["provider"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        MediaCommand::Templates { surface } => {
            let query: Vec<(&str, &str)> = surface
                .as_deref()
                .map(|s| ("surface", s))
                .into_iter()
                .collect();
            let res: Value = client.get_json_query(&["api", "prompt-templates"], &query)?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&res)?);
                return Ok(());
            }
            let list = res
                .get("promptTemplates")
                .or_else(|| res.get("templates"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for t in &list {
                println!(
                    "{}\t{}\t{}",
                    t["surface"].as_str().unwrap_or(""),
                    t["id"].as_str().unwrap_or(""),
                    t["title"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
    }
}
