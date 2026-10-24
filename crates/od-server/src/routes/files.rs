use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{FromRequest, Multipart, Path, Request, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use od_contracts::{
    ContentEncoding, CreateVersionRequest, FileResponse, FilesResponse, OkResponse,
    RenameFileRequest, RestoreVersionRequest, RestoreVersionResponse, UploadResponse,
    VersionContentResponse, VersionResponse, WriteFileRequest,
};
use od_store::paths::{sanitize_name, sanitize_path};

const NO_STORE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "no-store")];

/// What a `files/<rest>` URL addresses. Matches the original route order:
/// `<file>/versions`, then `<file>/versions/<vid>[/restore]`, then a file.
#[derive(Debug, PartialEq, Eq)]
enum Target<'a> {
    File(&'a str),
    Versions(&'a str),
    Version(&'a str, &'a str),
    Restore(&'a str, &'a str),
}

fn parse_target(rest: &str) -> Target<'_> {
    if let Some(file) = rest.strip_suffix("/versions").filter(|f| !f.is_empty()) {
        return Target::Versions(file);
    }
    if let Some((file, tail)) = rest
        .rsplit_once("/versions/")
        .filter(|(f, _)| !f.is_empty())
    {
        if let Some(vid) = tail
            .strip_suffix("/restore")
            .filter(|v| !v.is_empty() && !v.contains('/'))
        {
            return Target::Restore(file, vid);
        }
        if !tail.is_empty() && !tail.contains('/') {
            return Target::Version(file, tail);
        }
    }
    Target::File(rest)
}

pub async fn list(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let files = state.blocking(move |s| s.list_files(&id)).await?;
    Ok((NO_STORE, ApiJson(FilesResponse { files })).into_response())
}

/// `GET files/<path>`: the file bytes, or version history routes.
pub async fn get(
    State(state): State<AppState>,
    Path((id, rest)): Path<(String, String)>,
) -> ApiResult<Response> {
    match parse_target(&rest) {
        Target::Versions(file) => {
            let file = file.to_owned();
            let body = state.blocking(move |s| s.list_versions(&id, &file)).await?;
            Ok((NO_STORE, ApiJson(body)).into_response())
        }
        Target::Version(file, vid) => {
            let (file, vid) = (file.to_owned(), vid.to_owned());
            let (version, content) = state
                .blocking(move |s| s.read_version(&id, &file, &vid))
                .await?;
            Ok((
                NO_STORE,
                ApiJson(VersionContentResponse { version, content }),
            )
                .into_response())
        }
        Target::Restore(..) => Err(ApiErr::new(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            od_contracts::ErrorCode::BadRequest,
            "restore requires POST",
        )),
        Target::File(file) => {
            let file = file.to_owned();
            let (meta, bytes) = state.blocking(move |s| s.read_file(&id, &file)).await?;
            Ok(([(header::CONTENT_TYPE, meta.mime)], bytes).into_response())
        }
    }
}

/// `POST files/<path>/versions` and `POST files/<path>/versions/<vid>/restore`.
pub async fn post(
    State(state): State<AppState>,
    Path((id, rest)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    fn json_or_default<T: serde::de::DeserializeOwned + Default>(body: &[u8]) -> ApiResult<T> {
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(T::default());
        }
        serde_json::from_slice(body).map_err(|e| ApiErr::bad_request(e.to_string()))
    }
    if rest == "rename" {
        let req: RenameFileRequest =
            serde_json::from_slice(&body).map_err(|e| ApiErr::bad_request(e.to_string()))?;
        let file = state
            .blocking(move |s| s.rename_file(&id, &req.from, &req.to))
            .await?;
        return Ok(ApiJson(FileResponse { file }).into_response());
    }
    match parse_target(&rest) {
        Target::Versions(file) => {
            let req: CreateVersionRequest = json_or_default(&body)?;
            let file = file.to_owned();
            let version = state
                .blocking(move |s| s.create_version(&id, &file, req))
                .await?;
            Ok(ApiJson(VersionResponse { version }).into_response())
        }
        Target::Restore(file, vid) => {
            let req: RestoreVersionRequest = json_or_default(&body)?;
            let (file, vid) = (file.to_owned(), vid.to_owned());
            let (file, version) = state
                .blocking(move |s| {
                    s.restore_version_if(
                        &id,
                        &file,
                        &vid,
                        req.prompt,
                        req.expect_version_id.as_deref(),
                    )
                })
                .await?;
            Ok(ApiJson(RestoreVersionResponse { file, version }).into_response())
        }
        _ => Err(ApiErr::not_found("no such route")),
    }
}

pub async fn delete(
    State(state): State<AppState>,
    Path((id, rest)): Path<(String, String)>,
) -> ApiResult<ApiJson<OkResponse>> {
    state.blocking(move |s| s.delete_file(&id, &rest)).await?;
    Ok(ApiJson(OkResponse { ok: true }))
}

/// `POST files`: JSON `{name, content, encoding?, overwrite?}` or multipart
/// with a `file` part and optional `name` field.
pub async fn write(
    State(state): State<AppState>,
    Path(id): Path<String>,
    req: Request,
) -> ApiResult<ApiJson<FileResponse>> {
    let is_multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("multipart/form-data"));
    if is_multipart {
        let mut multipart = Multipart::from_request(req, &state)
            .await
            .map_err(|e| ApiErr::bad_request(e.body_text()))?;
        let mut name: Option<String> = None;
        let mut upload: Option<(String, Bytes)> = None;
        while let Some(field) = next_field(&mut multipart).await? {
            match field.name() {
                Some("name") => name = Some(field.text().await.map_err(multipart_err)?),
                Some("file") => {
                    let original = field.file_name().unwrap_or("file").to_owned();
                    upload = Some((original, field.bytes().await.map_err(multipart_err)?));
                }
                _ => {}
            }
        }
        let (original, bytes) = upload.ok_or_else(|| ApiErr::bad_request("file part required"))?;
        let target = match name.filter(|n| !n.trim().is_empty()) {
            Some(n) => sanitize_path(&n)?,
            None => sanitize_name(&original),
        };
        let file = state
            .blocking(move |s| s.write_file(&id, &target, &bytes, true, None))
            .await?;
        return Ok(ApiJson(FileResponse { file }));
    }
    let ApiJson(body) = ApiJson::<WriteFileRequest>::from_request(req, &state).await?;
    let bytes = match body.encoding {
        ContentEncoding::Utf8 => body.content.into_bytes(),
        ContentEncoding::Base64 => base64::engine::general_purpose::STANDARD
            .decode(body.content.trim())
            .map_err(|e| ApiErr::bad_request(format!("invalid base64 content: {e}")))?,
    };
    let overwrite = body.overwrite.unwrap_or(true);
    let manifest = body.artifact_manifest;
    let name = body.name;
    let file = state
        .blocking(move |s| s.write_file(&id, &name, &bytes, overwrite, manifest.as_ref()))
        .await?;
    Ok(ApiJson(FileResponse { file }))
}

/// `POST upload`: multipart with an optional `dir` field and file parts.
/// Names are sanitized and never overwrite an existing file.
pub async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<ApiJson<UploadResponse>> {
    let mut dir: Option<String> = None;
    let mut parts: Vec<(String, Bytes)> = Vec::new();
    while let Some(field) = next_field(&mut multipart).await? {
        if field.name() == Some("dir") && field.file_name().is_none() {
            dir = Some(field.text().await.map_err(multipart_err)?);
            continue;
        }
        if let Some(original) = field.file_name().map(str::to_owned) {
            parts.push((original, field.bytes().await.map_err(multipart_err)?));
        }
    }
    if parts.is_empty() {
        return Err(ApiErr::bad_request("no files in upload"));
    }
    let files = state
        .blocking(move |s| {
            parts
                .iter()
                .map(|(original, bytes)| s.upload_file(&id, dir.as_deref(), original, bytes))
                .collect()
        })
        .await?;
    Ok(ApiJson(UploadResponse { files }))
}

async fn next_field(m: &mut Multipart) -> ApiResult<Option<axum::extract::multipart::Field<'_>>> {
    m.next_field().await.map_err(multipart_err)
}

fn multipart_err(e: axum::extract::multipart::MultipartError) -> ApiErr {
    ApiErr::bad_request(e.body_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parsing_follows_original_route_order() {
        assert_eq!(parse_target("index.html"), Target::File("index.html"));
        assert_eq!(
            parse_target("a/index.html/versions"),
            Target::Versions("a/index.html")
        );
        assert_eq!(
            parse_target("i.html/versions/v1"),
            Target::Version("i.html", "v1")
        );
        assert_eq!(
            parse_target("i.html/versions/v1/restore"),
            Target::Restore("i.html", "v1")
        );
        assert_eq!(
            parse_target("versions/a.png"),
            Target::File("versions/a.png")
        );
    }
}
