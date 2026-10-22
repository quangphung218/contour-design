//! `od mcp install <agent>` (spec 2.5): registers the stdio server in a
//! coding agent's own config. claude and codex ship `mcp add/remove`, which
//! we drive; cursor reads `~/.cursor/mcp.json`, into which we deep-merge one
//! entry without touching the others.

use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Name of our entry in agent configs.
pub const SERVER_NAME: &str = "open-design";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Cursor,
}

impl Agent {
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Cursor];

    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }

    /// # Errors
    /// An unknown slug, listing the supported ones.
    pub fn parse(slug: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|a| a.slug() == slug)
            .ok_or_else(|| {
                let all: Vec<&str> = Self::ALL.iter().map(|a| a.slug()).collect();
                format!("unknown agent: {slug} (expected one of: {})", all.join(" "))
            })
    }
}

/// How the agent should start `od mcp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Run the agent's own `mcp add` / `mcp remove`.
    Cli {
        bin: &'static str,
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// Merge `entry` at `key_path`.`server_key` in a JSON file.
    Json {
        config_path: PathBuf,
        key_path: Vec<&'static str>,
        server_key: String,
        entry: Value,
    },
}

/// Pure planner: no I/O.
#[must_use]
pub fn plan(agent: Agent, spec: &LaunchSpec, home: &Path, name: &str) -> Plan {
    let mut launch = vec![spec.command.clone()];
    launch.extend(spec.args.iter().cloned());
    let owned = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match agent {
        Agent::Claude => {
            let mut add = owned(&["mcp", "add", "--scope", "user", name, "--"]);
            add.extend(launch);
            Plan::Cli {
                bin: "claude",
                add,
                remove: owned(&["mcp", "remove", "--scope", "user", name]),
            }
        }
        Agent::Codex => {
            let mut add = owned(&["mcp", "add", name, "--"]);
            add.extend(launch);
            Plan::Cli {
                bin: "codex",
                add,
                remove: owned(&["mcp", "remove", name]),
            }
        }
        Agent::Cursor => Plan::Json {
            config_path: home.join(".cursor").join("mcp.json"),
            key_path: vec!["mcpServers"],
            server_key: name.to_owned(),
            entry: json!({ "type": "stdio", "command": spec.command, "args": spec.args }),
        },
    }
}

fn parse_config(existing: Option<&str>, path: &Path) -> Result<Map<String, Value>, String> {
    match existing.map(str::trim) {
        None | Some("") => Ok(Map::new()),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(format!(
                "{} is not a JSON object; refusing to overwrite it",
                path.display()
            )),
            Err(e) => Err(format!(
                "{} is not valid JSON ({e}); refusing to overwrite it",
                path.display()
            )),
        },
    }
}

/// `existing` with our entry set, every other key kept.
///
/// # Errors
/// `existing` is not a JSON object, or a key on the path is not an object.
pub fn merge_json(
    existing: Option<&str>,
    path: &Path,
    key_path: &[&str],
    key: &str,
    entry: &Value,
) -> Result<String, String> {
    let mut root = parse_config(existing, path)?;
    let mut node = &mut root;
    for k in key_path {
        let slot = node.entry((*k).to_owned()).or_insert_with(|| json!({}));
        node = slot
            .as_object_mut()
            .ok_or_else(|| format!("{k} in {} is not an object", path.display()))?;
    }
    node.insert(key.to_owned(), entry.clone());
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    ))
}

/// `existing` without our entry; `None` when it was not there.
///
/// # Errors
/// `existing` is not a JSON object.
pub fn remove_json(
    existing: Option<&str>,
    path: &Path,
    key_path: &[&str],
    key: &str,
) -> Result<Option<String>, String> {
    let mut root = parse_config(existing, path)?;
    let mut node = &mut root;
    for k in key_path {
        match node.get_mut(*k).and_then(Value::as_object_mut) {
            Some(next) => node = next,
            None => return Ok(None),
        }
    }
    if node.remove(key).is_none() {
        return Ok(None);
    }
    Ok(Some(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    )))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InstallOptions {
    /// Dry run: print what would change.
    pub print: bool,
    pub uninstall: bool,
    /// Print one JSON result object.
    pub json: bool,
}

/// Runs `bin args…` and returns its exit code.
pub type Runner<'a> = &'a dyn Fn(&str, &[String]) -> std::io::Result<i32>;

/// Executes a plan. Returns the process exit code.
///
/// With `print` and a JSON plan, stdout carries exactly the resulting config
/// file (valid JSON) unless `json` asks for a result object.
///
/// # Errors
/// I/O errors writing `out`.
pub fn execute(
    agent: Agent,
    plan: &Plan,
    opts: InstallOptions,
    runner: Runner<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> std::io::Result<i32> {
    let slug = agent.slug();
    let report = |out: &mut dyn Write,
                  err: &mut dyn Write,
                  ok: bool,
                  result: Value,
                  text: &str|
     -> std::io::Result<i32> {
        if opts.json {
            writeln!(out, "{result}")?;
        } else if ok {
            writeln!(out, "{text}")?;
        } else {
            writeln!(err, "{text}")?;
        }
        Ok(if ok { 0 } else { 1 })
    };
    match plan {
        Plan::Cli { bin, add, remove } => {
            let argv = if opts.uninstall { remove } else { add };
            let command = format!("{bin} {}", argv.join(" "));
            if opts.print {
                return report(
                    out,
                    err,
                    true,
                    json!({ "ok": true, "agent": slug, "kind": "cli", "command": command }),
                    &format!("would run: {command}"),
                );
            }
            let code = match runner(bin, argv) {
                Ok(code) => code,
                Err(e) => {
                    let msg = format!("failed to run {bin}: {e} (is {slug} installed?)");
                    return report(
                        out,
                        err,
                        false,
                        json!({ "ok": false, "agent": slug, "kind": "cli", "message": msg }),
                        &msg,
                    );
                }
            };
            if code != 0 {
                let msg = format!("{bin} exited with code {code}");
                report(
                    out,
                    err,
                    false,
                    json!({ "ok": false, "agent": slug, "kind": "cli", "message": msg }),
                    &msg,
                )?;
                return Ok(code);
            }
            let msg = if opts.uninstall {
                format!("removed {SERVER_NAME} from {slug}")
            } else {
                format!("installed {SERVER_NAME} into {slug}")
            };
            report(
                out,
                err,
                true,
                json!({ "ok": true, "agent": slug, "kind": "cli", "message": msg }),
                &msg,
            )
        }
        Plan::Json {
            config_path,
            key_path,
            server_key,
            entry,
        } => {
            let existing = match std::fs::read_to_string(config_path) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => {
                    let msg = format!("reading {}: {e}", config_path.display());
                    return report(
                        out,
                        err,
                        false,
                        json!({ "ok": false, "agent": slug, "message": msg }),
                        &msg,
                    );
                }
            };
            let next = if opts.uninstall {
                remove_json(existing.as_deref(), config_path, key_path, server_key)
            } else {
                merge_json(
                    existing.as_deref(),
                    config_path,
                    key_path,
                    server_key,
                    entry,
                )
                .map(Some)
            };
            let next = match next {
                Ok(next) => next,
                Err(msg) => {
                    return report(
                        out,
                        err,
                        false,
                        json!({ "ok": false, "agent": slug, "message": msg }),
                        &msg,
                    );
                }
            };
            let path = config_path.display().to_string();
            let Some(next) = next else {
                let msg = format!("{server_key} not present in {path}; nothing to remove");
                return report(
                    out,
                    err,
                    true,
                    json!({ "ok": true, "agent": slug, "kind": "json", "configPath": path, "message": msg }),
                    &msg,
                );
            };
            if opts.print {
                if opts.json {
                    let preview: Value = serde_json::from_str(&next).unwrap_or(Value::Null);
                    writeln!(
                        out,
                        "{}",
                        json!({ "ok": true, "agent": slug, "kind": "json", "configPath": path, "preview": preview })
                    )?;
                } else {
                    out.write_all(next.as_bytes())?;
                    writeln!(err, "(dry run) would write {path}")?;
                }
                return Ok(0);
            }
            if let Err(e) = write_atomic(config_path, next.as_bytes()) {
                let msg = format!("writing {path}: {e}");
                return report(
                    out,
                    err,
                    false,
                    json!({ "ok": false, "agent": slug, "message": msg }),
                    &msg,
                );
            }
            let msg = if opts.uninstall {
                format!("removed {server_key} from {path}")
            } else {
                format!("installed {server_key} into {path}")
            };
            report(
                out,
                err,
                true,
                json!({ "ok": true, "agent": slug, "kind": "json", "configPath": path, "message": msg }),
                &msg,
            )
        }
    }
}

/// Write to a sibling temp file, then rename over the target.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// The user's home directory (`HOME`, or `USERPROFILE` on Windows).
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn spec() -> LaunchSpec {
        LaunchSpec {
            command: "/opt/od".into(),
            args: vec!["mcp".into()],
        }
    }

    #[test]
    fn cli_plans_match_the_agents_commands() {
        let home = Path::new("/home/u");
        assert_eq!(
            plan(Agent::Claude, &spec(), home, SERVER_NAME),
            Plan::Cli {
                bin: "claude",
                add: [
                    "mcp",
                    "add",
                    "--scope",
                    "user",
                    "open-design",
                    "--",
                    "/opt/od",
                    "mcp"
                ]
                .map(String::from)
                .to_vec(),
                remove: ["mcp", "remove", "--scope", "user", "open-design"]
                    .map(String::from)
                    .to_vec(),
            }
        );
        let Plan::Cli { add, .. } = plan(Agent::Codex, &spec(), home, SERVER_NAME) else {
            panic!()
        };
        assert_eq!(add, ["mcp", "add", "open-design", "--", "/opt/od", "mcp"]);
    }

    #[test]
    fn merge_keeps_other_entries_and_keys() {
        let existing = r#"{"mcpServers":{"other":{"command":"x"}},"theme":"dark"}"#;
        let out = merge_json(
            Some(existing),
            Path::new("c.json"),
            &["mcpServers"],
            "open-design",
            &json!({ "command": "od" }),
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["other"], json!({ "command": "x" }));
        assert_eq!(v["mcpServers"]["open-design"], json!({ "command": "od" }));
        assert_eq!(v["theme"], "dark");
        let removed = remove_json(
            Some(&out),
            Path::new("c.json"),
            &["mcpServers"],
            "open-design",
        )
        .unwrap()
        .unwrap();
        let v: Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(
            v,
            json!({ "mcpServers": { "other": { "command": "x" } }, "theme": "dark" })
        );
        assert_eq!(
            remove_json(
                Some(&removed),
                Path::new("c.json"),
                &["mcpServers"],
                "open-design"
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn invalid_config_is_never_overwritten() {
        assert!(
            merge_json(
                Some("{ nope"),
                Path::new("c.json"),
                &["mcpServers"],
                "k",
                &json!({})
            )
            .is_err()
        );
        assert!(
            merge_json(
                Some("[]"),
                Path::new("c.json"),
                &["mcpServers"],
                "k",
                &json!({})
            )
            .is_err()
        );
        assert!(
            merge_json(
                Some(r#"{"mcpServers":3}"#),
                Path::new("c.json"),
                &["mcpServers"],
                "k",
                &json!({})
            )
            .is_err()
        );
    }

    #[test]
    fn cli_print_does_not_run_and_failures_propagate() {
        let ran = RefCell::new(Vec::new());
        let runner = |bin: &str, args: &[String]| {
            ran.borrow_mut().push(format!("{bin} {}", args.join(" ")));
            Ok(3)
        };
        let p = plan(Agent::Claude, &spec(), Path::new("/h"), SERVER_NAME);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let print = InstallOptions {
            print: true,
            ..InstallOptions::default()
        };
        assert_eq!(
            execute(Agent::Claude, &p, print, &runner, &mut out, &mut err).unwrap(),
            0
        );
        assert!(ran.borrow().is_empty());
        assert!(String::from_utf8_lossy(&out).starts_with("would run: claude mcp add"));
        assert_eq!(
            execute(
                Agent::Claude,
                &p,
                InstallOptions::default(),
                &runner,
                &mut out,
                &mut err
            )
            .unwrap(),
            3
        );
        assert_eq!(
            ran.borrow().as_slice(),
            ["claude mcp add --scope user open-design -- /opt/od mcp"]
        );
    }
}
