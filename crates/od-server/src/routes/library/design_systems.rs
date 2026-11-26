//! The design-system library (spec 1.4 "Design system"): kit data, the
//! showcase, user packages (import, copy, edit, status, delete, assets,
//! archive), the token contract and revisions, and "Edit with agent".
//!
//! Bundled packages are read-only; a write to one is 403 (copy it first).
//!
//! - `GET    /api/design-systems/brands`: brands to start from.
//! - `POST   /api/design-systems/reload`: rescan after an agent edited a
//!   package in place.
//! - `POST   /api/design-systems/import` `{name, category?, designMd?, css?}`:
//!   a package from a pasted `DESIGN.md` and/or CSS variables (shadcn,
//!   Tailwind `@theme`, `:root`), no agent.
//! - `GET    /api/design-systems/:id/kit`: body (fresh from disk), tokens,
//!   palette, token score, assets, files, pending revisions.
//! - `GET    /api/design-systems/:id/showcase`: `components.html`, else a
//!   page built from the tokens.
//! - `GET    /api/design-systems/:id/file/*path`: a package file (assets,
//!   preview pages), sandboxed.
//! - `PUT    /api/design-systems/:id/design-md` `{body}`; with `If-Match`
//!   (the `ETag` of `GET …/kit`) only over that same DESIGN.md, else 412.
//! - `POST   /api/design-systems/:id/status` `{status: draft|published}`.
//! - `POST   /api/design-systems/:id/delete`.
//! - `POST   /api/design-systems/:id/copy` `{name?}`: an editable copy.
//! - `GET    /api/design-systems/:id/archive`: ZIP of the package.
//! - `POST   /api/design-systems/:id/assets` (multipart) and
//!   `POST /api/design-systems/:id/assets/:name/delete`: logos, fonts, images
//!   under `assets/`.
//! - `POST   /api/design-systems/:id/token-contract/rebuild`: a pending
//!   revision whose `tokens.css` binds every missing token to its fallback.
//! - `POST   /api/design-systems/:id/revisions` `{feedback, section?, agentId?}`:
//!   "Needs work": an agent revises a copy; the result waits for review.
//! - `GET    /api/design-systems/:id/revisions`,
//!   `POST /api/design-systems/:id/revisions/:rid` `{status: accepted|rejected}`.
//! - `POST   /api/design-systems/:id/workspace`: a project whose folder is
//!   the package, for "Edit with agent".

use super::write_and_reload;
use crate::cas::IfMatch;
use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::routes::content::rebrand_example;
use crate::state::AppState;
use axum::extract::{Multipart, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use od_content::DesignSystemEntry;
use od_content::design_kit::{self, Swatch, Token, TokenScore};
use od_content::design_system_gen::{self, GenerateRequest};
use od_content::user_content::copy_dir;
use od_contracts::{ContentSource, CreateProjectRequest, ProjectMetadata, RunStatus};
use od_runs::RunMeta;
use od_runtime::env::{RunEnv, agent_env};
use od_runtime::{LaunchSpec, McpLaunch, PermissionMode};
use od_store::paths::name_key;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, PathBuf};
use std::sync::Arc;

const REVISIONS_DIR: &str = "revisions";
const ASSETS_DIR: &str = "assets";
const REVISION_FILE: &str = "revision.json";
const MAX_ASSET_BYTES: usize = 20 * 1024 * 1024;
/// The most one package file read here holds (an asset at most).
const MAX_READ_BYTES: u64 = MAX_ASSET_BYTES as u64;
/// Files a revision may propose.
const REVISABLE: [&str; 2] = ["DESIGN.md", "tokens.css"];

/// Pages in a package are rendered without scripts or network.
const PAGE_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data: 'self'; \
     font-src data: 'self'; media-src data: 'self'; sandbox";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/design-systems/brands", get(brands))
        .route("/api/design-systems/reload", post(reload))
        .route("/api/design-systems/import", post(import))
        .route("/api/design-systems/{id}/kit", get(kit))
        .route("/api/design-systems/{id}/showcase", get(showcase))
        .route("/api/design-systems/{id}/file/{*path}", get(file))
        .route("/api/design-systems/{id}/design-md", put(save_design_md))
        .route("/api/design-systems/{id}/status", post(set_status))
        .route("/api/design-systems/{id}/delete", post(delete))
        .route("/api/design-systems/{id}/copy", post(copy))
        .route("/api/design-systems/{id}/archive", get(archive))
        .route("/api/design-systems/{id}/assets", post(upload_assets))
        .route(
            "/api/design-systems/{id}/assets/{name}/delete",
            post(delete_asset),
        )
        .route(
            "/api/design-systems/{id}/token-contract/rebuild",
            post(rebuild),
        )
        .route(
            "/api/design-systems/{id}/revisions",
            get(list_revisions).post(revise),
        )
        .route("/api/design-systems/{id}/revisions/{rid}", post(decide))
        .route("/api/design-systems/{id}/workspace", post(workspace))
}

// ------------------------------------------------------------------ helpers

fn entry(state: &AppState, id: &str) -> ApiResult<DesignSystemEntry> {
    state
        .content
        .snapshot()
        .find_design_system(id)
        .cloned()
        .ok_or_else(|| ApiErr::not_found("design system not found"))
}

/// The entry when the user may change it.
fn editable(state: &AppState, id: &str) -> ApiResult<DesignSystemEntry> {
    let e = entry(state, id)?;
    if e.summary.source != ContentSource::User || !e.summary.is_editable {
        return Err(ApiErr::forbidden(
            "bundled design systems are read-only; copy it to your systems first",
        ));
    }
    Ok(e)
}

fn a2(state: &AppState) -> Vec<Token> {
    let path = state
        .config
        .resource_root
        .join("design-systems/_schema/defaults.css");
    design_kit::a2_defaults(&read_text(path).unwrap_or_default())
}

fn user_root(state: &AppState) -> PathBuf {
    state.store.data_dir().root().join("design-systems")
}

/// The real path of `rel` inside a package: no `..`, no hidden segment,
/// and, like project files, resolved through the store's guard so a
/// symlink in the package (an agent editing it in place can plant one)
/// never takes a read or write outside it. Every package read and write
/// goes through here.
fn inside(dir: &std::path::Path, rel: &str) -> ApiResult<PathBuf> {
    let mut parts = 0;
    for c in std::path::Path::new(rel).components() {
        match c {
            Component::Normal(s) if !s.to_string_lossy().starts_with('.') => parts += 1,
            _ => return Err(ApiErr::bad_request("invalid path")),
        }
    }
    if parts == 0 {
        return Err(ApiErr::bad_request("invalid path"));
    }
    Ok(od_store::paths::resolve_safe_real(dir, rel)?)
}

/// [`inside`] for I/O helpers.
fn inside_io(dir: &std::path::Path, rel: &str) -> std::io::Result<PathBuf> {
    inside(dir, rel).map_err(|e| std::io::Error::other(e.message))
}

/// Reads the package file at `path` (a real path from [`inside`]): a
/// regular file only, no link planted since at its name, never waiting on
/// a FIFO, at most [`MAX_READ_BYTES`] (`od_content::nofollow::read_file`).
/// `NotFound` when it is missing or no regular file.
fn read_bytes(path: impl AsRef<std::path::Path>) -> std::io::Result<Vec<u8>> {
    use od_content::nofollow::TreeError;
    match od_content::nofollow::read_file(path.as_ref(), MAX_READ_BYTES) {
        Ok(Some(file)) => Ok(file.bytes),
        Ok(None) => Err(std::io::ErrorKind::NotFound.into()),
        Err(TreeError::TooLarge) => Err(std::io::Error::other("file too large")),
        Err(TreeError::Io(e)) => Err(e),
    }
}

/// [`read_bytes`] as UTF-8 text.
fn read_text(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    String::from_utf8(read_bytes(path)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn read(dir: &std::path::Path, name: &str) -> String {
    inside_io(dir, name).and_then(read_text).unwrap_or_default()
}

fn mime_of(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "md" | "txt" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

fn page(html: Vec<u8>, mime: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_SECURITY_POLICY, PAGE_CSP),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response()
}

// -------------------------------------------------------------------- brands

/// Well-known brands to start a design system from: picking one adds its
/// website as a source the agent studies.
const BRANDS: [(&str, &str, &str); 36] = [
    ("Stripe", "stripe.com", "software"),
    ("Linear", "linear.app", "software"),
    ("Notion", "notion.so", "software"),
    ("Figma", "figma.com", "software"),
    ("Vercel", "vercel.com", "software"),
    ("GitHub", "github.com", "software"),
    ("Slack", "slack.com", "software"),
    ("Shopify", "shopify.com", "software"),
    ("Airbnb", "airbnb.com", "travel"),
    ("Booking.com", "booking.com", "travel"),
    ("Apple", "apple.com", "electronics"),
    ("Samsung", "samsung.com", "electronics"),
    ("Sony", "sony.com", "electronics"),
    ("Tesla", "tesla.com", "automotive"),
    ("BMW", "bmw.com", "automotive"),
    ("Porsche", "porsche.com", "automotive"),
    ("Nike", "nike.com", "activewear"),
    ("Adidas", "adidas.com", "activewear"),
    ("Patagonia", "patagonia.com", "fashion"),
    ("Zara", "zara.com", "fashion"),
    ("Glossier", "glossier.com", "beauty"),
    ("Aesop", "aesop.com", "beauty"),
    ("Headspace", "headspace.com", "wellness"),
    ("Calm", "calm.com", "wellness"),
    ("Sweetgreen", "sweetgreen.com", "food"),
    ("Starbucks", "starbucks.com", "beverage"),
    ("Coca-Cola", "coca-cola.com", "beverage"),
    ("The Economist", "economist.com", "media"),
    ("The New York Times", "nytimes.com", "media"),
    ("Spotify", "spotify.com", "media"),
    ("Duolingo", "duolingo.com", "education"),
    ("Khan Academy", "khanacademy.org", "education"),
    ("Wise", "wise.com", "finance"),
    ("Revolut", "revolut.com", "finance"),
    ("Monzo", "monzo.com", "finance"),
    ("One Medical", "onemedical.com", "healthcare"),
];

pub async fn brands() -> Json<Value> {
    let list: Vec<Value> = BRANDS
        .iter()
        .map(|(name, domain, category)| json!({ "name": name, "domain": domain, "category": category }))
        .collect();
    Json(json!({ "brands": list }))
}

pub async fn reload(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    write_and_reload(&state, || Ok(())).await?;
    Ok(Json(json!({ "ok": true })))
}

// -------------------------------------------------------------------- import

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBody {
    name: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    design_md: Option<String>,
    #[serde(default)]
    css: Option<String>,
}

pub async fn import(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ImportBody>,
) -> ApiResult<Json<Value>> {
    let design = body
        .design_md
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let css = body.css.as_deref().map(str::trim).filter(|c| !c.is_empty());
    if design.is_none() && css.is_none() {
        return Err(ApiErr::bad_request("paste a DESIGN.md or CSS variables"));
    }
    let tokens = match css {
        Some(c) => design_kit::tokens_from_css_vars(c).ok_or_else(|| {
            ApiErr::bad_request("the CSS defines no background, foreground and primary colours")
        })?,
        None => design
            .map(design_kit::palette_from_markdown)
            .and_then(|p| design_kit::tokens_from_palette(&p))
            .ok_or_else(|| ApiErr::bad_request("the DESIGN.md names fewer than two colours"))?,
    };
    let req = GenerateRequest::new(
        &body.name,
        design.unwrap_or("Imported from CSS variables."),
        body.category.as_deref(),
    )?;
    let design = match design {
        Some(d) => d.to_owned(),
        None => design_kit::design_md_from_tokens(&req.name, &design_kit::parse_tokens(&tokens)),
    };
    let origin = if css.is_some() {
        "imported from CSS variables"
    } else {
        "imported from a pasted DESIGN.md"
    };
    let root = user_root(&state);
    let installed = write_and_reload(&state, move || {
        let staging = tempfile::tempdir().map_err(ApiErr::io)?;
        std::fs::write(staging.path().join("DESIGN.md"), design)
            .and_then(|()| std::fs::write(staging.path().join("tokens.css"), tokens))
            .map_err(ApiErr::io)?;
        Ok(design_system_gen::install_imported(
            staging.path(),
            &root,
            &req,
            origin,
        )?)
    })
    .await?;
    Ok(Json(json!({ "id": installed.id })))
}

// ----------------------------------------------------------------------- kit

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: String,
    /// `agent` (Needs work) or `token-rebuild`.
    pub kind: String,
    /// running, pending, accepted, rejected or failed.
    pub status: String,
    #[serde(default)]
    pub feedback: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Proposed files, by name, with their text (filled on read).
    #[serde(default)]
    pub files: Vec<RevisionFile>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionFile {
    pub name: String,
    #[serde(default)]
    pub content: String,
}

fn revisions_of(dir: &std::path::Path) -> Vec<Revision> {
    let Ok(read_dir) = inside_io(dir, REVISIONS_DIR).and_then(std::fs::read_dir) else {
        return Vec::new();
    };
    let mut out: Vec<Revision> = read_dir
        .filter_map(Result::ok)
        .filter_map(|e| {
            let rdir = format!("{REVISIONS_DIR}/{}", e.file_name().to_string_lossy());
            let file = |n: &str| inside_io(dir, &format!("{rdir}/{n}"));
            let mut r: Revision =
                serde_json::from_str(&file(REVISION_FILE).and_then(read_text).ok()?).ok()?;
            r.files = REVISABLE
                .iter()
                .filter_map(|n| {
                    file(n)
                        .and_then(read_text)
                        .ok()
                        .map(|content| RevisionFile {
                            name: (*n).to_owned(),
                            content,
                        })
                })
                .collect();
            Some(r)
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    out
}

fn write_revision(dir: &std::path::Path, r: &Revision) -> std::io::Result<()> {
    let rdir = format!("{REVISIONS_DIR}/{}", r.id);
    std::fs::create_dir_all(inside_io(dir, &rdir)?)?;
    let meta = Revision {
        files: Vec::new(),
        ..r.clone()
    };
    std::fs::write(
        inside_io(dir, &format!("{rdir}/{REVISION_FILE}"))?,
        serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?,
    )?;
    for f in &r.files {
        std::fs::write(inside_io(dir, &format!("{rdir}/{}", f.name))?, &f.content)?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub name: String,
    /// logo, font or image.
    pub kind: &'static str,
    pub size: u64,
}

fn asset_kind(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if [".woff2", ".woff", ".ttf", ".otf"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        "font"
    } else if lower.contains("logo") || lower.ends_with(".svg") {
        "logo"
    } else {
        "image"
    }
}

fn assets_of(dir: &std::path::Path) -> Vec<Asset> {
    let Ok(read_dir) = inside_io(dir, ASSETS_DIR).and_then(std::fs::read_dir) else {
        return Vec::new();
    };
    let mut out: Vec<Asset> = read_dir
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.')).then(|| Asset {
                kind: asset_kind(&name),
                size: e.metadata().map_or(0, |m| m.len()),
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kit {
    pub summary: od_contracts::DesignSystemSummary,
    pub body: String,
    pub tokens: Vec<Token>,
    pub palette: Vec<Swatch>,
    pub score: TokenScore,
    pub has_components: bool,
    pub assets: Vec<Asset>,
    /// Preview pages under `preview/`.
    pub previews: Vec<String>,
    pub revisions: Vec<Revision>,
    /// The company or product a brand-named system is inspired by
    /// (`manifest.json` `brand`), for the trademark note on its page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
}

/// The DESIGN.md of package `e` as the kit shows it: the file, or the
/// body indexed when it cannot be read. Its tag is the kit's `ETag`, the
/// one a Save of the editor sends back (`crate::cas`).
fn shown_design_md(e: &DesignSystemEntry) -> String {
    inside_io(&e.dir, "DESIGN.md")
        .and_then(read_text)
        .unwrap_or_else(|_| e.body.clone())
}

/// Held by every write of an editable package's DESIGN.md (Save, an
/// accepted revision): a Save compares and writes in one step.
static DESIGN_MD_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn design_md_lock() -> std::sync::MutexGuard<'static, ()> {
    DESIGN_MD_WRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub async fn kit(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let e = entry(&state, &id)?;
    let a2 = a2(&state);
    let kit = crate::spawn::spawn_blocking_in_span(move || {
        let body = shown_design_md(&e);
        let tokens = design_kit::parse_tokens(&read(&e.dir, "tokens.css"));
        let mut previews: Vec<String> = inside_io(&e.dir, "preview")
            .and_then(std::fs::read_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|d| d.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".html") && !n.starts_with('.'))
            .collect();
        previews.sort();
        let brand = e.summary.brand.clone();
        Kit {
            brand,
            palette: design_kit::palette_from_markdown(&body),
            score: design_kit::score(&tokens, &a2),
            has_components: inside(&e.dir, "components.html").is_ok_and(|p| p.is_file()),
            assets: assets_of(&e.dir),
            revisions: revisions_of(&e.dir),
            previews,
            tokens,
            body,
            summary: e.summary,
        }
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    let tag = crate::cas::tag_of(kit.body.as_bytes());
    Ok((crate::cas::etag(&tag), Json(kit)))
}

pub async fn showcase(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let e = entry(&state, &id)?;
    let components = inside(&e.dir, "components.html")?;
    let found = crate::spawn::spawn_blocking_in_span(move || read_bytes(&components)).await;
    let html = match found {
        Ok(Ok(bytes)) => bytes,
        _ => design_kit::showcase_html(&e.summary.title, &read(&e.dir, "tokens.css")).into_bytes(),
    };
    Ok(page(rebrand_example(html), "text/html; charset=utf-8"))
}

pub async fn file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> ApiResult<Response> {
    let e = entry(&state, &id)?;
    let target = inside(&e.dir, &path)?;
    if target.starts_with(inside(&e.dir, REVISIONS_DIR)?) {
        return Err(ApiErr::not_found("file not found"));
    }
    let bytes = crate::spawn::spawn_blocking_in_span(move || read_bytes(&target))
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| ApiErr::not_found("file not found"))?;
    let mime = mime_of(&path);
    let bytes = if mime.starts_with("text/") {
        rebrand_example(bytes)
    } else {
        bytes
    };
    Ok(page(bytes, mime))
}

// ------------------------------------------------------------------- writes

#[derive(Debug, Deserialize)]
pub struct DesignMdBody {
    body: String,
}

pub async fn save_design_md(
    State(state): State<AppState>,
    Path(id): Path<String>,
    if_match: IfMatch,
    ApiJson(body): ApiJson<DesignMdBody>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    if body.body.trim().is_empty() {
        return Err(ApiErr::bad_request("DESIGN.md cannot be empty"));
    }
    write_and_reload(&state, move || {
        let _one = design_md_lock();
        if_match.check(&crate::cas::tag_of(shown_design_md(&e).as_bytes()))?;
        std::fs::write(inside(&e.dir, "DESIGN.md")?, body.body).map_err(ApiErr::io)
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Merges `patch` into the package's `metadata.json`.
fn patch_metadata(dir: &std::path::Path, patch: &Value) -> std::io::Result<()> {
    let path = inside_io(dir, "metadata.json")?;
    let mut meta = read_text(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let (Some(m), Some(p)) = (meta.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            m.insert(k.clone(), v.clone());
        }
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?,
    )
}

#[derive(Debug, Deserialize)]
pub struct StatusBody {
    status: String,
}

pub async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<StatusBody>,
) -> ApiResult<Json<Value>> {
    if !matches!(body.status.as_str(), "draft" | "published") {
        return Err(ApiErr::bad_request("status must be draft or published"));
    }
    let e = editable(&state, &id)?;
    let status = body.status.clone();
    write_and_reload(&state, move || {
        patch_metadata(&e.dir, &json!({ "status": status })).map_err(ApiErr::io)
    })
    .await?;
    Ok(Json(json!({ "status": body.status })))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    let root = user_root(&state);
    write_and_reload(&state, move || {
        Ok(od_content::user_content::delete_user_dir(&root, &e.dir)?)
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Default, Deserialize)]
pub struct CopyBody {
    #[serde(default)]
    name: Option<String>,
}

pub async fn copy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let e = entry(&state, &id)?;
    let body: CopyBody = super::optional_json(&body)?;
    let name = body
        .name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("{} copy", e.summary.title));
    let root = user_root(&state);
    let new_id = write_and_reload(&state, move || {
        std::fs::create_dir_all(&root).map_err(ApiErr::io)?;
        let slug = design_system_gen::slugify(&name);
        let dest = (1..)
            .map(|n| {
                if n == 1 {
                    root.join(&slug)
                } else {
                    root.join(format!("{slug}-{n}"))
                }
            })
            .find(|p| !p.exists())
            .unwrap_or_else(|| root.join(&slug));
        let folder = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        copy_dir(&e.dir, &dest, u64::MAX)?;
        let _ = std::fs::remove_dir_all(dest.join(REVISIONS_DIR));
        // `copy_dir` leaves hidden files; the sent names go too, or the
        // copy's `logo-chính.png` would take `logo chính.png`'s (QA-109).
        write_sent_names(&dest, &read_sent_names(&e.dir)).map_err(ApiErr::io)?;
        // The manifest names its folder; keep it valid for the new one.
        let manifest_path = dest.join("manifest.json");
        if let Some(mut m) = read_text(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            m["id"] = json!(folder);
            m["name"] = json!(name);
            m["source"] =
                json!({ "type": "local", "origin": format!("copied from {}", e.summary.id) });
            let _ = std::fs::write(
                &manifest_path,
                serde_json::to_vec_pretty(&m).unwrap_or_default(),
            );
        }
        patch_metadata(
            &dest,
            &json!({ "title": name, "category": e.summary.category, "status": "draft" }),
        )
        .map_err(ApiErr::io)?;
        Ok(format!(
            "{}{folder}",
            od_content::design_systems::USER_ID_PREFIX
        ))
    })
    .await?;
    Ok(Json(json!({ "id": new_id })))
}

/// Package files for the archive: everything visible except revisions,
/// and the assets' sent names ([`SENT_NAMES`]).
fn package_files(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !(dir == base && name == SENT_NAMES) {
                continue;
            }
            let path = e.path();
            let rel = path
                .strip_prefix(base)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if rel == REVISIONS_DIR {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(base, &path, out),
                Ok(t) if t.is_file() => out.push((rel, path)),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

pub async fn archive(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let e = entry(&state, &id)?;
    let folder = e
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "design-system".to_owned());
    let bytes = crate::spawn::spawn_blocking_in_span(move || {
        let files = package_files(&e.dir)
            .into_iter()
            .map(|(rel, p)| (format!("{folder}/{rel}"), p))
            .collect::<Vec<_>>();
        od_export::archive::write_zip(Vec::new(), &files)
    })
    .await
    .map_err(|err| ApiErr::internal(err.to_string()))?
    .map_err(ApiErr::io)?;
    let filename = format!(
        "{}.zip",
        od_export::archive::sanitize_filename(&entry(&state, &id)?.summary.title)
    );
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                od_export::archive::content_disposition(&filename),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// The name a file the user sent is kept under in a design system's own
/// folder (its `assets/`, a new one's `source/`): its last segment, as
/// [`od_store::paths::upload_name`] keeps it (Unicode letters kept, a name
/// too long for the disk shortened; QA-106/107). `None` for no name or a
/// hidden one.
pub(crate) fn kept_name(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next()?.trim();
    (!base.is_empty() && !base.starts_with('.')).then(|| od_store::paths::upload_name(base))
}

/// Refuses a batch two of whose files would be kept under one name: the
/// second would be written over the first (QA-107).
pub(crate) fn refuse_same_name<'a>(
    names: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> ApiResult<()> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for (raw, name) in names {
        if let Some(first) = seen.insert(name_key(name), raw) {
            return Err(ApiErr::bad_request(format!(
                "{first} and {raw} would both be saved as {name}. Rename one, then try again."
            )));
        }
    }
    Ok(())
}

const ASSET_EXTENSIONS: [&str; 10] = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".woff2", ".woff", ".ttf", ".otf",
];

fn is_asset_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    ASSET_EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

/// The name a new asset is kept under (see [`kept_name`]).
fn safe_asset_name(raw: &str) -> Option<String> {
    kept_name(raw).filter(|clean| is_asset_file(clean))
}

/// The name a file was sent as: its last segment, as [`kept_name`] reads it.
fn sent_name(raw: &str) -> &str {
    raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim()
}

/// The names assets were sent as, where the cleaning changed them:
/// `{"_.png": "🎨.png", "logo-chính.png": "logo chính.png"}`. An asset not
/// in it was sent as the name it has. In the package folder, not
/// `assets/` (which holds assets only); hidden, yet copied and archived
/// with the package (QA-109).
const SENT_NAMES: &str = ".sent-names.json";

/// One upload or delete of assets at a time: each reads [`SENT_NAMES`],
/// picks names and writes it back.
static ASSET_WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// [`SENT_NAMES`] of package folder `dir`; empty when missing,
/// unreadable or a link (never followed out of the package).
fn read_sent_names(dir: &std::path::Path) -> BTreeMap<String, String> {
    read_bytes(dir.join(SENT_NAMES))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes [`SENT_NAMES`] whole (a new file renamed over it: a link
/// planted there is replaced, not followed). A few hundred bytes, so
/// written in place from async code too.
fn write_sent_names(
    dir: &std::path::Path,
    names: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    use std::io::Write as _;
    let path = dir.join(SENT_NAMES);
    if names.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        };
    }
    let tmp = dir.join(format!(
        "{SENT_NAMES}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let bytes = serde_json::to_vec_pretty(names).map_err(std::io::Error::other)?;
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| file.write_all(&bytes))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// The asset a file sent as `sent` replaces: the one sent under that name
/// before (NFC, any case), as [`SENT_NAMES`] records it, else the one
/// named so on disk. `None`: it is a new asset, even when its cleaned name
/// is taken (`🖼.png` is not `🎨.png`, though both clean to `_.png`).
fn replaced_asset(
    assets: &std::path::Path,
    sent_names: &BTreeMap<String, String>,
    sent: &str,
) -> Option<String> {
    let key = name_key(sent);
    let mut on_disk: Vec<String> = std::fs::read_dir(assets)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| existing_asset_name(n).is_some())
        .collect();
    on_disk.sort();
    let sent_as = |n: &String| {
        sent_names
            .get(n)
            .map_or(n.as_str(), String::as_str)
            .to_owned()
    };
    // The closest match first: on a disk that tells `Logo.png` from
    // `logo.png` (Linux), sending `logo.png` replaces `logo.png`, not the
    // first name that only folds to it.
    let nfc = |n: &str| {
        use unicode_normalization::UnicodeNormalization as _;
        n.nfc().collect::<String>()
    };
    on_disk
        .iter()
        .find(|n| sent_as(n) == sent)
        .or_else(|| on_disk.iter().find(|n| nfc(&sent_as(n)) == nfc(sent)))
        .or_else(|| on_disk.iter().find(|n| name_key(&sent_as(n)) == key))
        .cloned()
}

/// An asset already in the package, named as it is on disk: one segment,
/// not hidden, an asset's extension. Never cleaned or shortened again: a
/// name kept whole before (a 220-byte one) would then name another asset.
fn existing_asset_name(name: &str) -> Option<&str> {
    (!name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && is_asset_file(name))
    .then_some(name)
}

/// Candidate names for a new asset: its own, then `stem-1.ext`…
fn candidate_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = name.rsplit_once('.').map_or((name, ""), |(s, e)| (s, e));
    (0..10_000).map(move |i| {
        if i == 0 {
            name.to_owned()
        } else if ext.is_empty() {
            format!("{stem}-{i}")
        } else {
            format!("{stem}-{i}.{ext}")
        }
    })
}

/// Where one file of an upload goes.
enum Target {
    /// Over this asset, sent under the same name before.
    Replace(String, PathBuf),
    /// A new asset: its cleaned name, or the first free `stem-N.ext`.
    New(String),
}

pub async fn upload_assets(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    // Every file read and checked before any is written: one refused
    // and none of the batch lands.
    let mut parts = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiErr::bad_request(err.body_text()))?
    {
        let Some(raw) = field.file_name().map(str::to_owned) else {
            continue;
        };
        let name = safe_asset_name(&raw).ok_or_else(|| {
            ApiErr::bad_request(format!("{raw}: only images (PNG, JPG, SVG…) and fonts"))
        })?;
        let bytes = field
            .bytes()
            .await
            .map_err(|err| ApiErr::bad_request(err.body_text()))?;
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(ApiErr::bad_request(format!("{name} is larger than 20 MB")));
        }
        // Its path checked here too (a link planted at it): refused before
        // any of the batch is written.
        inside(&e.dir, &format!("{ASSETS_DIR}/{name}"))?;
        parts.push((raw, name, bytes));
    }
    if parts.is_empty() {
        return Err(ApiErr::bad_request("no files in upload"));
    }
    refuse_same_name(
        parts
            .iter()
            .map(|(raw, name, _)| (raw.as_str(), name.as_str())),
    )?;
    let assets = inside(&e.dir, ASSETS_DIR)?;
    tokio::fs::create_dir_all(&assets)
        .await
        .map_err(ApiErr::io)?;
    let _writes = ASSET_WRITES.lock().await;
    // A file replaces only the asset sent under its name before; any
    // other takes the first free `stem-N.ext`, never a name of the batch's
    // other files: `🖼.png` after `🎨.png` (both `_.png`), `logo-chính.png`
    // after `logo chính.png`, two long names alike up to the cut (QA-109).
    let mut sent_names = read_sent_names(&e.dir);
    let mut targets = Vec::with_capacity(parts.len());
    for (raw, name, _) in &parts {
        targets.push(match replaced_asset(&assets, &sent_names, sent_name(raw)) {
            Some(old) => {
                let path = inside(&e.dir, &format!("{ASSETS_DIR}/{old}"))?;
                Target::Replace(old, path)
            }
            None => Target::New(name.clone()),
        });
    }
    let batch: Vec<String> = parts.iter().map(|(_, n, _)| name_key(n)).collect();
    // A write that fails half way keeps those before it, and their sent
    // names: Try again replaces them, adding none.
    let mut saved = Vec::with_capacity(parts.len());
    for ((raw, _, bytes), target) in parts.into_iter().zip(targets) {
        let name = match target {
            Target::Replace(name, path) => {
                tokio::fs::write(path, &bytes).await.map_err(ApiErr::io)?;
                name
            }
            Target::New(name) => {
                let name = write_new_asset(&e.dir, &name, &batch, &bytes).await?;
                let sent = sent_name(&raw);
                let changed = if sent == name {
                    sent_names.remove(&name).is_some()
                } else {
                    sent_names.insert(name.clone(), sent.to_owned()).as_deref() != Some(sent)
                };
                // Not kept without its sent name: Try again would add a
                // `stem-1` copy, or let a cleaned name replace it.
                if changed && let Err(err) = write_sent_names(&e.dir, &sent_names) {
                    let _ = tokio::fs::remove_file(assets.join(&name)).await;
                    return Err(ApiErr::io(err));
                }
                name
            }
        };
        saved.push(json!({ "name": name, "kind": asset_kind(&name), "size": bytes.len() }));
    }
    Ok(Json(json!({ "assets": saved })))
}

/// Writes a new asset under the first of its candidate names free on
/// disk and not another file's of the batch, never over a file.
async fn write_new_asset(
    dir: &std::path::Path,
    name: &str,
    batch: &[String],
    bytes: &[u8],
) -> ApiResult<String> {
    use tokio::io::AsyncWriteExt as _;
    let own = name_key(name);
    for candidate in candidate_names(name) {
        let key = name_key(&candidate);
        if key != own && batch.contains(&key) {
            continue;
        }
        let path = inside(dir, &format!("{ASSETS_DIR}/{candidate}"))?;
        // Taken under another case or form (`Logo-1.png` on Linux): the
        // next one, as a Mac's disk would refuse it.
        if path
            .parent()
            .and_then(|assets| od_store::paths::entry_named(assets, &candidate))
            .is_some()
        {
            continue;
        }
        let opened = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await;
        match opened {
            Ok(mut file) => {
                let written = async {
                    file.write_all(bytes).await?;
                    file.flush().await
                }
                .await;
                if let Err(err) = written {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(ApiErr::io(err));
                }
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(ApiErr::io(err)),
        }
    }
    Err(ApiErr::bad_request(format!(
        "{name}: too many assets of this name"
    )))
}

pub async fn delete_asset(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    let name = existing_asset_name(&name).ok_or_else(|| ApiErr::bad_request("invalid asset"))?;
    let _writes = ASSET_WRITES.lock().await;
    tokio::fs::remove_file(inside(&e.dir, &format!("{ASSETS_DIR}/{name}"))?)
        .await
        .map_err(|_| ApiErr::not_found("asset not found"))?;
    // Its sent name goes with it: a file put there after (the agent's) was
    // not sent as that. The asset is gone either way.
    let mut sent_names = read_sent_names(&e.dir);
    if sent_names.remove(name).is_some()
        && let Err(err) = write_sent_names(&e.dir, &sent_names)
    {
        tracing::warn!(error = %err, "the sent name of a deleted asset kept");
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- revisions

fn new_revision(kind: &str, feedback: Option<String>, section: Option<String>) -> Revision {
    let now = od_store::now_ms();
    Revision {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.to_owned(),
        status: "running".to_owned(),
        feedback,
        section,
        run_id: None,
        error: None,
        files: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

pub async fn rebuild(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    let a2 = a2(&state);
    let css = read(&e.dir, "tokens.css");
    let s = design_kit::score(&design_kit::parse_tokens(&css), &a2);
    let Some(rebuilt) = design_kit::rebuild_tokens(&css, &a2) else {
        return Ok(Json(json!({ "decision": "up-to-date", "score": s })));
    };
    let mut r = new_revision("token-rebuild", None, Some("Tokens".to_owned()));
    r.status = "pending".to_owned();
    r.files = vec![RevisionFile {
        name: "tokens.css".to_owned(),
        content: rebuilt,
    }];
    let dir = e.dir.clone();
    let saved = r.clone();
    crate::spawn::spawn_blocking_in_span(move || write_revision(&dir, &saved))
        .await
        .map_err(|err| ApiErr::internal(err.to_string()))?
        .map_err(ApiErr::io)?;
    Ok(Json(
        json!({ "decision": "revision", "score": s, "revision": r }),
    ))
}

pub async fn list_revisions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let e = entry(&state, &id)?;
    Ok(Json(json!({ "revisions": revisions_of(&e.dir) })))
}

/// A package file an accept writes.
struct Prior {
    path: PathBuf,
    /// What it held before (`None`: it was not there).
    old: Option<Vec<u8>>,
    /// What the accept wrote; `None` while it is written (a write that
    /// fails half way leaves a part of it).
    wrote: Option<Vec<u8>>,
}

/// Writes an accepted revision's files into the package `dir`, all or
/// nothing: on an error those written (or being written) get their old
/// content back. Returns that old content, for [`put_back`] should the
/// decision itself not be saved.
fn accept_files(dir: &std::path::Path, files: &[RevisionFile]) -> ApiResult<Vec<Prior>> {
    accept_files_with(dir, files, |path, bytes| std::fs::write(path, bytes))
}

/// [`accept_files`], each file written by `write`, so a test can fail a
/// write half way as a full disk would.
fn accept_files_with(
    dir: &std::path::Path,
    files: &[RevisionFile],
    mut write: impl FnMut(&std::path::Path, &[u8]) -> std::io::Result<()>,
) -> ApiResult<Vec<Prior>> {
    let mut prior: Vec<Prior> = Vec::new();
    let wrote = files
        .iter()
        .filter(|f| REVISABLE.contains(&f.name.as_str()))
        .try_for_each(|f| {
            let path = inside(dir, &f.name)?;
            let old = match od_content::nofollow::read_file(&path, MAX_READ_BYTES) {
                Ok(Some(file)) => Some(file.bytes),
                // Missing: the accept makes it.
                Ok(None) if std::fs::symlink_metadata(&path).is_err() => None,
                // A FIFO or a device: writing would wait on it.
                Ok(None) => {
                    return Err(ApiErr::bad_request(format!(
                        "{} is not a regular file",
                        f.name
                    )));
                }
                Err(od_content::nofollow::TreeError::TooLarge) => {
                    return Err(ApiErr::bad_request(format!("{} is too large", f.name)));
                }
                Err(od_content::nofollow::TreeError::Io(err)) => return Err(ApiErr::io(err)),
            };
            // Listed before the write: one that fails half way is put back.
            prior.push(Prior {
                path: path.clone(),
                old,
                wrote: None,
            });
            io(write(&path, f.content.as_bytes()))?;
            if let Some(p) = prior.last_mut() {
                p.wrote = Some(f.content.clone().into_bytes());
            }
            Ok(())
        });
    match wrote {
        Ok(()) => Ok(prior),
        Err(err) => {
            put_back(&prior);
            Err(err)
        }
    }
}

/// Undoes [`accept_files`]: each file gets back what it held, if it still
/// holds what the accept wrote. One changed since (the agent revising the
/// package, the user's editor), or gone, is theirs now: it is left as it
/// is. What stays: a change that writes the very same content is taken for
/// the accept's.
fn put_back(prior: &[Prior]) {
    for p in prior.iter().rev() {
        if let Some(wrote) = &p.wrote {
            match read_bytes(&p.path) {
                Ok(now) if now == *wrote => {}
                _ => {
                    tracing::warn!(file = %p.path.display(), "a file an accept wrote changed since: left as it is");
                    continue;
                }
            }
        }
        let undone = match &p.old {
            Some(bytes) => std::fs::write(&p.path, bytes),
            None => std::fs::remove_file(&p.path).or_else(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(err)
                }
            }),
        };
        if let Err(err) = undone {
            tracing::warn!(file = %p.path.display(), error = %err, "an accept that failed cannot be undone");
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DecideBody {
    status: String,
}

pub async fn decide(
    State(state): State<AppState>,
    Path((id, rid)): Path<(String, String)>,
    ApiJson(body): ApiJson<DecideBody>,
) -> ApiResult<Json<Value>> {
    let accept = match body.status.as_str() {
        "accepted" => true,
        "rejected" => false,
        _ => return Err(ApiErr::bad_request("status must be accepted or rejected")),
    };
    let e = editable(&state, &id)?;
    let mut r = revisions_of(&e.dir)
        .into_iter()
        .find(|r| r.id == rid)
        .ok_or_else(|| ApiErr::not_found("revision not found"))?;
    if r.status != "pending" {
        return Err(ApiErr::new(
            StatusCode::CONFLICT,
            od_contracts::ErrorCode::Conflict,
            format!("the revision is {}", r.status),
        ));
    }
    r.status = body.status.clone();
    r.updated_at = od_store::now_ms();
    let dir = e.dir.clone();
    let out = r.clone();
    write_and_reload(&state, move || decide_on_disk(&dir, r, accept)).await?;
    Ok(Json(
        json!({ "revision": Revision { files: Vec::new(), ..out } }),
    ))
}

/// Writes decision `r` into package `dir`, and the revision's files when
/// `accept`, under the lock a Save of DESIGN.md compares and writes under:
/// an accept never lands between a Save's compare and its write.
fn decide_on_disk(dir: &std::path::Path, r: Revision, accept: bool) -> ApiResult<()> {
    let _one = design_md_lock();
    let prior = if accept {
        accept_files(dir, &r.files)?
    } else {
        Vec::new()
    };
    let decided = write_revision(
        dir,
        &Revision {
            files: Vec::new(),
            ..r
        },
    );
    // Still pending: the package goes back to what it was, so a
    // Reject after this leaves none of the revision in it.
    if decided.is_err() {
        put_back(&prior);
    }
    io(decided)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviseBody {
    feedback: String,
    #[serde(default)]
    section: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// The revision prompt: the reviewer's feedback, applied to the copies of
/// `DESIGN.md` and `tokens.css` in the working folder.
#[must_use]
pub fn revision_prompt(title: &str, feedback: &str, section: Option<&str>) -> String {
    let scope = section.map_or_else(
        || "the design system".to_owned(),
        |s| format!("the section \"{s}\" of the design system"),
    );
    format!(
        "# Design system revision\n\n\
         The current folder holds `DESIGN.md` and `tokens.css` of the design system **{title}**. \
         A reviewer marked {scope} as needing work:\n\n> {feedback}\n\n\
         Edit those two files in place to address the feedback. Keep every `## ` section and every \
         token that is not affected; keep `tokens.css` a single `:root {{ … }}` block. \
         Do not create other files and do not ask questions."
    )
}

/// Copies the package's [`REVISABLE`] files from `dir` into the working
/// folder `work`; one missing, not a regular file or too large is left out.
fn copy_revisable(dir: &std::path::Path, work: &std::path::Path) {
    for f in REVISABLE {
        if let Ok(bytes) = inside_io(dir, f).and_then(read_bytes) {
            let _ = std::fs::write(work.join(f), bytes);
        }
    }
}

pub async fn revise(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<ReviseBody>,
) -> ApiResult<impl IntoResponse> {
    let e = editable(&state, &id)?;
    let feedback = body.feedback.trim().to_owned();
    if feedback.is_empty() {
        return Err(ApiErr::bad_request("feedback is required"));
    }
    let section = body
        .section
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let mut r = new_revision("agent", Some(feedback.clone()), section.clone());
    let work = io(tempfile::Builder::new().prefix("od-ds-revision-").tempdir())?;
    // Read as every package file is (`read_bytes`: no FIFO wait, no
    // link at its name, bounded), then written into the working folder.
    let (from, to) = (e.dir.clone(), work.path().to_path_buf());
    crate::spawn::spawn_blocking_in_span(move || copy_revisable(&from, &to))
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let prompt = revision_prompt(&e.summary.title, &feedback, section.as_deref());
    let launched = launch(
        &state,
        body.agent_id.as_deref(),
        body.model,
        work.path().to_path_buf(),
        prompt,
    )
    .await?;
    r.run_id = Some(launched.run_id.clone());
    let dir = e.dir.clone();
    io(write_revision(&dir, &r))?;
    let out = r.clone();
    crate::spawn::spawn_in_span(async move {
        let outcome = launched.handle.await;
        let ok = matches!(&outcome, Ok(o) if o.status == RunStatus::Succeeded);
        r.updated_at = od_store::now_ms();
        if ok {
            r.files = REVISABLE
                .iter()
                .filter_map(|n| {
                    inside_io(work.path(), n)
                        .and_then(read_text)
                        .ok()
                        .map(|content| RevisionFile {
                            name: (*n).to_owned(),
                            content,
                        })
                })
                .collect();
            r.status = "pending".to_owned();
        } else {
            r.status = "failed".to_owned();
            r.error = Some(match outcome {
                Ok(o) => format!("the agent run {}", o.status.as_str()),
                Err(err) => err.to_string(),
            });
        }
        if let Err(err) = write_revision(&dir, &r) {
            tracing::warn!(error = %err, "could not save a design system revision");
        }
        drop(work);
    });
    Ok((StatusCode::ACCEPTED, Json(json!({ "revision": out }))))
}

/// A file system result, its error sorted by [`ApiErr::io`]: a refused
/// write reads as one, not as "try again" (QA-106).
fn io<T>(r: std::io::Result<T>) -> ApiResult<T> {
    r.map_err(ApiErr::io)
}

/// An agent run in `cwd`, outside any project, streamed like any run.
pub(crate) struct Launched {
    pub run_id: String,
    pub handle: tokio::task::JoinHandle<od_runtime::RunOutcome>,
}

pub(crate) async fn launch(
    state: &AppState,
    agent_id: Option<&str>,
    model: Option<String>,
    cwd: PathBuf,
    prompt: String,
) -> ApiResult<Launched> {
    let agent = agent_id.unwrap_or("claude");
    let def = od_runtime::def::find(agent).ok_or_else(|| {
        ApiErr::new(
            StatusCode::BAD_REQUEST,
            od_contracts::ErrorCode::AgentNotFound,
            format!("unknown agent {agent:?}"),
        )
    })?;
    let permission = state
        .store
        .data_dir()
        .app_setting("agent_permission_mode")
        .and_then(|v| v.as_str().map(PermissionMode::from_setting))
        .unwrap_or_default();
    let run_id = uuid::Uuid::new_v4().to_string();
    let (run, _) = state
        .runs
        .registry
        .create_or_reuse(RunMeta {
            run_id: run_id.clone(),
            agent_id: def.id.to_owned(),
            project_id: None,
            conversation_id: None,
            assistant_message_id: None,
            client_request_id: None,
            request_hash: None,
            created_at: od_runs::now_ms(),
            prompt: None,
        })
        .map_err(|e| ApiErr::internal(e.to_string()))?;
    let engine = &state.runs.engine;
    let data_root = state.store.data_dir().root().to_path_buf();
    let od_bin = std::env::current_exe().ok();
    let env = agent_env(
        def,
        &engine.config().base_env,
        &engine.config().path,
        &RunEnv {
            data_dir: Some(&data_root),
            daemon_url: state.config.daemon_url.as_deref(),
            tool_token: None,
            project_id: None,
            project_dir: Some(&cwd),
            od_bin: od_bin.as_deref(),
            run_id: Some(&run_id),
            hyperframes_bin: None,
        },
    );
    let handle = engine.launch(LaunchSpec {
        run: Arc::clone(&run),
        def,
        cwd,
        prompt,
        fallback_prompt: None,
        model: model.filter(|m| !m.trim().is_empty() && m != "default"),
        reasoning: None,
        service_tier: None,
        // Settings ▸ Advanced: a user-chosen agent executable applies here too.
        bin: crate::routes::manage::settings::custom_bin(&data_root, def.id),
        extra_dirs: Vec::new(),
        resume_session_id: None,
        permission,
        env,
        project_id: None,
        byok: None,
        mcp: McpLaunch::default(),
        next_step_key: None,
    });
    Ok(Launched { run_id, handle })
}

// ---------------------------------------------------------------- workspace

pub async fn workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    let base = e.dir.to_string_lossy().into_owned();
    let title = e.summary.title.clone();
    let ds_id = e.summary.id.clone();
    let (project, conversation_id) = state
        .blocking(move |s| {
            // One workspace per package: reuse the project already bound to it.
            if let Some(p) = s.list_projects()?.into_iter().find(|p| {
                p.metadata
                    .as_ref()
                    .is_some_and(|m| m.base_dir.as_deref() == Some(base.as_str()))
            }) {
                return Ok((p, String::new()));
            }
            s.create_project(CreateProjectRequest {
                name: title,
                design_system_id: Some(ds_id),
                metadata: Some(ProjectMetadata {
                    base_dir: Some(base),
                    imported_from: Some("design-system".to_owned()),
                    entry_file: Some("components.html".to_owned()),
                    ..ProjectMetadata::default()
                }),
                ..CreateProjectRequest::default()
            })
        })
        .await?;
    Ok(Json(
        json!({ "project": project, "conversationId": conversation_id }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_asset(raw: &str) -> Option<String> {
        safe_asset_name(raw)
    }

    #[test]
    fn asset_names_are_sanitised_and_typed() {
        assert_eq!(
            new_asset("../../Logo Mark.svg").as_deref(),
            Some("Logo-Mark.svg")
        );
        assert_eq!(new_asset("Inter.woff2").as_deref(), Some("Inter.woff2"));
        assert_eq!(new_asset(".hidden.png"), None);
        assert_eq!(new_asset("run.sh"), None);
        assert_eq!(asset_kind("brand-logo.png"), "logo");
        assert_eq!(asset_kind("Inter.woff2"), "font");
        assert_eq!(asset_kind("hero.jpg"), "image");
    }

    /// QA-107: Vietnamese names keep their letters, so `màu.png` and
    /// `mẫu.png` stay two assets; a name too long for the disk is
    /// shortened (QA-106); two files one name in a batch,
    /// NFC or NFD, any case, are refused.
    #[test]
    fn asset_names_keep_their_letters_and_never_collide_unseen() {
        let mau = new_asset("màu.png").unwrap();
        let mau2 = new_asset("mẫu.png").unwrap();
        assert_eq!((mau.as_str(), mau2.as_str()), ("màu.png", "mẫu.png"));
        assert_eq!(
            new_asset("logo chính.PNG").as_deref(),
            Some("logo-chính.PNG")
        );
        let long = new_asset(&format!("{}.png", "ảnh-".repeat(100))).unwrap();
        assert!(long.len() <= od_store::paths::MAX_UPLOAD_NAME_BYTES && long.ends_with(".png"));
        assert!(refuse_same_name([("màu.png", mau.as_str()), ("mẫu.png", mau2.as_str())]).is_ok());
        let err =
            refuse_same_name([("Logo.png", "Logo.png"), ("a/logo.png", "logo.png")]).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.message.contains("Logo.png") && err.message.contains("a/logo.png"),
            "{}",
            err.message
        );
        // `mẫu` composed (NFC) and decomposed (NFD): one file on APFS.
        let nfd = "ma\u{302}\u{303}u.png";
        assert!(refuse_same_name([("mẫu.png", "mẫu.png"), (nfd, nfd)]).is_err());
        assert!(refuse_same_name([("MẪU.png", "MẪU.png"), (nfd, nfd)]).is_err());
    }

    /// Where the disk keeps `Logo.png` and `logo.png` apart, a name sent
    /// again replaces itself, not the first name that only folds to it.
    #[test]
    fn the_closest_name_is_the_one_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Logo.png"), b"a").unwrap();
        let none = BTreeMap::new();
        // Folding still finds `Logo.png` when it is the only candidate.
        assert_eq!(
            replaced_asset(dir.path(), &none, "logo.png").as_deref(),
            Some("Logo.png")
        );
        std::fs::write(dir.path().join("logo.png"), b"b").unwrap();
        let both = std::fs::read_dir(dir.path()).unwrap().count() == 2;
        if both {
            // A case-sensitive disk (Linux CI): the exact name wins.
            assert_eq!(
                replaced_asset(dir.path(), &none, "logo.png").as_deref(),
                Some("logo.png")
            );
            assert_eq!(
                replaced_asset(dir.path(), &none, "Logo.png").as_deref(),
                Some("Logo.png")
            );
        }
    }

    /// QA-109: the sent names are read and written in the package only: a
    /// link planted at the file (an agent editing the package) is neither
    /// read through nor written through, only replaced.
    #[cfg(unix)]
    #[test]
    fn sent_names_never_go_through_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let (pkg, outside) = (tmp.path().join("pkg"), tmp.path().join("outside.json"));
        std::fs::create_dir(&pkg).unwrap();
        std::fs::write(&outside, br#"{"_.png": "secret.png"}"#).unwrap();
        std::os::unix::fs::symlink(&outside, pkg.join(SENT_NAMES)).unwrap();
        assert!(read_sent_names(&pkg).is_empty(), "read through a link");
        let names = BTreeMap::from([("_.png".to_owned(), "🎨.png".to_owned())]);
        write_sent_names(&pkg, &names).unwrap();
        assert_eq!(
            std::fs::read(&outside).unwrap(),
            br#"{"_.png": "secret.png"}"#,
            "written through a link"
        );
        assert_eq!(read_sent_names(&pkg), names);
        write_sent_names(&pkg, &BTreeMap::new()).unwrap();
        assert!(!pkg.join(SENT_NAMES).exists());
        let left: Vec<_> = std::fs::read_dir(&pkg).unwrap().collect();
        assert!(left.is_empty(), "a temporary file left");
    }

    /// An asset already on disk is named as it is, never cleaned or
    /// shortened again: a 220-byte name kept whole stays itself.
    #[test]
    fn existing_asset_names_are_taken_as_they_are() {
        let old = format!("{}.png", "o".repeat(216));
        assert_eq!(existing_asset_name(&old), Some(old.as_str()));
        assert_eq!(existing_asset_name("logo (1).png"), Some("logo (1).png"));
        assert_eq!(existing_asset_name(".hidden.png"), None);
        assert_eq!(existing_asset_name("a/b.png"), None);
        assert_eq!(existing_asset_name("a\\b.png"), None);
        assert_eq!(existing_asset_name("run.sh"), None);
        let c: Vec<String> = candidate_names("a.png").take(3).collect();
        assert_eq!(c, ["a.png", "a-1.png", "a-2.png"]);
    }

    #[test]
    fn package_paths_stay_inside() {
        // A real folder: on Windows `/ds/tea` is not absolute.
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert!(inside(d, "assets/logo.svg").is_ok());
        assert!(inside(d, "../other/DESIGN.md").is_err());
        assert!(inside(d, ".open-design/project.json").is_err());
        assert!(inside(d, "").is_err());
    }

    fn revision(files: &[(&str, &str)]) -> Vec<RevisionFile> {
        files
            .iter()
            .map(|(name, content)| RevisionFile {
                name: (*name).to_owned(),
                content: (*content).to_owned(),
            })
            .collect()
    }

    /// The decision cannot be saved after an accept wrote the package: each
    /// file goes back, but one the agent wrote in between is left as is.
    #[test]
    fn an_accept_undone_leaves_a_file_changed_since() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("DESIGN.md"), "old").unwrap();
        let files = revision(&[("DESIGN.md", "new"), ("tokens.css", ":root {}")]);
        let prior = accept_files(d, &files).unwrap();
        std::fs::write(d.join("tokens.css"), "the agent's").unwrap();
        put_back(&prior);
        assert_eq!(std::fs::read_to_string(d.join("DESIGN.md")).unwrap(), "old");
        assert_eq!(
            std::fs::read_to_string(d.join("tokens.css")).unwrap(),
            "the agent's",
            "a file the agent wrote deleted"
        );

        let prior = accept_files(d, &files).unwrap();
        std::fs::write(d.join("DESIGN.md"), "the agent's").unwrap();
        std::fs::remove_file(d.join("tokens.css")).unwrap();
        put_back(&prior);
        assert_eq!(
            std::fs::read_to_string(d.join("DESIGN.md")).unwrap(),
            "the agent's",
            "the agent's change written over"
        );
        assert!(
            !d.join("tokens.css").exists(),
            "a file deleted since put back"
        );
    }

    /// An accept waits for the lock a Save of DESIGN.md compares and writes
    /// under (PR #349 review): one that wrote while a Save held it would
    /// land between the Save's compare and its write, and the Save would
    /// write over the revision just accepted, no 412.
    #[test]
    fn an_accept_waits_for_a_save_of_design_md() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().to_path_buf();
        std::fs::write(d.join("DESIGN.md"), "old").unwrap();
        let r = Revision {
            id: "r1".to_owned(),
            kind: "agent".to_owned(),
            status: "accepted".to_owned(),
            feedback: None,
            section: None,
            run_id: None,
            error: None,
            files: revision(&[("DESIGN.md", "accepted")]),
            created_at: 0,
            updated_at: 0,
        };
        let save = design_md_lock();
        let accept = {
            let d = d.clone();
            std::thread::spawn(move || decide_on_disk(&d, r, true).is_ok())
        };
        // Nothing to wait on but time: what is checked is that nothing
        // happens while the Save holds the lock.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(
            std::fs::read_to_string(d.join("DESIGN.md")).unwrap(),
            "old",
            "the accept wrote while a Save held the lock"
        );
        assert!(!accept.is_finished(), "the accept did not wait");
        drop(save);
        assert!(accept.join().unwrap());
        assert_eq!(
            std::fs::read_to_string(d.join("DESIGN.md")).unwrap(),
            "accepted"
        );
    }

    /// A package an agent edits in place can hold a FIFO where a file is
    /// read (DESIGN.md, tokens.css, a revision, the sent names):
    /// every read comes back, reading it as nothing, and an accept refuses
    /// to write into it. A file past the limit is not read either.
    #[cfg(unix)]
    #[test]
    fn package_reads_never_wait_on_a_fifo() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().to_path_buf();
        let fifo = |p: &std::path::Path| {
            nix::unistd::mkfifo(p, nix::sys::stat::Mode::from_bits_truncate(0o644)).unwrap();
        };
        for name in ["DESIGN.md", "tokens.css", SENT_NAMES] {
            fifo(&d.join(name));
        }
        let rev = d.join(REVISIONS_DIR).join("r1");
        std::fs::create_dir_all(&rev).unwrap();
        fifo(&rev.join(REVISION_FILE));
        let big = "x".repeat(MAX_ASSET_BYTES + 1);
        std::fs::write(d.join("big.css"), &big).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let read_all = (
                read(&d, "tokens.css"),
                read(&d, "big.css"),
                revisions_of(&d).len(),
                read_sent_names(&d).len(),
                inside_io(&d, "DESIGN.md").and_then(read_bytes).is_err(),
            );
            let accepted = accept_files(&d, &revision(&[("DESIGN.md", "new")]));
            let _ = tx.send((read_all, accepted.is_err()));
        });
        let (read_all, accepted) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a package read never came back (blocked on a FIFO?)");
        assert_eq!(read_all, (String::new(), String::new(), 0, 0, true));
        assert!(accepted, "an accept wrote into a FIFO");
    }

    /// An accept failing on a file it cannot read (so not listed to put
    /// back) puts back those it wrote before.
    #[test]
    fn an_accept_failing_on_a_file_it_cannot_read_puts_back_those_before() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("DESIGN.md"), "old").unwrap();
        // Not a file: it cannot be read nor written.
        std::fs::create_dir(d.join("tokens.css")).unwrap();
        let files = revision(&[("DESIGN.md", "new"), ("tokens.css", ":root {}")]);
        assert!(accept_files(d, &files).is_err());
        assert_eq!(std::fs::read_to_string(d.join("DESIGN.md")).unwrap(), "old");
    }

    /// A write failing half way (a full disk) leaves a part of the file:
    /// it gets its old content back too, as do those written before.
    #[test]
    fn an_accept_failing_half_way_through_a_write_puts_that_file_back() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("DESIGN.md"), "old").unwrap();
        std::fs::write(d.join("tokens.css"), ":root { --old: 1 }").unwrap();
        let files = revision(&[("DESIGN.md", "new"), ("tokens.css", ":root { --new: 2 }")]);
        let res = accept_files_with(d, &files, |path, bytes| {
            if path.ends_with("tokens.css") {
                std::fs::write(path, &bytes[..bytes.len() / 2])?;
                return Err(std::io::Error::other("no space left on device"));
            }
            std::fs::write(path, bytes)
        });
        assert!(res.is_err());
        assert_eq!(std::fs::read_to_string(d.join("DESIGN.md")).unwrap(), "old");
        assert_eq!(
            std::fs::read_to_string(d.join("tokens.css")).unwrap(),
            ":root { --old: 1 }",
            "the half written file left in the package"
        );
    }

    #[test]
    fn revision_prompt_quotes_the_feedback_and_scope() {
        let p = revision_prompt("Tea", "Warmer greens", Some("Color"));
        assert!(p.contains("**Tea**"));
        assert!(p.contains("> Warmer greens"));
        assert!(p.contains("the section \"Color\""));
    }
}
