//! GenUI surfaces (track C4-depth), the original `routes/genui.ts`: a
//! plugin run asks the user to confirm, authorize, choose or fill a form,
//! and the chat answers.
//!
//! When a run with a plugin starts, [`raise_for_run`] asks every surface
//! the plugin declares (`od.genui.surfaces`) plus the diff-review choice
//! derived for each pipeline stage with the `diff-review` atom. Each asked
//! surface is a `genui_surface_request` event on the run's stream; one
//! answered earlier at the `conversation` or `project` tier is reused
//! instead (`genui_surface_response`, `respondedBy: "cache"`).
//!
//! - `GET  /api/runs/:runId/genui` → `{runId, surfaces}`
//! - `GET  /api/runs/:runId/genui/:surfaceId` → the latest row, with `spec`
//! - `POST /api/runs/:runId/genui/:surfaceId/respond` `{value, respondedBy?}`
//!   → `{ok, surface, diffReviewBridge?}`; 404 when nothing is pending.
//!   A diff-review answer also writes `<project>/review/decision.json`.
//! - `GET  /api/projects/:id/genui` → `{projectId, surfaces}`
//! - `POST /api/projects/:id/genui/:surfaceId/revoke` → `{ok, invalidated}`
//! - `POST /api/projects/:id/genui/prefill` `{snapshotId, surfaceId, kind?,
//!   persist?, value, schema?, expiresAt?}` → `{ok, surface}`

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use od_contracts::{ChatRequest, Project};
use od_runs::Run;
use od_store::genui::{Answer, GenuiError, GenuiStore, SurfaceRequest, SurfaceRow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Surface ids the daemon derives for `diff-review` stages.
pub const DIFF_REVIEW_PREFIX: &str = "__auto_diff_review_";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/runs/{run_id}/genui", get(for_run))
        .route("/api/runs/{run_id}/genui/{surface_id}", get(one))
        .route(
            "/api/runs/{run_id}/genui/{surface_id}/respond",
            post(respond),
        )
        .route("/api/projects/{id}/genui", get(for_project))
        .route("/api/projects/{id}/genui/prefill", post(prefill))
        .route("/api/projects/{id}/genui/{surface_id}/revoke", post(revoke))
}

/// One open database per data folder, shared by the routes and the run
/// launcher.
fn store(state: &AppState) -> ApiResult<Arc<GenuiStore>> {
    static OPEN: Mutex<Option<HashMap<PathBuf, Arc<GenuiStore>>>> = Mutex::new(None);
    let path = state.store.data_dir().root().join("genui.sqlite");
    let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
    let map = open.get_or_insert_with(HashMap::new);
    if let Some(s) = map.get(&path) {
        return Ok(Arc::clone(s));
    }
    let s = Arc::new(GenuiStore::open(&path).map_err(ApiErr::from)?);
    map.insert(path, Arc::clone(&s));
    Ok(s)
}

impl From<GenuiError> for ApiErr {
    fn from(e: GenuiError) -> Self {
        match e {
            GenuiError::BadRequest(m) => ApiErr::bad_request(m),
            GenuiError::NotPending => ApiErr::not_found(e.to_string()),
            GenuiError::Db(_) | GenuiError::Io(_) => {
                tracing::error!(error = %e, "genui store");
                ApiErr::internal("genui store error")
            }
        }
    }
}

async fn blocking<T, F>(f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ApiResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
}

/// The diff-review choice of a stage, as the original's
/// `deriveAutoAtomSurfaces`.
fn diff_review_surface(stage_id: &str) -> Value {
    json!({
        "id": format!("{DIFF_REVIEW_PREFIX}{stage_id}"),
        "kind": "choice",
        "persist": "run",
        "trigger": { "stageId": stage_id, "atom": "diff-review" },
        "prompt": "Review the diff and choose how to proceed.",
        "schema": {
            "type": "object",
            "title": "Diff review",
            "description": "Accept the patch, reject it, or pick which files to keep.",
            "properties": {
                "decision": { "type": "string", "enum": ["accept", "reject", "partial"], "title": "Decision" },
                "accepted_files": { "type": "array", "items": { "type": "string" }, "title": "Files to accept (only required when decision=partial)" },
                "rejected_files": { "type": "array", "items": { "type": "string" }, "title": "Files to reject (only required when decision=partial)" },
                "reason": { "type": "string", "title": "Notes for the patch author" }
            },
            "required": ["decision"]
        },
        "timeout": 24 * 60 * 60 * 1000,
        "onTimeout": "abort",
        "capabilitiesRequired": []
    })
}

/// A plugin's surfaces: declared first, then derived ones whose id
/// (case-insensitive) is not declared.
#[must_use]
pub fn plugin_surfaces(manifest: &serde_json::Map<String, Value>) -> Vec<Value> {
    let od = manifest.get("od");
    let mut out: Vec<Value> = od
        .and_then(|od| od.pointer("/genui/surfaces"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|s| {
            s.get("id")
                .and_then(Value::as_str)
                .is_some_and(|i| !i.trim().is_empty())
        })
        .cloned()
        .collect();
    let stages = od
        .and_then(|od| od.pointer("/pipeline/stages"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for stage in stages {
        let has_review = stage
            .get("atoms")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|x| x == "diff-review"));
        let Some(id) = stage.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !has_review {
            continue;
        }
        let surface = diff_review_surface(id);
        let sid = surface["id"].as_str().unwrap_or_default().to_lowercase();
        let taken = out.iter().any(|s| {
            s.get("id")
                .and_then(Value::as_str)
                .is_some_and(|i| i.to_lowercase() == sid)
        });
        if !taken {
            out.push(surface);
        }
    }
    out
}

/// Files the plugin's patch steps touched (`plan/receipts/step-*.json`),
/// for the diff-review checklist.
fn touched_files(cwd: &FsPath) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(cwd.join("plan/receipts")) else {
        return Vec::new();
    };
    let mut files: Vec<String> = dir
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("step-") && n.ends_with(".json")
        })
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice::<Value>(&b).ok())
        .flat_map(|r| {
            r.get("files")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Asks the surfaces of the run's plugin (see the module docs). Failures
/// are logged: a surface never stops a run.
pub async fn raise_for_run(state: &AppState, run: &Arc<Run>, req: &ChatRequest, project: &Project) {
    let bound = crate::routes::library::plugins::project_plugin_request(req, project);
    let req = bound.as_ref().unwrap_or(req);
    let Some(plugin_id) = req
        .plugin_id
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        return;
    };
    let content = state.content.snapshot();
    let Some(plugin) = content.find_plugin(plugin_id) else {
        return;
    };
    let surfaces = plugin_surfaces(&plugin.manifest);
    if surfaces.is_empty() {
        return;
    }
    let plugin_id = plugin_id.to_owned();
    let state2 = state.clone();
    let project = project.clone();
    let run = Arc::clone(run);
    let res = blocking(move || {
        let store = store(&state2)?;
        let cwd = state2.store.project_root(&project).map(|r| r.dir).ok();
        let files = cwd.as_deref().map(touched_files).unwrap_or_default();
        let conversation = run.meta().conversation_id.clone();
        for mut spec in surfaces {
            if spec["id"]
                .as_str()
                .is_some_and(|i| i.starts_with(DIFF_REVIEW_PREFIX))
            {
                spec["context"] = json!({ "touchedFiles": files });
            }
            let ask = SurfaceRequest {
                project_id: &project.id,
                conversation_id: conversation.as_deref(),
                run_id: run.id(),
                plugin_id: &plugin_id,
                spec: &spec,
            };
            let (row, reused) = store.request_or_reuse(&ask, od_runs::now_ms())?;
            if reused {
                run.emit(
                    "genui_surface_response",
                    json!({
                        "kind": "genui_surface_response",
                        "surfaceId": row.surface_id,
                        "runId": run.id(),
                        "value": row.value,
                        "respondedAt": row.responded_at,
                        "respondedBy": "cache",
                    }),
                );
            } else {
                run.emit(
                    "genui_surface_request",
                    json!({
                        "kind": "genui_surface_request",
                        "surfaceId": row.surface_id,
                        "runId": run.id(),
                        "payload": spec,
                        "requestedAt": row.requested_at,
                    }),
                );
            }
        }
        Ok(())
    })
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e.message, "raising GenUI surfaces");
    }
}

/// The run's project, or 404 for an unknown run.
fn run_project(state: &AppState, run_id: &str) -> ApiResult<Option<String>> {
    let run = state
        .runs
        .registry
        .get(run_id)
        .ok_or_else(|| ApiErr::not_found("run not found"))?;
    Ok(run.meta().project_id.clone())
}

async fn for_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> ApiResult<ApiJson<Value>> {
    run_project(&state, &run_id)?;
    blocking(move || {
        let surfaces = store(&state)?.for_run(&run_id)?;
        Ok(ApiJson(json!({ "runId": run_id, "surfaces": surfaces })))
    })
    .await
}

async fn one(
    State(state): State<AppState>,
    Path((run_id, surface_id)): Path<(String, String)>,
) -> ApiResult<ApiJson<SurfaceRow>> {
    run_project(&state, &run_id)?;
    blocking(move || {
        store(&state)?
            .for_run(&run_id)?
            .into_iter()
            .rev()
            .find(|s| s.surface_id == surface_id)
            .map(ApiJson)
            .ok_or_else(|| ApiErr::not_found("surface not found"))
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondBody {
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub responded_by: Option<String>,
}

/// `review/decision.json` from a diff-review answer, as the original's
/// `composeDecisionFile`: `partial` must place every touched file.
fn decision_file(value: &Value, files: &[String], reviewer: &str) -> Result<Value, String> {
    let decision = value
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(decision, "accept" | "reject" | "partial") {
        return Err(format!(
            "decision must be one of accept / reject / partial; got {decision:?}"
        ));
    }
    let list = |k: &str| {
        value.get(k).and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
    };
    let all = files.to_vec();
    let (mut accepted, mut rejected) = match decision {
        "accept" => (
            list("accepted_files").unwrap_or(all),
            list("rejected_files").unwrap_or_default(),
        ),
        "reject" => (Vec::new(), list("rejected_files").unwrap_or(all)),
        _ => (
            list("accepted_files").unwrap_or_default(),
            list("rejected_files").unwrap_or_default(),
        ),
    };
    if decision == "partial" {
        if let Some(missing) = files
            .iter()
            .find(|f| !accepted.contains(f) && !rejected.contains(f))
        {
            return Err(format!(
                "diff-review: 'partial' decision must cover every touched file; missing {missing}"
            ));
        }
    }
    for l in [&mut accepted, &mut rejected] {
        l.sort();
        l.dedup();
    }
    let mut out = json!({
        "decision": decision,
        "accepted_files": accepted,
        "rejected_files": rejected,
        "reviewer": reviewer,
        "decidedAt": od_runs::now_ms(),
    });
    if let Some(r) = value
        .get("reason")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        out["reason"] = json!(r);
    }
    Ok(out)
}

async fn respond(
    State(state): State<AppState>,
    Path((run_id, surface_id)): Path<(String, String)>,
    ApiJson(body): ApiJson<RespondBody>,
) -> ApiResult<ApiJson<Value>> {
    let project_id = run_project(&state, &run_id)?;
    let by = match body.responded_by.as_deref() {
        Some(b @ ("agent" | "auto")) => b.to_owned(),
        _ => "user".to_owned(),
    };
    blocking(move || {
        let store = store(&state)?;
        let row = store.respond(
            Answer {
                run_id: &run_id,
                surface_id: &surface_id,
                value: &body.value,
                responded_by: &by,
            },
            od_runs::now_ms(),
        )?;
        if let Some(run) = state.runs.registry.get(&run_id) {
            run.emit(
                "genui_surface_response",
                json!({
                    "kind": "genui_surface_response",
                    "surfaceId": row.surface_id,
                    "runId": run_id,
                    "value": row.value,
                    "respondedAt": row.responded_at,
                    "respondedBy": by,
                }),
            );
            if row.persist != "run" {
                run.emit(
                    "genui_state_synced",
                    json!({
                        "kind": "genui_state_synced",
                        "surfaceId": row.surface_id,
                        "runId": run_id,
                        "persistTier": row.persist,
                    }),
                );
            }
        }
        let mut out = json!({ "ok": true, "surface": row });
        if surface_id.starts_with(DIFF_REVIEW_PREFIX) {
            out["diffReviewBridge"] = diff_review_bridge(&state, project_id.as_deref(), &row, &by);
        }
        Ok(ApiJson(out))
    })
    .await
}

/// Writes the diff-review decision into the run's project folder.
fn diff_review_bridge(
    state: &AppState,
    project_id: Option<&str>,
    row: &SurfaceRow,
    by: &str,
) -> Value {
    let Some(project_id) = project_id else {
        return json!({ "ok": false, "error": "run is not linked to a project" });
    };
    let write = || -> Result<(), String> {
        let project = state
            .store
            .get_project(project_id)
            .map_err(|e| e.to_string())?;
        let cwd = state
            .store
            .project_root(&project)
            .map_err(|e| e.to_string())?
            .dir;
        let files = touched_files(&cwd);
        let reviewer = if by == "user" { "user" } else { "agent" };
        let decision = decision_file(&row.value, &files, reviewer)?;
        let dir = cwd.join("review");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(&decision).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("decision.json"), text + "\n").map_err(|e| e.to_string())
    };
    match write() {
        Ok(()) => json!({ "ok": true }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

async fn for_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<Value>> {
    blocking(move || {
        state.store.get_project(&id)?;
        let surfaces = store(&state)?.for_project(&id)?;
        Ok(ApiJson(json!({ "projectId": id, "surfaces": surfaces })))
    })
    .await
}

async fn revoke(
    State(state): State<AppState>,
    Path((id, surface_id)): Path<(String, String)>,
) -> ApiResult<ApiJson<Value>> {
    blocking(move || {
        state.store.get_project(&id)?;
        let changed = store(&state)?.revoke(&id, &surface_id)?;
        Ok(ApiJson(json!({ "ok": true, "invalidated": changed })))
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefillBody {
    #[serde(default)]
    pub snapshot_id: String,
    #[serde(default)]
    pub surface_id: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub persist: Option<String>,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

async fn prefill(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<PrefillBody>,
) -> ApiResult<ApiJson<Value>> {
    blocking(move || {
        state.store.get_project(&id)?;
        let spec = json!({
            "id": body.surface_id,
            "kind": body.kind,
            "persist": body.persist,
            "schema": body.schema,
            "expiresAt": body.expires_at,
        });
        let row = store(&state)?.prefill(
            &id,
            &body.snapshot_id,
            &spec,
            &body.value,
            od_runs::now_ms(),
        )?;
        Ok(ApiJson(json!({ "ok": true, "surface": row })))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_diff_review_follows_declared_ids() {
        let manifest = json!({ "od": {
            "genui": { "surfaces": [
                { "id": "go", "kind": "confirmation" },
                { "id": "__AUTO_DIFF_REVIEW_b", "kind": "form" },
                { "kind": "form" }
            ]},
            "pipeline": { "stages": [
                { "id": "a", "atoms": ["diff-review"] },
                { "id": "b", "atoms": ["diff-review", "handoff"] },
                { "id": "c", "atoms": ["patch-edit"] }
            ]}
        }});
        let s = plugin_surfaces(manifest.as_object().unwrap_or(&serde_json::Map::new()));
        let ids: Vec<&str> = s.iter().filter_map(|s| s["id"].as_str()).collect();
        assert_eq!(ids, ["go", "__AUTO_DIFF_REVIEW_b", "__auto_diff_review_a"]);
        assert_eq!(s[2]["kind"], "choice");
        assert_eq!(s[2]["persist"], "run");
    }

    #[test]
    fn decision_files_follow_the_original() {
        let files = vec!["a.js".to_owned(), "b.js".to_owned()];
        let d = decision_file(&json!({"decision": "accept"}), &files, "user").unwrap_or_default();
        assert_eq!(d["accepted_files"], json!(["a.js", "b.js"]));
        assert_eq!(d["rejected_files"], json!([]));
        let d = decision_file(
            &json!({"decision": "reject", "reason": "no"}),
            &files,
            "agent",
        )
        .unwrap_or_default();
        assert_eq!(
            (d["rejected_files"].clone(), d["reason"].clone()),
            (json!(["a.js", "b.js"]), json!("no"))
        );
        assert!(
            decision_file(
                &json!({"decision": "partial", "accepted_files": ["a.js"]}),
                &files,
                "user"
            )
            .is_err()
        );
        let d = decision_file(
            &json!({"decision": "partial", "accepted_files": ["b.js"], "rejected_files": ["a.js"]}),
            &files,
            "user",
        )
        .unwrap_or_default();
        assert_eq!(d["accepted_files"], json!(["b.js"]));
        assert!(decision_file(&json!({"decision": "maybe"}), &files, "user").is_err());
    }
}
