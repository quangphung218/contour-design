//! Settings ▸ Memory beyond the entry list (the original's
//! `routes/memory.ts`):
//!
//! - `GET/PATCH /api/memory/config`: master switch and memory hooks.
//! - `GET/PUT /api/memory/index` `{index}`: the editable `MEMORY.md`.
//! - `POST /api/memory/import` `{text}` → `{suggestions}`: facts found in a
//!   Markdown file, for the user to pick and save.
//! - `GET /api/memory/events`: SSE `change` `{kind, count?, source?, scope,
//!   at}`; the UI's memory toast listens for `extract`.
//!
//! Runs call [`prepare_turn`] for the memory a prompt carries.

use crate::cas::IfMatch;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use od_store::memory_index::{
    self, MAX_INDEX_BYTES, MemoryConfig, MemoryConfigPatch, MemoryFiles, Stored, StoredConfig,
    Suggestion,
};
use od_store::{Store, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;
use tokio::sync::broadcast;

const KEEPALIVE: Duration = Duration::from_secs(15);

/// One memory event hub per data dir: tests run several daemons in one
/// process, and a toast must only hear its own daemon.
fn hub(root: PathBuf) -> broadcast::Sender<Value> {
    static HUBS: OnceLock<Mutex<HashMap<PathBuf, broadcast::Sender<Value>>>> = OnceLock::new();
    let mut hubs = HUBS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    hubs.entry(root)
        .or_insert_with(|| broadcast::channel(64).0)
        .clone()
}

/// Announces a memory change to every Settings page and toast.
pub fn announce(store: &Store, change: Value) {
    let _ = hub(store.data_dir().root().to_path_buf()).send(change);
}

fn files(store: &Store) -> MemoryFiles {
    MemoryFiles::in_data_dir(store.data_dir().root())
}

/// What a turn's prompt carries from memory.
#[derive(Debug, Default)]
pub struct TurnMemory {
    /// Global facts (when memory is on), then the project's.
    pub facts: Vec<String>,
    /// Guidance of the memory hooks that are on.
    pub hooks: Option<String>,
}

/// Records what `message` asks to remember and returns the memory of the
/// turn. A "remember…" line always goes to the project's memory (spec 1.5);
/// with "Learn from chats" on, a remembered fact or a stated preference
/// also becomes global memory, announced as an `extract` change.
///
/// # Errors
/// SQLite errors.
pub fn prepare_turn(s: &Store, project_id: &str, message: &str) -> Result<TurnMemory, StoreError> {
    let files = files(s);
    let config = files.config();
    let mut learned = 0usize;
    if let Some(fact) = od_store::memory_from_message(message) {
        s.add_memory(project_id, &fact, od_contracts::MemorySource::Chat)?;
        announce(
            s,
            change(
                "extract",
                json!({ "count": 1, "source": "heuristic", "scope": "project" }),
            ),
        );
    }
    if config.enabled
        && config.chat_extraction_enabled
        && let Some(Suggestion { kind, text }) = memory_index::learned_from_message(message)
    {
        let known = s.list_global_memory()?.iter().any(|m| m.text == text);
        // A full list is not this turn's failure.
        if !known && let Ok(entry) = s.add_global_memory(&kind, &text) {
            // The fact is saved; an index that could not take its
            // line is not this run's failure.
            if let Err(e) = files.link(&entry) {
                tracing::warn!(error = %e, id = %entry.id, "linking a learned memory in MEMORY.md");
            }
            learned += 1;
        }
    }
    if learned > 0 {
        announce(
            s,
            change(
                "extract",
                json!({ "count": learned, "source": "heuristic", "scope": "global" }),
            ),
        );
    }
    let entries = s.list_global_memory()?;
    // An index there that cannot be read links no entry (fail closed).
    let index = files.stored_index();
    let active = memory_index::active(&config, &index, &entries);
    let has_rules = active.iter().any(|e| e.kind == "rule");
    let facts = active
        .iter()
        .map(|m| m.text.clone())
        .chain(s.list_memory(project_id)?.into_iter().map(|m| m.text))
        .collect();
    Ok(TurnMemory {
        facts,
        hooks: memory_index::hook_instructions(&config, has_rules),
    })
}

fn change(kind: &str, extra: Value) -> Value {
    let mut v = json!({ "kind": kind, "at": od_store::now_ms() });
    if let (Some(map), Value::Object(more)) = (v.as_object_mut(), extra) {
        map.extend(more);
    }
    v
}

/// The config Settings shows: the one in force, and whether
/// `config.json` is there but cannot be read (a link, a FIFO, too large,
/// not a config): memory is then off until a switch writes it again.
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    #[serde(flatten)]
    pub config: MemoryConfig,
    pub unreadable: bool,
}

pub async fn get_config(State(state): State<AppState>) -> ApiJson<ConfigResponse> {
    let stored = files(&state.store).stored_config();
    ApiJson(ConfigResponse {
        config: stored.config(),
        unreadable: stored == StoredConfig::Unreadable,
    })
}

/// Applies `patch` to the config Settings shows (memory off for one that
/// cannot be read) and writes it in place of what is there.
pub async fn patch_config(
    State(state): State<AppState>,
    ApiJson(patch): ApiJson<MemoryConfigPatch>,
) -> ApiResult<ApiJson<ConfigResponse>> {
    let next = state
        .blocking(move |s| {
            let f = files(s);
            let next = f.config().apply(patch);
            f.write_config(&next)?;
            Ok(next)
        })
        .await?;
    announce(
        &state.store,
        change("config", json!({ "enabled": next.enabled })),
    );
    Ok(ApiJson(ConfigResponse {
        config: next,
        unreadable: false,
    }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexBody {
    pub index: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexResponse {
    pub index: String,
    /// Where the memory files live, for "copy path".
    pub root_dir: String,
    /// `MEMORY.md` is there but cannot be read (a link, a FIFO, too large,
    /// not UTF-8): no entry reaches a chat until a Save writes it again.
    pub unreadable: bool,
}

/// `GET /api/memory/index`, with the `ETag` of the index shown: the
/// editor saves with it as `If-Match` (`crate::cas`).
pub async fn get_index(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let res = state
        .blocking(|s| {
            let f = files(s);
            let stored = f.stored_index();
            Ok(IndexResponse {
                unreadable: stored == Stored::Unreadable,
                index: memory_index::shown(stored, &s.list_global_memory()?),
                root_dir: f.dir().display().to_string(),
            })
        })
        .await?;
    let tag = crate::cas::tag_of(res.index.as_bytes());
    Ok((crate::cas::etag(&tag), ApiJson(res)))
}

/// `PUT /api/memory/index`, only over the index its `If-Match` tags (a
/// memory learnt since links itself in), else 412; answers the `ETag` of
/// the index written, the tag of the editor's next Save.
pub async fn put_index(
    State(state): State<AppState>,
    if_match: IfMatch,
    ApiJson(body): ApiJson<IndexBody>,
) -> ApiResult<impl IntoResponse> {
    if body.index.len() > MAX_INDEX_BYTES {
        return Err(ApiErr::bad_request(format!(
            "the index is limited to {} KiB",
            MAX_INDEX_BYTES / 1024
        )));
    }
    let res = state
        .blocking(move |s| {
            let f = files(s);
            let written = f.write_index_if(&body.index, |stored| {
                let shown = memory_index::shown(stored, &s.list_global_memory()?);
                Ok::<_, StoreError>(if_match.allows(&crate::cas::tag_of(shown.as_bytes())))
            })?;
            Ok(written.then(|| IndexResponse {
                index: body.index,
                root_dir: f.dir().display().to_string(),
                unreadable: false,
            }))
        })
        .await?
        .ok_or_else(crate::cas::changed)?;
    announce(&state.store, change("index", json!({})));
    let tag = crate::cas::tag_of(res.index.as_bytes());
    Ok((crate::cas::etag(&tag), ApiJson(res)))
}

#[derive(Debug, Deserialize)]
pub struct ImportBody {
    pub text: String,
}

pub async fn import(ApiJson(body): ApiJson<ImportBody>) -> ApiResult<Json> {
    if body.text.len() > MAX_INDEX_BYTES * 4 {
        return Err(ApiErr::bad_request("the file is too large to import"));
    }
    Ok(axum::Json(
        json!({ "suggestions": memory_index::parse_import(&body.text) }),
    ))
}

type Json = axum::Json<Value>;

pub async fn events(State(state): State<AppState>) -> Response {
    let rx = hub(state.store.data_dir().root().to_path_buf()).subscribe();
    let connected = Event::default().event("connected").data("{}");
    let stream =
        futures_util::stream::unfold((Some(connected), rx), |(first, mut rx)| async move {
            if let Some(ev) = first {
                return Some((Ok::<_, Infallible>(ev), (None, rx)));
            }
            loop {
                match rx.recv().await {
                    Ok(change) => {
                        let ev = Event::default().event("change").json_data(&change).ok()?;
                        return Some((Ok(ev), (None, rx)));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
    // Ends when the daemon stops: open on every screen, it would hold the
    // shutdown for the whole drain (QA-092).
    let stream = futures_util::StreamExt::take_until(stream, state.runs.stopping());
    let mut res = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(KEEPALIVE))
        .into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    res
}
