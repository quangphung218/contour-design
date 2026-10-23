//! Queued media generation (spec 1.5): the same request as
//! `POST /api/projects/:id/media/generate`, run in the background through
//! [`od_media::tasks::TaskQueue`].
//!
//! - `POST /api/projects/:id/media/tasks`: queue one generation (202).
//! - `GET  /api/media/tasks[?projectId=]`: tasks, oldest first.
//! - `GET  /api/media/tasks/:id[?waitMs=]`: one task; with `waitMs`, waits
//!   (at most 30 s) until it ends.
//! - `POST /api/media/tasks/:id/cancel`.
//!
//! Records live in the `media_tasks` table of the main database. The table
//! is created with `CREATE TABLE IF NOT EXISTS` instead of a numbered
//! migration, so it never collides with another track's `user_version`.

use super::Deps;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use od_media::tasks::{TaskPersist, TaskQueue, TaskRecord, Worker};
use od_media::{ConfigFile, GenerateRequest};
use od_store::Store;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const MAX_WAIT: Duration = Duration::from_secs(30);

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS media_tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  status TEXT NOT NULL,
  record_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_media_tasks_created ON media_tasks(created_at);";

/// [`TaskPersist`] over the daemon's SQLite store.
pub struct SqlitePersist {
    store: Arc<Store>,
}

impl SqlitePersist {
    /// Creates the table if needed.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn open(store: Arc<Store>) -> Result<Self, od_store::StoreError> {
        store.with_conn(|c| Ok(c.execute_batch(SCHEMA)?))?;
        Ok(Self { store })
    }
}

impl TaskPersist for SqlitePersist {
    fn save(&self, r: &TaskRecord) {
        let Ok(json) = serde_json::to_string(r) else {
            return;
        };
        let res = self.store.with_conn(|c| {
            c.execute(
                "INSERT INTO media_tasks (id, project_id, status, record_json, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(id) DO UPDATE SET status = ?3, record_json = ?4, updated_at = ?6",
                rusqlite_params(r, &json),
            )?;
            Ok(())
        });
        if let Err(e) = res {
            tracing::warn!(task = %r.id, error = %e, "could not persist a media task");
        }
    }

    fn load_all(&self) -> Vec<TaskRecord> {
        self.store
            .with_conn(|c| {
                let mut stmt =
                    c.prepare("SELECT record_json FROM media_tasks ORDER BY created_at, id")?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                Ok(rows
                    .filter_map(Result::ok)
                    .filter_map(|j| serde_json::from_str(&j).ok())
                    .collect())
            })
            .unwrap_or_default()
    }
}

fn rusqlite_params<'a>(
    r: &'a TaskRecord,
    json: &'a str,
) -> (&'a str, &'a str, &'static str, &'a str, i64, i64) {
    (
        &r.id,
        &r.project_id,
        r.status.as_str(),
        json,
        r.created_at,
        od_store::now_ms(),
    )
}

fn parse(body: &Value) -> ApiResult<GenerateRequest> {
    serde_json::from_value(body.clone()).map_err(|e| {
        ApiErr::bad_request(format!(
            "surface must be image, video or audio and model is required: {e}"
        ))
    })
}

/// Generates and writes the file; the result is what `media/generate`
/// returns.
fn worker(store: Arc<Store>, deps: &Deps) -> Worker {
    let env = Arc::clone(&deps.env);
    let limits = deps.limits;
    Arc::new(move |task: TaskRecord| {
        let store = Arc::clone(&store);
        let env = Arc::clone(&env);
        Box::pin(async move {
            let req: GenerateRequest =
                serde_json::from_value(task.request.clone()).map_err(|e| e.to_string())?;
            let config = ConfigFile::in_dir(store.data_dir().root());
            let http = od_media::provider::http_client().map_err(|e| e.to_string())?;
            let generated = od_media::generate(&http, &config, env.as_ref(), &req, limits)
                .await
                .map_err(|e| e.to_string())?;
            let (name, bytes) = (generated.name.clone(), generated.bytes);
            let project = task.project_id.clone();
            let file = tokio::task::spawn_blocking(move || {
                store.write_file(&project, &name, &bytes, true, None)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            tracing::info!(task = %task.id, file = %file.name, "media task done");
            Ok(json!({
                "file": file,
                "name": file.name,
                "model": generated.model,
                "surface": generated.surface,
                "provider": generated.provider,
                "providerNote": generated.note,
            }))
        })
    })
}

/// The queue over the store; persisted tasks are restored when a runtime
/// is running (always, in the daemon).
pub fn open_queue(state: &AppState, deps: &Deps) -> TaskQueue {
    let persist: Arc<dyn TaskPersist> = match SqlitePersist::open(Arc::clone(&state.store)) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            tracing::warn!(error = %e, "media tasks will not survive a restart");
            Arc::new(od_media::tasks::MemoryPersist::default())
        }
    };
    let queue = TaskQueue::new(
        deps.media_concurrency,
        persist,
        worker(Arc::clone(&state.store), deps),
    );
    if tokio::runtime::Handle::try_current().is_ok() {
        let (requeued, interrupted) = queue.restore();
        if requeued + interrupted > 0 {
            tracing::info!(requeued, interrupted, "restored media tasks");
        }
    }
    queue
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/media/tasks", post(submit))
        .route("/api/media/tasks", get(list))
        .route("/api/media/tasks/{id}", get(get_one))
        .route("/api/media/tasks/{id}/cancel", post(cancel))
}

fn not_found(id: &str) -> ApiErr {
    ApiErr::not_found(format!("unknown media task {id:?}"))
}

pub async fn submit(
    State(state): State<AppState>,
    Extension(queue): Extension<TaskQueue>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<impl IntoResponse> {
    let req = parse(&body)?;
    if od_media::find_model(req.model.trim(), req.surface).is_none() {
        return Err(ApiErr::bad_request(format!(
            "unknown media model {:?}",
            req.model
        )));
    }
    let pid = id.clone();
    state.blocking(move |s| s.get_project(&pid)).await?;
    let task = queue.submit(&id, body);
    Ok((StatusCode::ACCEPTED, Json(json!({ "task": task }))))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default)]
    project_id: Option<String>,
}

pub async fn list(
    Extension(queue): Extension<TaskQueue>,
    Query(q): Query<ListQuery>,
) -> Json<Value> {
    Json(json!({ "tasks": queue.list(q.project_id.as_deref()) }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetQuery {
    #[serde(default)]
    wait_ms: Option<u64>,
}

pub async fn get_one(
    Extension(queue): Extension<TaskQueue>,
    Path(id): Path<String>,
    Query(q): Query<GetQuery>,
) -> ApiResult<Json<Value>> {
    let task = match q.wait_ms {
        Some(ms) => {
            queue
                .wait(&id, Duration::from_millis(ms).min(MAX_WAIT))
                .await
        }
        None => queue.get(&id),
    }
    .ok_or_else(|| not_found(&id))?;
    Ok(Json(json!({ "task": task })))
}

pub async fn cancel(
    Extension(queue): Extension<TaskQueue>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let task = queue.cancel(&id).ok_or_else(|| not_found(&id))?;
    Ok(Json(json!({ "task": task })))
}
