//! The 20 MCP tools of spec 2.5 (the original's 22 minus the two cloud
//! login tools). Definitions are plain JSON Schema; every call is one or a
//! few HTTP requests to the daemon, so listing works with the daemon down.

use crate::brief::BriefStore;
use crate::client::{Daemon, DaemonError, agent_text, end_artifact_paths};
use base64::Engine;
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::sync::LazyLock;

/// What a tool call returns: text blocks, optional structured JSON, error
/// flag. `server` turns it into an MCP `CallToolResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub text: Vec<String>,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    fn json(payload: Value) -> Self {
        let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
        Self {
            text: vec![text],
            structured: payload.is_object().then_some(payload),
            is_error: false,
        }
    }

    fn texts(text: Vec<String>) -> Self {
        Self {
            text,
            structured: None,
            is_error: false,
        }
    }

    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            text: vec![message.into()],
            structured: None,
            is_error: true,
        }
    }
}

/// Tool failure carried to the client as `isError: true`.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Daemon(#[from] DaemonError),
}

type ToolResult = Result<ToolOutput, ToolError>;

fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::Invalid(msg.into())
}

const PROJECT_ARG: &str = "Project id or name (exact, slug or unique substring). Optional: defaults to the project the user has open in Contour. That active context expires about 5 minutes after the last interaction.";

const ARTIFACT_TYPES: [&str; 8] = [
    "website",
    "product-prototype",
    "presentation",
    "document",
    "image",
    "video",
    "audio",
    "design-system",
];

fn read_only(title: &str) -> Value {
    json!({ "title": title, "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false })
}

fn write(title: &str) -> Value {
    json!({ "title": title, "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

fn destructive(title: &str) -> Value {
    json!({ "title": title, "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false })
}

fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    annotations: Value,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
        "annotations": annotations,
    })
}

/// Tool definitions in MCP `tools/list` shape.
pub static TOOL_DEFS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    let project = json!({ "type": "string", "description": PROJECT_ARG });
    vec![
        tool(
            "collect_brief",
            "Start an Contour brief for a new artifact. Returns the questions to ask (each with stable option values and a recommended default) plus an internal draft id and nonce for confirm_brief. Never show the draft id or nonce to the user.",
            json!({
                "artifactType": { "type": "string", "enum": ARTIFACT_TYPES, "description": "Artifact workflow whose brief should be collected." },
                "projectTitle": { "type": "string", "description": "Concise human-readable project title." },
                "locale": { "type": "string", "description": "BCP-47 language of the user's request." },
                "knownAnswers": { "type": "object", "additionalProperties": true, "description": "Question-id answers the user already gave." },
                "skip": { "type": "boolean", "description": "Use recommended defaults without asking. Defaults to false." },
            }),
            &["artifactType"],
            write("Collect Contour brief"),
        ),
        tool(
            "confirm_brief",
            "Confirm the answers to a brief from collect_brief. Returns a readable summary to pass to start_run as the prompt.",
            json!({
                "briefDraftId": { "type": "string", "description": "Internal draft id returned by collect_brief." },
                "nonce": { "type": "string", "description": "Internal nonce returned by collect_brief." },
                "answers": { "type": "object", "additionalProperties": true, "description": "Question id to selected option value." },
                "locale": { "type": "string", "description": "BCP-47 locale, used when collect_brief had none." },
            }),
            &["briefDraftId", "nonce", "answers"],
            write("Confirm Contour brief"),
        ),
        tool(
            "list_projects",
            "List every Contour project on this daemon.",
            json!({}),
            &[],
            read_only("List Contour projects"),
        ),
        tool(
            "get_active_context",
            "Project + file the user has open in Contour right now. Returns {active:false, hint} when none is active (the context expires about 5 minutes after the last interaction). Most tools default to it when project is omitted.",
            json!({}),
            &[],
            read_only("What is the user looking at?"),
        ),
        tool(
            "get_artifact",
            "PREFER THIS over several get_file calls. Bundles the entry file plus every sibling it references (HTML src/href/srcset, CSS url()/@import, JS import/require) up to depth 3, skipping remote and data: URLs. include=\"all\" returns every text file; include=\"shallow\" only the entry.",
            json!({
                "project": project,
                "entry": { "type": "string", "description": "Entry file path. Defaults to the active file, then the project's entryFile, then index.html." },
                "include": { "type": "string", "enum": ["auto", "all", "shallow"], "description": "auto (default) | all | shallow" },
                "maxBytes": { "type": "number", "description": "Soft cap on total text bytes (default 1500000). Also capped at 200 files; excess files are dropped and truncated:true is set." },
            }),
            &[],
            read_only("Pull design bundle"),
        ),
        tool(
            "get_project",
            "Single project metadata: name, skill and design-system ids, entryFile, kind, timestamps and, when it has an entry file, a browser-openable previewUrl.",
            json!({ "project": project }),
            &[],
            read_only("Get Contour project"),
        ),
        tool(
            "get_file",
            "Read one text file (HTML, JSX, CSS, JSON, SVG, Markdown…); binary files are an error. Returns up to `limit` lines from `offset` (defaults 0 and 2000). A longer file carries an [od:file-window …] marker with totalLines so you can page.",
            json!({
                "project": project,
                "path": { "type": "string", "description": "File path relative to the project root. Defaults to the active file when project is omitted too." },
                "offset": { "type": "number", "description": "0-indexed first line. Defaults to 0." },
                "limit": { "type": "number", "description": "Maximum lines. Defaults to 2000." },
            }),
            &[],
            read_only("Read project file"),
        ),
        tool(
            "search_files",
            "Case-insensitive literal substring search across a project's text files. Returns up to max matches with file, 1-indexed line and snippet.",
            json!({
                "project": project,
                "query": { "type": "string", "description": "Literal substring (not a regex), case-insensitive." },
                "pattern": { "type": "string", "description": "Optional glob on the file path, e.g. \"*.jsx\"." },
                "max": { "type": "number", "description": "Cap on matches (default 200, hard cap 1000)." },
            }),
            &["query"],
            read_only("Search project files"),
        ),
        tool(
            "list_files",
            "Project file metadata: name, path, mime, kind, size, mtime, optional artifactManifest. Pass since=<unix-ms> to poll for changes cheaply.",
            json!({
                "project": project,
                "since": { "type": "number", "description": "Unix ms; only files with mtime > since." },
            }),
            &[],
            read_only("List project files"),
        ),
        tool(
            "create_artifact",
            "Create one artifact entry file. Rejects an existing target. HTML, Markdown and SVG entries get a default artifactManifest when none is given. Project defaults to the active project.",
            json!({
                "project": project,
                "name": { "type": "string", "description": "Path relative to the project root, e.g. \"deck.html\"." },
                "content": { "type": "string", "description": "File contents; base64 when encoding=\"base64\"." },
                "encoding": { "type": "string", "enum": ["utf8", "base64"], "description": "utf8 (default) | base64" },
                "artifactManifest": { "type": "object", "additionalProperties": true, "description": "Optional ArtifactManifest sidecar." },
            }),
            &["name", "content"],
            write("Create Contour artifact"),
        ),
        tool(
            "write_file",
            "Write or overwrite a project file. Unlike create_artifact it tolerates existing targets: use it to iterate on a file. Project defaults to the active project.",
            json!({
                "project": project,
                "path": { "type": "string", "description": "Path relative to the project root, e.g. \"components/Hero.tsx\"." },
                "content": { "type": "string", "description": "File contents; base64 when encoding=\"base64\"." },
                "encoding": { "type": "string", "enum": ["utf8", "base64"], "description": "utf8 (default) | base64" },
            }),
            &["path", "content"],
            write("Write Contour project file"),
        ),
        tool(
            "delete_file",
            "Delete one file from a project (nested paths allowed). Project defaults to the active project.",
            json!({
                "project": project,
                "path": { "type": "string", "description": "Project-relative path of the file to delete." },
            }),
            &["path"],
            destructive("Delete Contour project file"),
        ),
        tool(
            "delete_project",
            "Permanently delete a project with its files and conversations. Requires an explicit project AND confirm:true; there is no active-project fallback.",
            json!({
                "project": { "type": "string", "description": "Project id or name. Required: no active-context fallback." },
                "confirm": { "type": "boolean", "description": "Must be literally true." },
            }),
            &["project", "confirm"],
            destructive("Delete Contour project"),
        ),
        tool(
            "create_project",
            "Create an empty project to generate into, then call start_run on it. Returns the project (with its id) and a conversationId.",
            json!({
                "name": { "type": "string", "description": "Human-readable project name." },
                "id": { "type": "string", "description": "Optional id slug ([A-Za-z0-9._-], <=128 chars)." },
                "designSystem": { "type": "string", "description": "Optional design system id (see od://design-systems/… resources)." },
                "skill": { "type": "string", "description": "Optional skill id to seed the project with." },
            }),
            &["name"],
            write("Create Contour project"),
        ),
        tool(
            "list_skills",
            "List Contour skills you can pass to start_run. Discovery only: Contour runs the skill, not you.",
            json!({}),
            &[],
            read_only("List Contour skills"),
        ),
        tool(
            "list_plugins",
            "List installed Contour plugins you can pass to start_run as plugin + inputs.",
            json!({}),
            &[],
            read_only("List Contour plugins"),
        ),
        tool(
            "start_run",
            "Commission Contour to generate or refine a design. Contour runs its own agent and returns a runId at once; poll get_run until the status is terminal. Project defaults to the active project and must exist (create_project first).",
            json!({
                "project": project,
                "prompt": { "type": "string", "description": "What to make or change, in natural language." },
                "skill": { "type": "string", "description": "Skill id from list_skills. Optional." },
                "skills": { "type": "array", "items": { "type": "string" }, "description": "More skill ids to compose in. Optional." },
                "plugin": { "type": "string", "description": "Plugin id from list_plugins. Optional." },
                "inputs": { "type": "object", "additionalProperties": true, "description": "Plugin inputs (with plugin). Optional." },
                "agent": { "type": "string", "description": "Agent id from list_agents. Optional; defaults to the first installed agent." },
                "model": { "type": "string", "description": "Model override. Optional." },
                "requestId": { "type": "string", "description": "Stable id for this generation; reuse it verbatim on retry so the run is not started twice." },
            }),
            &[],
            write("Generate with Contour"),
        ),
        tool(
            "get_run",
            "Poll a run from start_run: status (queued|running|succeeded|failed|canceled) and error info. On success adds previewUrl (open it in a browser) and agentMessage (the inner agent's text, useful when it asked a question instead of writing files).",
            json!({ "runId": { "type": "string", "description": "Run id returned by start_run." } }),
            &["runId"],
            read_only("Check Contour run"),
        ),
        tool(
            "cancel_run",
            "Request cancellation of an in-flight run started by start_run.",
            json!({ "runId": { "type": "string", "description": "Run id returned by start_run." } }),
            &["runId"],
            write("Cancel Contour run"),
        ),
        tool(
            "list_agents",
            "List the agent CLIs Contour can run for start_run.agent. Only installed agents by default; includeUnavailable:true adds the rest. Up to 10 sample models each.",
            json!({ "includeUnavailable": { "type": "boolean", "description": "Include agents that are not installed. Defaults to false." } }),
            &[],
            read_only("List Contour agents"),
        ),
    ]
});

// ---------------------------------------------------------------------------
// Argument helpers

fn opt_str<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn req_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, ToolError> {
    opt_str(args, key).ok_or_else(|| invalid(format!("{key} is required (string).")))
}

fn opt_usize(args: &Map<String, Value>, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n as usize)
}

fn path_segments<'a>(base: &[&'a str], path: &'a str) -> Vec<&'a str> {
    let mut segs = base.to_vec();
    segs.extend(path.split('/').filter(|s| !s.is_empty()));
    segs
}

// ---------------------------------------------------------------------------
// Project resolution (spec 2.5: omitted project → active project)

/// Active context as `GET /api/active` reports it.
#[derive(Debug, Clone, PartialEq)]
struct Active {
    project_id: String,
    project_name: Option<String>,
    file_name: Option<String>,
    age_ms: Option<i64>,
}

impl Active {
    fn echo(&self) -> Value {
        json!({
            "projectId": self.project_id,
            "projectName": self.project_name,
            "fileName": self.file_name,
            "ageMs": self.age_ms,
        })
    }
}

/// How the project argument was resolved.
#[derive(Debug, Clone)]
struct Resolved {
    id: String,
    /// Set when it came from the active context.
    active: Option<Active>,
    /// Set when a fuzzy name match picked it.
    matched: Option<(String, &'static str)>,
}

impl Resolved {
    fn annotate(&self, mut payload: Value) -> Value {
        if let Some(obj) = payload.as_object_mut() {
            if let Some(active) = &self.active {
                obj.insert("usedActiveContext".into(), active.echo());
            }
            if let Some((name, via)) = &self.matched {
                obj.insert(
                    "resolvedProject".into(),
                    json!({ "id": self.id, "name": name, "via": via }),
                );
            }
        }
        payload
    }

    fn markers(&self, path: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(a) = &self.active {
            let project = a.project_name.as_deref().unwrap_or(&a.project_id);
            let mut note = format!("[od:active-context project=\"{project}\" file=\"{path}\"]");
            if a.file_name.as_deref() != Some(path) {
                note.push_str(&format!(
                    " (active file: {})",
                    a.file_name.as_deref().unwrap_or("none")
                ));
            }
            out.push(note);
        }
        if let Some((name, via)) = &self.matched {
            out.push(format!(
                "[od:resolved-project id=\"{}\" name=\"{name}\" via=\"{via}\"]",
                self.id
            ));
        }
        out
    }
}

pub const NO_ACTIVE_PROJECT: &str = "project omitted and Contour has no active project. The active context expires about 5 minutes after the last interaction with Contour: ask the user to click into the project, or pass project=\"<id-or-name>\".";

async fn active(daemon: &Daemon) -> Result<Option<Active>, ToolError> {
    let v = daemon.get(&["api", "active"]).await?;
    if v.get("active") != Some(&Value::Bool(true)) {
        return Ok(None);
    }
    let Some(project_id) = v.get("projectId").and_then(Value::as_str) else {
        return Ok(None);
    };
    Ok(Some(Active {
        project_id: project_id.to_owned(),
        project_name: v
            .get("projectName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        file_name: v.get("fileName").and_then(Value::as_str).map(str::to_owned),
        age_ms: v.get("ageMs").and_then(Value::as_i64),
    }))
}

fn slug(s: &str) -> String {
    static SUFFIX: LazyLock<regex::Regex> = LazyLock::new(|| {
        #[allow(clippy::unwrap_used)] // literal pattern
        regex::Regex::new(r"\s*\(\d+\)\s*$").unwrap()
    });
    static SEP: LazyLock<regex::Regex> = LazyLock::new(|| {
        #[allow(clippy::unwrap_used)] // literal pattern
        regex::Regex::new(r"[\s_-]+").unwrap()
    });
    let lower = s.to_lowercase();
    SEP.replace_all(&SUFFIX.replace(&lower, ""), "-")
        .into_owned()
}

/// Id, then exact name, slug, unique substring.
async fn resolve_named(daemon: &Daemon, arg: &str) -> Result<Resolved, ToolError> {
    let list = daemon.get(&["api", "projects"]).await?;
    let projects: Vec<(String, String)> = list
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p.get("id")?.as_str()?.to_owned(),
                p.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            ))
        })
        .collect();
    if let Some((id, _)) = projects.iter().find(|(id, _)| id == arg) {
        return Ok(Resolved {
            id: id.clone(),
            active: None,
            matched: None,
        });
    }
    let lower = arg.to_lowercase();
    let pick = |hits: Vec<&(String, String)>, via: &'static str| -> Option<Resolved> {
        match hits.as_slice() {
            [(id, name)] => Some(Resolved {
                id: id.clone(),
                active: None,
                matched: Some((name.clone(), via)),
            }),
            _ => None,
        }
    };
    if let Some(r) = pick(
        projects
            .iter()
            .filter(|(_, n)| n.to_lowercase() == lower)
            .collect(),
        "exact",
    ) {
        return Ok(r);
    }
    let target = slug(arg);
    if let Some(r) = pick(
        projects.iter().filter(|(_, n)| slug(n) == target).collect(),
        "slug",
    ) {
        return Ok(r);
    }
    let subs: Vec<&(String, String)> = projects
        .iter()
        .filter(|(_, n)| n.to_lowercase().contains(&lower))
        .collect();
    if subs.len() > 1 {
        let opts = subs
            .iter()
            .map(|(id, n)| format!("{n} ({id})"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(invalid(format!(
            "multiple projects match \"{arg}\": {opts}. Pass the id instead."
        )));
    }
    pick(subs, "substring").ok_or_else(|| {
        if projects.is_empty() {
            invalid("no projects on this daemon")
        } else {
            invalid(format!("no project matches \"{arg}\""))
        }
    })
}

async fn resolve(daemon: &Daemon, args: &Map<String, Value>) -> Result<Resolved, ToolError> {
    if let Some(arg) = opt_str(args, "project") {
        return resolve_named(daemon, arg).await;
    }
    let a = active(daemon)
        .await
        .map_err(|e| invalid(format!("project omitted and the active context lookup failed: {e}. Pass project=\"<id-or-name>\".")))?
        .ok_or_else(|| invalid(NO_ACTIVE_PROJECT))?;
    Ok(Resolved {
        id: a.project_id.clone(),
        active: Some(a),
        matched: None,
    })
}

// ---------------------------------------------------------------------------
// Dispatch

/// Runs tool `name` with `args`.
pub async fn call(
    daemon: &Daemon,
    briefs: &BriefStore,
    name: &str,
    args: Map<String, Value>,
) -> ToolOutput {
    let result = match name {
        "collect_brief" => briefs.collect(&args).map(ToolOutput::json).map_err(invalid),
        "confirm_brief" => briefs.confirm(&args).map(ToolOutput::json).map_err(invalid),
        "list_projects" => daemon
            .get(&["api", "projects"])
            .await
            .map(ToolOutput::json)
            .map_err(Into::into),
        "get_active_context" => get_active_context(daemon).await,
        "get_artifact" => get_artifact(daemon, &args).await,
        "get_project" => get_project(daemon, &args).await,
        "get_file" => get_file(daemon, &args).await,
        "search_files" => search_files(daemon, &args).await,
        "list_files" => list_files(daemon, &args).await,
        "create_artifact" => create_artifact(daemon, &args).await,
        "write_file" => write_file(daemon, &args).await,
        "delete_file" => delete_file(daemon, &args).await,
        "delete_project" => delete_project(daemon, &args).await,
        "create_project" => create_project(daemon, &args).await,
        "list_skills" => list_skills(daemon).await,
        "list_plugins" => list_plugins(daemon).await,
        "start_run" => start_run(daemon, &args).await,
        "get_run" => get_run(daemon, &args).await,
        "cancel_run" => cancel_run(daemon, &args).await,
        "list_agents" => list_agents(daemon, &args).await,
        other => Err(invalid(format!("unknown tool: {other}"))),
    };
    result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
}

async fn get_active_context(daemon: &Daemon) -> ToolResult {
    match active(daemon).await? {
        Some(a) => Ok(ToolOutput::json(json!({
            "active": true,
            "projectId": a.project_id,
            "projectName": a.project_name,
            "fileName": a.file_name,
            "ageMs": a.age_ms,
        }))),
        None => Ok(ToolOutput::json(json!({
            "active": false,
            "hint": "No project is open in Contour (the active context expires about 5 minutes after the last interaction). Ask the user to open the project, or pass project explicitly.",
        }))),
    }
}

async fn project_json(daemon: &Daemon, id: &str) -> Result<Value, ToolError> {
    let v = daemon.get(&["api", "projects", id]).await?;
    Ok(v.get("project").cloned().unwrap_or(v))
}

fn entry_file(project: &Value) -> Option<String> {
    project
        .pointer("/metadata/entryFile")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

async fn get_project(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let project = project_json(daemon, &r.id).await?;
    let mut payload = json!({ "project": project });
    if let Some(entry) = entry_file(&payload["project"]) {
        payload["previewUrl"] = json!(daemon.raw_url(&r.id, &entry));
    }
    Ok(ToolOutput::json(r.annotate(payload)))
}

fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/javascript"
                | "application/xml"
                | "image/svg+xml"
                | "application/x-javascript"
                | "application/typescript"
                | "application/ld+json"
                | "application/manifest+json"
        )
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
}

async fn get_file(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let path = match opt_str(args, "path") {
        Some(p) => p.to_owned(),
        None => r
            .active
            .as_ref()
            .and_then(|a| a.file_name.clone())
            .ok_or_else(|| invalid("path is required (no active file to default to)."))?,
    };
    let raw = daemon.raw(&r.id, &path).await?;
    if !is_text_mime(&raw.mime) {
        return Err(invalid(format!(
            "file at \"{path}\" has mime \"{}\"; od mcp only returns text. Use list_files for its metadata.",
            raw.mime
        )));
    }
    let text = String::from_utf8_lossy(&raw.bytes);
    let lines: Vec<&str> = text.split('\n').collect();
    let total = lines.len();
    let offset = opt_usize(args, "offset").unwrap_or(0).min(total);
    let limit = opt_usize(args, "limit").unwrap_or(2_000).max(1);
    let slice = &lines[offset..(offset + limit).min(total)];
    let truncated = offset + slice.len() < total;
    let mut out = r.markers(&path);
    if truncated || offset > 0 {
        let next = if truncated {
            format!(
                "; call get_file again with offset={} to read more",
                offset + slice.len()
            )
        } else {
            String::new()
        };
        out.push(format!(
            "[od:file-window offset={offset} returnedLines={} totalLines={total}{next}]",
            slice.len()
        ));
    }
    out.push(slice.join("\n"));
    Ok(ToolOutput::texts(out))
}

async fn files(daemon: &Daemon, id: &str) -> Result<Vec<Value>, ToolError> {
    let v = daemon.get(&["api", "projects", id, "files"]).await?;
    Ok(v.get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn file_name(f: &Value) -> &str {
    f.get("name").and_then(Value::as_str).unwrap_or("")
}

fn file_mime(f: &Value) -> &str {
    f.get("mime").and_then(Value::as_str).unwrap_or("")
}

async fn list_files(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let mut list = files(daemon, &r.id).await?;
    if let Some(since) = args.get("since").and_then(Value::as_f64) {
        list.retain(|f| {
            f.get("mtime")
                .and_then(Value::as_f64)
                .is_some_and(|m| m > since)
        });
    }
    Ok(ToolOutput::json(r.annotate(json!({ "files": list }))))
}

/// `*` and `?` glob on the whole path, or on the base name when the pattern
/// has no `/`.
fn glob_match(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => go(&p[1..], s) || (!s.is_empty() && go(p, &s[1..])),
            (Some(b'?'), Some(_)) => go(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => go(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let target = if pattern.contains('/') {
        path
    } else {
        path.rsplit('/').next().unwrap_or(path)
    };
    go(pattern.as_bytes(), target.as_bytes())
}

async fn search_files(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let query = req_str(args, "query")?.to_lowercase();
    let max = opt_usize(args, "max").unwrap_or(200).clamp(1, 1_000);
    let pattern = opt_str(args, "pattern");
    let mut matches = Vec::new();
    let mut truncated = false;
    'files: for f in files(daemon, &r.id).await? {
        let name = file_name(&f);
        if !is_text_mime(file_mime(&f)) || pattern.is_some_and(|p| !glob_match(p, name)) {
            continue;
        }
        let Ok(raw) = daemon.raw(&r.id, name).await else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw.bytes);
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&query) {
                if matches.len() == max {
                    truncated = true;
                    break 'files;
                }
                let snippet: String = line.trim().chars().take(200).collect();
                matches.push(json!({ "file": name, "line": i + 1, "snippet": snippet }));
            }
        }
    }
    Ok(ToolOutput::json(r.annotate(
        json!({ "matches": matches, "truncated": truncated }),
    )))
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

static REF_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern
    regex::Regex::new(
        r#"(?:\b(?:src|href|poster|data)\s*=\s*["']([^"']+)["']|url\(\s*["']?([^"')]+)["']?\s*\)|@import\s+["']([^"']+)["']|\bfrom\s*["']([^"']+)["']|\bimport\s*\(?\s*["']([^"']+)["']|\brequire\(\s*["']([^"']+)["']\s*\)|\bsrcset\s*=\s*["']([^"']+)["'])"#,
    )
    .unwrap()
});

/// Local references in `text` (file at `owner`), as project paths.
fn local_refs(owner: &str, text: &str) -> Vec<String> {
    let dir = owner.rsplit_once('/').map_or("", |(d, _)| d);
    let mut out = Vec::new();
    for caps in REF_RE.captures_iter(text) {
        let values: Vec<&str> = match caps.get(7) {
            Some(srcset) => srcset
                .as_str()
                .split(',')
                .filter_map(|c| c.split_whitespace().next())
                .collect(),
            None => (1..=6)
                .filter_map(|i| caps.get(i))
                .map(|m| m.as_str())
                .collect(),
        };
        for v in values {
            let v = v.trim();
            let lower = v.to_ascii_lowercase();
            if v.is_empty()
                || v.starts_with('#')
                || v.starts_with("//")
                || lower.starts_with("data:")
                || lower.contains("://")
                || lower.starts_with("mailto:")
                || lower.starts_with("javascript:")
            {
                continue;
            }
            // Bare module specifiers ("react") are packages, not files.
            let is_path = v.starts_with('.') || v.starts_with('/') || v.contains('.');
            if !is_path {
                continue;
            }
            let v = v.split(['?', '#']).next().unwrap_or("");
            let joined = match v.strip_prefix('/') {
                Some(abs) => abs.to_owned(),
                None if dir.is_empty() => v.to_owned(),
                None => format!("{dir}/{v}"),
            };
            if let Some(p) = normalize(&joined).filter(|p| !p.is_empty()) {
                out.push(p);
            }
        }
    }
    out
}

const ARTIFACT_MAX_FILES: usize = 200;

async fn get_artifact(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let include = opt_str(args, "include").unwrap_or("auto");
    if !matches!(include, "auto" | "all" | "shallow") {
        return Err(invalid("include must be auto, all or shallow"));
    }
    let max_bytes = opt_usize(args, "maxBytes").unwrap_or(1_500_000);
    let listing = files(daemon, &r.id).await?;
    let known: BTreeSet<&str> = listing.iter().map(file_name).collect();
    let entry = match opt_str(args, "entry") {
        Some(e) => e.to_owned(),
        None => {
            let active_file = r.active.as_ref().and_then(|a| a.file_name.clone());
            let project = project_json(daemon, &r.id).await?;
            active_file
                .or_else(|| entry_file(&project))
                .or_else(|| {
                    known
                        .contains("index.html")
                        .then(|| "index.html".to_owned())
                })
                .or_else(|| {
                    listing
                        .iter()
                        .map(file_name)
                        .find(|n| n.ends_with(".html"))
                        .map(str::to_owned)
                })
                .ok_or_else(|| invalid("no entry file: pass entry=\"<path>\""))?
        }
    };

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    if include == "all" {
        queue.push_back((entry.clone(), 0));
        for f in &listing {
            if is_text_mime(file_mime(f)) && file_name(f) != entry {
                queue.push_back((file_name(f).to_owned(), 0));
            }
        }
    } else {
        queue.push_back((entry.clone(), 0));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    while let Some((path, depth)) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        if out.len() == ARTIFACT_MAX_FILES {
            truncated = true;
            break;
        }
        let raw = match daemon.raw(&r.id, &path).await {
            Ok(raw) => raw,
            Err(e) if path == entry => return Err(e.into()),
            Err(_) => {
                skipped.push(json!({ "path": path, "reason": "missing" }));
                continue;
            }
        };
        if !is_text_mime(&raw.mime) {
            skipped.push(json!({ "path": path, "reason": "binary", "mime": raw.mime, "size": raw.bytes.len() }));
            continue;
        }
        if bytes + raw.bytes.len() > max_bytes && !out.is_empty() {
            truncated = true;
            skipped.push(json!({ "path": path, "reason": "maxBytes" }));
            continue;
        }
        bytes += raw.bytes.len();
        let text = String::from_utf8_lossy(&raw.bytes).into_owned();
        if include == "auto" && depth < 3 {
            for dep in local_refs(&path, &text) {
                if known.contains(dep.as_str()) && !seen.contains(&dep) {
                    queue.push_back((dep, depth + 1));
                }
            }
        }
        out.push(json!({ "path": path, "mime": raw.mime, "content": text }));
    }
    Ok(ToolOutput::json(r.annotate(json!({
        "project": r.id,
        "entry": entry,
        "files": out,
        "skipped": skipped,
        "totalBytes": bytes,
        "truncated": truncated,
    }))))
}

fn encoding(args: &Map<String, Value>) -> Result<&'static str, ToolError> {
    match opt_str(args, "encoding") {
        None | Some("utf8") => Ok("utf8"),
        Some("base64") => {
            let content = opt_str(args, "content").unwrap_or("");
            base64::engine::general_purpose::STANDARD
                .decode(content)
                .map_err(|e| invalid(format!("content is not valid base64: {e}")))?;
            Ok("base64")
        }
        Some(other) => Err(invalid(format!(
            "encoding must be utf8 or base64, got {other:?}"
        ))),
    }
}

/// Default manifest for HTML, Markdown and SVG entries.
fn default_manifest(name: &str) -> Option<Value> {
    let lower = name.to_ascii_lowercase();
    let kind = if lower.ends_with(".html") || lower.ends_with(".htm") {
        "html"
    } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
        "markdown-document"
    } else if lower.ends_with(".svg") {
        "svg"
    } else {
        return None;
    };
    let base = name.rsplit('/').next().unwrap_or(name);
    let title = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    Some(json!({
        "version": 1,
        "kind": kind,
        "renderer": kind,
        "entry": name,
        "title": title,
        "status": "complete",
        "exports": [],
    }))
}

async fn create_artifact(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let name = req_str(args, "name")?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("content is required (string)."))?;
    let encoding = encoding(args)?;
    let manifest = match args.get("artifactManifest") {
        Some(m) if m.is_object() => Some(m.clone()),
        Some(Value::Null) | None => default_manifest(name),
        Some(_) => return Err(invalid("artifactManifest must be an object")),
    };
    let mut body =
        json!({ "name": name, "content": content, "encoding": encoding, "overwrite": false });
    if let Some(m) = manifest {
        body["artifactManifest"] = m;
    }
    let res = daemon
        .post(&["api", "projects", &r.id, "files"], &body)
        .await?;
    Ok(ToolOutput::json(r.annotate(res)))
}

async fn write_file(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let path = req_str(args, "path")?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("content is required (string)."))?;
    let body =
        json!({ "name": path, "content": content, "encoding": encoding(args)?, "overwrite": true });
    let res = daemon
        .post(&["api", "projects", &r.id, "files"], &body)
        .await?;
    Ok(ToolOutput::json(r.annotate(res)))
}

async fn delete_file(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let path = req_str(args, "path")?;
    let res = daemon
        .delete(&path_segments(&["api", "projects", &r.id, "files"], path))
        .await?;
    let mut payload = if res.is_object() {
        res
    } else {
        json!({ "ok": true })
    };
    payload["deleted"] = json!(path);
    Ok(ToolOutput::json(r.annotate(payload)))
}

async fn delete_project(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let arg = opt_str(args, "project").ok_or_else(|| {
        invalid("delete_project requires an explicit project (no active-project fallback).")
    })?;
    if args.get("confirm") != Some(&Value::Bool(true)) {
        return Err(invalid("delete_project requires confirm: true."));
    }
    let r = resolve_named(daemon, arg).await?;
    daemon.delete(&["api", "projects", &r.id]).await?;
    Ok(ToolOutput::json(
        r.annotate(json!({ "ok": true, "deleted": r.id })),
    ))
}

async fn create_project(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let mut body = json!({ "name": req_str(args, "name")? });
    if let Some(id) = opt_str(args, "id") {
        body["id"] = json!(id);
    }
    if let Some(ds) = opt_str(args, "designSystem") {
        body["designSystemId"] = json!(ds);
    }
    if let Some(skill) = opt_str(args, "skill") {
        body["skillId"] = json!(skill);
    }
    Ok(ToolOutput::json(
        daemon.post(&["api", "projects"], &body).await?,
    ))
}

async fn list_skills(daemon: &Daemon) -> ToolResult {
    let v = daemon.get(&["api", "skills"]).await?;
    let skills: Vec<Value> = v
        .get("skills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|s| {
            json!({
                "id": s.get("id"),
                "name": s.get("name"),
                "description": s.get("description"),
                "mode": s.get("mode"),
                "surface": s.get("surface"),
            })
        })
        .collect();
    Ok(ToolOutput::json(json!({ "skills": skills })))
}

async fn list_plugins(daemon: &Daemon) -> ToolResult {
    match daemon.get(&["api", "plugins"]).await {
        Ok(v) => Ok(ToolOutput::json(v)),
        Err(e) if e.status() == Some(404) => Ok(ToolOutput::json(json!({
            "plugins": [],
            "note": "this daemon has no plugin catalogue yet",
        }))),
        Err(e) => Err(e.into()),
    }
}

async fn agents(daemon: &Daemon) -> Result<Vec<Value>, ToolError> {
    let v = daemon.get(&["api", "agents"]).await?;
    Ok(v.get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn available(agent: &Value) -> bool {
    agent
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn list_agents(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let include_all = args.get("includeUnavailable") == Some(&Value::Bool(true));
    let list: Vec<Value> = agents(daemon)
        .await?
        .into_iter()
        .filter(|a| include_all || available(a))
        .map(|mut a| {
            if let Some(models) = a.get("models").and_then(Value::as_array).cloned() {
                a["modelsCount"] = json!(models.len());
                a["models"] = Value::Array(models.into_iter().take(10).collect());
            }
            a
        })
        .collect();
    Ok(ToolOutput::json(json!({ "agents": list })))
}

async fn start_run(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let r = resolve(daemon, args).await?;
    let prompt = opt_str(args, "prompt").unwrap_or("");
    if prompt.is_empty() && opt_str(args, "plugin").is_none() {
        return Err(invalid(
            "prompt is required unless a plugin supplies the brief.",
        ));
    }
    let agent = match opt_str(args, "agent") {
        Some(a) => a.to_owned(),
        None => agents(daemon)
            .await?
            .iter()
            .find(|a| available(a))
            .and_then(|a| a.get("id").and_then(Value::as_str).map(str::to_owned))
            .ok_or_else(|| invalid("no installed agent found; install one (see list_agents includeUnavailable:true) or pass agent."))?,
    };
    let request_id =
        opt_str(args, "requestId").map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
    let mut body = json!({
        "agentId": agent,
        "message": prompt,
        "projectId": r.id,
        "clientRequestId": request_id,
    });
    if let Some(skill) = opt_str(args, "skill") {
        body["skillId"] = json!(skill);
    }
    if let Some(skills) = args.get("skills").and_then(Value::as_array) {
        body["skillIds"] = Value::Array(skills.iter().filter(|s| s.is_string()).cloned().collect());
    }
    if let Some(model) = opt_str(args, "model") {
        body["model"] = json!(model);
    }
    if let Some(plugin) = opt_str(args, "plugin") {
        body["pluginId"] = json!(plugin);
        if let Some(inputs) = args.get("inputs").filter(|v| v.is_object()) {
            body["pluginInputs"] = inputs.clone();
        }
    }
    let mut res = daemon.post(&["api", "runs"], &body).await?;
    if let Some(obj) = res.as_object_mut() {
        obj.insert("projectId".into(), json!(r.id));
        obj.insert("requestId".into(), json!(request_id));
        obj.insert(
            "hint".into(),
            json!("Run started. Poll get_run(runId) every 30-60 s until status is succeeded, failed or canceled."),
        );
    }
    Ok(ToolOutput::json(r.annotate(res)))
}

async fn get_run(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let run_id = req_str(args, "runId")?;
    let v = daemon.get(&["api", "runs", run_id]).await?;
    let mut run = v.get("run").cloned().unwrap_or(v);
    let status = run
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let terminal = matches!(status.as_str(), "succeeded" | "failed" | "canceled");
    let events = if terminal {
        daemon.run_events(run_id).await
    } else {
        None
    };
    if status == "succeeded" {
        if let Some(project) = run
            .get("projectId")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            if let Some(entry) = preview_entry(daemon, &project, events.as_deref()).await {
                run["previewUrl"] = json!(daemon.raw_url(&project, &entry));
                run["artifactRef"] = json!({ "projectId": project, "entryFile": entry });
            }
        }
    }
    if terminal {
        let message = events.as_deref().map(agent_text).unwrap_or_default();
        if !message.trim().is_empty() {
            run["agentMessage"] = json!(message);
        }
    } else {
        run["hint"] = json!("Run still in flight. Keep polling get_run every 30-60 s.");
    }
    Ok(ToolOutput::json(run))
}

/// The file a finished run should preview: the project's `entryFile`,
/// else the first HTML file the run wrote (`end.artifactPaths`), else
/// `index.html` when the project has one. New projects have no
/// `entryFile`, so without the fallbacks `get_run` had no `previewUrl`.
async fn preview_entry(daemon: &Daemon, project: &str, events: Option<&str>) -> Option<String> {
    if let Some(entry) = project_json(daemon, project)
        .await
        .ok()
        .as_ref()
        .and_then(entry_file)
    {
        return Some(entry);
    }
    let names: Vec<String> = files(daemon, project)
        .await
        .ok()?
        .iter()
        .map(|f| file_name(f).to_owned())
        .filter(|n| !n.is_empty())
        .collect();
    let written = events.map(end_artifact_paths).unwrap_or_default();
    let is_html = |n: &str| {
        let n = n.to_ascii_lowercase();
        n.ends_with(".html") || n.ends_with(".htm")
    };
    // Absolute paths: match the longest project file name they end with.
    let from_run = written.iter().filter(|p| is_html(p)).find_map(|path| {
        names
            .iter()
            .filter(|n| path.ends_with(&format!("/{n}")) || path == *n)
            .max_by_key(|n| n.len())
            .cloned()
    });
    from_run.or_else(|| names.iter().find(|n| *n == "index.html").cloned())
}

async fn cancel_run(daemon: &Daemon, args: &Map<String, Value>) -> ToolResult {
    let run_id = req_str(args, "runId")?;
    Ok(ToolOutput::json(
        daemon
            .post(&["api", "runs", run_id, "cancel"], &json!({}))
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_tools_with_unique_names() {
        let names: BTreeSet<&str> = TOOL_DEFS
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(names.len(), 20);
        assert_eq!(TOOL_DEFS.len(), 20);
        assert!(!names.iter().any(|n| n.contains("login")));
    }

    #[test]
    fn glob_on_base_name_or_path() {
        assert!(glob_match("*.jsx", "src/App.jsx"));
        assert!(!glob_match("*.jsx", "src/App.tsx"));
        assert!(glob_match("src/*.css", "src/a.css"));
        assert!(glob_match("a?.html", "ab.html"));
    }

    #[test]
    fn refs_resolve_relative_to_owner() {
        let html = r##"<link href="css/a.css"><img srcset="i/1.png 1x, i/2.png 2x"><script src="https://x/y.js"></script><a href="#top">"##;
        assert_eq!(
            local_refs("site/index.html", html),
            ["site/css/a.css", "site/i/1.png", "site/i/2.png"]
        );
        let js = "import React from 'react'; import { x } from '../lib/x.js'; const y = require('./y.json');";
        assert_eq!(
            local_refs("src/app/main.js", js),
            ["src/lib/x.js", "src/app/y.json"]
        );
        assert_eq!(
            local_refs("a.css", "@import 'b.css'; body{background:url(/img/c.png)}"),
            ["b.css", "img/c.png"]
        );
    }

    #[test]
    fn slugs_ignore_case_separators_and_copy_suffix() {
        assert_eq!(slug("My Deck (2)"), "my-deck");
        assert_eq!(slug("my_deck"), "my-deck");
    }

    #[test]
    fn manifests_only_for_known_entries() {
        assert_eq!(default_manifest("a/deck.html").unwrap()["kind"], "html");
        assert_eq!(default_manifest("notes.md").unwrap()["title"], "notes");
        assert!(default_manifest("app.js").is_none());
    }
}
