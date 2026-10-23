//! `POST /api/import/claude-design`: a multipart `file` (a Claude Design
//! `.zip` export) becomes a new project, like the original's importer.
//!
//! Entries go through the store's path validation (no `..`, absolute or
//! reserved paths); stored and deflate entries only, no encryption, at most
//! [`MAX_FILES`] files, [`MAX_FILE_BYTES`] each and [`MAX_TOTAL_BYTES`] in
//! all. The entry file is `index.html` at the root, else the first
//! root-level HTML file, else the first HTML file anywhere; an archive
//! without HTML is refused. A failed import leaves no project behind.

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Multipart, State};
use od_contracts::{CreateProjectRequest, Project, ProjectKind, ProjectMetadata};
use od_store::{Store, StoreError};
use serde::Serialize;
use serde_json::json;
use std::io::{Cursor, Read};

pub const MAX_FILES: usize = 5000;
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
const CONVERSATION_TITLE: &str = "Imported Claude Design project";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportZipResponse {
    pub project: Project,
    pub conversation_id: String,
    pub entry_file: String,
    pub files: Vec<String>,
}

fn is_html(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm")
}

/// The page a project opens on (see the module docs).
#[must_use]
pub fn entry_file(paths: &[String]) -> Option<String> {
    let root = |p: &&String| !p.contains('/');
    paths
        .iter()
        .find(|p| root(p) && p.eq_ignore_ascii_case("index.html"))
        .or_else(|| paths.iter().filter(root).find(|p| is_html(p)))
        .or_else(|| paths.iter().find(|p| is_html(p)))
        .cloned()
}

/// Reads every file entry, enforcing the limits.
///
/// # Errors
/// A readable message for a corrupt, encrypted, oversized or unsafe archive.
pub fn read_entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip archive: {e}"))?;
    let mut out = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("unreadable entry: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .map_err(|e| format!("unreadable entry name: {e}"))?
            .replace('\\', "/");
        if entry.encrypted() {
            return Err("encrypted archives are not supported".to_owned());
        }
        if !matches!(
            entry.compression(),
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
        ) {
            return Err(format!("{name}: unsupported compression"));
        }
        if name.starts_with('/') || name.contains('\0') || name.get(1..2) == Some(":") {
            return Err(format!("{name}: unsafe path"));
        }
        if out.len() >= MAX_FILES {
            return Err(format!("more than {MAX_FILES} files"));
        }
        if entry.size() > MAX_FILE_BYTES {
            return Err(format!("{name} is larger than 25 MB"));
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("{name}: {e}"))?;
        let len = data.len() as u64;
        if len > MAX_FILE_BYTES {
            return Err(format!("{name} is larger than 25 MB"));
        }
        if entry.size() != 0 && entry.size() != len {
            return Err(format!("{name}: size does not match the archive"));
        }
        total += len;
        if total > MAX_TOTAL_BYTES {
            return Err("the archive is larger than 100 MB".to_owned());
        }
        out.push((name, data));
    }
    Ok(out)
}

/// Creates the project and writes the files; deletes it again on failure.
///
/// # Errors
/// [`StoreError::BadRequest`] for an unusable archive or unsafe path.
pub fn import(
    store: &Store,
    file_name: &str,
    bytes: &[u8],
) -> Result<ImportZipResponse, StoreError> {
    let bad = |m: String| StoreError::BadRequest(m);
    if !file_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(bad("the file must be a .zip".to_owned()));
    }
    let entries = read_entries(bytes).map_err(bad)?;
    let paths: Vec<String> = entries.iter().map(|(p, _)| p.clone()).collect();
    let entry =
        entry_file(&paths).ok_or_else(|| bad("the archive has no HTML file to open".to_owned()))?;
    let stem = file_name[..file_name.len() - ".zip".len()].trim();
    let name = if stem.is_empty() {
        "Claude Design import"
    } else {
        stem
    };
    let mut extra = serde_json::Map::new();
    extra.insert("sourceFileName".to_owned(), json!(file_name));
    // No pending prompt: the Studio would send it as a first turn.
    let (project, conversation_id) = store.create_project(CreateProjectRequest {
        name: name.to_owned(),
        metadata: Some(ProjectMetadata {
            kind: Some(ProjectKind::Prototype),
            imported_from: Some("claude-design".to_owned()),
            entry_file: Some(entry.clone()),
            extra,
            ..ProjectMetadata::default()
        }),
        ..CreateProjectRequest::default()
    })?;
    let written = (|| {
        let mut files = Vec::with_capacity(entries.len());
        for (path, data) in &entries {
            files.push(store.write_file(&project.id, path, data, true, None)?.name);
        }
        store.update_conversation(
            &project.id,
            &conversation_id,
            od_contracts::UpdateConversationRequest {
                title: Some(CONVERSATION_TITLE.to_owned()),
                session_mode: None,
            },
        )?;
        Ok::<_, StoreError>(files)
    })();
    match written {
        Ok(files) => Ok(ImportZipResponse {
            project,
            conversation_id,
            entry_file: entry,
            files,
        }),
        Err(e) => {
            if let Err(cleanup) = store.delete_project(&project.id) {
                tracing::warn!(error = %cleanup, "removing a failed ZIP import");
            }
            Err(e)
        }
    }
}

pub async fn import_route(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<ApiJson<ImportZipResponse>> {
    let mut upload: Option<(String, Bytes)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiErr::bad_request(e.body_text()))?
    {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("import.zip").to_owned();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiErr::bad_request(e.body_text()))?;
            upload = Some((name, bytes));
        }
    }
    let (name, bytes) = upload.ok_or_else(|| ApiErr::bad_request("no file in the upload"))?;
    let res = state.blocking(move |s| import(s, &name, &bytes)).await?;
    tracing::info!(project = %res.project.id, files = res.files.len(), "imported a Claude Design zip");
    Ok(ApiJson(res))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_prefers_root_index_then_root_html_then_any() {
        let p = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            entry_file(&p(&["a/index.html", "Index.HTML", "b.html"])).as_deref(),
            Some("Index.HTML")
        );
        assert_eq!(
            entry_file(&p(&["a/index.html", "style.css", "page.htm"])).as_deref(),
            Some("page.htm")
        );
        assert_eq!(
            entry_file(&p(&["x.css", "deep/a.html"])).as_deref(),
            Some("deep/a.html")
        );
        assert_eq!(entry_file(&p(&["x.css"])), None);
    }
}
