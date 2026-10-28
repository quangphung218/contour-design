//! Settings ▸ Project locations (spec 1.5): work bases that hold project
//! folders. New projects go into a folder inside the default base; the
//! built-in base `default` is the data directory.
//!
//! - `GET /api/project-locations` → `{locations, defaultId}`
//! - `PUT /api/project-locations` `{locations: [{id?, name, path}],
//!   defaultId?}` (stored in `app-config.json` as `projectLocations` and
//!   `defaultProjectLocationId`)
//! - `POST /api/project-locations/scan` registers project folders found
//!   one level inside each base (those with `.open-design/project.json`).

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use od_contracts::{CreateProjectRequest, ProjectMetadata};
use od_store::{PROJECT_MANIFEST_PATH, Store, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const DEFAULT_ID: &str = "default";
const LOCATIONS_KEY: &str = "projectLocations";
const DEFAULT_KEY: &str = "defaultProjectLocationId";
const MAX_LOCATIONS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationsResponse {
    pub locations: Vec<Location>,
    pub default_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutLocations {
    pub locations: Vec<Location>,
    #[serde(default)]
    pub default_id: Option<String>,
}

fn custom(store: &Store) -> Vec<Location> {
    store
        .data_dir()
        .app_setting(LOCATIONS_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn default_id(store: &Store, custom: &[Location]) -> String {
    store
        .data_dir()
        .app_setting(DEFAULT_KEY)
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|id| custom.iter().any(|l| &l.id == id))
        .unwrap_or_else(|| DEFAULT_ID.to_owned())
}

fn listing(store: &Store) -> LocationsResponse {
    let custom = custom(store);
    let default_id = default_id(store, &custom);
    let mut locations = vec![Location {
        id: DEFAULT_ID.to_owned(),
        name: "Contour projects".to_owned(),
        path: store
            .data_dir()
            .projects_dir()
            .to_string_lossy()
            .into_owned(),
    }];
    locations.extend(custom);
    LocationsResponse {
        locations,
        default_id,
    }
}

/// A base must be an absolute existing folder, not hidden, not the home
/// folder or one of its parents, and apart from the data directory.
fn check_base(store: &Store, raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw.trim());
    if !path.is_absolute() {
        return Err(format!("{raw}: use an absolute path"));
    }
    let dir = path
        .canonicalize()
        .map_err(|_| format!("{raw}: folder not found"))?;
    if !dir.is_dir() {
        return Err(format!("{raw}: not a folder"));
    }
    if dir
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Err(format!("{raw}: hidden folders cannot hold projects"));
    }
    let data_root = store
        .data_dir()
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.data_dir().root().to_path_buf());
    if dir.starts_with(&data_root) || data_root.starts_with(&dir) {
        return Err(format!("{raw}: overlaps the data directory"));
    }
    if crate::routes::ui::is_home_or_parent(&dir) {
        return Err(format!("{raw}: the home folder itself cannot be a base"));
    }
    Ok(dir)
}

fn slug_id(name: &str, taken: &[Location]) -> String {
    let base: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() || base == DEFAULT_ID {
        "location".to_owned()
    } else {
        base
    };
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|l| l.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

pub async fn get(State(state): State<AppState>) -> ApiResult<ApiJson<LocationsResponse>> {
    Ok(ApiJson(state.blocking(|s| Ok(listing(s))).await?))
}

pub async fn put(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<PutLocations>,
) -> ApiResult<ApiJson<LocationsResponse>> {
    if req.locations.len() > MAX_LOCATIONS {
        return Err(ApiErr::bad_request(format!(
            "at most {MAX_LOCATIONS} locations"
        )));
    }
    let res = state
        .blocking(move |s| {
            let bad = StoreError::BadRequest;
            let mut out: Vec<Location> = Vec::new();
            for loc in req.locations.into_iter().filter(|l| l.id != DEFAULT_ID) {
                let dir = check_base(s, &loc.path).map_err(bad)?;
                let path = dir.to_string_lossy().into_owned();
                if out.iter().any(|l| l.path == path) {
                    return Err(bad(format!("{path} is listed twice")));
                }
                let name = loc.name.trim();
                let name = if name.is_empty() {
                    dir.file_name()
                        .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned())
                } else {
                    name.to_owned()
                };
                let id = if loc.id.trim().is_empty() || out.iter().any(|l| l.id == loc.id) {
                    slug_id(&name, &out)
                } else {
                    loc.id
                };
                out.push(Location { id, name, path });
            }
            let default = req
                .default_id
                .filter(|id| out.iter().any(|l| &l.id == id))
                .unwrap_or_else(|| DEFAULT_ID.to_owned());
            crate::routes::ui::patch_config(
                s,
                json!({ LOCATIONS_KEY: out, DEFAULT_KEY: default }),
            )?;
            Ok(listing(s))
        })
        .await?;
    Ok(ApiJson(res))
}

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub imported: usize,
    pub existing: usize,
}

/// Imports the unregistered project folders inside every custom base.
///
/// # Errors
/// SQLite errors; a folder that fails to import is skipped with a warning.
pub fn scan(store: &Store) -> Result<ScanResponse, StoreError> {
    let registered: Vec<String> = store
        .list_projects()?
        .into_iter()
        .filter_map(|p| p.metadata.and_then(|m| m.base_dir))
        .collect();
    let mut res = ScanResponse {
        imported: 0,
        existing: 0,
    };
    for loc in custom(store) {
        let Ok(entries) = std::fs::read_dir(&loc.path) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join(PROJECT_MANIFEST_PATH).is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            let path = dir.to_string_lossy().into_owned();
            if registered.contains(&path) {
                res.existing += 1;
                continue;
            }
            let name = std::fs::read(dir.join(PROJECT_MANIFEST_PATH))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v.get("name").and_then(Value::as_str).map(str::to_owned));
            match crate::routes::ui::import_folder(store, &dir, name) {
                Ok(_) => res.imported += 1,
                Err(e) => tracing::warn!(dir = %path, error = %e, "skipping a project folder"),
            }
        }
    }
    Ok(res)
}

pub async fn scan_route(State(state): State<AppState>) -> ApiResult<ApiJson<ScanResponse>> {
    Ok(ApiJson(state.blocking(scan).await?))
}

/// A folder name from a project name: letters, digits, `-`, `_`, spaces.
fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_owned();
    let cleaned: String = cleaned.chars().take(80).collect();
    if cleaned.is_empty() {
        "Untitled project".to_owned()
    } else {
        cleaned
    }
}

/// With a custom default base, `req` gets a fresh folder inside it as its
/// `baseDir`; with the built-in base it is left alone.
///
/// # Errors
/// I/O errors creating the folder.
pub fn place_in_default(store: &Store, req: &mut CreateProjectRequest) -> Result<(), StoreError> {
    let custom = custom(store);
    let id = default_id(store, &custom);
    let Some(loc) = custom.into_iter().find(|l| l.id == id) else {
        return Ok(());
    };
    let base = Path::new(&loc.path);
    let stem = folder_name(&req.name);
    let mut dir = base.join(&stem);
    let mut n = 2;
    while dir.exists() {
        dir = base.join(format!("{stem} {n}"));
        n += 1;
    }
    std::fs::create_dir_all(&dir)?;
    let meta = req.metadata.get_or_insert_with(ProjectMetadata::default);
    meta.base_dir = Some(dir.to_string_lossy().into_owned());
    meta.imported_from = Some("location".to_owned());
    meta.extra
        .insert("projectLocationId".to_owned(), json!(loc.id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_folder_names_are_safe() {
        let taken = vec![Location {
            id: "work".into(),
            name: "Work".into(),
            path: "/w".into(),
        }];
        assert_eq!(slug_id("Work", &taken), "work-2");
        assert_eq!(slug_id("!!", &[]), "location");
        assert_eq!(slug_id("Default", &[]), "location");
        assert_eq!(folder_name("../etc/passwd"), "---etc-passwd");
        assert_eq!(folder_name("  "), "Untitled project");
        assert_eq!(folder_name("Trà sữa: v2"), "Trà sữa- v2");
    }
}
