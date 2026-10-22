//! The run engine (spec 2.3, 5.4): one generic loop drives every runtime
//! definition — spawn, parse, steer, cancel, timeout, retry and resume
//! fallback — and writes everything into the run's event log.

use crate::byok::{self, ByokConfig, ByokRequest};
use crate::def::{
    ArgvInput, PermissionMode, PromptInput, RuntimeDef, SessionIdSource, StreamFormat,
};
use crate::guard::{ToolLoopGuard, ToolLoopLimits, Verdict};
use crate::parse::{SessionContext, StreamParser, parser_for};
use crate::process::{self, KILL_GRACE, SpawnSpec};
use crate::{artifact, detect};
use od_contracts::{
    AgentEvent, FailureCategory, RUN_PROTOCOL_VERSION, RunChunkPayload, RunEndPayload,
    RunErrorPayload, RunRetryPayload, RunStartPayload, RunStatus, sse,
};
use od_runs::Run;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{Notify, mpsc};

/// Longest stdout/stderr line kept; the rest of the line is dropped.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
/// Automatic retries per run (spec 2.3).
pub const MAX_RETRIES: u32 = 1;
const STDERR_TAIL_BYTES: usize = 16 * 1024;
/// After the agent exits, how long to wait for its pipes to close (a
/// background grandchild may hold them open).
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(500);
/// After a protocol session completes and stdin closes, how long the agent
/// may take to exit before it is stopped (ACP agents wait for the next
/// prompt instead of exiting).
const EXIT_AFTER_COMPLETION: Duration = Duration::from_millis(500);

/// Engine-wide settings.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// `PATH` used to find agents and passed to them.
    pub path: OsString,
    /// Environment the agent env is built from.
    pub base_env: Vec<(OsString, OsString)>,
    /// Overrides every runtime's idle timeout (tests).
    pub idle_timeout: Option<Duration>,
    /// Wait between SIGTERM and SIGKILL.
    pub kill_grace: Duration,
    /// First retry delay; doubled for rate limits, capped at 8 s.
    pub retry_base: Duration,
    pub tool_loop: ToolLoopLimits,
}

impl EngineConfig {
    /// The daemon's own `PATH` and environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            path: std::env::var_os("PATH").unwrap_or_default(),
            base_env: std::env::vars_os().collect(),
            idle_timeout: None,
            kill_grace: KILL_GRACE,
            retry_base: Duration::from_millis(500),
            tool_loop: ToolLoopLimits::default(),
        }
    }

    /// `dir` first on `PATH` (tests put mock CLIs there).
    #[must_use]
    pub fn with_path_prefix(mut self, dir: &Path) -> Self {
        let mut parts = vec![dir.to_path_buf()];
        parts.extend(std::env::split_paths(&self.path));
        self.path = std::env::join_paths(parts).unwrap_or(self.path);
        self
    }
}

/// Everything one run needs.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub run: Arc<Run>,
    pub def: &'static RuntimeDef,
    pub cwd: PathBuf,
    pub prompt: String,
    /// Prompt carrying the full transcript, used when resuming fails and the
    /// engine silently starts a fresh session.
    pub fallback_prompt: Option<String>,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    pub extra_dirs: Vec<PathBuf>,
    pub resume_session_id: Option<String>,
    pub permission: PermissionMode,
    /// The agent's environment (see [`crate::env::agent_env`]).
    pub env: Vec<(OsString, OsString)>,
    pub project_id: Option<String>,
    /// Provider settings for [`crate::def::BYOK`]; ignored by CLI runtimes.
    pub byok: Option<ByokConfig>,
}

/// How a run ended, for whoever persists messages and sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub status: RunStatus,
    /// The session to resume next turn, if any.
    pub session_id: Option<String>,
    /// Whether the final attempt continued a stored session.
    pub resumed: bool,
    /// A stored session was rejected; the caller should forget it.
    pub resume_failed: bool,
    /// Concatenated assistant text.
    pub text: String,
    pub exit_code: Option<i32>,
    pub failure_category: Option<FailureCategory>,
}

/// Why a steer was refused (spec 2.3: 409 with one of these reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SteerError {
    #[error("run not found")]
    NotFound,
    #[error("runtime_unsupported")]
    RuntimeUnsupported,
    #[error("run_terminal")]
    RunTerminal,
    #[error("stdin_closed")]
    StdinClosed,
}

/// Live control handle of an in-flight run.
#[derive(Debug)]
struct Active {
    def: &'static RuntimeDef,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    cancel: Notify,
    canceled: AtomicBool,
    pgid: AtomicI32,
}

/// Runs agents. Cheap to clone.
#[derive(Debug, Clone)]
pub struct RunEngine {
    config: Arc<EngineConfig>,
    active: Arc<Mutex<HashMap<String, Arc<Active>>>>,
    capabilities: Arc<Mutex<HashMap<PathBuf, Vec<String>>>>,
}

impl RunEngine {
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config: Arc::new(config),
            active: Arc::default(),
            capabilities: Arc::default(),
        }
    }

    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    fn active(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Active>>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts the run in the background; the handle yields its outcome.
    pub fn launch(&self, spec: LaunchSpec) -> tokio::task::JoinHandle<RunOutcome> {
        let active = Arc::new(Active {
            def: spec.def,
            stdin: tokio::sync::Mutex::new(None),
            cancel: Notify::new(),
            canceled: AtomicBool::new(false),
            pgid: AtomicI32::new(0),
        });
        self.active()
            .insert(spec.run.id().to_owned(), Arc::clone(&active));
        let engine = self.clone();
        tokio::spawn(async move {
            let run_id = spec.run.id().to_owned();
            let outcome = engine.drive(spec, &active).await;
            engine.active().remove(&run_id);
            outcome
        })
    }

    /// Asks a run to stop: abort line (if the protocol has one), close
    /// stdin, SIGTERM the group, SIGKILL after the grace period. Returns
    /// false when the run is not in flight.
    pub fn cancel(&self, run_id: &str) -> bool {
        let Some(active) = self.active().get(run_id).cloned() else {
            return false;
        };
        active.canceled.store(true, Ordering::SeqCst);
        // `notify_one` keeps a permit if the loop is not waiting right now.
        active.cancel.notify_one();
        true
    }

    /// Writes one more user message to a live stream-json stdin.
    ///
    /// # Errors
    /// See [`SteerError`].
    pub async fn steer(&self, run: &Run, message: &str) -> Result<(), SteerError> {
        let active = self.active().get(run.id()).cloned();
        let Some(active) = active else {
            return Err(if run.status().is_terminal() {
                SteerError::RunTerminal
            } else {
                SteerError::NotFound
            });
        };
        if !active.def.supports_steer() {
            return Err(SteerError::RuntimeUnsupported);
        }
        if run.status().is_terminal() || active.canceled.load(Ordering::SeqCst) {
            return Err(SteerError::RunTerminal);
        }
        let mut stdin = active.stdin.lock().await;
        let Some(pipe) = stdin.as_mut() else {
            return Err(SteerError::StdinClosed);
        };
        let line = stream_json_user_line(message);
        if pipe.write_all(line.as_bytes()).await.is_err() || pipe.flush().await.is_err() {
            *stdin = None;
            return Err(SteerError::StdinClosed);
        }
        Ok(())
    }

    async fn capabilities_for(&self, def: &RuntimeDef, bin: &Path) -> Vec<String> {
        if let Some(found) = self
            .capabilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(bin)
        {
            return found.clone();
        }
        let caps = detect::probe_capabilities(def, bin, &self.config.path).await;
        self.capabilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(bin.to_path_buf(), caps.clone());
        caps
    }

    async fn drive(&self, spec: LaunchSpec, active: &Active) -> RunOutcome {
        let run = &spec.run;
        let def = spec.def;
        let is_byok = def.stream_format == StreamFormat::Byok;
        let bin = if is_byok {
            None
        } else {
            detect::resolve_bin(def, &self.config.path)
        };
        let shown_bin = match (&bin, &spec.byok) {
            (Some(b), _) => b.to_string_lossy().into_owned(),
            (None, Some(cfg)) if is_byok => cfg.label(),
            _ => def.bin.to_owned(),
        };
        run.emit_json(
            sse::START,
            &RunStartPayload {
                run_id: run.id().to_owned(),
                agent_id: def.id.to_owned(),
                bin: shown_bin,
                protocol_version: RUN_PROTOCOL_VERSION,
                cwd: Some(spec.cwd.to_string_lossy().into_owned()),
                project_id: spec.project_id.clone(),
                model: spec.model.clone(),
                reasoning: spec.reasoning.clone(),
                resumed: spec.resume_session_id.is_some(),
            },
        );
        let bin = match bin {
            Some(bin) => bin,
            None if is_byok => PathBuf::new(),
            None => {
                let message = format!("`{}` was not found on PATH", def.bin);
                return finish(
                    run,
                    Finish::failed(FailureCategory::SpawnFailed, message, None),
                    &[],
                );
            }
        };
        let capabilities = if is_byok {
            Vec::new()
        } else {
            self.capabilities_for(def, &bin).await
        };

        let mut resume = spec.resume_session_id.clone();
        let mut prompt = spec.prompt.clone();
        let mut retries = 0u32;
        let mut resume_failed = false;
        loop {
            let session_id = match (def.session, &resume) {
                (_, Some(id)) => Some(id.clone()),
                (SessionIdSource::Minted, None) => Some(uuid::Uuid::new_v4().to_string()),
                (SessionIdSource::Captured | SessionIdSource::None, None) => None,
            };
            let args = def.build_args(&ArgvInput {
                model: spec.model.as_deref(),
                reasoning: spec.reasoning.as_deref(),
                cwd: Some(&spec.cwd),
                extra_dirs: &spec.extra_dirs,
                resume_session_id: resume.as_deref(),
                new_session_id: session_id.as_deref(),
                permission: spec.permission,
                capabilities: &capabilities,
            });
            let attempt = if is_byok {
                self.attempt_byok(&spec, active, &prompt).await
            } else {
                self.attempt(
                    &spec,
                    active,
                    SpawnSpec {
                        program: bin.clone(),
                        args,
                        cwd: spec.cwd.clone(),
                        env: spec.env.clone(),
                    },
                    &prompt,
                    resume.as_deref(),
                )
                .await
            };
            let session = attempt.session_id.clone().or(session_id);
            if attempt.canceled {
                let mut f = Finish::canceled(attempt.exit_code, attempt.signal.clone());
                f.session_id = session;
                f.resumed = resume.is_some();
                return finish(run, f, &attempt.text_parts);
            }
            let failed = attempt.failure.is_some();
            if failed
                && resume.is_some()
                && !attempt.visible_output
                && attempt.mentions_any(def.resume_failure_markers)
            {
                // The stored session is gone: start over with the full
                // transcript, silently (spec 2.3).
                tracing::info!(run = %run.id(), "resume failed; reseeding a fresh session");
                resume = None;
                resume_failed = true;
                if let Some(full) = &spec.fallback_prompt {
                    prompt.clone_from(full);
                }
                continue;
            }
            if let Some((category, message)) = &attempt.failure {
                let transient = matches!(
                    category,
                    FailureCategory::RateLimit | FailureCategory::Transient
                );
                if transient && !attempt.visible_output && retries < MAX_RETRIES {
                    retries += 1;
                    let delay = retry_delay(self.config.retry_base, *category, retries);
                    tracing::info!(run = %run.id(), %message, ?delay, "retrying after a transient failure");
                    run.emit_json(
                        sse::RUN_RETRY_ATTEMPTED,
                        &RunRetryPayload {
                            retry_attempt_index: retries,
                            retry_max_attempts: MAX_RETRIES,
                            failure_category: Some(*category),
                            delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                        },
                    );
                    tokio::time::sleep(delay).await;
                    if active.canceled.load(Ordering::SeqCst) {
                        let f = Finish::canceled(None, None);
                        return finish(run, f, &[]);
                    }
                    continue;
                }
            }
            for err in &attempt.held_errors {
                run.emit_json(sse::ERROR, err);
            }
            let mut attempt = attempt;
            if attempt.failure.is_none()
                && matches!(def.stream_format, StreamFormat::Plain | StreamFormat::Byok)
            {
                if let Err(message) = write_artifacts(run, &spec.cwd, &mut attempt) {
                    run.emit_error(&message, None);
                    attempt.failure = Some((FailureCategory::AgentError, message));
                }
            }
            let mut f = match attempt.failure {
                Some((category, message)) => Finish::failed(category, message, attempt.exit_code),
                None => Finish::succeeded(attempt.exit_code),
            };
            f.signal = attempt.signal;
            f.session_id = session;
            f.resumed = resume.is_some();
            f.resume_failed = resume_failed;
            f.artifact_paths = attempt.written_files;
            return finish(run, f, &attempt.text_parts);
        }
    }

    /// One process lifetime.
    async fn attempt(
        &self,
        spec: &LaunchSpec,
        active: &Active,
        spawn: SpawnSpec,
        prompt: &str,
        resume: Option<&str>,
    ) -> Attempt {
        let run = &spec.run;
        let def = spec.def;
        let mut att = Attempt::default();
        let mut parser: Box<dyn StreamParser> = parser_for(
            def.stream_format,
            &SessionContext {
                prompt: prompt.to_owned(),
                cwd: spec.cwd.clone(),
                model: spec.model.clone(),
                reasoning: spec.reasoning.clone(),
                resume_session_id: resume.map(str::to_owned),
                permission: spec.permission,
                request_id: uuid::Uuid::new_v4().to_string(),
                acp_turn_end_completes: def.acp_turn_end_completes,
            },
        );
        let mut child = match process::spawn(&spawn) {
            Ok(c) => c,
            Err(err) => {
                att.failure = Some((
                    FailureCategory::SpawnFailed,
                    format!("could not start {}: {err}", spawn.program.display()),
                ));
                return att;
            }
        };
        let pgid = child.id().and_then(|p| i32::try_from(p).ok()).unwrap_or(0);
        active.pgid.store(pgid, Ordering::SeqCst);
        run.set_process(
            Some(pgid),
            Some(spawn.program.to_string_lossy().into_owned()),
        );

        let (tx, mut rx) = mpsc::channel::<Line>(256);
        if let Some(out) = child.stdout.take() {
            tokio::spawn(read_lines(out, tx.clone(), Line::Stdout));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(read_lines(err, tx.clone(), Line::Stderr));
        }
        drop(tx);
        if let Some(mut stdin) = child.stdin.take() {
            let payload = match def.prompt_input {
                PromptInput::StreamJsonStdin => stream_json_user_line(prompt),
                PromptInput::TextStdin => prompt.to_owned(),
                // The protocol session writes its own requests.
                PromptInput::Rpc => String::new(),
            };
            let wrote =
                stdin.write_all(payload.as_bytes()).await.is_ok() && stdin.flush().await.is_ok();
            if wrote && def.prompt_input != PromptInput::TextStdin {
                // Held open for steering until the turn ends, or for the
                // protocol until the prompt completes.
                *active.stdin.lock().await = Some(stdin);
            }
        }
        write_stdin(active, parser.take_writes()).await;

        let mut guard = ToolLoopGuard::new(self.config.tool_loop);
        let idle = self.config.idle_timeout.unwrap_or(def.idle_timeout);
        let mut deadline = tokio::time::Instant::now() + idle;
        let mut events = Vec::new();
        let mut stop: Option<(FailureCategory, String)> = None;
        let mut exit: Option<std::process::ExitStatus> = None;
        let mut drain_deadline: Option<tokio::time::Instant> = None;
        let mut streams_open = true;
        let mut escalation: Option<tokio::task::JoinHandle<bool>> = None;
        // Set when the protocol completed and stdin was closed.
        let mut completion_deadline: Option<tokio::time::Instant> = None;
        let wait = child.wait();
        tokio::pin!(wait);

        loop {
            if !streams_open && exit.is_some() {
                break;
            }
            let sleep_until = drain_deadline.or(completion_deadline).unwrap_or(deadline);
            tokio::select! {
                line = rx.recv(), if streams_open => {
                    let Some(line) = line else {
                        streams_open = false;
                        continue;
                    };
                    deadline = tokio::time::Instant::now() + idle;
                    match line {
                        Line::Stdout(text) => {
                            parser.line(&text, &mut events);
                            write_stdin(active, parser.take_writes()).await;
                            let verdicts = self
                                .apply_events(run, active, &mut att, &mut guard, &mut events)
                                .await;
                            if let Some(message) = verdicts {
                                if stop.is_none() {
                                    stop = Some((FailureCategory::ToolLoop, message));
                                    let abort = parser.abort_line();
                                    escalation = Some(self.escalate(active, pgid, abort).await);
                                }
                            }
                            if parser.completed() && !att.protocol_completed {
                                att.protocol_completed = true;
                                *active.stdin.lock().await = None;
                                completion_deadline =
                                    Some(tokio::time::Instant::now() + EXIT_AFTER_COMPLETION);
                            }
                        }
                        Line::Stderr(text) => {
                            att.push_stderr(&text);
                            run.emit_json(sse::STDERR, &RunChunkPayload { chunk: format!("{text}\n") });
                        }
                    }
                }
                status = &mut wait, if exit.is_none() => {
                    exit = Some(status.unwrap_or_else(|_| std::process::ExitStatus::default()));
                    drain_deadline = Some(tokio::time::Instant::now() + DRAIN_AFTER_EXIT);
                }
                () = active.cancel.notified(), if escalation.is_none() => {
                    att.canceled = true;
                    let abort = parser.abort_line();
                    escalation = Some(self.escalate(active, pgid, abort).await);
                }
                () = tokio::time::sleep_until(sleep_until) => {
                    if drain_deadline.is_some() {
                        // Exited; a grandchild still holds the pipes.
                        break;
                    }
                    if completion_deadline.take().is_some() {
                        // Done, but the agent waits for another prompt.
                        if escalation.is_none() {
                            escalation = Some(self.escalate(active, pgid, None).await);
                        }
                        continue;
                    }
                    if stop.is_none() && escalation.is_none() {
                        let message = format!("no output for {} s; stopping the agent", idle.as_secs());
                        stop = Some((FailureCategory::Timeout, message));
                        let abort = parser.abort_line();
                        escalation = Some(self.escalate(active, pgid, abort).await);
                    }
                    deadline = tokio::time::Instant::now() + idle;
                }
            }
        }
        parser.finish(&mut events);
        let _ = self
            .apply_events(run, active, &mut att, &mut guard, &mut events)
            .await;
        *active.stdin.lock().await = None;
        if let Some(handle) = escalation {
            let _ = handle.await;
        }
        // The agent is gone but its group may not be (`cmd &`, a dev
        // server): the run owns those processes, so they end with it.
        if pgid > 0 && process::group_alive(pgid) {
            tracing::warn!(run = %run.id(), pgid, "agent left processes behind; stopping its group");
            process::terminate_group(pgid, self.config.kill_grace).await;
        }
        run.set_process(None, None);
        let exit = exit.unwrap_or_default();
        att.exit_code = exit.code();
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            att.signal = exit.signal().map(signal_name);
        }
        if att.canceled {
            return att;
        }
        // A completed protocol session is stopped by us; its signal exit is
        // not a failure.
        att.failure = stop.or_else(|| att.classify_exit(exit.success() || att.protocol_completed));
        att
    }

    /// One BYOK call: the provider stream instead of a process.
    async fn attempt_byok(&self, spec: &LaunchSpec, active: &Active, prompt: &str) -> Attempt {
        let mut att = Attempt::default();
        let Some(config) = spec.byok.clone() else {
            att.failure = Some((
                FailureCategory::SpawnFailed,
                "no API key is configured; add one in Settings".to_owned(),
            ));
            return att;
        };
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
        let mut task = tokio::spawn(byok::stream(
            ByokRequest {
                config,
                model: spec.model.clone(),
                prompt: prompt.to_owned(),
            },
            tx,
        ));
        let mut guard = ToolLoopGuard::new(self.config.tool_loop);
        let idle = self.config.idle_timeout.unwrap_or(spec.def.idle_timeout);
        let mut deadline = tokio::time::Instant::now() + idle;
        let mut events = Vec::new();
        let mut result = None;
        loop {
            tokio::select! {
                ev = rx.recv() => {
                    let Some(ev) = ev else {
                        // The stream task dropped its sender: it is done.
                        if result.is_none() {
                            result = Some((&mut task).await);
                        }
                        break;
                    };
                    deadline = tokio::time::Instant::now() + idle;
                    events.push(ev);
                    let _ = self
                        .apply_events(&spec.run, active, &mut att, &mut guard, &mut events)
                        .await;
                }
                joined = &mut task, if result.is_none() => {
                    result = Some(joined);
                }
                () = active.cancel.notified() => {
                    task.abort();
                    att.canceled = true;
                    return att;
                }
                () = tokio::time::sleep_until(deadline) => {
                    task.abort();
                    att.failure = Some((
                        FailureCategory::Timeout,
                        format!("no output for {} s; stopping the request", idle.as_secs()),
                    ));
                    return att;
                }
            }
        }
        let failure = match result {
            Some(Ok(Ok(()))) | None => None,
            Some(Ok(Err(f))) => Some((f.category, f.message)),
            Some(Err(join)) => Some((FailureCategory::AgentError, join.to_string())),
        };
        if let Some((category, message)) = failure {
            att.error_text.push_str(&message);
            att.held_errors.push(RunErrorPayload {
                message: message.clone(),
                code: None,
            });
            att.failure = Some((category, message));
        } else {
            att.failure = att.classify_exit(true);
        }
        att
    }

    /// Emits parsed events, applies Claude's stdin bookkeeping and the
    /// tool-loop guard. Returns a stop message when the guard says stop.
    async fn apply_events(
        &self,
        run: &Run,
        active: &Active,
        att: &mut Attempt,
        guard: &mut ToolLoopGuard,
        events: &mut Vec<AgentEvent>,
    ) -> Option<String> {
        let mut stop = None;
        for event in events.drain(..) {
            if event.is_visible_output() {
                att.visible_output = true;
            }
            match &event {
                AgentEvent::Error { message, code, .. } => {
                    att.error_text.push_str(message);
                    att.error_text.push('\n');
                    // Held until the attempt ends: a retried attempt's
                    // error is never shown.
                    att.held_errors.push(RunErrorPayload {
                        message: message.clone(),
                        code: code.clone(),
                    });
                    continue;
                }
                AgentEvent::Status {
                    session_id: Some(id),
                    ..
                } if active.def.session == SessionIdSource::Captured => {
                    att.session_id = Some(id.clone());
                }
                AgentEvent::TextDelta { delta } => att.text_parts.push(delta.clone()),
                AgentEvent::ToolUse { name, input, .. } => {
                    if matches!(name.as_str(), "Write" | "Edit" | "MultiEdit") {
                        if let Some(path) = input.get("file_path").and_then(Value::as_str) {
                            if !att.written_files.iter().any(|p| p == path) {
                                att.written_files.push(path.to_owned());
                            }
                        }
                    }
                }
                AgentEvent::TurnEnd { stop_reason }
                    if stop_reason != "tool_use"
                        && active.def.prompt_input == PromptInput::StreamJsonStdin =>
                {
                    // Turn over: close stdin so the CLI exits. Closing on
                    // `tool_use` would cut the answer short.
                    *active.stdin.lock().await = None;
                }
                AgentEvent::Usage {
                    stop_reason,
                    is_error,
                    ..
                } => {
                    if *is_error {
                        att.result_error = true;
                    }
                    let steerable = active.def.prompt_input == PromptInput::StreamJsonStdin;
                    if steerable && (*is_error || stop_reason.as_deref() != Some("tool_use")) {
                        *active.stdin.lock().await = None;
                    }
                }
                _ => {}
            }
            match guard.observe(&event) {
                Verdict::Continue => {}
                Verdict::Warn(warning) => {
                    run.emit_json(sse::AGENT, &event);
                    run.emit_json(sse::AGENT, &warning);
                    continue;
                }
                Verdict::Stop(warning) => {
                    run.emit_json(sse::AGENT, &event);
                    run.emit_json(sse::AGENT, &warning);
                    stop = Some("the agent kept repeating failing tool calls".to_owned());
                    continue;
                }
            }
            run.emit_json(sse::AGENT, &event);
        }
        stop
    }

    /// Spawns the stop sequence for the live attempt.
    async fn escalate(
        &self,
        active: &Active,
        pgid: i32,
        abort: Option<String>,
    ) -> tokio::task::JoinHandle<bool> {
        if let Some(line) = abort {
            let mut stdin = active.stdin.lock().await;
            if let Some(pipe) = stdin.as_mut() {
                let _ = pipe.write_all(line.as_bytes()).await;
                let _ = pipe.flush().await;
            }
            drop(stdin);
            let grace = self.config.kill_grace;
            if process::wait_group_gone(pgid, grace).await {
                return tokio::spawn(async { false });
            }
        }
        *active.stdin.lock().await = None;
        let grace = self.config.kill_grace;
        tokio::spawn(async move { process::terminate_group(pgid, grace).await })
    }
}

#[derive(Debug)]
enum Line {
    Stdout(String),
    Stderr(String),
}

async fn read_lines<R>(reader: R, tx: mpsc::Sender<Line>, wrap: fn(String) -> Line)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match read_line_bounded(&mut reader, &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                    buf.pop();
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                if tx.send(wrap(text)).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Reads one line, keeping at most [`MAX_LINE_BYTES`] of it.
async fn read_line_bounded<R>(
    reader: &mut BufReader<R>,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut total = 0;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(total);
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        let room = MAX_LINE_BYTES.saturating_sub(buf.len());
        buf.extend_from_slice(&chunk[..take.min(room)]);
        reader.consume(take);
        total += take;
        if done {
            return Ok(total);
        }
    }
}

/// Writes protocol lines to the live stdin; a broken pipe closes it.
async fn write_stdin(active: &Active, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    let mut stdin = active.stdin.lock().await;
    let Some(pipe) = stdin.as_mut() else {
        return;
    };
    for line in lines {
        if pipe.write_all(line.as_bytes()).await.is_err() {
            *stdin = None;
            return;
        }
    }
    if pipe.flush().await.is_err() {
        *stdin = None;
    }
}

/// Splits the answer's `<artifact>` blocks into files in `cwd` and reports
/// each as a `Write` tool call, so the chat and `artifactPaths` see them.
fn write_artifacts(run: &Run, cwd: &Path, att: &mut Attempt) -> Result<(), String> {
    let artifacts = artifact::extract(&att.text_parts.concat());
    let paths = artifact::persist(cwd, &artifacts)
        .map_err(|e| format!("could not write the artifact into the project: {e}"))?;
    for (i, (a, path)) in artifacts.iter().zip(paths).enumerate() {
        let path = path.to_string_lossy().into_owned();
        let id = format!("artifact-{}", i + 1);
        run.emit_json(
            sse::AGENT,
            &AgentEvent::ToolUse {
                id: id.clone(),
                name: "Write".to_owned(),
                input: json!({"file_path": path, "title": a.title}),
            },
        );
        run.emit_json(
            sse::AGENT,
            &AgentEvent::ToolResult {
                tool_use_id: id,
                content: format!("wrote {}", a.file_name),
                is_error: false,
            },
        );
        if !att.written_files.contains(&path) {
            att.written_files.push(path);
        }
    }
    Ok(())
}

/// One stream-json user message line.
#[must_use]
pub fn stream_json_user_line(text: &str) -> String {
    let mut line = json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]}
    })
    .to_string();
    line.push('\n');
    line
}

#[derive(Debug, Default)]
struct Attempt {
    canceled: bool,
    /// The protocol session reported the prompt complete.
    protocol_completed: bool,
    visible_output: bool,
    held_errors: Vec<RunErrorPayload>,
    error_text: String,
    stderr_tail: String,
    session_id: Option<String>,
    text_parts: Vec<String>,
    written_files: Vec<String>,
    result_error: bool,
    exit_code: Option<i32>,
    signal: Option<String>,
    failure: Option<(FailureCategory, String)>,
}

impl Attempt {
    fn push_stderr(&mut self, line: &str) {
        self.stderr_tail.push_str(line);
        self.stderr_tail.push('\n');
        if self.stderr_tail.len() > STDERR_TAIL_BYTES {
            let mut cut = self.stderr_tail.len() - STDERR_TAIL_BYTES;
            while !self.stderr_tail.is_char_boundary(cut) {
                cut += 1;
            }
            self.stderr_tail.drain(..cut);
        }
    }

    fn mentions_any(&self, markers: &[&str]) -> bool {
        markers
            .iter()
            .any(|m| self.stderr_tail.contains(m) || self.error_text.contains(m))
    }

    fn classify_exit(&self, success: bool) -> Option<(FailureCategory, String)> {
        if success && !self.result_error && self.held_errors.is_empty() {
            return None;
        }
        let evidence = format!("{}\n{}", self.error_text, self.stderr_tail);
        let message = self
            .held_errors
            .last()
            .map(|e| e.message.clone())
            .or_else(|| {
                self.stderr_tail
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| match self.exit_code {
                Some(code) => format!("agent exited with code {code}"),
                None => "agent was terminated by a signal".to_owned(),
            });
        Some((classify(&evidence), message))
    }
}

/// Transient vs. permanent, from error text and stderr.
#[must_use]
pub fn classify(evidence: &str) -> FailureCategory {
    let lower = evidence.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if any(&[
        "rate limit",
        "rate_limit",
        "429",
        "too many requests",
        "overloaded",
    ]) {
        FailureCategory::RateLimit
    } else if any(&[
        "econnreset",
        "etimedout",
        "econnrefused",
        "socket hang up",
        "network error",
        "connection reset",
        "stream disconnected",
        "503",
        "502",
        "504",
        "service unavailable",
    ]) {
        FailureCategory::Transient
    } else {
        FailureCategory::AgentError
    }
}

/// `base` (×2 for rate limits) doubled per retry, capped at 8 s, ±20 %.
#[must_use]
pub fn retry_delay(base: Duration, category: FailureCategory, attempt: u32) -> Duration {
    let base = if category == FailureCategory::RateLimit {
        base * 2
    } else {
        base
    };
    let doubled = base.saturating_mul(1 << attempt.saturating_sub(1).min(5));
    let capped = doubled.min(Duration::from_secs(8));
    // Jitter from the clock's sub-second noise; no RNG dependency needed.
    let noise = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() % 400);
    let factor = 0.8 + f64::from(noise) / 1000.0;
    capped.mul_f64(factor)
}

fn signal_name(sig: i32) -> String {
    match sig {
        9 => "SIGKILL".to_owned(),
        15 => "SIGTERM".to_owned(),
        2 => "SIGINT".to_owned(),
        n => format!("signal {n}"),
    }
}

struct Finish {
    status: RunStatus,
    code: Option<i32>,
    signal: Option<String>,
    category: Option<FailureCategory>,
    error: Option<String>,
    session_id: Option<String>,
    resumed: bool,
    resume_failed: bool,
    artifact_paths: Vec<String>,
}

impl Finish {
    fn base(status: RunStatus, code: Option<i32>) -> Self {
        Self {
            status,
            code,
            signal: None,
            category: None,
            error: None,
            session_id: None,
            resumed: false,
            resume_failed: false,
            artifact_paths: Vec::new(),
        }
    }

    fn succeeded(code: Option<i32>) -> Self {
        Self::base(RunStatus::Succeeded, code)
    }

    fn canceled(code: Option<i32>, signal: Option<String>) -> Self {
        let mut f = Self::base(RunStatus::Canceled, code);
        f.signal = signal;
        f
    }

    fn failed(category: FailureCategory, message: String, code: Option<i32>) -> Self {
        let mut f = Self::base(RunStatus::Failed, code);
        f.category = Some(category);
        f.error = Some(message);
        f
    }
}

fn finish(run: &Run, f: Finish, text_parts: &[String]) -> RunOutcome {
    if f.status == RunStatus::Failed {
        if let Some(message) = &f.error {
            // Spawn and guard failures have no agent error of their own.
            if matches!(
                f.category,
                Some(
                    FailureCategory::SpawnFailed
                        | FailureCategory::Timeout
                        | FailureCategory::ToolLoop
                )
            ) {
                run.emit_error(message, None);
            }
        }
    }
    run.finish(RunEndPayload {
        status: f.status,
        code: f.code,
        signal: f.signal,
        terminal_at: od_runs::now_ms(),
        artifact_paths: f.artifact_paths,
        resumable: f.session_id.is_some() && f.status != RunStatus::Succeeded,
        retryable: f.status == RunStatus::Failed,
        failure_category: f.category,
        error: f.error,
    });
    RunOutcome {
        status: f.status,
        session_id: f.session_id,
        resumed: f.resumed,
        resume_failed: f.resume_failed,
        text: text_parts.concat(),
        exit_code: f.code,
        failure_category: f.category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(
            classify("API Error: 429 Too Many Requests"),
            FailureCategory::RateLimit
        );
        assert_eq!(classify("read ECONNRESET"), FailureCategory::Transient);
        assert_eq!(classify("invalid model"), FailureCategory::AgentError);
    }

    #[test]
    fn retry_delay_doubles_for_rate_limit_and_caps() {
        let base = Duration::from_millis(500);
        let d = retry_delay(base, FailureCategory::Transient, 1);
        assert!(d >= Duration::from_millis(400) && d <= Duration::from_millis(600));
        let r = retry_delay(base, FailureCategory::RateLimit, 1);
        assert!(r >= Duration::from_millis(800) && r <= Duration::from_millis(1200));
        assert!(retry_delay(base, FailureCategory::Transient, 9) <= Duration::from_millis(9600));
    }
}
