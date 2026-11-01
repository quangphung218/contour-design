//! `od tools`: the agent tool group the bundled skills call (spec 2.6).
//!
//! - `tools live-artifacts {create,list,update,refresh}`: a thin wrapper
//!   over `/api/tools/live-artifacts/*` like the original
//!   `tools-live-artifacts-cli.ts`. `--input artifact.json` is read with
//!   the `data.json`, `template.html` and `provenance.json` next to it, and
//!   the run's `OD_TOOL_TOKEN` is sent as the bearer token, so the daemon
//!   takes the project from the run.
//! - `tools connectors {list,execute}`: Contour has no connectors yet.
//!   `list` prints an empty list; `execute` fails with what to do instead,
//!   so the agent stops after one call.
//!
//! Output is one JSON line on stdout, `{"ok":true,…}`; `--format compact`
//! (the default) keeps the fields an agent needs.

use crate::client::Client;
use anyhow::{Context, bail};
use clap::{Subcommand, ValueEnum};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Environment variable with the run's tool token.
pub const TOOL_TOKEN_ENV: &str = "OD_TOOL_TOKEN";

/// What `connectors execute` says: there is nothing to execute.
pub const NO_CONNECTORS: &str =
    "Contour has no connectors yet; use project files or ask the user for the data.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// The fields an agent needs.
    #[default]
    Compact,
    /// The daemon's whole response.
    Json,
}

#[derive(Debug, Subcommand)]
pub enum ToolsCommand {
    /// Live artifacts of the run's project (inside an agent run: needs
    /// OD_TOOL_TOKEN).
    #[command(subcommand)]
    LiveArtifacts(LiveCommand),
    /// Connectors: Contour has none yet (`list` is empty).
    #[command(subcommand)]
    Connectors(ConnectorCommand),
}

#[derive(Debug, Subcommand)]
pub enum LiveCommand {
    /// Register a live artifact from artifact.json (with data.json,
    /// template.html and provenance.json from the same folder).
    Create {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, value_enum, default_value_t)]
        format: Format,
    },
    /// List the project's live artifacts.
    List {
        #[arg(long, value_enum, default_value_t)]
        format: Format,
    },
    /// Update a live artifact from artifact.json (and its folder's files).
    Update {
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long, value_enum, default_value_t)]
        format: Format,
    },
    /// Refresh a live artifact from its source (a file in the project).
    Refresh {
        #[arg(long)]
        artifact_id: String,
        #[arg(long, value_enum, default_value_t)]
        format: Format,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConnectorCommand {
    /// Lists connectors: always empty in Contour.
    List {
        /// Accepted for the original command line; ignored.
        #[arg(long)]
        use_case: Option<String>,
        #[arg(long, value_enum, default_value_t)]
        format: Format,
    },
    /// Fails: Contour has no connectors yet.
    Execute {
        #[arg(long)]
        connector: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t)]
        format: Format,
    },
}

/// Runs `od tools …`. `client` makes the daemon client (connectors need
/// none); `token` is the value of [`TOOL_TOKEN_ENV`].
///
/// # Errors
/// No token, no daemon, an unreadable input, a request the daemon
/// refuses (its message), or `connectors execute`.
pub fn run(
    client: impl FnOnce() -> anyhow::Result<Client>,
    token: Option<String>,
    cmd: ToolsCommand,
) -> anyhow::Result<()> {
    let live = match cmd {
        ToolsCommand::Connectors(ConnectorCommand::List { .. }) => {
            println!("{}", json!({ "ok": true, "connectors": [] }));
            return Ok(());
        }
        ToolsCommand::Connectors(ConnectorCommand::Execute { .. }) => bail!(NO_CONNECTORS),
        ToolsCommand::LiveArtifacts(live) => live,
    };
    let token = token
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .with_context(|| {
            format!(
                "{TOOL_TOKEN_ENV} is required: run `od tools live-artifacts` from a Contour agent run, which sets it"
            )
        })?;
    let client = client()?.with_token(token);
    let (body, format, refresh) = match live {
        LiveCommand::Create { input, format } => {
            let body = read_input(&input)?;
            let res: Value = client.send_json("POST", &live_path("create"), &body)?;
            (res, format, false)
        }
        LiveCommand::List { format } => {
            let res: Value = client.get_json(&live_path("list"))?;
            (res, format, false)
        }
        LiveCommand::Update {
            artifact_id,
            input,
            format,
        } => {
            let mut body = read_input(&input)?;
            body.insert("artifactId".to_owned(), json!(artifact_id));
            let res: Value = client.send_json("POST", &live_path("update"), &body)?;
            (res, format, false)
        }
        LiveCommand::Refresh {
            artifact_id,
            format,
        } => {
            let res: Value = client.send_json(
                "POST",
                &live_path("refresh"),
                &json!({ "artifactId": artifact_id }),
            )?;
            (res, format, true)
        }
    };
    let out = match format {
        Format::Json => body,
        Format::Compact => compact(&body, refresh),
    };
    println!("{}", ok(out));
    Ok(())
}

fn live_path(cmd: &'static str) -> [&'static str; 4] {
    ["api", "tools", "live-artifacts", cmd]
}

/// `{"ok":true}` merged with `body` when it is an object.
fn ok(body: Value) -> Value {
    let mut out = Map::new();
    out.insert("ok".to_owned(), json!(true));
    match body {
        Value::Object(m) => out.extend(m),
        other => {
            out.insert("result".to_owned(), other);
        }
    }
    Value::Object(out)
}

/// The fields of an artifact an agent needs.
fn compact_artifact(a: &Value) -> Value {
    let mut out = Map::new();
    for key in [
        "id",
        "title",
        "status",
        "refreshStatus",
        "preview",
        "updatedAt",
    ] {
        if let Some(v) = a.get(key) {
            out.insert(key.to_owned(), v.clone());
        }
    }
    Value::Object(out)
}

fn compact(body: &Value, refresh: bool) -> Value {
    if let Some(list) = body.get("artifacts").and_then(Value::as_array) {
        return json!({ "artifacts": list.iter().map(compact_artifact).collect::<Vec<_>>() });
    }
    let mut out = Map::new();
    if let Some(a) = body.get("artifact") {
        out.insert("artifact".to_owned(), compact_artifact(a));
    }
    if refresh {
        if let Some(r) = body.get("refresh") {
            out.insert("refresh".to_owned(), r.clone());
        }
    }
    Value::Object(out)
}

/// The request body for `create`/`update`: `{"input": artifact.json}`,
/// with `data.json` as `input.document.dataJson`, `template.html` as
/// `templateHtml` and `provenance.json` as `provenanceJson` when those
/// files sit next to it.
///
/// # Errors
/// An unreadable `input`, invalid JSON, or a `data.json` /
/// `provenance.json` that is not a JSON object.
pub fn read_input(input: &Path) -> anyhow::Result<Map<String, Value>> {
    let mut artifact = read_json(input)?;
    let dir = input.parent().unwrap_or(Path::new("."));
    let mut body = Map::new();
    if let Some(data) = read_optional_object(&dir.join("data.json"))? {
        if let Some(doc) = artifact.get_mut("document").and_then(Value::as_object_mut) {
            doc.insert("dataJson".to_owned(), Value::Object(data));
        }
    }
    let template = dir.join("template.html");
    match std::fs::read_to_string(&template) {
        Ok(text) => {
            body.insert("templateHtml".to_owned(), json!(text));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("reading {}", template.display())),
    }
    if let Some(prov) = read_optional_object(&dir.join("provenance.json"))? {
        body.insert("provenanceJson".to_owned(), Value::Object(prov));
    }
    body.insert("input".to_owned(), artifact);
    Ok(body)
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn read_optional_object(path: &Path) -> anyhow::Result<Option<Map<String, Value>>> {
    if !path.exists() {
        return Ok(None);
    }
    match read_json(path)? {
        Value::Object(m) => Ok(Some(m)),
        _ => bail!("{} must contain a JSON object", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_artifact_folder_like_the_original_wrapper() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("artifact.json"),
            r#"{"title":"T","document":{"format":"html_template_v1","dataJson":{}}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("data.json"), r#"{"a":1}"#).unwrap();
        std::fs::write(dir.join("template.html"), "<p>{{data.a}}</p>").unwrap();
        let body = read_input(&dir.join("artifact.json")).unwrap();
        assert_eq!(body["input"]["document"]["dataJson"], json!({"a": 1}));
        assert_eq!(body["templateHtml"], "<p>{{data.a}}</p>");
        assert!(!body.contains_key("provenanceJson"));

        std::fs::write(dir.join("provenance.json"), "[1]").unwrap();
        let err = read_input(&dir.join("artifact.json")).unwrap_err();
        assert!(err.to_string().contains("JSON object"), "{err}");
        std::fs::write(dir.join("provenance.json"), r#"{"sources":[]}"#).unwrap();
        let body = read_input(&dir.join("artifact.json")).unwrap();
        assert_eq!(body["provenanceJson"], json!({"sources": []}));

        let err = read_input(&dir.join("missing.json")).unwrap_err();
        assert!(err.to_string().contains("missing.json"), "{err}");
    }

    #[test]
    fn compact_keeps_what_an_agent_needs() {
        let artifact = json!({"id": "la-1", "title": "T", "status": "active",
            "refreshStatus": "idle", "preview": {"type": "html"}, "updatedAt": "x",
            "document": {"dataJson": {"big": true}}});
        let one = compact(&json!({ "artifact": artifact }), false);
        assert_eq!(one["artifact"]["id"], "la-1");
        assert!(one["artifact"].get("document").is_none());
        let list = compact(&json!({ "artifacts": [artifact] }), false);
        assert_eq!(list["artifacts"][0]["refreshStatus"], "idle");
        let refreshed = compact(
            &json!({ "artifact": {}, "refresh": {"status": "succeeded"} }),
            true,
        );
        assert_eq!(refreshed["refresh"]["status"], "succeeded");
        assert_eq!(ok(json!({"a": 1})), json!({"ok": true, "a": 1}));
    }

    #[test]
    fn connectors_are_empty_and_execute_says_what_to_do() {
        let never = || -> anyhow::Result<Client> { panic!("connectors need no daemon") };
        run(
            never,
            None,
            ToolsCommand::Connectors(ConnectorCommand::List {
                use_case: None,
                format: Format::Compact,
            }),
        )
        .unwrap();
        let err = run(
            never,
            None,
            ToolsCommand::Connectors(ConnectorCommand::Execute {
                connector: Some("gmail".into()),
                tool: None,
                input: None,
                format: Format::Compact,
            }),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), NO_CONNECTORS);
    }

    #[test]
    fn live_artifacts_need_the_tool_token() {
        let never = || -> anyhow::Result<Client> { panic!("refused before any request") };
        for token in [None, Some("  ".to_owned())] {
            let err = run(
                never,
                token,
                ToolsCommand::LiveArtifacts(LiveCommand::List {
                    format: Format::Compact,
                }),
            )
            .unwrap_err();
            assert!(err.to_string().contains(TOOL_TOKEN_ENV), "{err}");
        }
    }
}
