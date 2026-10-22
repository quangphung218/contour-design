//! Manual refresh (spec 1.5, 4.2): re-reads the artifact's source, maps
//! the output into `data.json`, re-renders `index.html`, and appends one
//! summary line to `refreshes.jsonl`. Sources are local and read-only:
//! `local_file` / `project_files.read_json` and `project_files.search`.
//! Connector and network sources are not supported (no outbound network).
//!
//! One refresh per artifact at a time: `refresh.lock` is created
//! exclusively; a lock older than [`STALE_LOCK`] is taken over.

use crate::model::{
    self, DataPath, LiveArtifact, RefreshPermission, RefreshStatus, Source, SourceType, Transform,
    now_iso,
};
use crate::store::{LOCK_FILE, LiveStore};
use crate::{LiveError, Result};
use od_store::files::ProjectRoot;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant, SystemTime};

/// A lock left behind longer than the original's total timeout is stale.
pub const STALE_LOCK: Duration = Duration::from_secs(120);
/// `project_files.read_json` reads at most 256 KiB.
pub const MAX_JSON_FILE: u64 = 256 * 1024;
/// `project_files.search` reads text files up to 128 KiB.
const MAX_SEARCH_FILE: u64 = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSummary {
    pub id: String,
    pub status: RefreshStatus,
    pub refreshed_source_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RefreshOutcome {
    pub artifact: LiveArtifact,
    pub refresh: RefreshSummary,
}

/// Exclusive `refresh.lock`, removed on drop.
struct Lock(std::path::PathBuf);

impl Lock {
    fn acquire(path: std::path::PathBuf, refresh_id: &str) -> Result<Self> {
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = writeln!(
                        f,
                        "{}",
                        json!({ "refreshId": refresh_id, "startedAt": now_iso() })
                    );
                    return Ok(Self(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| SystemTime::now().duration_since(t).ok())
                        .is_some_and(|age| age > STALE_LOCK);
                    if !stale {
                        return Err(LiveError::RefreshInProgress);
                    }
                    let _ = std::fs::remove_file(&path);
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(LiveError::RefreshInProgress)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn source_failure(message: impl Into<String>) -> LiveError {
    LiveError::Source(message.into())
}

/// `local_file` and `daemon_tool` sources.
///
/// # Errors
/// [`LiveError::Source`] for unsupported tools, unreadable or oversized
/// files, invalid JSON.
pub fn execute_source(root: &ProjectRoot, source: &Source) -> Result<Map<String, Value>> {
    let tool = source.tool_name.as_deref();
    match (source.kind, tool) {
        (SourceType::LocalFile, None | Some("project_files.read_json"))
        | (SourceType::DaemonTool, Some("project_files.read_json")) => {
            read_json(root, &source.input)
        }
        (SourceType::DaemonTool, Some("project_files.search")) => search(root, &source.input),
        (SourceType::LocalFile, Some(other)) => Err(source_failure(format!(
            "unsupported local_file refresh tool: {other}"
        ))),
        (SourceType::DaemonTool, other) => Err(source_failure(format!(
            "unsupported local daemon refresh tool: {}",
            other.unwrap_or("<missing>")
        ))),
        (SourceType::ConnectorTool, _) => Err(source_failure(
            "connector refresh sources are not supported in this local build",
        )),
    }
}

fn input_str<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn read_json(root: &ProjectRoot, input: &Map<String, Value>) -> Result<Map<String, Value>> {
    let path = ["path", "file", "name"]
        .iter()
        .find_map(|k| input_str(input, k))
        .ok_or_else(|| source_failure("project_files.read_json requires input.path"))?;
    if !path.to_ascii_lowercase().ends_with(".json") {
        return Err(source_failure(
            "project_files.read_json only supports .json files",
        ));
    }
    let (file, _) = od_store::files::stat_file(root, path)
        .map_err(|e| source_failure(format!("project_files.read_json {path}: {e}")))?;
    if file.size > MAX_JSON_FILE {
        return Err(source_failure("project_files.read_json file exceeds 256KB"));
    }
    let (_, bytes) = od_store::files::read_file(root, path)
        .map_err(|e| source_failure(format!("project_files.read_json {path}: {e}")))?;
    let json: Value = serde_json::from_slice(&bytes).map_err(|_| {
        source_failure(format!(
            "project_files.read_json could not parse JSON at {path}"
        ))
    })?;
    let mut out = Map::new();
    out.insert("toolName".to_owned(), json!("project_files.read_json"));
    out.insert("path".to_owned(), json!(file.name));
    out.insert("size".to_owned(), json!(file.size));
    out.insert("json".to_owned(), json);
    Ok(out)
}

fn compact_preview(text: &str, query: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = normalized.chars().collect();
    if chars.len() <= 240 {
        return normalized;
    }
    let lower = normalized.to_lowercase();
    let start = lower
        .find(&query.to_lowercase())
        .map_or(0, |byte| lower[..byte].chars().count().saturating_sub(80));
    let slice: String = chars.iter().skip(start).take(240).collect();
    format!("{}{slice}…", if start > 0 { "…" } else { "" })
}

fn search(root: &ProjectRoot, input: &Map<String, Value>) -> Result<Map<String, Value>> {
    let query = input_str(input, "query");
    let max = input
        .get("maxResults")
        .and_then(Value::as_u64)
        .map_or(25, |n| n.clamp(1, 100));
    let files = od_store::files::list_files(root)
        .map_err(|e| source_failure(format!("project_files.search: {e}")))?;
    let needle = query.map(str::to_lowercase);
    let mut matches = Vec::new();
    for file in files {
        if matches.len() as u64 >= max {
            break;
        }
        let mut preview = None;
        let matched = match &needle {
            None => true,
            Some(q) if file.name.to_lowercase().contains(q.as_str()) => true,
            Some(q) => {
                let texty = file.mime.starts_with("text/")
                    || file.name.ends_with(".json")
                    || matches!(
                        file.kind,
                        od_contracts::FileKind::Code
                            | od_contracts::FileKind::Text
                            | od_contracts::FileKind::Html
                    );
                texty
                    && file.size <= MAX_SEARCH_FILE
                    && od_store::files::read_file(root, &file.name).is_ok_and(|(_, b)| {
                        let text = String::from_utf8_lossy(&b);
                        let hit = text.to_lowercase().contains(q.as_str());
                        if hit {
                            preview = Some(compact_preview(&text, q));
                        }
                        hit
                    })
            }
        };
        if matched {
            let mut m = json!({ "path": file.name, "size": file.size, "mtime": file.mtime, "kind": file.kind });
            if let Some(p) = preview {
                m["preview"] = json!(p);
            }
            matches.push(m);
        }
    }
    let mut out = Map::new();
    out.insert("toolName".to_owned(), json!("project_files.search"));
    if let Some(q) = query {
        out.insert("query".to_owned(), json!(q));
    }
    out.insert("matches".to_owned(), Value::Array(matches));
    Ok(out)
}

fn read_mapped<'a>(root: &'a Map<String, Value>, segments: &[&str]) -> Option<&'a Value> {
    let (first, rest) = segments.split_first()?;
    let mut current = root.get(*first)?;
    for segment in rest {
        current = match current {
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            Value::Object(map) => map.get(*segment)?,
            _ => return None,
        };
    }
    Some(current)
}

fn is_index(s: &str) -> bool {
    s == "0" || (!s.starts_with('0') && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

fn write_mapped(root: &mut Map<String, Value>, segments: &[&str], value: Value) -> Result<()> {
    fn container(next: &str) -> Value {
        if is_index(next) { json!([]) } else { json!({}) }
    }
    fn step<'a>(slot: &'a mut Value, segment: &str, next: Option<&str>) -> Result<&'a mut Value> {
        if let Value::Array(items) = slot {
            let i: usize = segment.parse().map_err(|_| {
                LiveError::Validation(
                    "outputMapping.dataPaths.to array segments must be non-negative integers"
                        .to_owned(),
                )
            })?;
            if i >= model::MAX_ARRAY_LEN {
                return Err(LiveError::Validation(
                    "array index out of bounds".to_owned(),
                ));
            }
            if items.len() <= i {
                items.resize(i + 1, Value::Null);
            }
            let item = &mut items[i];
            if let Some(n) = next {
                if !item.is_object() && !item.is_array() {
                    *item = container(n);
                }
            }
            return Ok(item);
        }
        if !slot.is_object() {
            *slot = json!({});
        }
        let map = slot.as_object_mut().expect("object");
        let item = map.entry(segment.to_owned()).or_insert(Value::Null);
        if let Some(n) = next {
            if !item.is_object() && !item.is_array() {
                *item = container(n);
            }
        }
        Ok(item)
    }
    let mut holder = Value::Object(std::mem::take(root));
    let result = (|| {
        let mut slot = &mut holder;
        for (i, segment) in segments.iter().enumerate() {
            slot = step(slot, segment, segments.get(i + 1).copied())?;
        }
        *slot = value;
        Ok(())
    })();
    if let Value::Object(map) = holder {
        *root = map;
    }
    result
}

/// Selects `dataPaths` from the output (all of it when there are none).
///
/// # Errors
/// Invalid paths or transforms other than `identity`.
pub fn apply_mapping(source: &Source, output: Map<String, Value>) -> Result<Map<String, Value>> {
    let mapping = source.output_mapping.clone().unwrap_or_default();
    let paths: &[DataPath] = mapping.data_paths.as_deref().unwrap_or_default();
    let selected = if paths.is_empty() {
        output
    } else {
        let mut mapped = Map::new();
        for p in paths {
            let from = model::mapping_segments(&p.from, "outputMapping.dataPaths.from")?;
            let to = model::mapping_segments(&p.to, "outputMapping.dataPaths.to")?;
            if let Some(v) = read_mapped(&output, &from) {
                write_mapped(&mut mapped, &to, v.clone())?;
            }
        }
        mapped
    };
    match mapping.transform.unwrap_or_default() {
        Transform::Identity => Ok(selected),
        other => Err(LiveError::Source(format!(
            "output transform {other:?} is not supported yet; use identity with dataPaths"
        ))),
    }
}

/// Objects merge key by key; anything else replaces.
pub fn deep_merge(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (k, v) in source {
        match (target.get_mut(&k), v) {
            (Some(Value::Object(t)), Value::Object(s)) => deep_merge(t, s),
            (_, v) => {
                target.insert(k, v);
            }
        }
    }
}

impl LiveStore {
    /// Refreshes `id` from its source. Always appends one line to
    /// `refreshes.jsonl` (succeeded or failed) once the lock is held.
    ///
    /// # Errors
    /// [`LiveError::RefreshInProgress`], [`LiveError::NotRefreshable`],
    /// source and validation errors (the artifact is marked `failed`).
    pub fn refresh(&self, root: &ProjectRoot, id: &str) -> Result<RefreshOutcome> {
        let mut artifact = self.get(id)?;
        let refresh_id = format!("lr-{}", uuid::Uuid::new_v4().simple());
        let _lock = Lock::acquire(self.paths(id)?.file(LOCK_FILE), &refresh_id)?;
        let started_at = now_iso();
        let clock = Instant::now();
        artifact.refresh_status = RefreshStatus::Running;
        self.save_record(&artifact)?;

        let source = artifact.document.source_json.clone();
        let attempt = (|| -> Result<(Map<String, Value>, Source)> {
            let source = source.ok_or(LiveError::NotRefreshable(
                "this live artifact has no refresh source",
            ))?;
            if source.refresh_permission != RefreshPermission::ManualRefreshGrantedForReadOnly {
                return Err(LiveError::NotRefreshable(
                    "refresh is disabled for this artifact source",
                ));
            }
            let output = execute_source(root, &source)?;
            let mapped = apply_mapping(&source, output)?;
            let mut data = artifact.document.data_json.clone();
            deep_merge(&mut data, mapped);
            model::validate_bounded(&Value::Object(data.clone()), "dataJson")?;
            Ok((data, source))
        })();

        let finished_at = now_iso();
        let duration_ms = u64::try_from(clock.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut entry = json!({
            "schemaVersion": model::SCHEMA_VERSION,
            "projectId": artifact.project_id,
            "artifactId": artifact.id,
            "refreshId": refresh_id,
            "sequence": 0,
            "step": "refresh",
            "startedAt": started_at,
            "finishedAt": finished_at,
            "durationMs": duration_ms,
            "createdAt": finished_at,
        });
        if let Some(s) = &artifact.document.source_json {
            entry["source"] = json!({ "sourceType": "document", "toolName": s.tool_name });
        }
        match attempt {
            Ok((data, _source)) => {
                artifact.document.data_json = data;
                let written = self
                    .save_data(id, &artifact.document)
                    .and_then(|()| self.update_provenance(id, &finished_at));
                if let Err(e) = written {
                    return self.fail(artifact, entry, e);
                }
                artifact.refresh_status = RefreshStatus::Succeeded;
                artifact.last_refreshed_at = Some(finished_at.clone());
                artifact.updated_at = finished_at;
                self.save_record(&artifact)?;
                entry["status"] = json!("succeeded");
                entry["metadata"] = json!({ "refreshedSourceCount": 1 });
                self.append_refresh(id, &entry)?;
                Ok(RefreshOutcome {
                    artifact,
                    refresh: RefreshSummary {
                        id: refresh_id,
                        status: RefreshStatus::Succeeded,
                        refreshed_source_count: 1,
                    },
                })
            }
            Err(e) => self.fail(artifact, entry, e),
        }
    }

    fn fail(
        &self,
        mut artifact: LiveArtifact,
        mut entry: Value,
        e: LiveError,
    ) -> Result<RefreshOutcome> {
        artifact.refresh_status = RefreshStatus::Failed;
        let _ = self.save_record(&artifact);
        entry["status"] = json!("failed");
        entry["error"] = json!({ "code": e.code(), "message": e.to_string() });
        let _ = self.append_refresh(&artifact.id, &entry);
        Err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_writes_nested_paths() {
        let mut m = Map::new();
        write_mapped(&mut m, &["a", "b", "0", "c"], json!(1)).unwrap();
        assert_eq!(
            Value::Object(m.clone()),
            json!({ "a": { "b": [{ "c": 1 }] } })
        );
        let out = json!({ "json": { "rows": [{ "v": 3 }] } });
        assert_eq!(
            read_mapped(out.as_object().unwrap(), &["json", "rows", "0", "v"]),
            Some(&json!(3))
        );
        let mut target = json!({ "a": { "x": 1, "y": 2 }, "b": 1 })
            .as_object()
            .unwrap()
            .clone();
        deep_merge(
            &mut target,
            json!({ "a": { "y": 9 }, "b": [1] })
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(
            Value::Object(target),
            json!({ "a": { "x": 1, "y": 9 }, "b": [1] })
        );
    }
}
