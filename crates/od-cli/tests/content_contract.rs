//! Content ↔ host contract: every `$OD_*` variable the bundled content
//! (`content/`) tells an agent to read is one the daemon sets
//! ([`od_runtime::env::agent_env`]), and every `"$OD_BIN" <group> <command>
//! --flag` it tells an agent to run is a command and flag of `od`.
//!
//! Content copied from the original app may name what Contour does not
//! have yet: [`ALLOWED`] lists each such name with the folders it may
//! appear in and why. `skills/motion-design` gets no exception.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_runtime::env::{RunEnv, agent_env};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `(name, folders under content/ where it may appear, why)`. A name is
/// a variable (`OD_X`) or a command (`group sub`).
const ALLOWED: &[(&str, &[&str], &str)] = &[
    (
        "OD_HYPERFRAMES_BIN",
        &["design-templates/hyperframes/"],
        "HyperFrames CLI (lint, tts, transcribe, inspect, validate) for the HyperFrames chip: waiting on the owner's decision, beta.11",
    ),
    (
        "media stock-search",
        &["skills/od-next-media-inputs/"],
        "OD Next only; not part of Contour (N/A)",
    ),
    (
        "browser snapshot",
        &["skills/agent-browser/"],
        "quoted by the skill as an invalid command (a negative example)",
    ),
    (
        "tools directions",
        &["plugins/_official/atoms/direction-picker/"],
        "N/A: Contour inlines the whole direction library in the prompt (spec 2.4 legacy prompt); the atom's \"In Contour\" line says never to call it",
    ),
];

/// Text files under `dir`, relative paths with `/`.
fn text_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            text_files(root, &path, out);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, text));
        }
    }
}

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

/// The `OD_*` names the daemon gives an agent run.
fn agent_vars() -> BTreeSet<String> {
    let dir = Path::new("/d");
    let env = agent_env(
        &od_runtime::def::CLAUDE,
        &[],
        &"/bin".into(),
        &RunEnv {
            data_dir: Some(dir),
            daemon_url: Some("http://127.0.0.1:1"),
            tool_token: Some("t"),
            project_id: Some("p"),
            project_dir: Some(dir),
            od_bin: Some(dir),
            run_id: Some("r"),
        },
    );
    env.into_iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .filter(|k| k.starts_with("OD_"))
        .collect()
}

/// One use of a host name in the content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Use {
    file: String,
    name: String,
    /// `--flags` on the same command line (commands only).
    flags: Vec<String>,
}

fn scan(files: &[(String, String)]) -> (Vec<Use>, Vec<Use>) {
    let var = Regex::new(r"\$\{?(OD_[A-Z0-9_]+)").unwrap();
    let cmd =
        Regex::new(r#"\$\{?OD_BIN\}?"?\s+([a-z][a-z0-9-]*)(?:\s+([a-z][a-z0-9-]*))?(?:\s+([a-z][a-z0-9-]*))?([^\n`|;]*)"#)
            .unwrap();
    let flag = Regex::new(r"(?:^|\s)--([a-z][a-z0-9-]*)").unwrap();
    let (mut vars, mut cmds) = (Vec::new(), Vec::new());
    for (file, text) in files {
        for c in var.captures_iter(text) {
            vars.push(Use {
                file: file.clone(),
                name: c[1].to_owned(),
                flags: Vec::new(),
            });
        }
        for c in cmd.captures_iter(text) {
            // Up to three words (`tools live-artifacts create`); a word
            // that is not a subcommand is checked in `check_command`.
            let name = [c.get(1), c.get(2), c.get(3)]
                .into_iter()
                .flatten()
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let rest = c.get(4).map_or("", |m| m.as_str());
            let flags = flag.captures_iter(rest).map(|f| f[1].to_owned()).collect();
            cmds.push(Use {
                file: file.clone(),
                name,
                flags,
            });
        }
    }
    (vars, cmds)
}

/// Why `u` is wrong, or `None` when `od` (or the allowlist) covers it.
fn check_command(od: &clap::Command, u: &Use) -> Option<String> {
    let mut words = u.name.split(' ');
    let group = words.next()?;
    let Some(mut target) = od.find_subcommand(group) else {
        return Some(format!("`od {}` does not exist (no `od {group}`)", u.name));
    };
    let mut path = vec![group];
    // Walk down the subcommands; a word after a leaf is an argument.
    for word in words {
        if !target.has_subcommands() {
            break;
        }
        match target.find_subcommand(word) {
            Some(sub) => {
                target = sub;
                path.push(word);
            }
            None => return Some(format!("`od {} {word}` does not exist", path.join(" "))),
        }
    }
    let known = |f: &str| {
        target.get_arguments().any(|a| a.get_long() == Some(f))
            || od.get_arguments().any(|a| a.get_long() == Some(f))
            || f == "help"
    };
    u.flags
        .iter()
        .find(|f| !known(f))
        .map(|f| format!("`od {}` has no --{f}", path.join(" ")))
}

fn allowed(u: &Use) -> bool {
    ALLOWED.iter().any(|(name, dirs, _)| {
        (u.name == *name || u.name.starts_with(&format!("{name} ")))
            && dirs.iter().any(|d| u.file.starts_with(d))
    })
}

/// Violations of the contract in `files`.
fn violations(files: &[(String, String)]) -> Vec<String> {
    let vars = agent_vars();
    let od = od_cli::command();
    let (var_uses, cmd_uses) = scan(files);
    let mut out = BTreeSet::new();
    for u in var_uses {
        if !vars.contains(&u.name) && !allowed(&u) {
            out.insert(format!("{}: ${} is not set for agent runs", u.file, u.name));
        }
    }
    for u in cmd_uses {
        if let Some(why) = check_command(&od, &u) {
            if !allowed(&u) {
                out.insert(format!("{}: {why}", u.file));
            }
        }
    }
    out.into_iter().collect()
}

#[test]
fn bundled_content_only_calls_what_the_host_has() {
    let root = content_root();
    let mut files = Vec::new();
    text_files(&root, &root, &mut files);
    assert!(
        files.len() > 100,
        "content/ not found at {}",
        root.display()
    );
    let bad = violations(&files);
    assert!(
        bad.is_empty(),
        "content ↔ host contract broken:\n{}",
        bad.join("\n")
    );
    // The motion skill really calls the host (the scan is not vacuous).
    let (_, cmds) = scan(&files);
    for want in ["media scaffold", "media generate", "media wait"] {
        assert!(
            cmds.iter()
                .any(|u| u.file.starts_with("skills/motion-design/") && u.name == want),
            "motion-design no longer calls `od {want}`"
        );
    }
}

#[test]
fn motion_design_has_no_exception() {
    for (name, dirs, why) in ALLOWED {
        assert!(!why.is_empty(), "{name} needs a reason");
        assert!(
            !dirs.iter().any(|d| d.starts_with("skills/motion-design")),
            "{name} must not be allowed in skills/motion-design"
        );
    }
}

#[test]
fn a_missing_host_command_or_variable_fails() {
    let file = |text: &str| vec![("skills/demo/SKILL.md".to_owned(), text.to_owned())];
    let ok = file(
        r#"Run `"$OD_NODE_BIN" "$OD_BIN" media generate --project "$OD_PROJECT_ID" --model hyperframes-html --composition-dir a --output a.mp4`."#,
    );
    assert_eq!(violations(&ok), Vec::<String>::new());
    let cases = [
        (
            r#"`"$OD_NODE_BIN" "$OD_BIN" media teleport --x 1`"#,
            "media teleport",
        ),
        (r#"`"$OD_BIN" render now`"#, "no `od render`"),
        (r#"`"$OD_BIN" media generate --warp 9`"#, "--warp"),
        ("Read `$OD_SECRET_THING` first.", "OD_SECRET_THING"),
        // An allowed name outside its folders is still a violation.
        (r#"`"$OD_BIN" tools directions --id x`"#, "tools directions"),
        (
            r#"`"$OD_BIN" tools live-artifacts publish`"#,
            "tools live-artifacts publish",
        ),
        ("`$OD_HYPERFRAMES_BIN lint`", "OD_HYPERFRAMES_BIN"),
    ];
    for (text, want) in cases {
        let bad = violations(&file(text));
        assert!(
            bad.iter().any(|b| b.contains(want)),
            "{text}: expected a violation naming {want}, got {bad:?}"
        );
    }
}
