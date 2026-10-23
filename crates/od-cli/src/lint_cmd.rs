//! `od lint`: the anti-slop lint (`GET /api/projects/:id/lint/<path>`) of
//! one project file, or of every HTML file in the project. Exits 1 when a
//! finding reaches `--fail-on` (spec 2.6).

use crate::client::Client;
use crate::exit::Exit;
use clap::{Args, ValueEnum};
use od_contracts::{FileKind, FilesResponse, LintResponse, LintSeverity};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FailOn {
    P0,
    P1,
    P2,
    /// Never exit 1.
    None,
}

impl FailOn {
    fn fails(self, severity: LintSeverity) -> bool {
        match self {
            Self::P0 => severity == LintSeverity::P0,
            Self::P1 => severity <= LintSeverity::P1,
            Self::P2 => true,
            Self::None => false,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "p0",
            Self::P1 => "p1",
            Self::P2 => "p2",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Args)]
pub struct LintArgs {
    /// Project id.
    project: String,
    /// HTML file to lint (default: every HTML file in the project).
    file: Option<String>,
    /// Exit 1 when a finding has this severity or a worse one.
    #[arg(long, value_enum, default_value_t = FailOn::P0)]
    fail_on: FailOn,
    /// Also print the block the agent gets on its next turn.
    #[arg(long)]
    agent_message: bool,
}

/// Findings counted by severity: `[P0, P1, P2]`.
fn counts(results: &[(String, LintResponse)]) -> [usize; 3] {
    let mut counts = [0; 3];
    for f in results.iter().flat_map(|(_, r)| &r.findings) {
        counts[f.severity as usize] += 1;
    }
    counts
}

pub fn run(client: &Client, args: LintArgs, json: bool) -> anyhow::Result<()> {
    let files = match args.file {
        Some(file) => vec![file],
        None => {
            let list: FilesResponse =
                client.get_json(&["api", "projects", &args.project, "files"])?;
            let mut html: Vec<String> = list
                .files
                .into_iter()
                .filter(|f| f.kind == FileKind::Html)
                .map(|f| f.name)
                .collect();
            html.sort();
            if html.is_empty() {
                anyhow::bail!("project {} has no HTML files", args.project);
            }
            html
        }
    };
    let mut results = Vec::with_capacity(files.len());
    for file in files {
        let mut segs = vec!["api", "projects", args.project.as_str(), "lint"];
        segs.extend(file.split('/').filter(|s| !s.is_empty()));
        let res: LintResponse = client.get_json(&segs)?;
        results.push((file, res));
    }
    let failed = results
        .iter()
        .flat_map(|(_, r)| &r.findings)
        .any(|f| args.fail_on.fails(f.severity));
    let [p0, p1, p2] = counts(&results);

    if json {
        let files: Vec<_> = results
            .iter()
            .map(|(file, r)| json!({"file": file, "findings": r.findings, "agentMessage": r.agent_message}))
            .collect();
        let envelope = json!({
            "ok": !failed,
            "failOn": args.fail_on.as_str(),
            "counts": {"p0": p0, "p1": p1, "p2": p2},
            "files": files,
        });
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    } else {
        for (file, r) in &results {
            for f in &r.findings {
                println!("{file}: [{:?}] {} — {}", f.severity, f.id, f.message);
                if let Some(snippet) = &f.snippet {
                    println!("      {snippet}");
                }
                println!("      fix: {}", f.fix);
            }
            if args.agent_message && !r.agent_message.is_empty() {
                println!("\n{}\n", r.agent_message);
            }
        }
        let total = p0 + p1 + p2;
        let summary = format!(
            "{total} finding{} in {} file{} (P0 {p0} · P1 {p1} · P2 {p2})",
            if total == 1 { "" } else { "s" },
            results.len(),
            if results.len() == 1 { "" } else { "s" },
        );
        println!(
            "{}",
            if total == 0 {
                format!("clean — {summary}")
            } else {
                summary
            }
        );
    }
    if failed {
        return Err(Exit::printed(
            1,
            format!("lint findings at or above {}", args.fail_on.as_str()),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_on_is_a_threshold() {
        use LintSeverity::{P0, P1, P2};
        let table = [
            (FailOn::P0, [true, false, false]),
            (FailOn::P1, [true, true, false]),
            (FailOn::P2, [true, true, true]),
            (FailOn::None, [false, false, false]),
        ];
        for (fail_on, expected) in table {
            assert_eq!(
                [fail_on.fails(P0), fail_on.fails(P1), fail_on.fails(P2)],
                expected,
                "{fail_on:?}"
            );
        }
    }
}
