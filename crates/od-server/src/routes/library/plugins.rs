//! Plugin library writes (spec 1.4 "Plugin và marketplace", local part):
//!
//! - `POST   /api/plugins/install` `{path}`: copies a local plugin folder
//!   into `<data>/plugins/<id>` (an older copy is replaced).
//! - `POST   /api/plugins/:id/uninstall`: uninstalls a user plugin; bundled ones are
//!   refused.
//! - `POST   /api/plugins/:id/use` `{inputs, locale?, designSystemId?}`:
//!   validates the inputs and creates a project bound to the plugin; the
//!   first turn is the plugin's example brief with the inputs filled in.
//! - `POST   /api/plugins/author` `{brief}` or `{source}`: a project whose
//!   first turn asks the agent to write a plugin (or fetch the one at a
//!   GitHub or archive link: the daemon itself never goes online) and
//!   install it.
//!
//! A project bound to a plugin (`metadata.pluginId`, `pluginInputs`)
//! applies it on every run that does not name another one
//! ([`project_plugin_request`]).

use super::write_and_reload;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use od_content::user_content;
use od_contracts::{ChatRequest, CreateProjectRequest, Project, ProjectMetadata};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// Project metadata keys of a plugin-bound project.
pub const PLUGIN_ID_KEY: &str = "pluginId";
pub const PLUGIN_INPUTS_KEY: &str = "pluginInputs";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/plugins/install", post(install))
        .route("/api/plugins/author", post(author))
        .route("/api/plugins/{id}/use", post(use_plugin))
        .route("/api/plugins/{id}/uninstall", post(uninstall))
}

#[derive(Debug, Deserialize)]
pub struct InstallBody {
    path: String,
}

pub async fn install(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<InstallBody>,
) -> ApiResult<Json<Value>> {
    let source = PathBuf::from(body.path.trim());
    if !source.is_absolute() {
        return Err(ApiErr::bad_request("path must be an absolute folder path"));
    }
    let root = state.store.data_dir().root().join("plugins");
    let plugin = write_and_reload(&state, move || {
        Ok(user_content::install_plugin(&source, &root)?)
    })
    .await?;
    tracing::info!(plugin = %plugin.id, "plugin installed");
    Ok(Json(json!({ "plugin": plugin })))
}

pub async fn uninstall(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let plugin = state
        .content
        .snapshot()
        .find_plugin(&id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found(format!("unknown plugin {id:?}")))?;
    if plugin.source != "user" {
        return Err(ApiErr::forbidden("bundled plugins cannot be uninstalled"));
    }
    let root = state.store.data_dir().root().join("plugins");
    write_and_reload(&state, move || {
        Ok(user_content::delete_user_dir(&root, &plugin.dir)?)
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UseBody {
    #[serde(default)]
    inputs: Map<String, Value>,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    design_system_id: Option<String>,
}

pub async fn use_plugin(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<UseBody>,
) -> ApiResult<Json<Value>> {
    let content = state.content.snapshot();
    let plugin = content
        .find_plugin(&id)
        .ok_or_else(|| ApiErr::not_found(format!("unknown plugin {id:?}")))?;
    let applied = plugin.apply(&body.inputs, body.locale.as_deref())?;
    let prompt = applied
        .query
        .clone()
        .filter(|q| !q.trim().is_empty())
        .or_else(|| applied.plugin_description.clone())
        .unwrap_or_else(|| applied.plugin_title.clone());
    let mut extra = Map::new();
    extra.insert(PLUGIN_ID_KEY.to_owned(), json!(plugin.id));
    extra.insert(PLUGIN_INPUTS_KEY.to_owned(), Value::Object(applied.inputs));
    let req = CreateProjectRequest {
        id: None,
        name: applied.plugin_title.clone(),
        skill_id: applied.skill_ids.first().cloned(),
        design_system_id: body
            .design_system_id
            .filter(|d| !d.is_empty())
            .or(applied.design_system_id),
        pending_prompt: Some(prompt),
        metadata: Some(ProjectMetadata {
            intent: Some(applied.task_kind),
            extra,
            ..ProjectMetadata::default()
        }),
        custom_instructions: None,
        conversation_id: None,
    };
    let (project, conversation_id) = state.blocking(move |s| s.create_project(req)).await?;
    Ok(Json(
        json!({ "project": project, "conversationId": conversation_id }),
    ))
}

/// `req` with the project's bound plugin filled in when it names none.
#[must_use]
pub fn project_plugin_request(req: &ChatRequest, project: &Project) -> Option<ChatRequest> {
    if req
        .plugin_id
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty())
    {
        return None;
    }
    let extra = &project.metadata.as_ref()?.extra;
    let id = extra.get(PLUGIN_ID_KEY)?.as_str()?.to_owned();
    let mut out = req.clone();
    out.plugin_id = Some(id);
    out.plugin_inputs = extra
        .get(PLUGIN_INPUTS_KEY)
        .and_then(Value::as_object)
        .cloned();
    Some(out)
}

/// Where an authoring run writes the plugin, relative to the project.
pub const AUTHORING_DIR: &str = "generated-plugin";

/// The first turn of a "Create plugin with agent" project (the original
/// `plugin-authoring` handoff prompt, local install only).
#[must_use]
pub fn authoring_prompt(brief: &str) -> String {
    format!(
        "# Create an Contour plugin\n\n\
         {brief}\n\n\
         ## What to write\n\n\
         Write a plugin folder `{AUTHORING_DIR}/` in the current project:\n\n\
         1. `{AUTHORING_DIR}/open-design.json`: `name` (kebab-case id), `title`, `version` \"0.1.0\", \
         `description`, `tags`, and an `od` block with `kind` \"scenario\", `taskKind` \"new-generation\", \
         `useCase.query` (an example brief that uses `{{{{input}}}}` placeholders), `inputs` (each with \
         `name`, `type` string|text|select|number|boolean, `label`, `required`, optional `options`, \
         `default`, `placeholder`) and `capabilities` [\"prompt:inject\"].\n\
         2. `{AUTHORING_DIR}/SKILL.md`: front matter with `name` and `description`, then the \
         instructions the agent should follow when the plugin is applied.\n\n\
         ## Install it\n\n\
         When both files are written, install the folder (absolute path) with \
         `od plugin install \"$PWD/{AUTHORING_DIR}\"`, or if `od` is not on PATH: \
         `curl -sS -X POST \"$OD_DAEMON_URL/api/plugins/install\" -H 'content-type: application/json' \
         -d \"{{\\\"path\\\": \\\"$PWD/{AUTHORING_DIR}\\\"}}\"`. Report the plugin id when done."
    )
}

/// The first turn of an "Add plugin from link" project.
#[must_use]
pub fn import_prompt(source: &str) -> String {
    format!(
        "# Install an Contour plugin from a link\n\n\
         Fetch the plugin at {source} into `{AUTHORING_DIR}/` in the current project: \
         `git clone --depth 1` for a GitHub repository (a `github:owner/repo[@ref][/path]` \
         reference or a github.com URL, keeping only the plugin's folder), or download and \
         unpack a `.zip` / `.tar.gz` archive. The folder must hold `SKILL.md` or \
         `open-design.json`; if it does not, stop and say so.\n\n\
         Then install it with `od plugin install \"$PWD/{AUTHORING_DIR}\"`, or if `od` is not on \
         PATH: `curl -sS -X POST \"$OD_DAEMON_URL/api/plugins/install\" -H 'content-type: \
         application/json' -d \"{{\\\"path\\\": \\\"$PWD/{AUTHORING_DIR}\\\"}}\"`. \
         Report the plugin id and its declared capabilities."
    )
}

#[derive(Debug, Deserialize)]
pub struct AuthorBody {
    #[serde(default)]
    brief: String,
    #[serde(default)]
    source: String,
}

/// A link the agent may fetch: `github:…`, or an http(s) URL.
fn valid_source(s: &str) -> bool {
    (s.starts_with("github:") || s.starts_with("https://") || s.starts_with("http://"))
        && s.len() <= 2048
        && !s.chars().any(|c| c.is_whitespace() || c == '`' || c == '"')
}

pub async fn author(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AuthorBody>,
) -> ApiResult<Json<Value>> {
    let brief = body.brief.trim().to_owned();
    let source = body.source.trim().to_owned();
    let (title, prompt) = if !source.is_empty() {
        if !valid_source(&source) {
            return Err(ApiErr::bad_request(
                "source must be a github: reference or an http(s) link",
            ));
        }
        (source.clone(), import_prompt(&source))
    } else if !brief.is_empty() {
        (brief.clone(), authoring_prompt(&brief))
    } else {
        return Err(ApiErr::bad_request("brief or source is required"));
    };
    let name: String = title.chars().take(48).collect();
    let req = CreateProjectRequest {
        id: None,
        name: format!("Plugin: {name}"),
        skill_id: None,
        design_system_id: None,
        pending_prompt: Some(prompt),
        metadata: Some(ProjectMetadata {
            intent: Some("plugin-authoring".to_owned()),
            ..ProjectMetadata::default()
        }),
        custom_instructions: None,
        conversation_id: None,
    };
    let (project, conversation_id) = state.blocking(move |s| s.create_project(req)).await?;
    Ok(Json(
        json!({ "project": project, "conversationId": conversation_id }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(extra: Map<String, Value>) -> Project {
        serde_json::from_value(json!({
            "id": "p", "name": "P", "skillId": null, "designSystemId": null,
            "metadata": extra, "createdAt": 0, "updatedAt": 0,
        }))
        .unwrap()
    }

    #[test]
    fn a_bound_project_fills_in_its_plugin() {
        let mut extra = Map::new();
        extra.insert(PLUGIN_ID_KEY.into(), json!("od-media-generation"));
        extra.insert(PLUGIN_INPUTS_KEY.into(), json!({ "subject": "tea" }));
        let p = project(extra);
        let req = ChatRequest::default();
        let out = project_plugin_request(&req, &p).unwrap();
        assert_eq!(out.plugin_id.as_deref(), Some("od-media-generation"));
        assert_eq!(out.plugin_inputs.unwrap()["subject"], "tea");
        // A run that names its own plugin keeps it.
        let named = ChatRequest {
            plugin_id: Some("other".into()),
            ..ChatRequest::default()
        };
        assert!(project_plugin_request(&named, &p).is_none());
        assert!(project_plugin_request(&req, &project(Map::new())).is_none());
    }

    #[test]
    fn import_prompt_fetches_then_installs() {
        let p = import_prompt("github:acme/plugins@main/recipe");
        assert!(p.contains("github:acme/plugins@main/recipe"));
        assert!(p.contains("git clone --depth 1"));
        assert!(p.contains("od plugin install"));
        assert!(valid_source("https://example.com/p.zip"));
        assert!(valid_source("github:a/b"));
        assert!(!valid_source("file:///etc"));
        assert!(!valid_source("https://a b"));
        assert!(!valid_source("https://x\"; rm -rf"));
    }

    #[test]
    fn authoring_prompt_names_the_folder_and_the_install_command() {
        let p = authoring_prompt("A recipe card maker");
        assert!(p.contains("A recipe card maker"));
        assert!(p.contains("generated-plugin/open-design.json"));
        assert!(p.contains("od plugin install"));
        assert!(p.contains("{{input}}"));
    }
}
