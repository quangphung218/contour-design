//! Export routes (spec 1.3, 2.2, 4.2) and the active-project channel used by
//! `od mcp` (spec 2.5). Mounted with one `merge` in [`crate::router`].
//!
//! - `GET  /api/projects/:id/archive[?root=dir]`: streamed ZIP.
//! - `POST /api/projects/:id/export/html`: one-file HTML (413 over limits).
//! - `POST /api/projects/:id/export/{pdf,pdf-image,image,pptx}`: through the
//!   [`Renderer`] (Chrome via `od-render-cdp` by default).
//! - `POST/GET /api/active`.

pub mod active;

use crate::error::ApiErr;
use crate::state::AppState;
use active::ActiveContext;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use od_contracts::{ErrorCode, Project};
use od_export::archive::{content_disposition, sanitize_filename, write_zip};
use od_export::standalone::{self, MAX_ENTRY_BYTES};
use od_export::{
    AssetMeta, AssetSource, CaptureOptions, DeckMode, ImageFormat, Options, RenderError,
    RenderTarget, Renderer, StandaloneError,
};
use od_store::StoreError;
use od_store::files::ProjectRoot;
use od_store::paths::{is_html_name, validate_project_path};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Arc;

/// Routes with the Chrome renderer.
pub fn routes() -> Router<AppState> {
    routes_with(Arc::new(od_render_cdp::CdpRenderer::new()))
}

/// Routes with any renderer (tests use a fake).
pub fn routes_with(renderer: Arc<dyn Renderer>) -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/archive", get(archive))
        .route("/api/projects/{id}/export/html", post(export_html))
        .route("/api/projects/{id}/export/pdf", post(export_pdf))
        .route(
            "/api/projects/{id}/export/pdf-image",
            post(export_pdf_image),
        )
        .route("/api/projects/{id}/export/image", post(export_image))
        .route("/api/projects/{id}/export/pptx", post(export_pptx))
        .route("/api/active", get(active::get).post(active::post))
        .layer(Extension(renderer))
        .layer(Extension(Arc::new(ActiveContext::default())))
}

// ---------------------------------------------------------------------------
// Errors

/// An API error with optional `details` (missing dependency, limit hit).
#[derive(Debug)]
pub struct ExportErr {
    status: StatusCode,
    code: ErrorCode,
    message: String,
    details: Option<Value>,
}

impl ExportErr {
    pub(crate) fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl From<ApiErr> for ExportErr {
    fn from(e: ApiErr) -> Self {
        Self::new(e.status, e.code, e.message)
    }
}

impl From<StoreError> for ExportErr {
    fn from(e: StoreError) -> Self {
        ApiErr::from(e).into()
    }
}

impl From<StandaloneError> for ExportErr {
    fn from(e: StandaloneError) -> Self {
        let mut details = json!({ "kind": e.kind() });
        let (status, code) = match &e {
            StandaloneError::LimitExceeded { limit, .. } => {
                details["limit"] = json!(limit);
                (StatusCode::PAYLOAD_TOO_LARGE, ErrorCode::PayloadTooLarge)
            }
            StandaloneError::MissingDependency { dependency, chain } => {
                details["dependency"] = json!(dependency);
                details["chain"] = json!(chain);
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::ValidationFailed,
                )
            }
            StandaloneError::InvalidSource { .. } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::ValidationFailed,
            ),
            StandaloneError::PathOutsideProject { dependency } => {
                details["dependency"] = json!(dependency);
                (StatusCode::BAD_REQUEST, ErrorCode::BadRequest)
            }
            StandaloneError::DependencyCycle { chain } => {
                details["chain"] = json!(chain);
                (StatusCode::BAD_REQUEST, ErrorCode::BadRequest)
            }
            StandaloneError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
        };
        Self::new(status, code, e.to_string()).with_details(details)
    }
}

impl From<RenderError> for ExportErr {
    fn from(e: RenderError) -> Self {
        let (status, code) = match &e {
            RenderError::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::UpstreamUnavailable,
            ),
            RenderError::NotADeck => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::BadRequest),
            RenderError::Unsupported(_) => {
                (StatusCode::NOT_IMPLEMENTED, ErrorCode::UpstreamUnavailable)
            }
            RenderError::Failed(_) => (StatusCode::BAD_GATEWAY, ErrorCode::UpstreamUnavailable),
        };
        Self::new(status, code, e.to_string())
    }
}

impl IntoResponse for ExportErr {
    fn into_response(self) -> Response {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(details) = self.details {
            error["details"] = details;
        }
        (self.status, axum::Json(json!({ "error": error }))).into_response()
    }
}

pub(crate) type ExportResult<T> = Result<T, ExportErr>;

pub(crate) fn internal(e: impl std::fmt::Display) -> ExportErr {
    ExportErr::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorCode::InternalError,
        e.to_string(),
    )
}

// ---------------------------------------------------------------------------
// Shared pieces

/// Reads project files for the standalone bundler.
struct StoreSource {
    root: ProjectRoot,
}

impl AssetSource for StoreSource {
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>> {
        match od_store::files::stat_file(&self.root, path) {
            Ok((file, _)) => Ok(Some(AssetMeta {
                mime: file.mime,
                size: file.size,
            })),
            Err(StoreError::FileNotFound) => Ok(None),
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                e.to_string(),
            )),
        }
    }

    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        od_store::files::read_file(&self.root, path)
            .map(|(_, bytes)| bytes)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

/// Body shared by every `export/*` route (original `ExportRequest` fields).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    #[serde(default)]
    pub(crate) file_name: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    /// `true` deck, `false` page, absent: detect slides.
    #[serde(default)]
    deck: Option<bool>,
    /// Image export: one slide instead of the stitched deck.
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    image_format: Option<String>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
}

impl ExportRequest {
    fn mode(&self) -> DeckMode {
        match self.deck {
            Some(true) => DeckMode::Deck,
            Some(false) => DeckMode::Page,
            None => DeckMode::Auto,
        }
    }
}

pub(crate) fn parse_body(body: &Bytes) -> ExportResult<ExportRequest> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(ExportRequest::default());
    }
    serde_json::from_slice(body).map_err(|e| {
        ExportErr::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            e.to_string(),
        )
    })
}

/// The project, its root, and the validated HTML entry path.
pub(crate) struct Entry {
    pub(crate) project: Project,
    pub(crate) root: ProjectRoot,
    pub(crate) path: String,
    size: u64,
}

impl Entry {
    /// Download name: the title, else the entry's file stem.
    pub(crate) fn filename(&self, title: Option<&str>, ext: &str) -> String {
        let base = title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let name = self.path.rsplit('/').next().unwrap_or(&self.path);
                name.rsplit_once('.')
                    .map_or(name, |(stem, _)| stem)
                    .to_owned()
            });
        let base = sanitize_filename(&base);
        format!("{}.{ext}", if base.is_empty() { "artifact" } else { &base })
    }
}

pub(crate) async fn resolve_entry(
    state: &AppState,
    id: String,
    req: &ExportRequest,
) -> ExportResult<Entry> {
    let file_name = req
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_owned);
    let entry = state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            let file_name = match file_name {
                Some(f) => f,
                None => project
                    .metadata
                    .as_ref()
                    .and_then(|m| m.entry_file.clone())
                    .ok_or_else(|| StoreError::BadRequest("fileName required".to_owned()))?,
            };
            let root = s.project_root(&project)?;
            let (file, _) = od_store::files::stat_file(&root, &file_name)?;
            Ok(Entry {
                project,
                root,
                path: file.name,
                size: file.size,
            })
        })
        .await?;
    if !is_html_name(&entry.path) {
        return Err(ExportErr::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ErrorCode::UnsupportedMediaType,
            "export only supports HTML entry files",
        ));
    }
    if entry.size > MAX_ENTRY_BYTES {
        return Err(StandaloneError::LimitExceeded {
            limit: "entryBytes",
            actual: entry.size,
            max: MAX_ENTRY_BYTES,
        }
        .into());
    }
    Ok(entry)
}

async fn bundle(entry: &Entry, lenient: bool) -> ExportResult<od_export::StandaloneHtml> {
    let source = StoreSource {
        root: entry.root.clone(),
    };
    let path = entry.path.clone();
    tokio::task::spawn_blocking(move || {
        let (_, bytes) = od_store::files::read_file(&source.root, &path)?;
        let html = String::from_utf8(bytes).map_err(|_| StandaloneError::InvalidSource {
            path: path.clone(),
            message: "not UTF-8 text".to_owned(),
        })?;
        let options = Options {
            lenient,
            ..Options::default()
        };
        Ok::<_, ExportErr>(standalone::bundle(&path, &html, &source, &options)?)
    })
    .await
    .map_err(internal)?
}

/// A bundled copy of the entry in a scratch folder under the data dir,
/// removed when dropped.
pub(crate) struct Scratch {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) target: RenderTarget,
}

pub(crate) async fn scratch(
    state: &AppState,
    entry: &Entry,
    req: &ExportRequest,
) -> ExportResult<Scratch> {
    let bundled = bundle(entry, true).await?;
    let base = state.store.data_dir().root().join("export-render");
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&base)?;
        let dir = tempfile::Builder::new()
            .prefix("render-")
            .tempdir_in(&base)?;
        let path = dir.path().join("index.html");
        std::fs::write(&path, bundled.html)?;
        Ok::<_, std::io::Error>((dir, path))
    })
    .await
    .map_err(internal)?
    .map_err(internal)
    .map(|(dir, path)| {
        let mut target = RenderTarget::new(path);
        if let Some(w) = req.width.filter(|w| (1..=7_680).contains(w)) {
            target.width = w;
        }
        if let Some(h) = req.height.filter(|h| (1..=7_680).contains(h)) {
            target.height = h;
        }
        Scratch { dir, target }
    })
}

pub(crate) fn attachment(mime: &str, filename: &str, body: impl Into<Body>) -> Response {
    let mut res = Response::new(body.into());
    let headers = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(mime) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&content_disposition(filename)) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

// ---------------------------------------------------------------------------
// Handlers

#[derive(Debug, Deserialize)]
pub struct ArchiveQuery {
    #[serde(default)]
    root: Option<String>,
}

/// Forwards ZIP bytes to the response body in 64 KiB chunks.
struct ChannelWriter {
    tx: tokio::sync::mpsc::Sender<std::io::Result<Bytes>>,
    buf: Vec<u8>,
}

const CHUNK: usize = 64 * 1024;

impl ChannelWriter {
    fn send(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = Bytes::from(std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK)));
        self.tx
            .blocking_send(Ok(chunk))
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= CHUNK {
            self.send()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.send()
    }
}

pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ArchiveQuery>,
) -> ExportResult<Response> {
    let root_arg = q
        .root
        .as_deref()
        .map(str::trim)
        .map(|r| r.trim_matches('/'))
        .filter(|r| !r.is_empty())
        .map(validate_project_path)
        .transpose()?;
    let prefix = root_arg.clone();
    let (project, root, files) = state
        .blocking(move |s| {
            let project = s.get_project(&id)?;
            let root = s.project_root(&project)?;
            let files = od_store::files::list_files(&root)?;
            Ok((project, root, files))
        })
        .await?;
    let entries: Vec<(String, std::path::PathBuf)> = files
        .into_iter()
        .filter_map(|f| {
            let name = match &prefix {
                Some(p) => f
                    .name
                    .strip_prefix(p.as_str())?
                    .strip_prefix('/')?
                    .to_owned(),
                None => f.name.clone(),
            };
            Some((name, root.dir.join(&f.name)))
        })
        .collect();
    if let Some(p) = &prefix {
        if entries.is_empty() && !root.dir.join(p).is_dir() {
            return Err(ApiErr::new(
                StatusCode::NOT_FOUND,
                ErrorCode::FileNotFound,
                format!("no folder {p} in this project"),
            )
            .into());
        }
    }
    let base = prefix
        .as_deref()
        .and_then(|p| p.rsplit('/').next())
        .unwrap_or(&project.name);
    let base = sanitize_filename(base);
    let filename = format!("{}.zip", if base.is_empty() { "project" } else { &base });

    let (tx, rx) = tokio::sync::mpsc::channel::<std::io::Result<Bytes>>(8);
    tokio::task::spawn_blocking(move || {
        let writer = ChannelWriter {
            tx: tx.clone(),
            buf: Vec::with_capacity(CHUNK),
        };
        match write_zip(writer, &entries) {
            Ok(mut w) => {
                let _ = w.flush();
            }
            Err(e) => {
                tracing::warn!(error = %e, "archive aborted");
                let _ = tx.blocking_send(Err(e));
            }
        }
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Ok(attachment(
        "application/zip",
        &filename,
        Body::from_stream(stream),
    ))
}

pub async fn export_html(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let req = parse_body(&body)?;
    let entry = resolve_entry(&state, id, &req).await?;
    let bundled = bundle(&entry, false).await?;
    let filename = entry.filename(req.title.as_deref(), "html");
    let count = bundled.external_dependencies.len().to_string();
    let mut res = attachment("text/html; charset=utf-8", &filename, bundled.html);
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox allow-scripts"),
    );
    if let Ok(v) = HeaderValue::from_str(&count) {
        headers.insert("x-open-design-external-dependencies", v);
    }
    Ok(res)
}

/// Vector PDF through the print pipeline.
pub async fn export_pdf(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let req = parse_body(&body)?;
    let entry = resolve_entry(&state, id, &req).await?;
    let scratch = scratch(&state, &entry, &req).await?;
    let pdf = renderer.print_pdf(&scratch.target, req.mode()).await?;
    Ok(attachment(
        "application/pdf",
        &entry.filename(req.title.as_deref(), "pdf"),
        pdf,
    ))
}

/// Raster PDF: one page per slide (or per viewport-high page slice). The
/// default PDF of the CLI, as in the original app.
pub async fn export_pdf_image(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let req = parse_body(&body)?;
    let entry = resolve_entry(&state, id, &req).await?;
    let scratch = scratch(&state, &entry, &req).await?;
    let slides = renderer.render_slides(&scratch.target, req.mode()).await?;
    let pdf = tokio::task::spawn_blocking(move || od_export::pdf::pngs_to_pdf(&slides.images))
        .await
        .map_err(internal)?
        .map_err(|e| {
            ExportErr::new(
                StatusCode::BAD_GATEWAY,
                ErrorCode::UpstreamUnavailable,
                e.to_string(),
            )
        })?;
    Ok(attachment(
        "application/pdf",
        &entry.filename(req.title.as_deref(), "pdf"),
        pdf,
    ))
}

/// One image: the requested slide, the whole deck stitched top to bottom,
/// or a full-page capture of an ordinary page.
pub async fn export_image(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let req = parse_body(&body)?;
    let format = match req.image_format.as_deref() {
        None | Some("png") => ImageFormat::Png,
        Some("jpeg" | "jpg") => ImageFormat::Jpeg,
        Some(other) => {
            return Err(ExportErr::new(
                StatusCode::BAD_REQUEST,
                ErrorCode::BadRequest,
                format!("imageFormat must be png or jpeg, got {other:?}"),
            ));
        }
    };
    let entry = resolve_entry(&state, id, &req).await?;
    let scratch = scratch(&state, &entry, &req).await?;
    let mode = req.mode();
    let image = if mode == DeckMode::Page {
        page_capture(renderer.as_ref(), &scratch.target, format).await?
    } else {
        let slides = renderer.render_slides(&scratch.target, mode).await?;
        if !slides.deck {
            page_capture(renderer.as_ref(), &scratch.target, format).await?
        } else if let Some(i) = req.index {
            let count = slides.images.len();
            slides.images.into_iter().nth(i).ok_or_else(|| {
                ExportErr::new(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::BadRequest,
                    format!("slide index {i} out of range (deck has {count})"),
                )
            })?
        } else {
            tokio::task::spawn_blocking(move || od_export::image::stitch_vertical(&slides.images))
                .await
                .map_err(internal)?
                .map_err(|e| {
                    ExportErr::new(
                        StatusCode::BAD_GATEWAY,
                        ErrorCode::UpstreamUnavailable,
                        e.to_string(),
                    )
                })?
        }
    };
    // Slides are always PNG; JPEG applies to page captures.
    let (mime, ext) = if image.starts_with(b"\x89PNG") {
        ("image/png", "png")
    } else {
        (format.mime(), format.extension())
    };
    Ok(attachment(
        mime,
        &entry.filename(req.title.as_deref(), ext),
        image,
    ))
}

async fn page_capture(
    renderer: &dyn Renderer,
    target: &RenderTarget,
    format: ImageFormat,
) -> ExportResult<Vec<u8>> {
    Ok(renderer
        .capture_page(
            target,
            CaptureOptions {
                format,
                full_page: true,
            },
        )
        .await?)
}

/// Image PPTX: one full-bleed screenshot per slide. Deck only.
pub async fn export_pptx(
    State(state): State<AppState>,
    Extension(renderer): Extension<Arc<dyn Renderer>>,
    Path(id): Path<String>,
    body: Bytes,
) -> ExportResult<Response> {
    let req = parse_body(&body)?;
    let entry = resolve_entry(&state, id, &req).await?;
    let scratch = scratch(&state, &entry, &req).await?;
    let slides = renderer
        .render_slides(&scratch.target, DeckMode::Deck)
        .await?;
    let title = req
        .title
        .clone()
        .unwrap_or_else(|| entry.project.name.clone());
    let pptx =
        tokio::task::spawn_blocking(move || od_export::pptx::pngs_to_pptx(&slides.images, &title))
            .await
            .map_err(internal)?
            .map_err(|e| {
                ExportErr::new(
                    StatusCode::BAD_GATEWAY,
                    ErrorCode::UpstreamUnavailable,
                    e.to_string(),
                )
            })?;
    Ok(attachment(
        od_export::pptx::PPTX_MIME,
        &entry.filename(req.title.as_deref(), "pptx"),
        pptx,
    ))
}
