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
//! - `PUT    /api/design-systems/:id/design-md` `{body}`.
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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Component, PathBuf};
use std::sync::Arc;

const REVISIONS_DIR: &str = "revisions";
const ASSETS_DIR: &str = "assets";
const REVISION_FILE: &str = "revision.json";
const MAX_ASSET_BYTES: usize = 20 * 1024 * 1024;
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
    design_kit::a2_defaults(&std::fs::read_to_string(path).unwrap_or_default())
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
    inside(dir, rel)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.message))
}

fn read(dir: &std::path::Path, name: &str) -> String {
    inside_io(dir, name)
        .and_then(std::fs::read_to_string)
        .unwrap_or_default()
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
        let staging = tempfile::tempdir().map_err(|e| ApiErr::internal(e.to_string()))?;
        std::fs::write(staging.path().join("DESIGN.md"), design)
            .and_then(|()| std::fs::write(staging.path().join("tokens.css"), tokens))
            .map_err(|e| ApiErr::internal(e.to_string()))?;
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
                serde_json::from_str(&file(REVISION_FILE).and_then(std::fs::read_to_string).ok()?)
                    .ok()?;
            r.files = REVISABLE
                .iter()
                .filter_map(|n| {
                    file(n)
                        .and_then(std::fs::read_to_string)
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

pub async fn kit(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Kit>> {
    let e = entry(&state, &id)?;
    let a2 = a2(&state);
    let kit = tokio::task::spawn_blocking(move || {
        let body = inside_io(&e.dir, "DESIGN.md")
            .and_then(std::fs::read_to_string)
            .unwrap_or(e.body.clone());
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
    Ok(Json(kit))
}

pub async fn showcase(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let e = entry(&state, &id)?;
    let components = inside(&e.dir, "components.html")?;
    let html = match tokio::fs::read(&components).await {
        Ok(bytes) => bytes,
        Err(_) => {
            design_kit::showcase_html(&e.summary.title, &read(&e.dir, "tokens.css")).into_bytes()
        }
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
    let bytes = tokio::fs::read(&target)
        .await
        .map_err(|_| ApiErr::not_found("file not found"))?;
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
    ApiJson(body): ApiJson<DesignMdBody>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    if body.body.trim().is_empty() {
        return Err(ApiErr::bad_request("DESIGN.md cannot be empty"));
    }
    write_and_reload(&state, move || {
        std::fs::write(inside(&e.dir, "DESIGN.md")?, body.body)
            .map_err(|err| ApiErr::internal(err.to_string()))
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Merges `patch` into the package's `metadata.json`.
fn patch_metadata(dir: &std::path::Path, patch: &Value) -> std::io::Result<()> {
    let path = inside_io(dir, "metadata.json")?;
    let mut meta = std::fs::read_to_string(&path)
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
        patch_metadata(&e.dir, &json!({ "status": status }))
            .map_err(|err| ApiErr::internal(err.to_string()))
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
        std::fs::create_dir_all(&root).map_err(|err| ApiErr::internal(err.to_string()))?;
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
        // The manifest names its folder; keep it valid for the new one.
        let manifest_path = dest.join("manifest.json");
        if let Some(mut m) = std::fs::read_to_string(&manifest_path)
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
        .map_err(|err| ApiErr::internal(err.to_string()))?;
        Ok(format!(
            "{}{folder}",
            od_content::design_systems::USER_ID_PREFIX
        ))
    })
    .await?;
    Ok(Json(json!({ "id": new_id })))
}

/// Package files for the archive: everything visible except revisions.
fn package_files(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
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
    let bytes = tokio::task::spawn_blocking(move || {
        let files = package_files(&e.dir)
            .into_iter()
            .map(|(rel, p)| (format!("{folder}/{rel}"), p))
            .collect::<Vec<_>>();
        od_export::archive::write_zip(Vec::new(), &files)
    })
    .await
    .map_err(|err| ApiErr::internal(err.to_string()))?
    .map_err(|err| ApiErr::internal(err.to_string()))?;
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

fn safe_asset_name(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next()?.trim();
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let ok = !clean.is_empty() && !clean.starts_with('.') && clean.contains('.');
    let allowed = [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".woff2", ".woff", ".ttf", ".otf",
    ];
    let lower = clean.to_ascii_lowercase();
    (ok && allowed.iter().any(|e| lower.ends_with(e))).then_some(clean)
}

pub async fn upload_assets(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    let mut saved = Vec::new();
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
        tokio::fs::create_dir_all(inside(&e.dir, ASSETS_DIR)?)
            .await
            .map_err(|err| ApiErr::internal(err.to_string()))?;
        tokio::fs::write(inside(&e.dir, &format!("{ASSETS_DIR}/{name}"))?, &bytes)
            .await
            .map_err(|err| ApiErr::internal(err.to_string()))?;
        saved.push(json!({ "name": name, "kind": asset_kind(&name), "size": bytes.len() }));
    }
    if saved.is_empty() {
        return Err(ApiErr::bad_request("no files in upload"));
    }
    Ok(Json(json!({ "assets": saved })))
}

pub async fn delete_asset(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let e = editable(&state, &id)?;
    let name = safe_asset_name(&name).ok_or_else(|| ApiErr::bad_request("invalid asset"))?;
    tokio::fs::remove_file(inside(&e.dir, &format!("{ASSETS_DIR}/{name}"))?)
        .await
        .map_err(|_| ApiErr::not_found("asset not found"))?;
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
    tokio::task::spawn_blocking(move || write_revision(&dir, &saved))
        .await
        .map_err(|err| ApiErr::internal(err.to_string()))?
        .map_err(|err| ApiErr::internal(err.to_string()))?;
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
    write_and_reload(&state, move || {
        if accept {
            for f in &r.files {
                if REVISABLE.contains(&f.name.as_str()) {
                    std::fs::write(inside(&dir, &f.name)?, &f.content)
                        .map_err(|err| ApiErr::internal(err.to_string()))?;
                }
            }
        }
        write_revision(
            &dir,
            &Revision {
                files: Vec::new(),
                ..r
            },
        )
        .map_err(|err| ApiErr::internal(err.to_string()))
    })
    .await?;
    Ok(Json(
        json!({ "revision": Revision { files: Vec::new(), ..out } }),
    ))
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
    let work = internal(tempfile::Builder::new().prefix("od-ds-revision-").tempdir())?;
    for f in REVISABLE {
        if let Ok(from) = inside(&e.dir, f) {
            let _ = std::fs::copy(from, work.path().join(f));
        }
    }
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
    internal(write_revision(&dir, &r))?;
    let out = r.clone();
    tokio::spawn(async move {
        let outcome = launched.handle.await;
        let ok = matches!(&outcome, Ok(o) if o.status == RunStatus::Succeeded);
        r.updated_at = od_store::now_ms();
        if ok {
            r.files = REVISABLE
                .iter()
                .filter_map(|n| {
                    inside_io(work.path(), n)
                        .and_then(std::fs::read_to_string)
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

fn internal<T, E: std::fmt::Display>(r: Result<T, E>) -> ApiResult<T> {
    r.map_err(|e| ApiErr::internal(e.to_string()))
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

    #[test]
    fn asset_names_are_sanitised_and_typed() {
        assert_eq!(
            safe_asset_name("../../Logo Mark.svg").as_deref(),
            Some("Logo-Mark.svg")
        );
        assert_eq!(
            safe_asset_name("Inter.woff2").as_deref(),
            Some("Inter.woff2")
        );
        assert_eq!(safe_asset_name(".hidden.png"), None);
        assert_eq!(safe_asset_name("run.sh"), None);
        assert_eq!(asset_kind("brand-logo.png"), "logo");
        assert_eq!(asset_kind("Inter.woff2"), "font");
        assert_eq!(asset_kind("hero.jpg"), "image");
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

    #[test]
    fn revision_prompt_quotes_the_feedback_and_scope() {
        let p = revision_prompt("Tea", "Warmer greens", Some("Color"));
        assert!(p.contains("**Tea**"));
        assert!(p.contains("> Warmer greens"));
        assert!(p.contains("the section \"Color\""));
    }
}
