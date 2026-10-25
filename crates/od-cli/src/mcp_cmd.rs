//! `od mcp` (stdio server) and `od mcp install <agent>`.

use anyhow::Context;
use clap::{Args, Subcommand};
use od_mcp::install::{self, Agent, InstallOptions, LaunchSpec, SERVER_NAME};
use std::process::ExitCode;

#[derive(Debug, Args)]
pub struct McpArgs {
    #[command(subcommand)]
    command: Option<McpCommand>,
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    /// Second stdio server: live artifact tools. Uses `OD_TOOL_TOKEN` when
    /// an agent run launches it; otherwise `--project`.
    LiveArtifacts {
        /// Default project for tools called without `projectId`.
        #[arg(long, env = od_mcp::live::PROJECT_ENV)]
        project: Option<String>,
    },
    /// Register `od mcp` in a coding agent (claude, codex, cursor).
    Install {
        agent: String,
        /// Dry run: print the command or the resulting config.
        #[arg(long)]
        print: bool,
        /// Remove the entry instead.
        #[arg(long)]
        uninstall: bool,
        /// Server name in the agent's config.
        #[arg(long, default_value = SERVER_NAME)]
        name: String,
    },
}

pub fn run(args: McpArgs, daemon_url: &str, json: bool) -> ExitCode {
    let result = match args.command {
        None => serve(daemon_url).map(|()| ExitCode::SUCCESS),
        Some(McpCommand::LiveArtifacts { project }) => {
            serve_live(daemon_url, project).map(|()| ExitCode::SUCCESS)
        }
        Some(McpCommand::Install {
            agent,
            print,
            uninstall,
            name,
        }) => install(
            &agent,
            daemon_url,
            &name,
            InstallOptions {
                print,
                uninstall,
                json,
            },
        ),
    };
    result.unwrap_or_else(|err| {
        eprintln!("error: {err:#}");
        ExitCode::FAILURE
    })
}

fn stdio_runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    // stdout carries the protocol: logs go to stderr only.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the async runtime")
}

fn serve(daemon_url: &str) -> anyhow::Result<()> {
    stdio_runtime()?.block_on(od_mcp::serve_stdio(daemon_url))
}

fn serve_live(daemon_url: &str, project: Option<String>) -> anyhow::Result<()> {
    let token = std::env::var(od_mcp::live::TOOL_TOKEN_ENV).ok();
    stdio_runtime()?.block_on(od_mcp::serve_live_stdio(
        daemon_url,
        token.as_deref(),
        project.filter(|p| !p.trim().is_empty()),
    ))
}

fn install(
    agent: &str,
    daemon_url: &str,
    name: &str,
    opts: InstallOptions,
) -> anyhow::Result<ExitCode> {
    let agent = Agent::parse(agent).map_err(anyhow::Error::msg)?;
    let exe = std::env::current_exe().context("locating the od binary")?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let mut args = vec!["mcp".to_owned()];
    if daemon_url.trim_end_matches('/') != od_mcp::DEFAULT_DAEMON_URL {
        args.extend(["--daemon-url".to_owned(), daemon_url.to_owned()]);
    }
    let spec = LaunchSpec {
        command: exe.to_string_lossy().into_owned(),
        args,
    };
    let home = install::home_dir().context("cannot find the home directory (HOME)")?;
    let plan = install::plan(agent, &spec, &home, name);
    let runner = |bin: &str, argv: &[String]| -> std::io::Result<i32> {
        let status = std::process::Command::new(bin).args(argv).status()?;
        Ok(status.code().unwrap_or(1))
    };
    let code = install::execute(
        agent,
        &plan,
        opts,
        &runner,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )?;
    // After an install, and on uninstall even when no entry under `name`
    // was there: the old name goes either way.
    let legacy = (code == 0 || opts.uninstall)
        && !opts.print
        && name != install::LEGACY_SERVER_NAME
        && install::remove_legacy(agent, &spec, &home, &runner);
    if legacy && !opts.json {
        println!(
            "removed the old {} entry from {}",
            install::LEGACY_SERVER_NAME,
            agent.slug()
        );
    }
    // The old entry gone and no entry under `name` left: uninstalled, even
    // when the agent's own `mcp remove` failed for want of one.
    let code = if opts.uninstall
        && install::uninstall_ok(code, legacy, !install::has_entry(agent, &home, name))
    {
        0
    } else {
        code
    };
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}
