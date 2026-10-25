//! Handoff (spec 1.3): hand the design to a coding agent. Mounted with one
//! `merge` in [`crate::router`].
//!
//! - `GET  /api/projects/:id/handoff`: project folder and a ready prompt.
//! - `POST /api/projects/:id/handoff/open`: opens the folder in the OS file
//!   manager (`open`, `xdg-open`, `explorer`; `OD_OPEN_COMMAND` overrides).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use od_contracts::{FileKind, HandoffResponse, OkResponse, Project, ProjectFile};

/// Overrides the program that opens a folder (tests set a no-op).
pub const OPEN_COMMAND_ENV: &str = "OD_OPEN_COMMAND";

/// Files listed in the prompt at most.
const MAX_LISTED: usize = 40;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/handoff", get(handoff))
        .route("/api/projects/{id}/handoff/open", post(open_folder))
}

/// The prompt a coding agent gets: where the design lives and what to do.
#[must_use]
pub fn handoff_prompt(project: &Project, dir: &str, files: &[ProjectFile]) -> String {
    let entry = project
        .metadata
        .as_ref()
        .and_then(|m| m.entry_file.clone())
        .or_else(|| {
            files
                .iter()
                .find(|f| f.path == "index.html")
                .or_else(|| files.iter().find(|f| f.kind == FileKind::Html))
                .map(|f| f.path.clone())
        });
    let mut out = format!(
        "Implement the design \"{}\" in this codebase.\n\n\
         The design was made with Contour. Its files are in `{dir}`.\n",
        project.name
    );
    if let Some(entry) = entry {
        out.push_str(&format!("Start from `{entry}`: it is the main screen.\n"));
    }
    let shown: Vec<&ProjectFile> = files
        .iter()
        .filter(|f| !f.path.starts_with('.') && !f.path.contains("/."))
        .take(MAX_LISTED)
        .collect();
    if !shown.is_empty() {
        out.push_str("\nDesign files:\n");
        for f in &shown {
            out.push_str(&format!("- `{}`\n", f.path));
        }
    }
    out.push_str(
        "\nMatch the layout, spacing, colors and typography exactly. Reuse this \
         codebase's components and styling conventions instead of copying the HTML \
         verbatim. Treat any copy and data in the design as placeholders unless \
         they look final. Ask before adding new dependencies.\n",
    );
    out
}

async fn handoff(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<HandoffResponse>> {
    let (project, dir, files) = state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            let dir = s.project_root(&project)?.dir;
            let files = s.list_files(&project.id)?;
            Ok((project, dir, files))
        })
        .await?;
    let dir = dir.to_string_lossy().into_owned();
    let prompt = handoff_prompt(&project, &dir, &files);
    Ok(ApiJson(HandoffResponse { dir, prompt }))
}

pub fn opener() -> String {
    if let Some(cmd) = std::env::var(OPEN_COMMAND_ENV)
        .ok()
        .filter(|c| !c.trim().is_empty())
    {
        return cmd;
    }
    if cfg!(target_os = "macos") {
        "open".to_owned()
    } else if cfg!(windows) {
        "explorer".to_owned()
    } else {
        "xdg-open".to_owned()
    }
}

async fn open_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<OkResponse>> {
    let dir = state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            Ok(s.project_root(&project)?.dir)
        })
        .await?;
    if !dir.is_dir() {
        return Err(ApiErr::bad_request("project folder is missing"));
    }
    if crate::routes::ui::is_bundle(&dir) {
        return Err(ApiErr::bad_request(
            "the project folder is an app bundle; opening it would launch it",
        ));
    }
    let program = opener();
    // Not awaited: file managers may stay attached to the child.
    let mut child = tokio::process::Command::new(&program)
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| ApiErr::internal(format!("cannot run {program}: {e}")))?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(ApiJson(OkResponse { ok: true }))
}
