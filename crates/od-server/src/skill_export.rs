//! "Copy to Claude Code": a Personal skill copied into the agents'
//! `HOME/.claude/skills/<id>` (decision #9).
//!
//! No HTTP route writes it. An agent in a run reaches the daemon on
//! loopback from outside its sandbox, so a route would let it put a skill
//! of its own making into every later Claude Code session. Only the
//! desktop app copies, through its Tauri command (`copy_skill_to_claude`,
//! `apps/desktop`), which calls [`copy_to_claude`] in process. The read
//! only preview (`GET /api/skills/:id/export?target=claude`) shares
//! [`export_source`] with it; the web app shows a `cp` command instead.

use crate::error::{ApiErr, ApiResult};
use crate::state::AppState;
use axum::http::StatusCode;
use od_content::skills::SkillEntry;
use od_content::user_content::{self, ExistingEntry, WriteError};
use od_contracts::ContentSource;
use std::path::PathBuf;

/// `HOME` of the agents' environment (the folders they read their skills
/// from): the last `HOME` set, as an agent process gets it, else the
/// daemon's.
pub(crate) fn agent_home(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Option<PathBuf> {
    env.iter()
        .rfind(|(k, _)| k == "HOME")
        .filter(|(_, v)| !v.is_empty())
        .map(|(_, v)| PathBuf::from(v))
        .or_else(od_runtime::login_path::home_dir)
}

/// The Personal skill `id` and the folder it is copied into
/// (`<agents' HOME>/.claude/skills`), after the checks every copy and
/// preview make: a valid target and id, a user skill.
pub(crate) fn export_source(
    state: &AppState,
    id: &str,
    target: &str,
) -> ApiResult<(SkillEntry, PathBuf)> {
    if target != "claude" {
        return Err(ApiErr::bad_request("target must be \"claude\""));
    }
    if !od_content::design_systems::is_valid_id(id) || id.starts_with(['.', '_']) {
        return Err(ApiErr::bad_request(format!("invalid skill id {id:?}")));
    }
    let entry = state
        .content
        .snapshot()
        .find_skill(id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("skill not found"))?;
    if entry.info.source != ContentSource::User {
        return Err(ApiErr::forbidden("only Personal skills can be copied"));
    }
    let home = agent_home(&state.runs.engine.config().base_env)
        .ok_or_else(|| ApiErr::internal("no home folder"))?;
    // Per component: `.claude/skills` as one string shows `/` on Windows.
    Ok((entry, home.join(".claude").join("skills")))
}

/// A copy made.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    pub id: String,
    pub path: PathBuf,
    /// It calls Contour's tools (`od`, `$OD_BIN`…): it may not work there.
    pub contour_tools: bool,
}

/// Why a copy was not made, for the app to say in the user's words.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFailure {
    /// `invalid`, `notFound`, `forbidden`, `noDescription`, `exists`,
    /// `failed`.
    pub kind: &'static str,
    pub message: String,
    /// What stands there already, for `exists`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing: Option<ExistingEntry>,
}

impl CopyFailure {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            existing: None,
        }
    }
}

/// Copies the Personal skill `id` into the agents' Claude Code skills
/// ([`user_content::export_skill`]: never over anything, nothing hidden
/// there, `SKILL.md` last). Blocking: call it off the async runtime.
///
/// # Errors
/// [`CopyFailure`] with its `kind`.
pub fn copy_to_claude(state: &AppState, id: &str) -> Result<Copied, CopyFailure> {
    let (entry, dest_root) = export_source(state, id, "claude").map_err(|e| {
        let kind = match e.status {
            StatusCode::BAD_REQUEST => "invalid",
            StatusCode::NOT_FOUND => "notFound",
            StatusCode::FORBIDDEN => "forbidden",
            _ => "failed",
        };
        CopyFailure::new(kind, e.message)
    })?;
    let id = entry.info.id.clone();
    let root = state.store.data_dir().user_skills_dir();
    match user_content::export_skill(&root, &entry.dir, &id, &dest_root) {
        Ok(path) => {
            tracing::info!(skill = %id, path = %path.display(), "skill copied to Claude Code");
            Ok(Copied {
                id,
                path,
                contour_tools: user_content::export_check(&entry.dir).contour_tools,
            })
        }
        Err(WriteError::Exists(name)) => {
            let existing = user_content::existing_export(&dest_root, &id)
                .ok()
                .flatten();
            let path = existing
                .as_ref()
                .map_or_else(|| dest_root.join(&name), |e| e.path.clone());
            Err(CopyFailure {
                kind: "exists",
                message: format!(
                    "Claude Code already has {name:?} at {}. Remove that folder to copy again.",
                    path.display()
                ),
                existing,
            })
        }
        Err(WriteError::NoDescription(m)) => Err(CopyFailure::new("noDescription", m)),
        Err(WriteError::NotFound(m)) => Err(CopyFailure::new("notFound", m)),
        Err(WriteError::BadRequest(m)) => Err(CopyFailure::new("invalid", m)),
        Err(e) => Err(CopyFailure::new("failed", e.to_string())),
    }
}
