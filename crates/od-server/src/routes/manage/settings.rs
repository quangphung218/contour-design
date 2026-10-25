//! Settings support (spec 1.5):
//!
//! - `POST /api/test/connection` `{mode: "agent", agentId, model?,
//!   reasoning?}` runs the real CLI once in a scratch folder with "Reply
//!   with only: ok" (45 s budget); `{mode: "provider", protocol, baseUrl?,
//!   apiKey?, model?}` sends one small BYOK completion (12 s).
//! - `POST /api/provider/models` lists the models a BYOK key can use.
//!   Without `apiKey` both use the stored key, but only for the stored
//!   base URL: a typed URL never receives the saved key.
//! - `GET/PUT /api/agent-cli-env`: "Advanced: proxy & custom paths", extra
//!   env per agent (`<data>/agent-cli-env.json`, `0600`); key values come
//!   back as a hint only and a `PUT` without them keeps the stored ones.
//!   Runs add the agent's entries to its environment ([`env_for`]).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use od_runs::{RunMeta, Runs};
use od_runtime::byok::{self, ByokConfig, ByokProtocol};
use od_runtime::byok_probe::{self, TestResult};
use od_runtime::def::StreamFormat;
use od_runtime::{LaunchSpec, McpLaunch, PermissionMode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The original's agent smoke-test budget.
pub const AGENT_TEST_TIMEOUT: Duration = Duration::from_secs(45);
const AGENT_CLI_ENV_FILE: &str = "agent-cli-env.json";
/// Env keys the "Advanced" panel may set, per agent.
pub const AGENT_CLI_ENV_KEYS: &[(&str, &[&str])] = &[
    (
        "claude",
        &[
            "CLAUDE_CONFIG_DIR",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_API_KEY",
        ],
    ),
    (
        "codex",
        &[
            "CODEX_HOME",
            "CODEX_BIN",
            "OPENAI_BASE_URL",
            "CODEX_API_KEY",
            "OPENAI_API_KEY",
        ],
    ),
];

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn is_secret(key: &str) -> bool {
    key.ends_with("_API_KEY") || key.ends_with("_TOKEN")
}

/// Keys that decide where a saved secret goes: a base URL, or a config dir,
/// home or binary that can set its own.
fn is_routing(key: &str) -> bool {
    key.ends_with("_BASE_URL") || matches!(key, "CLAUDE_CONFIG_DIR" | "CODEX_HOME" | "CODEX_BIN")
}

fn allowed_keys(agent: &str) -> &'static [&'static str] {
    AGENT_CLI_ENV_KEYS
        .iter()
        .find(|(a, _)| *a == agent)
        .map_or(&[], |(_, keys)| keys)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRequest {
    /// `agent` or `provider`; unused by the model listing.
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Azure OpenAI `api-version`.
    #[serde(default)]
    pub api_version: Option<String>,
}

/// Result of an agent test, shaped like the provider one.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTestResult {
    pub ok: bool,
    /// `success`, `agent_not_installed`, `agent_spawn_failed`, `timeout`
    /// or `unknown`.
    pub kind: &'static str,
    pub latency_ms: u64,
    pub agent_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn clean(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Whether two base URLs name the same endpoint (empty means the protocol's
/// own host).
pub(crate) fn same_base(a: Option<&str>, b: Option<&str>) -> bool {
    let norm = |v: Option<&str>| clean(v).map(|s| byok_probe::normalize_base_url(&s).unwrap_or(s));
    norm(a) == norm(b)
}

/// The BYOK config a test or model listing uses: the request's fields, with
/// the stored key only when the protocol and base URL are the stored ones.
fn provider_config(state: &AppState, req: &TestRequest) -> ApiResult<ByokConfig> {
    let protocol = match req.protocol.as_deref().and_then(ByokProtocol::parse) {
        Some(p) => p,
        None => {
            return Err(ApiErr::bad_request(format!(
                "protocol must be anthropic, openai or azure, got {:?}",
                req.protocol
            )));
        }
    };
    let base_url = clean(req.base_url.as_deref());
    let api_key = match clean(req.api_key.as_deref()) {
        Some(k) => Some(k),
        None => state
            .store
            .data_dir()
            .app_setting(byok::APP_CONFIG_KEY)
            .and_then(|stored| ByokConfig::from_value(&stored))
            .filter(|stored| {
                stored.protocol == protocol
                    && same_base(stored.base_url.as_deref(), base_url.as_deref())
            })
            .map(|stored| stored.api_key),
    };
    let api_key = api_key.ok_or_else(|| ApiErr::bad_request("an API key is required"))?;
    Ok(ByokConfig {
        protocol,
        api_key,
        base_url,
        model: clean(req.model.as_deref()),
        max_tokens: None,
        api_version: clean(req.api_version.as_deref()),
    })
}

pub async fn test_connection(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<TestRequest>,
) -> ApiResult<ApiJson<Value>> {
    match req.mode.as_str() {
        "provider" => {
            let config = provider_config(&state, &req)?;
            let result: TestResult =
                byok_probe::test_connection(&config, byok_probe::TEST_TIMEOUT).await;
            tracing::info!(agent = "byok", ok = result.ok, category = ?result.kind, "connection test");
            Ok(ApiJson(serde_json::to_value(result).unwrap_or(Value::Null)))
        }
        "agent" => {
            let agent = clean(req.agent_id.as_deref())
                .ok_or_else(|| ApiErr::bad_request("agentId is required"))?;
            let result = test_agent(&state, &agent, &req, AGENT_TEST_TIMEOUT).await?;
            tracing::info!(
                agent = %agent,
                ok = result.ok,
                category = test_category(&result),
                "connection test"
            );
            Ok(ApiJson(serde_json::to_value(result).unwrap_or(Value::Null)))
        }
        other => Err(ApiErr::bad_request(format!(
            "mode must be agent or provider, got {other:?}"
        ))),
    }
}

/// The result's kind, or `auth` when a failure reads like a missing sign-in
/// (what a freshly installed Claude Code or Codex answers).
fn test_category(result: &AgentTestResult) -> &'static str {
    let detail = result.detail.as_deref().unwrap_or_default().to_lowercase();
    let auth = [
        "login",
        "log in",
        "sign in",
        "api key",
        "401",
        "unauthorized",
        "authenticat",
    ];
    if !result.ok && auth.iter().any(|w| detail.contains(w)) {
        "auth"
    } else {
        result.kind
    }
}

/// Runs `agent` once with the smoke prompt in a scratch folder.
async fn test_agent(
    state: &AppState,
    agent: &str,
    req: &TestRequest,
    timeout: Duration,
) -> ApiResult<AgentTestResult> {
    let def = od_runtime::def::find(agent)
        .ok_or_else(|| ApiErr::bad_request(format!("unknown agent {agent:?}")))?;
    let name = def.name.to_owned();
    let result = |ok, kind, ms, sample: Option<String>, detail: Option<String>| AgentTestResult {
        ok,
        kind,
        latency_ms: ms,
        agent_name: name.clone(),
        sample,
        detail,
    };
    let engine = &state.runs.engine;
    let byok = if def.stream_format == StreamFormat::Byok {
        let cfg = state
            .store
            .data_dir()
            .app_setting(byok::APP_CONFIG_KEY)
            .and_then(|v| ByokConfig::from_value(&v));
        if cfg.is_none() {
            return Ok(result(
                false,
                "agent_not_installed",
                0,
                None,
                Some("no API key configured".to_owned()),
            ));
        }
        cfg
    } else {
        let custom = custom_bin(state.store.data_dir().root(), def.id)
            .filter(|b| od_runtime::detect::is_executable(b));
        if custom.is_none() && od_runtime::detect::resolve_bin(def, &engine.config().path).is_none()
        {
            return Ok(result(
                false,
                "agent_not_installed",
                0,
                None,
                Some(format!("`{}` not found on PATH", def.bin)),
            ));
        }
        None
    };
    let scratch =
        tempfile::tempdir().map_err(|e| ApiErr::internal(format!("scratch folder: {e}")))?;
    let runs = Runs::new(scratch.path().join("runs"));
    let run_id = uuid::Uuid::new_v4().to_string();
    let (run, _) = runs
        .create_or_reuse(RunMeta {
            run_id: run_id.clone(),
            agent_id: def.id.to_owned(),
            project_id: None,
            conversation_id: None,
            assistant_message_id: None,
            client_request_id: None,
            request_hash: None,
            created_at: od_runs::now_ms(),
        })
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let cwd = scratch.path().join("work");
    std::fs::create_dir_all(&cwd).map_err(|e| ApiErr::internal(e.to_string()))?;
    let mut env = od_runtime::env::agent_env(
        def,
        &engine.config().base_env,
        &engine.config().path,
        &od_runtime::env::RunEnv {
            data_dir: None,
            daemon_url: None,
            tool_token: None,
            project_id: None,
            project_dir: Some(&cwd),
            od_bin: None,
            run_id: Some(&run_id),
        },
    );
    env.extend(env_for(state.store.data_dir().root(), def.id));
    let started = Instant::now();
    let handle = engine.launch(LaunchSpec {
        run: Arc::clone(&run),
        def,
        cwd,
        prompt: byok_probe::SMOKE_PROMPT.to_owned(),
        fallback_prompt: None,
        model: clean(req.model.as_deref()).filter(|m| m != "default"),
        reasoning: clean(req.reasoning.as_deref()).filter(|m| m != "default"),
        service_tier: None,
        bin: custom_bin(state.store.data_dir().root(), def.id),
        extra_dirs: Vec::new(),
        resume_session_id: None,
        permission: PermissionMode::Ask,
        env,
        project_id: None,
        byok,
        mcp: McpLaunch::default(),
    });
    let ms = |s: Instant| u64::try_from(s.elapsed().as_millis()).unwrap_or(u64::MAX);
    let outcome = match tokio::time::timeout(timeout, handle).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            return Ok(result(
                false,
                "unknown",
                ms(started),
                None,
                Some(e.to_string()),
            ));
        }
        Err(_) => {
            engine.cancel(&run_id);
            return Ok(result(
                false,
                "timeout",
                ms(started),
                None,
                Some(format!("no answer within {} ms", timeout.as_millis())),
            ));
        }
    };
    let elapsed = ms(started);
    let sample: String = outcome.text.trim().chars().take(120).collect();
    if outcome.status == od_contracts::RunStatus::Succeeded {
        return Ok(result(true, "success", elapsed, Some(sample), None));
    }
    let detail = run
        .end_payload()
        .and_then(|e| e.error)
        .unwrap_or_else(|| format!("the agent ended {}", outcome.status.as_str()));
    let kind = if matches!(
        outcome.failure_category,
        Some(od_contracts::FailureCategory::SpawnFailed)
    ) {
        "agent_spawn_failed"
    } else {
        "unknown"
    };
    Ok(result(false, kind, elapsed, None, Some(detail)))
}

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub models: Vec<String>,
}

pub async fn provider_models(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<TestRequest>,
) -> ApiResult<ApiJson<ModelsResponse>> {
    let config = provider_config(&state, &req)?;
    let models = byok_probe::list_models(&config, byok_probe::TEST_TIMEOUT)
        .await
        .map_err(|e| {
            ApiErr::new(
                axum::http::StatusCode::BAD_GATEWAY,
                od_contracts::ErrorCode::UpstreamUnavailable,
                e,
            )
        })?;
    Ok(ApiJson(ModelsResponse { models }))
}

// ------------------------------------------------------------ agent CLI env

type AgentEnv = BTreeMap<String, BTreeMap<String, String>>;

fn env_path(root: &Path) -> std::path::PathBuf {
    root.join(AGENT_CLI_ENV_FILE)
}

fn read_env(root: &Path) -> AgentEnv {
    std::fs::read(env_path(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The executable the user picked for `agent` (`CODEX_BIN`).
#[must_use]
pub fn custom_bin(root: &Path, agent: &str) -> Option<std::path::PathBuf> {
    let key = format!("{}_BIN", agent.to_ascii_uppercase());
    if !allowed_keys(agent).contains(&key.as_str()) {
        return None;
    }
    read_env(root)
        .remove(agent)?
        .remove(&key)
        .filter(|v| Path::new(v).is_absolute())
        .map(std::path::PathBuf::from)
}

/// The extra environment of `agent`'s runs.
#[must_use]
pub fn env_for(root: &Path, agent: &str) -> Vec<(OsString, OsString)> {
    let allowed = allowed_keys(agent);
    read_env(root)
        .remove(agent)
        .unwrap_or_default()
        .into_iter()
        // `CODEX_BIN` picks the executable; it is not the agent's env.
        .filter(|(k, v)| allowed.contains(&k.as_str()) && !k.ends_with("_BIN") && !v.is_empty())
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

fn hint(value: &str) -> String {
    let tail: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if value.chars().count() >= 8 {
        format!("…{tail}")
    } else {
        "…".to_owned()
    }
}

/// Secrets replaced by `{set: true, hint}`; other values as strings.
fn redacted(env: &AgentEnv) -> Value {
    let mut out = Map::new();
    for (agent, vars) in env {
        let mut o = Map::new();
        for (k, v) in vars {
            let value = if is_secret(k) {
                json!({"set": true, "hint": hint(v)})
            } else {
                json!(v)
            };
            o.insert(k.clone(), value);
        }
        out.insert(agent.clone(), Value::Object(o));
    }
    json!({"env": out, "keys": AGENT_CLI_ENV_KEYS.iter().map(|(a, k)| (a.to_string(), json!(k))).collect::<Map<_, _>>()})
}

pub async fn get_agent_env(State(state): State<AppState>) -> ApiResult<ApiJson<Value>> {
    let root = state.store.data_dir().root().to_path_buf();
    Ok(ApiJson(redacted(&read_env(&root))))
}

/// `{env: {agent: {KEY: "value" | null}}}`: a string sets, `null` or `""`
/// clears, a missing secret keeps the stored value, unless the agent's base
/// URL, config dir, home or binary changed: a saved key belongs to the
/// route it was saved with.
pub async fn put_agent_env(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<ApiJson<Value>> {
    let patch = body
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiErr::bad_request("env must be an object"))?
        .clone();
    let root = state.store.data_dir().root().to_path_buf();
    let saved = tokio::task::spawn_blocking(move || -> Result<AgentEnv, String> {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut env = read_env(&root);
        for (agent, vars) in patch {
            let allowed = allowed_keys(&agent);
            let vars = vars
                .as_object()
                .ok_or_else(|| format!("env.{agent} must be an object"))?;
            let stored = env.entry(agent.clone()).or_default();
            let routes = |m: &BTreeMap<String, String>| {
                m.iter()
                    .filter(|(k, _)| is_routing(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            };
            let before = routes(stored);
            for (key, value) in vars {
                if !allowed.contains(&key.as_str()) {
                    return Err(format!("{key} cannot be set for {agent}"));
                }
                match value {
                    Value::Null => {
                        stored.remove(key);
                    }
                    Value::String(s) if s.trim().is_empty() => {
                        stored.remove(key);
                    }
                    Value::String(s) if s.chars().any(char::is_control) => {
                        return Err(format!("{key} must be one line"));
                    }
                    Value::String(s) => {
                        stored.insert(key.clone(), s.trim().to_owned());
                    }
                    // `{set: true}` echoes a redacted secret: keep it.
                    Value::Object(_) => {}
                    other => return Err(format!("{key} must be a string, got {other}")),
                }
            }
            if routes(stored) != before {
                stored
                    .retain(|k, _| !is_secret(k) || matches!(vars.get(k), Some(Value::String(_))));
            }
            if stored.is_empty() {
                env.remove(&agent);
            }
        }
        let bytes = serde_json::to_vec_pretty(&env).map_err(|e| e.to_string())?;
        crate::routes::ui::write_atomic(&env_path(&root), &bytes).map_err(|e| e.to_string())?;
        Ok(env)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?
    .map_err(ApiErr::bad_request)?;
    Ok(ApiJson(redacted(&saved)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_key_follows_the_stored_base_url_only() {
        assert!(same_base(None, None));
        assert!(same_base(Some("https://a.com/v1/"), Some("a.com/v1")));
        assert!(!same_base(
            Some("https://a.com"),
            Some("https://evil.example")
        ));
        assert!(!same_base(None, Some("https://evil.example")));
    }

    #[test]
    fn secrets_are_hinted_and_unknown_agents_get_nothing() {
        let mut env = AgentEnv::new();
        env.entry("claude".into()).or_default().extend([
            (
                "ANTHROPIC_API_KEY".to_owned(),
                "sk-ant-1234567890".to_owned(),
            ),
            ("ANTHROPIC_BASE_URL".to_owned(), "http://proxy".to_owned()),
        ]);
        let v = redacted(&env);
        assert_eq!(v["env"]["claude"]["ANTHROPIC_API_KEY"]["hint"], "…7890");
        assert_eq!(v["env"]["claude"]["ANTHROPIC_BASE_URL"], "http://proxy");
        assert!(!v.to_string().contains("sk-ant-1234567890"));
        assert!(allowed_keys("cursor").is_empty());
    }
}
