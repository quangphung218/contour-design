//! `od`: with no subcommand (or `od daemon start`) runs the daemon; every
//! other subcommand is a thin client of the daemon's HTTP API.

mod client;
mod export_cmd;
mod mcp_cmd;

use anyhow::Context;
use base64::Engine;
use clap::{Args, Parser, Subcommand};
use client::Client;
use od_contracts::{
    ChatRequest, CreateProjectRequest, CreateProjectResponse, CreateRunResponse,
    DuplicateProjectRequest, DuplicateProjectResponse, FileResponse, FilesResponse, ProjectKind,
    ProjectMetadata, ProjectResponse, ProjectsResponse, RunActionResponse, RunStatus, RunsResponse,
    WriteFileRequest,
};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "od", version, about = "open-design-rs daemon and CLI")]
struct Cli {
    /// Daemon base URL for client commands.
    #[arg(
        long,
        global = true,
        env = "OD_DAEMON_URL",
        default_value = "http://127.0.0.1:7456"
    )]
    daemon_url: String,
    /// Print raw JSON responses.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run or query the daemon.
    #[command(subcommand)]
    Daemon(DaemonCommand),
    /// Manage projects.
    #[command(subcommand)]
    Project(ProjectCommand),
    /// Read and write project files.
    #[command(subcommand)]
    Files(FilesCommand),
    /// Start, watch and cancel agent runs.
    #[command(subcommand)]
    Run(RunCommand),
    /// Export a project file (HTML, PDF, PPTX, image) or the project (ZIP).
    Export(export_cmd::ExportArgs),
    /// MCP stdio server for coding agents; `od mcp install <agent>`.
    Mcp(mcp_cmd::McpArgs),
}

#[derive(Debug, Subcommand)]
enum RunCommand {
    /// Start a run; prints the run id (or, with --watch, its events).
    Start {
        #[arg(long)]
        project: String,
        /// Agent id: claude or codex.
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long)]
        message: String,
        #[arg(long)]
        conversation: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        skill: Option<String>,
        /// Idempotency key: the same key returns the same run.
        #[arg(long)]
        client_request_id: Option<String>,
        /// Stream the run's events as NDJSON until it ends.
        #[arg(long)]
        watch: bool,
    },
    /// Stream a run's events as NDJSON until it ends.
    Watch {
        id: String,
        /// Resume after this event id.
        #[arg(long)]
        after: Option<u64>,
    },
    /// Cancel a run (the whole process tree is stopped).
    Cancel { id: String },
    /// List runs, newest first.
    List {
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DaemonCommand {
    /// Run the daemon in the foreground.
    Start(StartArgs),
    /// Check whether a daemon answers at --daemon-url.
    Status,
}

#[derive(Debug, Clone, Args)]
struct StartArgs {
    #[arg(long, env = "OD_HOST", default_value = od_server::DEFAULT_HOST)]
    host: String,
    #[arg(long, env = "OD_PORT", default_value_t = od_server::DEFAULT_PORT)]
    port: u16,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Create a project.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        skill: Option<String>,
        #[arg(long)]
        design_system: Option<String>,
        /// prototype, deck, template, image, video, audio or other.
        #[arg(long)]
        kind: Option<String>,
    },
    /// List projects, newest first.
    List,
    /// Show one project.
    Info { id: String },
    /// Delete a project and its folder.
    Delete { id: String },
    /// Copy a project's files into a new project.
    Duplicate {
        id: String,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum FilesCommand {
    /// List files, newest first.
    List { project: String },
    /// Print a file to stdout (or --out).
    Read {
        project: String,
        path: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Write a file from --content, --from, or stdin.
    Write {
        project: String,
        path: String,
        #[arg(long, conflicts_with = "from")]
        content: Option<String>,
        #[arg(long)]
        from: Option<PathBuf>,
        /// Fail instead of replacing an existing file.
        #[arg(long)]
        no_overwrite: bool,
    },
    /// Delete a file.
    Delete { project: String, path: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        None => run_daemon(StartArgs {
            host: std::env::var("OD_HOST").unwrap_or_else(|_| od_server::DEFAULT_HOST.to_owned()),
            port: std::env::var("OD_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(od_server::DEFAULT_PORT),
        }),
        Some(Command::Daemon(DaemonCommand::Start(args))) => run_daemon(args),
        Some(Command::Daemon(DaemonCommand::Status)) => {
            Client::new(&cli.daemon_url).and_then(|c| daemon_status(&c, cli.json))
        }
        Some(Command::Project(cmd)) => {
            Client::new(&cli.daemon_url).and_then(|c| project(&c, cmd, cli.json))
        }
        Some(Command::Files(cmd)) => {
            Client::new(&cli.daemon_url).and_then(|c| files(&c, cmd, cli.json))
        }
        Some(Command::Run(cmd)) => {
            Client::new(&cli.daemon_url).and_then(|c| run(&c, cmd, cli.json))
        }
        Some(Command::Export(args)) => {
            Client::new(&cli.daemon_url).and_then(|c| export_cmd::run(&c, args, cli.json))
        }
        Some(Command::Mcp(args)) => return mcp_cmd::run(args, &cli.daemon_url, cli.json),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_daemon(args: StartArgs) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(od_server::serve(od_server::ServeOptions::from_env(
        args.host, args.port,
    )))
}

fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn daemon_status(client: &Client, json: bool) -> anyhow::Result<()> {
    let health: od_contracts::HealthResponse = client.get_json(&["api", "health"])?;
    if json {
        return print_json(&health);
    }
    println!("running {} (version {})", client.base(), health.version);
    Ok(())
}

fn print_project_line(p: &od_contracts::Project) {
    println!("{}\t{}", p.id, p.name);
}

fn project(client: &Client, cmd: ProjectCommand, json: bool) -> anyhow::Result<()> {
    match cmd {
        ProjectCommand::Create {
            name,
            id,
            skill,
            design_system,
            kind,
        } => {
            let metadata = kind
                .map(|k| {
                    serde_json::from_value::<ProjectKind>(serde_json::Value::String(k.clone()))
                        .with_context(|| format!("unknown project kind {k:?}"))
                        .map(|kind| ProjectMetadata {
                            kind: Some(kind),
                            ..ProjectMetadata::default()
                        })
                })
                .transpose()?;
            let req = CreateProjectRequest {
                id,
                name,
                skill_id: skill,
                design_system_id: design_system,
                metadata,
                ..CreateProjectRequest::default()
            };
            let res: CreateProjectResponse =
                client.send_json("POST", &["api", "projects"], &req)?;
            if json {
                return print_json(&res);
            }
            print_project_line(&res.project);
        }
        ProjectCommand::List => {
            let res: ProjectsResponse = client.get_json(&["api", "projects"])?;
            if json {
                return print_json(&res);
            }
            res.projects.iter().for_each(print_project_line);
        }
        ProjectCommand::Info { id } => {
            let res: ProjectResponse = client.get_json(&["api", "projects", &id])?;
            if json {
                return print_json(&res);
            }
            let p = &res.project;
            println!("id:            {}", p.id);
            println!("name:          {}", p.name);
            println!("skill:         {}", p.skill_id.as_deref().unwrap_or("-"));
            println!(
                "design system: {}",
                p.design_system_id.as_deref().unwrap_or("-")
            );
            println!("created:       {}", p.created_at);
            println!("updated:       {}", p.updated_at);
        }
        ProjectCommand::Delete { id } => {
            let res: serde_json::Value = client.send_empty("DELETE", &["api", "projects", &id])?;
            if json {
                return print_json(&res);
            }
            println!("deleted {id}");
        }
        ProjectCommand::Duplicate { id, name } => {
            let res: DuplicateProjectResponse = client.send_json(
                "POST",
                &["api", "projects", &id, "duplicate"],
                &DuplicateProjectRequest { name },
            )?;
            if json {
                return print_json(&res);
            }
            print_project_line(&res.project);
        }
    }
    Ok(())
}

/// `["api","projects",id,"files", ...path segments]`.
fn file_segments<'a>(project: &'a str, path: &'a str) -> Vec<&'a str> {
    let mut segs = vec!["api", "projects", project, "files"];
    segs.extend(path.split('/').filter(|s| !s.is_empty()));
    segs
}

fn files(client: &Client, cmd: FilesCommand, json: bool) -> anyhow::Result<()> {
    match cmd {
        FilesCommand::List { project } => {
            let res: FilesResponse = client.get_json(&["api", "projects", &project, "files"])?;
            if json {
                return print_json(&res);
            }
            for f in &res.files {
                println!("{}\t{}", f.size, f.name);
            }
        }
        FilesCommand::Read { project, path, out } => {
            let bytes = client.get_bytes(&file_segments(&project, &path))?;
            match out {
                Some(out) => std::fs::write(&out, &bytes)
                    .with_context(|| format!("writing {}", out.display()))?,
                None => std::io::stdout().write_all(&bytes)?,
            }
        }
        FilesCommand::Write {
            project,
            path,
            content,
            from,
            no_overwrite,
        } => {
            let bytes = match (content, from) {
                (Some(c), _) => c.into_bytes(),
                (None, Some(file)) => {
                    std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?
                }
                (None, None) => {
                    let mut buf = Vec::new();
                    std::io::stdin().read_to_end(&mut buf)?;
                    buf
                }
            };
            let (content, encoding) = match String::from_utf8(bytes) {
                Ok(text) => (text, od_contracts::ContentEncoding::Utf8),
                Err(e) => (
                    base64::engine::general_purpose::STANDARD.encode(e.as_bytes()),
                    od_contracts::ContentEncoding::Base64,
                ),
            };
            let req = WriteFileRequest {
                name: path,
                content,
                encoding,
                overwrite: Some(!no_overwrite),
                artifact_manifest: None,
            };
            let res: FileResponse =
                client.send_json("POST", &["api", "projects", &project, "files"], &req)?;
            if json {
                return print_json(&res);
            }
            println!("wrote {} ({} bytes)", res.file.name, res.file.size);
        }
        FilesCommand::Delete { project, path } => {
            let res: serde_json::Value =
                client.send_empty("DELETE", &file_segments(&project, &path))?;
            if json {
                return print_json(&res);
            }
            println!("deleted {path}");
        }
    }
    Ok(())
}

fn run(client: &Client, cmd: RunCommand, json: bool) -> anyhow::Result<()> {
    match cmd {
        RunCommand::Start {
            project,
            agent,
            message,
            conversation,
            model,
            skill,
            client_request_id,
            watch,
        } => {
            let req = ChatRequest {
                agent_id: agent,
                message,
                project_id: project,
                conversation_id: conversation,
                client_request_id,
                skill_id: skill,
                model,
                ..ChatRequest::default()
            };
            let res: CreateRunResponse = client.send_json("POST", &["api", "runs"], &req)?;
            if watch {
                return watch_run(client, &res.run_id, None);
            }
            if json {
                return print_json(&res);
            }
            println!("{}", res.run_id);
        }
        RunCommand::Watch { id, after } => return watch_run(client, &id, after),
        RunCommand::Cancel { id } => {
            let res: RunActionResponse =
                client.send_empty("POST", &["api", "runs", &id, "cancel"])?;
            if json {
                return print_json(&res);
            }
            println!("{id}\t{}", res.status.as_str());
        }
        RunCommand::List { project } => {
            let query: Vec<(&str, &str)> = project
                .as_deref()
                .map(|p| ("projectId", p))
                .into_iter()
                .collect();
            let res: RunsResponse = client.get_json_query(&["api", "runs"], &query)?;
            if json {
                return print_json(&res);
            }
            for r in &res.runs {
                println!("{}\t{}\t{}", r.id, r.status.as_str(), r.agent_id);
            }
        }
    }
    Ok(())
}

/// Prints each SSE event as one NDJSON line `{id, event, data}`. Fails when
/// the run did not succeed.
fn watch_run(client: &Client, id: &str, after: Option<u64>) -> anyhow::Result<()> {
    let mut status = None;
    client.stream_events(id, after, |ev| {
        if ev.event == "end" {
            status = ev
                .data
                .get("status")
                .and_then(|s| serde_json::from_value::<RunStatus>(s.clone()).ok());
        }
        let mut out = std::io::stdout().lock();
        writeln!(out, "{}", serde_json::to_string(&ev)?)?;
        out.flush()?;
        Ok(())
    })?;
    match status {
        Some(RunStatus::Succeeded) => Ok(()),
        Some(other) => anyhow::bail!("run {}", other.as_str()),
        None => anyhow::bail!("stream closed before the run ended"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn file_paths_split_into_segments() {
        assert_eq!(
            file_segments("p", "a/b c.html"),
            ["api", "projects", "p", "files", "a", "b c.html"]
        );
    }
}
