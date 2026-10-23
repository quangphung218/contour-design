//! Runs (spec 2.2, 2.3): create, stream (SSE with replay), cancel, steer,
//! list; agent detection.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use od_content::prompt::{self, PromptInput, TranscriptTurn};
use od_content::skills::skill_cwd_alias_segment;
use od_contracts::{
    AgentsResponse, ChatRequest, CommentStatus, CreateRunResponse, ErrorCode, MessageRole,
    RunActionResponse, RunResponse, RunStatus, RunsResponse, SteerRequest, UpdateCommentRequest,
    sse,
};
use od_runs::{Run, RunMeta, RunsError};
use od_runtime::env::{RunEnv, agent_env};
use od_runtime::stage::{SkillToStage, stage_skill};
use od_runtime::{LaunchSpec, PermissionMode, RunOutcome, SteerError};
use od_store::{AgentSession, NewTurn, Store, StoreError};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// SSE keepalive comment interval (spec 2.2).
pub const KEEPALIVE: Duration = Duration::from_secs(25);
/// Persisted event batches are written at least this often…
const BATCH_INTERVAL: Duration = Duration::from_millis(250);
/// …or when this many events are pending.
const BATCH_MAX: usize = 64;

fn conflict(code: ErrorCode, message: impl Into<String>) -> ApiErr {
    ApiErr::new(StatusCode::CONFLICT, code, message)
}

fn run_not_found() -> ApiErr {
    ApiErr::new(
        StatusCode::NOT_FOUND,
        ErrorCode::RunNotFound,
        "run not found",
    )
}

fn non_default(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "default")
        .map(str::to_owned)
}

/// Fingerprint of the logical request behind a `clientRequestId`.
fn request_hash(req: &ChatRequest) -> String {
    let logical = json!({
        "agentId": req.agent_id,
        "message": req.message,
        "projectId": req.project_id,
        "conversationId": req.conversation_id,
        "model": req.model,
        "skillId": req.skill_id,
        "skillIds": req.skill_ids,
        "designSystemId": req.design_system_id,
        "attachments": req.attachments,
        "pluginId": req.plugin_id,
        "pluginInputs": req.plugin_inputs,
        "commentAttachments": req.comment_attachments,
    });
    hex::encode(&Sha256::digest(logical.to_string().as_bytes())[..16])
}

fn safe_client_id(id: Option<&str>) -> ApiResult<Option<String>> {
    match id {
        None => Ok(None),
        Some(id)
            if !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)) =>
        {
            Ok(Some(id.to_owned()))
        }
        Some(_) => Err(ApiErr::bad_request("invalid id")),
    }
}

/// `POST /api/runs` → 202.
pub async fn create(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<ChatRequest>,
) -> ApiResult<Response> {
    let def = od_runtime::def::find(&req.agent_id).ok_or_else(|| {
        ApiErr::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::AgentNotFound,
            format!("unknown agent {:?}", req.agent_id),
        )
    })?;
    if req.message.trim().is_empty() {
        return Err(ApiErr::bad_request("message required"));
    }
    // An unknown plugin or missing plugin inputs fail before any run exists.
    crate::routes::media::plugins::applied_plugin(&state.content.snapshot(), &req)?;
    let client_request_id = safe_client_id(req.client_request_id.as_deref())?;
    let user_message_id = safe_client_id(req.user_message_id.as_deref())?.unwrap_or_else(new_id);
    let assistant_message_id =
        safe_client_id(req.assistant_message_id.as_deref())?.unwrap_or_else(new_id);
    let project_id = req.project_id.clone();
    let conversation_id = req.conversation_id.clone();
    let (project, conversation) = state
        .blocking(move |s| {
            let project = s.get_project(&project_id)?;
            let conversation = match conversation_id {
                Some(cid) => s.get_conversation(&project.id, &cid)?,
                None => s.latest_or_new_conversation(&project.id)?,
            };
            Ok((project, conversation))
        })
        .await?;

    let run_id = new_id();
    let meta = RunMeta {
        run_id: run_id.clone(),
        agent_id: def.id.to_owned(),
        project_id: Some(project.id.clone()),
        conversation_id: Some(conversation.id.clone()),
        assistant_message_id: Some(assistant_message_id.clone()),
        client_request_id: client_request_id.clone(),
        request_hash: Some(request_hash(&req)),
        created_at: od_runs::now_ms(),
    };
    let (run, reused) = match state.runs.registry.create_or_reuse(meta) {
        Ok(found) => found,
        Err(RunsError::IdempotencyConflict) => {
            return Err(conflict(
                ErrorCode::IdempotencyConflict,
                "clientRequestId is already associated with a different logical run request",
            ));
        }
        Err(err) => return Err(ApiErr::internal(err.to_string())),
    };
    if reused {
        // Same key and same request (the registry checked the hash).
        return Ok(reused_response(&run));
    }

    // axum drops this future when the client disconnects. Once the run is
    // registered it must launch or end, so the rest runs in its own task.
    let task = tokio::spawn({
        let state = state.clone();
        let run = Arc::clone(&run);
        let conversation_id = conversation.id.clone();
        async move {
            let res = prepare_and_launch(
                &state,
                def,
                &req,
                &project,
                &conversation_id,
                &run,
                &user_message_id,
            )
            .await;
            if let Err(err) = &res {
                end_unlaunched(
                    &state,
                    &run,
                    &comment_ids(&req),
                    RunStatus::Failed,
                    Some(&err.message),
                    Some(od_contracts::FailureCategory::SpawnFailed),
                )
                .await;
            }
            res
        }
    });
    let resumed = task
        .await
        .map_err(|e| ApiErr::internal(format!("run launch failed: {e}")))??;
    Ok((
        StatusCode::ACCEPTED,
        ApiJson(CreateRunResponse {
            run_id,
            conversation_id: conversation.id,
            assistant_message_id,
            client_request_id,
            reused: false,
            resumed,
        }),
    )
        .into_response())
}

/// Ends a run whose agent never started (prepare failed, or a cancel came
/// first). The turn may already be recorded; never leave it "running".
async fn end_unlaunched(
    state: &AppState,
    run: &Arc<Run>,
    comments: &[String],
    status: RunStatus,
    error: Option<&str>,
    failure_category: Option<od_contracts::FailureCategory>,
) {
    state.runs.tokens.revoke_run(run.id());
    state.runs.pending_cancels().remove(run.id());
    let meta = run.meta().clone();
    let comments = comments.to_vec();
    let _ = state
        .blocking(move |s| {
            if let Some(msg) = &meta.assistant_message_id {
                if s.get_message(msg)?.is_some() {
                    s.finish_assistant_message(msg, "", status, 0)?;
                }
            }
            if let (Some(project), Some(conversation)) = (&meta.project_id, &meta.conversation_id) {
                set_comments(
                    s,
                    project,
                    conversation,
                    &comments,
                    settled_comment_status(status),
                )?;
            }
            Ok(())
        })
        .await;
    if let Some(error) = error {
        run.emit_error(error, None);
    }
    run.finish(od_contracts::RunEndPayload {
        status,
        code: None,
        signal: None,
        terminal_at: od_runs::now_ms(),
        artifact_paths: Vec::new(),
        resumable: false,
        retryable: false,
        failure_category,
        error: error.map(str::to_owned),
    });
}

fn reused_response(run: &Arc<Run>) -> Response {
    let meta = run.meta();
    (
        StatusCode::ACCEPTED,
        ApiJson(CreateRunResponse {
            run_id: meta.run_id.clone(),
            conversation_id: meta.conversation_id.clone().unwrap_or_default(),
            assistant_message_id: meta.assistant_message_id.clone().unwrap_or_default(),
            client_request_id: meta.client_request_id.clone(),
            reused: true,
            resumed: false,
        }),
    )
        .into_response()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn comment_ids(req: &ChatRequest) -> Vec<String> {
    req.comment_attachments
        .iter()
        .filter_map(|c| c.id.clone())
        .collect()
}

/// Moves the turn's preview comments to `status`: `applying` while the run
/// works on them, then `resolved` on success or back to `attached` so the
/// next turn carries them again. Comments deleted meanwhile are skipped.
fn set_comments(
    s: &Store,
    project_id: &str,
    conversation_id: &str,
    ids: &[String],
    status: CommentStatus,
) -> Result<(), StoreError> {
    for id in ids {
        let req = UpdateCommentRequest {
            status: Some(status),
            ..UpdateCommentRequest::default()
        };
        match s.update_comment(project_id, conversation_id, id, req) {
            Ok(_) | Err(StoreError::CommentNotFound) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn settled_comment_status(run: RunStatus) -> CommentStatus {
    if run == RunStatus::Succeeded {
        CommentStatus::Resolved
    } else {
        CommentStatus::Attached
    }
}

/// Everything resolved on the blocking pool before launch.
struct Prepared {
    cwd: PathBuf,
    prompt: String,
    fallback_prompt: Option<String>,
    resume_session_id: Option<String>,
    stable_hash: String,
    permission: PermissionMode,
    byok: Option<od_runtime::byok::ByokConfig>,
    /// Design Jury on for this run.
    critique: bool,
}

/// Design Jury for this run (spec 1.5): a skill that requires it or opts
/// out decides; else the project's switch; else `OD_CRITIQUE_ENABLED`.
fn critique_enabled(project: &od_contracts::Project, skills: &[&od_content::SkillEntry]) -> bool {
    use od_contracts::CritiquePolicy;
    let policies = skills.iter().filter_map(|s| s.info.critique_policy);
    let mut decided = None;
    for policy in policies {
        match policy {
            CritiquePolicy::Required => decided = Some(true),
            CritiquePolicy::OptOut if decided.is_none() => decided = Some(false),
            _ => {}
        }
    }
    decided
        .or_else(|| crate::routes::agent_w3::critique_switch(project))
        .or_else(|| {
            std::env::var(crate::routes::agent_w3::CRITIQUE_ENV)
                .ok()
                .map(|v| matches!(v.trim(), "1" | "true" | "on"))
        })
        .unwrap_or(false)
}

/// Records the turn, composes the prompt, decides on resume, stages
/// skills and launches. Returns whether the run resumes a session.
async fn prepare_and_launch(
    state: &AppState,
    def: &'static od_runtime::RuntimeDef,
    req: &ChatRequest,
    project: &od_contracts::Project,
    conversation_id: &str,
    run: &Arc<Run>,
    user_message_id: &str,
) -> ApiResult<bool> {
    let assistant_message_id = run.meta().assistant_message_id.clone().unwrap_or_default();
    let model = non_default(req.model.as_deref());
    let content = state.content.snapshot();
    let req2 = req.clone();
    let project2 = project.clone();
    let conversation = conversation_id.to_owned();
    let run_id = run.id().to_owned();
    let assistant = assistant_message_id.clone();
    let user_message = user_message_id.to_owned();
    let model2 = model.clone();
    let bound = crate::routes::library::plugins::project_plugin_request(req, project);
    let plugin =
        crate::routes::media::plugins::applied_plugin(&content, bound.as_ref().unwrap_or(req))?;
    let prepared = state
        .blocking(move |s| {
            let root = s.project_root(&project2)?;
            let cwd = root.dir;
            let prior = s.list_messages(&conversation)?;
            s.begin_turn(&NewTurn {
                conversation_id: &conversation,
                user_message_id: &user_message,
                assistant_message_id: &assistant,
                content: &req2.message,
                attachments: &req2.attachments,
                agent_id: def.id,
                run_id: &run_id,
            })?;
            set_comments(
                s,
                &project2.id,
                &conversation,
                &comment_ids(&req2),
                CommentStatus::Applying,
            )?;

            // Skills: explicit ids, else the project's skill.
            let mut ids: Vec<String> = req2.skill_ids.clone();
            ids.extend(req2.skill_id.clone());
            if ids.is_empty() {
                ids.extend(project2.skill_id.clone());
            }
            if let Some(p) = &plugin {
                ids.extend(p.skill_ids.iter().cloned());
            }
            // First mention wins; `dedup` alone only drops neighbours.
            let mut seen = std::collections::HashSet::new();
            ids.retain(|id| seen.insert(id.clone()));
            let skills: Vec<&od_content::SkillEntry> = ids
                .iter()
                .filter_map(|id| {
                    content
                        .find_skill(id)
                        .or_else(|| content.find_design_template(id))
                })
                .collect();
            let ds_id = req2
                .design_system_id
                .clone()
                .or_else(|| project2.design_system_id.clone());
            let ds = ds_id
                .as_deref()
                .and_then(|id| content.find_design_system(id));
            let is_deck = project2
                .metadata
                .as_ref()
                .and_then(|m| m.kind)
                .is_some_and(|k| k == od_contracts::ProjectKind::Deck);
            let skill_requires: Vec<String> = skills
                .iter()
                .flat_map(|s| s.info.craft_requires.iter().cloned())
                .chain(plugin.iter().flat_map(|p| p.craft_requires.iter().cloned()))
                .collect();
            let requested =
                od_content::craft::resolve_craft_requirements(&od_content::craft::CraftRequest {
                    is_deck,
                    skill_requires: &skill_requires,
                    design_system_applies: &[],
                    design_system_exemptions: &[],
                });
            let (craft_body, craft_sections) =
                od_content::craft::load_craft_sections(&content.craft_dir, &requested);
            // The request's mode wins and sticks to the conversation;
            // otherwise the conversation's own mode.
            let session_mode = match req2.session_mode {
                Some(mode) => {
                    s.update_conversation(
                        &project2.id,
                        &conversation,
                        od_contracts::UpdateConversationRequest {
                            session_mode: Some(mode),
                            title: None,
                        },
                    )?;
                    mode
                }
                None => {
                    s.get_conversation(&project2.id, &conversation)?
                        .session_mode
                }
            };
            // "Remember…" and Settings ▸ Memory (switch, MEMORY.md, hooks):
            // global facts first, then the project's (track C2).
            let turn_memory =
                crate::routes::c2::memory::prepare_turn(s, &project2.id, &req2.message)?;
            let memory = turn_memory.facts;
            let critique = critique_enabled(&project2, &skills).then(|| prompt::CritiqueBrief {
                brand_name: ds.map_or("Unbranded", |d| d.summary.title.as_str()),
                brand_body: ds.map_or("", |d| d.body.as_str()),
                skill_id: skills.first().map_or("freeform", |s| s.info.id.as_str()),
            });
            // Settings ▸ Instructions & rules (the original's customInstructions).
            let user_instructions = s
                .data_dir()
                .app_setting("customInstructions")
                .and_then(|v| v.as_str().map(str::to_owned));
            let user_instructions = match (user_instructions, turn_memory.hooks) {
                (Some(u), Some(h)) => Some(format!("{u}\n\n{h}")),
                (u, h) => u.or(h),
            };
            let instructions = prompt::compose_instructions(&PromptInput {
                session_mode,
                locale: req2.locale.as_deref(),
                metadata: project2.metadata.as_ref(),
                user_instructions: user_instructions.as_deref(),
                project_instructions: project2.custom_instructions.as_deref(),
                design_system: ds,
                craft: Some((craft_body.as_str(), craft_sections.as_slice())),
                skills: &skills,
                plugin_block: plugin.as_ref().map(|p| p.prompt_block.as_str()),
                memory: &memory,
                critique,
            });
            let critique = critique.is_some() && session_mode == od_contracts::SessionMode::Design;
            let stable_hash = prompt::stable_hash(&instructions);
            let cwd_str = cwd.to_string_lossy().into_owned();

            // Resume only when the stored session still matches (spec 2.3).
            let stored = s.get_agent_session(&conversation, def.id)?;
            let latest = s.latest_completed_assistant_id(&conversation)?;
            let resume = stored.filter(|st| {
                st.model == model2
                    && st.cwd.as_deref() == Some(cwd_str.as_str())
                    && st.last_message_id == latest
            });
            let turns: Vec<TranscriptTurn<'_>> = prior
                .iter()
                .map(|m| TranscriptTurn {
                    assistant: m.role == MessageRole::Assistant,
                    content: &m.content,
                })
                .collect();
            let transcript = prompt::transcript(&turns);
            let request =
                prompt::with_comment_attachments(&req2.message, &req2.comment_attachments);
            let full = prompt::compose_user_message(
                Some(&instructions),
                Some(&cwd_str),
                Some(&transcript),
                &request,
                &req2.attachments,
            );
            let (prompt, fallback, resume_id) = match resume {
                Some(st) => {
                    let same = st.stable_prompt_hash.as_deref() == Some(stable_hash.as_str());
                    let short = prompt::compose_user_message(
                        (!same).then_some(instructions.as_str()),
                        None,
                        None,
                        &request,
                        &req2.attachments,
                    );
                    (short, Some(full), Some(st.session_id))
                }
                None => (full, None, None),
            };

            for skill in &skills {
                let alias = skill_cwd_alias_segment(&skill.dir);
                stage_skill(
                    &cwd,
                    &SkillToStage {
                        source_dir: &skill.dir,
                        alias: &alias,
                        skill_md: &skill.body,
                    },
                )?;
            }
            let byok = s
                .data_dir()
                .app_setting(od_runtime::byok::APP_CONFIG_KEY)
                .and_then(|v| od_runtime::byok::ByokConfig::from_value(&v));
            let permission = s
                .data_dir()
                .app_setting("agent_permission_mode")
                .or_else(|| s.data_dir().app_setting("agentPermissionMode"))
                .and_then(|v| v.as_str().map(PermissionMode::from_setting))
                .unwrap_or_default();
            Ok(Prepared {
                cwd,
                prompt,
                fallback_prompt: fallback,
                resume_session_id: resume_id,
                stable_hash,
                permission,
                byok,
                critique,
            })
        })
        .await?;

    let resumed = prepared.resume_session_id.is_some();
    let mcp = crate::routes::platform::mcp::for_agent(state, def.mcp, run.id()).await;
    let engine = &state.runs.engine;
    let launched = {
        // Held across the check and `launch` so a concurrent cancel either
        // finds the run in the engine or leaves a pending cancel seen here.
        let mut pending = state.runs.pending_cancels();
        if pending.remove(run.id()) {
            None
        } else {
            let token = state.runs.tokens.issue(
                run.id(),
                Some(&project.id),
                od_runtime::tokens::MIN_TOOL_TOKEN_TTL,
            );
            let od_bin = std::env::current_exe().ok();
            let data_root = state.store.data_dir().root().to_path_buf();
            let mut env = agent_env(
                def,
                &engine.config().base_env,
                &engine.config().path,
                &RunEnv {
                    data_dir: Some(&data_root),
                    daemon_url: state.config.daemon_url.as_deref(),
                    tool_token: Some(&token),
                    project_id: Some(&project.id),
                    project_dir: Some(&prepared.cwd),
                    od_bin: od_bin.as_deref(),
                    run_id: Some(run.id()),
                },
            );
            env.extend(mcp.env);
            env.extend(crate::routes::manage::settings::env_for(&data_root, def.id));
            Some(engine.launch(LaunchSpec {
                run: Arc::clone(run),
                def,
                cwd: prepared.cwd.clone(),
                prompt: prepared.prompt,
                fallback_prompt: prepared.fallback_prompt,
                model: model.clone(),
                reasoning: non_default(req.reasoning.as_deref()).or_else(|| {
                    // Settings ▸ Reasoning effort, per agent.
                    let models = state.store.data_dir().app_setting("agentModels")?;
                    non_default(models.get(def.id)?.get("reasoning")?.as_str())
                }),
                service_tier: {
                    // Settings ▸ Service tier, per agent.
                    let models = state.store.data_dir().app_setting("agentModels");
                    models.and_then(|m| non_default(m.get(def.id)?.get("serviceTier")?.as_str()))
                },
                bin: crate::routes::manage::settings::custom_bin(&data_root, def.id),
                extra_dirs: super::chat_ops::linked_dirs(&state.store, project),
                resume_session_id: prepared.resume_session_id,
                permission: prepared.permission,
                env,
                project_id: Some(project.id.clone()),
                byok: prepared.byok,
                mcp: mcp.launch,
            }))
        }
    };
    let Some(handle) = launched else {
        if let Some(file) = &mcp.file {
            let _ = std::fs::remove_file(file);
        }
        end_unlaunched(
            state,
            run,
            &comment_ids(req),
            RunStatus::Canceled,
            None,
            None,
        )
        .await;
        return Ok(resumed);
    };
    crate::routes::genui::raise_for_run(state, run, req, project).await;
    tokio::spawn(persist_run(
        state.clone(),
        Arc::clone(run),
        handle,
        Finalize {
            project_id: project.id.clone(),
            comment_ids: comment_ids(req),
            conversation_id: conversation_id.to_owned(),
            agent_id: def.id.to_owned(),
            assistant_message_id,
            model,
            cwd: prepared.cwd.to_string_lossy().into_owned(),
            stable_hash: prepared.stable_hash,
            critique: prepared.critique,
            mcp_file: mcp.file,
        },
    ));
    Ok(resumed)
}

struct Finalize {
    project_id: String,
    comment_ids: Vec<String>,
    conversation_id: String,
    agent_id: String,
    assistant_message_id: String,
    model: Option<String>,
    cwd: String,
    stable_hash: String,
    critique: bool,
    /// The run's MCP config (holds tokens); removed when the run ends.
    mcp_file: Option<PathBuf>,
}

/// Agent payloads that are only meaningful live.
fn persisted(data: &Value) -> bool {
    !matches!(
        data.get("type").and_then(Value::as_str),
        Some("tool_input_delta" | "tool_in_flight")
    )
}

/// Writes the run's `agent` events into `message_event_batches` while it
/// streams, then finalizes the assistant message and the agent session.
async fn persist_run(
    state: AppState,
    run: Arc<Run>,
    handle: tokio::task::JoinHandle<RunOutcome>,
    fin: Finalize,
) {
    let mut sub = run.subscribe(0);
    let mut pending: Vec<Value> = Vec::new();
    let mut seq = 0i64;
    let mut last_id = 0u64;
    let mut tick = tokio::time::interval(BATCH_INTERVAL);
    let mut jury = fin.critique.then(od_runtime::critique::CritiqueReader::new);
    loop {
        let flush = tokio::select! {
            ev = sub.next() => match ev {
                Some(ev) => {
                    last_id = ev.id;
                    if ev.event == sse::AGENT && persisted(&ev.data) {
                        pending.push(ev.data.clone());
                    }
                    if let (Some(reader), Some(delta)) = (jury.as_mut(), text_delta(&ev)) {
                        if let Some(report) = reader.push(delta) {
                            // Comes back through `sub` and is persisted then.
                            run.emit(sse::AGENT, critique_event(&report));
                        }
                    }
                    pending.len() >= BATCH_MAX
                }
                None => break,
            },
            _ = tick.tick() => !pending.is_empty(),
        };
        if flush {
            let batch = std::mem::take(&mut pending);
            let (msg, rid) = (fin.assistant_message_id.clone(), run.id().to_owned());
            let n = seq;
            seq += 1;
            if let Err(err) = state
                .blocking(move |s| s.append_event_batch(&msg, &rid, n, &batch, last_id))
                .await
            {
                tracing::warn!(run = %run.id(), error = %err.message, "event batch write failed");
            }
        }
    }
    if !pending.is_empty() {
        let (msg, rid) = (fin.assistant_message_id.clone(), run.id().to_owned());
        let _ = state
            .blocking(move |s| s.append_event_batch(&msg, &rid, seq, &pending, last_id))
            .await;
    }
    let outcome = match handle.await {
        Ok(o) => o,
        Err(err) => {
            tracing::error!(run = %run.id(), error = %err, "run task failed");
            RunOutcome {
                status: run.status(),
                session_id: None,
                resumed: false,
                resume_failed: false,
                text: String::new(),
                exit_code: None,
                failure_category: None,
            }
        }
    };
    state.runs.tokens.revoke_run(run.id());
    if let Some(file) = &fin.mcp_file {
        let _ = std::fs::remove_file(file);
    }
    let status = run.status();
    let verdict = jury
        .as_mut()
        .and_then(od_runtime::critique::CritiqueReader::finish);
    let run_id = run.id().to_owned();
    let res = state
        .blocking(move |s| {
            // The jury's file and report are extras: failing them must not
            // leave the message running.
            if let Some((mut report, artifact)) = verdict {
                // An interrupted jury ships its best closed round (track C2).
                let ships = status == RunStatus::Succeeded
                    || (status == RunStatus::Canceled
                        && report.status == od_contracts::CritiqueStatus::Interrupted);
                if let Some(html) = artifact.filter(|_| ships) {
                    let path = format!(
                        "critique/{}.html",
                        run_id.chars().take(8).collect::<String>()
                    );
                    match s.write_file(&fin.project_id, &path, html.as_bytes(), true, None) {
                        Ok(_) => report.artifact_path = Some(path),
                        Err(err) => {
                            tracing::warn!(run = %run_id, error = %err, "critique artifact write failed");
                            report.degraded_reason =
                                Some(format!("the shipped artifact could not be saved: {err}"));
                        }
                    }
                }
                // The settled report closes the stored events.
                if let Err(err) = s.append_event_batch(
                    &fin.assistant_message_id,
                    &run_id,
                    i64::MAX,
                    &[critique_event(&report)],
                    last_id,
                ) {
                    tracing::warn!(run = %run_id, error = %err, "critique report write failed");
                }
            }
            s.finish_assistant_message(&fin.assistant_message_id, &outcome.text, status, last_id)?;
            set_comments(
                s,
                &fin.project_id,
                &fin.conversation_id,
                &fin.comment_ids,
                settled_comment_status(status),
            )?;
            match (&outcome.session_id, status) {
                (Some(session), RunStatus::Succeeded) => s.save_agent_session(&AgentSession {
                    conversation_id: fin.conversation_id,
                    agent_id: fin.agent_id,
                    session_id: session.clone(),
                    stable_prompt_hash: Some(fin.stable_hash),
                    model: fin.model,
                    cwd: Some(fin.cwd),
                    last_message_id: Some(fin.assistant_message_id),
                })?,
                _ if outcome.resume_failed => {
                    s.delete_agent_session(&fin.conversation_id, &fin.agent_id)?;
                }
                _ => {}
            }
            Ok(())
        })
        .await;
    if let Err(err) = res {
        tracing::warn!(run = %run.id(), error = %err.message, "finalizing the message failed");
    }
}

fn text_delta(ev: &od_runs::RunEvent) -> Option<&str> {
    if ev.event != sse::AGENT || ev.data.get("type").and_then(Value::as_str) != Some("text_delta") {
        return None;
    }
    ev.data.get("delta").and_then(Value::as_str)
}

/// The `agent` payload carrying a Design Jury report.
fn critique_event(report: &od_contracts::CritiqueReport) -> Value {
    serde_json::to_value(od_contracts::AgentEvent::Critique {
        report: report.clone(),
    })
    .unwrap_or(Value::Null)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsQuery {
    #[serde(default)]
    after: Option<u64>,
}

/// `GET /api/runs/:id/events`: SSE with replay from `Last-Event-ID` or
/// `?after=N`; a finished run sends its history and closes.
pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let run = state.runs.registry.get(&id).ok_or_else(run_not_found)?;
    let after = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .or(q.after)
        .unwrap_or(0);
    let sub = run.subscribe(after);
    let body = stream::unfold(sub, |mut sub| async move {
        let ev = sub.next().await?;
        let event = Event::default()
            .id(ev.id.to_string())
            .event(ev.event.clone())
            .data(ev.data.to_string());
        Some((Ok::<_, Infallible>(event), sub))
    });
    let sse = Sse::new(body).keep_alive(KeepAlive::new().interval(KEEPALIVE).text("keepalive"));
    Ok((
        [
            (header::CACHE_CONTROL, "no-cache, no-transform"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        sse,
    )
        .into_response())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    conversation_id: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> ApiJson<RunsResponse> {
    let runs = state
        .runs
        .registry
        .list()
        .into_iter()
        .filter(|r| {
            q.project_id
                .as_deref()
                .is_none_or(|p| r.meta().project_id.as_deref() == Some(p))
                && q.conversation_id
                    .as_deref()
                    .is_none_or(|c| r.meta().conversation_id.as_deref() == Some(c))
        })
        .map(|r| r.info())
        .collect();
    ApiJson(RunsResponse { runs })
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<RunResponse>> {
    let run = state.runs.registry.get(&id).ok_or_else(run_not_found)?;
    Ok(ApiJson(RunResponse { run: run.info() }))
}

/// `POST /api/runs/:id/cancel`. Idempotent: a finished run reports its
/// final status.
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<RunActionResponse>> {
    let run = state.runs.registry.get(&id).ok_or_else(run_not_found)?;
    {
        let mut pending = state.runs.pending_cancels();
        if !state.runs.engine.cancel(&id) && !run.status().is_terminal() {
            // Registered but not launched yet: the launch step ends it.
            pending.insert(id);
        }
    }
    Ok(ApiJson(RunActionResponse {
        ok: true,
        status: run.status(),
    }))
}

/// `POST /api/runs/:id/steer`: 202, or 409 with `runtime_unsupported`,
/// `run_terminal` or `stdin_closed`.
pub async fn steer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<SteerRequest>,
) -> ApiResult<Response> {
    let run = state.runs.registry.get(&id).ok_or_else(run_not_found)?;
    if body.message.trim().is_empty() {
        return Err(ApiErr::bad_request("message required"));
    }
    match state.runs.engine.steer(&run, &body.message).await {
        Ok(()) => Ok((
            StatusCode::ACCEPTED,
            ApiJson(RunActionResponse {
                ok: true,
                status: run.status(),
            }),
        )
            .into_response()),
        Err(SteerError::NotFound) => Err(run_not_found()),
        Err(err) => {
            let code = match err {
                SteerError::RuntimeUnsupported => ErrorCode::RuntimeUnsupported,
                SteerError::RunTerminal | SteerError::NotFound => ErrorCode::RunTerminal,
                SteerError::StdinClosed => ErrorCode::StdinClosed,
            };
            Err(conflict(code, err.to_string()))
        }
    }
}

/// `GET /api/agents`: fresh detection, never cached on disk.
/// Whether a BYOK key is saved, which makes the BYOK agent available.
pub(crate) fn byok_configured(state: &AppState) -> bool {
    state
        .store
        .data_dir()
        .app_setting(od_runtime::byok::APP_CONFIG_KEY)
        .and_then(|v| od_runtime::byok::ByokConfig::from_value(&v))
        .is_some()
}

pub async fn agents(State(state): State<AppState>) -> ApiJson<AgentsResponse> {
    let path = state.runs.engine.config().path.clone();
    ApiJson(AgentsResponse {
        agents: od_runtime::detect::detect_all_in(
            path,
            state.runs.engine.config().base_env.clone(),
            byok_configured(&state),
        )
        .await,
    })
}
