//! `od media`: queued media generation, its tasks, models and prompt
//! templates, through the daemon's `/api/media/*` routes. `scaffold` and
//! `generate --model hyperframes-html` make and render a Motion film.

use crate::client::Client;
use crate::exit::Exit;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    /// Queue a generation; waits for it unless --no-wait. With
    /// `--model hyperframes-html --composition-dir <dir>`, renders that
    /// composition to MP4 with the installed Chrome and ffmpeg.
    Generate(GenerateArgs),
    /// Create a HyperFrames composition folder (index.html,
    /// hyperframes.json, meta.json) to edit, then render with generate.
    /// A folder that already has index.html is left as it is.
    Scaffold(ScaffoldArgs),
    /// Wait for a task to end; prints it. Fails unless it is done: exit 2
    /// when it is still running at --timeout, 1 when it failed.
    Wait {
        task: String,
        /// Give up after this many seconds.
        #[arg(long, default_value_t = 900)]
        timeout: u64,
        /// Progress cursor of the original's polling loop; accepted and
        /// ignored (this wait blocks until the task ends or --timeout).
        #[arg(long)]
        since: Option<String>,
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
    #[arg(long, env = "OD_PROJECT_ID", hide_env_values = true)]
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
    /// hyperframes-html: the composition folder, relative to the project.
    #[arg(long)]
    composition_dir: Option<String>,
    /// hyperframes-html: frame rate instead of the composition's data-fps.
    #[arg(long)]
    fps: Option<u32>,
    /// The agent run the output belongs to (shown in its turn).
    #[arg(long, env = "OD_RUN_ID", hide_env_values = true)]
    run: Option<String>,
    /// Print the queued task and return at once.
    #[arg(long)]
    no_wait: bool,
    #[arg(long, default_value_t = 900)]
    timeout: u64,
}

#[derive(Debug, Args)]
pub struct ScaffoldArgs {
    #[arg(long, env = "OD_PROJECT_ID", hide_env_values = true)]
    project: String,
    /// New folder for the composition, relative to the project.
    #[arg(long)]
    composition_dir: String,
    #[arg(long)]
    width: Option<u32>,
    #[arg(long)]
    height: Option<u32>,
    #[arg(long)]
    fps: Option<u32>,
    /// Film length in seconds.
    #[arg(long)]
    duration: Option<f64>,
    #[arg(long, env = "OD_RUN_ID", hide_env_values = true)]
    run: Option<String>,
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
        ("done", Some(name), _) => {
            println!("{id}\t{status}\t{name}");
            // A local render says what it made and where to look.
            if let Some(note) = task["result"]["providerNote"]
                .as_str()
                .filter(|_| task["result"]["provider"] == "hyperframes")
            {
                println!("{note}");
                for f in task["result"]["reviewFrames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if let Some(f) = f.as_str() {
                        println!("review frame\t{f}");
                    }
                }
            }
        }
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
        // 2, "still running", as the original's polling loop expects.
        Some("queued" | "running") => Err(Exit::new(2, "timed out waiting for the task").into()),
        Some(other) => {
            let why = task["error"]
                .as_str()
                .map_or(String::new(), |e| format!(": {e}"));
            Err(Exit::new(1, format!("task {other}{why}")).into())
        }
        None => Err(Exit::new(1, "no task in the response").into()),
    }
}

pub fn run(client: &Client, cmd: MediaCommand, json_out: bool) -> anyhow::Result<()> {
    match cmd {
        MediaCommand::Generate(a) => {
            let mut body = json!({ "surface": a.surface, "model": a.model, "prompt": a.prompt });
            for (k, v) in [
                ("output", a.output.map(Value::from)),
                ("compositionDir", a.composition_dir.map(Value::from)),
                ("fps", a.fps.map(Value::from)),
                ("runId", a.run.map(Value::from)),
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
            // First, before any wait: an agent whose shell cuts the wait
            // short still knows which task to `media wait` for.
            if let Some(id) = task["id"].as_str() {
                eprintln!("queued {id}");
            }
            if a.no_wait {
                return print_task(task, json_out);
            }
            let id = task["id"].as_str().unwrap_or_default().to_owned();
            let task = wait(client, &id, Duration::from_secs(a.timeout))?;
            finish(&task, json_out)
        }
        MediaCommand::Scaffold(a) => {
            let mut body = json!({ "compositionDir": a.composition_dir });
            for (k, v) in [
                ("width", a.width.map(Value::from)),
                ("height", a.height.map(Value::from)),
                ("fps", a.fps.map(Value::from)),
                ("duration", a.duration.map(Value::from)),
                ("runId", a.run.map(Value::from)),
            ] {
                if let Some(v) = v {
                    body[k] = v;
                }
            }
            let res: Value = client.send_json(
                "POST",
                &["api", "projects", &a.project, "media", "scaffold"],
                &body,
            )?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&res)?);
                return Ok(());
            }
            println!("{}", res["message"].as_str().unwrap_or_default());
            for f in res["files"].as_array().into_iter().flatten() {
                println!("{}", f.as_str().unwrap_or_default());
            }
            Ok(())
        }
        MediaCommand::Wait { task, timeout, .. } => {
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
            for m in res["models"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(res["localModels"].as_array().into_iter().flatten())
            {
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
