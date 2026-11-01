//! Preview tools (spec 1.2). Mounted with one `merge` in [`crate::router`].
//!
//! - `POST /api/artifacts/lint`: anti-slop lint of posted HTML.
//! - `GET /api/projects/:id/lint/<path>`: the same for a project file.
//! - `POST /api/projects/:id/source-edits`: style, text, attribute, element
//!   HTML and removal edits made in the
//!   preview, written back to the file as a manual version.
//! - `GET /api/runs/:id/deliverable`: did the run deliver its entry file?

use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use od_contracts::{
    DeliverableResponse, DeliverableValidation, ErrorCode, FileKind, LintRequest, LintResponse,
    Project, ProjectFile, ProjectKind, RunStatus, SourceEditRequest, SourceEditResponse,
};
use od_preview::lint::{lint_artifact, render_for_agent};
use od_preview::source_patch::{PatchError, apply_edits};
use od_store::Store;
use std::collections::HashSet;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

/// Label of the version an edit in the preview creates.
const DEFAULT_EDIT_LABEL: &str = "Edited in preview";
/// Edits accepted in one request.
const MAX_EDITS: usize = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/artifacts/lint", post(lint_posted))
        .route("/api/projects/{id}/lint/{*path}", get(lint_file))
        .route("/api/projects/{id}/source-edits", post(source_edits))
        .route("/api/runs/{id}/deliverable", get(deliverable))
}

fn lint_response(html: &str) -> LintResponse {
    let findings = lint_artifact(html);
    let agent_message = render_for_agent(&findings);
    LintResponse {
        findings,
        agent_message,
    }
}

/// Runs `f` on the blocking pool with errors already mapped to HTTP.
pub(crate) async fn blocking<T, F>(state: &AppState, f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Store) -> ApiResult<T> + Send + 'static,
{
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?
}

async fn lint_posted(ApiJson(req): ApiJson<LintRequest>) -> ApiResult<ApiJson<LintResponse>> {
    if req.html.is_empty() {
        return Err(ApiErr::bad_request("html required"));
    }
    let res = tokio::task::spawn_blocking(move || lint_response(&req.html))
        .await
        .map_err(|e| ApiErr::internal(format!("worker failed: {e}")))?;
    Ok(ApiJson(res))
}

async fn lint_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> ApiResult<ApiJson<LintResponse>> {
    let res = blocking(&state, move |s| {
        let (file, bytes) = s.read_file(&id, &path)?;
        if file.kind != FileKind::Html {
            return Err(ApiErr::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                ErrorCode::UnsupportedMediaType,
                "only HTML files are linted",
            ));
        }
        Ok(lint_response(&String::from_utf8_lossy(&bytes)))
    })
    .await?;
    Ok(ApiJson(res))
}

pub(crate) fn patch_err(err: PatchError) -> ApiErr {
    match err {
        PatchError::InvalidSelector(_)
        | PatchError::InvalidStyle(_)
        | PatchError::Empty
        | PatchError::InvalidAttribute(_)
        | PatchError::ProtectedAttribute(_)
        | PatchError::InvalidEdit(_)
        | PatchError::InvalidHtml(_)
        | PatchError::NotRemovable(_)
        | PatchError::LastElement => ApiErr::bad_request(err.to_string()),
        PatchError::Rewrite(_) => ApiErr::internal(err.to_string()),
        PatchError::NotFound(_)
        | PatchError::Ambiguous { .. }
        | PatchError::NotTextOnly
        | PatchError::TextMismatch { .. }
        | PatchError::TagMismatch { .. }
        | PatchError::NoEndTag => {
            ApiErr::new(StatusCode::CONFLICT, ErrorCode::Conflict, err.to_string())
        }
    }
}

async fn source_edits(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<SourceEditRequest>,
) -> ApiResult<ApiJson<SourceEditResponse>> {
    if req.edits.is_empty() || req.edits.len() > MAX_EDITS {
        return Err(ApiErr::bad_request(format!(
            "between 1 and {MAX_EDITS} edits required"
        )));
    }
    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .unwrap_or(DEFAULT_EDIT_LABEL)
        .chars()
        .take(120)
        .collect::<String>();
    let (file, version, previous) = blocking(&state, move |s| {
        s.edit_html_tracked(&id, &req.file, &label, |html| {
            apply_edits(html, &req.edits).map_err(patch_err)
        })
    })
    .await?;
    Ok(ApiJson(SourceEditResponse {
        file,
        version,
        previous_version_id: Some(previous.id),
    }))
}

async fn deliverable(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<DeliverableResponse>> {
    let run = state.runs.registry.get(&id).ok_or_else(|| {
        ApiErr::new(
            StatusCode::NOT_FOUND,
            ErrorCode::RunNotFound,
            "run not found",
        )
    })?;
    let end = run.end_payload();
    let project_id = run.meta().project_id.clone();
    let run_id = id;
    let res = blocking(&state, move |s| {
        let succeeded = end
            .as_ref()
            .is_some_and(|e| e.status == RunStatus::Succeeded);
        let mut touched = end.map(|e| e.artifact_paths).unwrap_or_default();
        // What its media tasks rendered (`od media generate`) is the run's
        // output too: a Motion run delivers the MP4.
        for path in crate::routes::w3::media_tasks::run_outputs(s, &run_id) {
            if !touched.contains(&path) {
                touched.push(path);
            }
        }
        let project = project_id.and_then(|p| s.get_project(&p).ok());
        let listing = project.as_ref().and_then(|p| {
            let dir = s.project_root(p).ok()?.dir;
            let files = s.list_files(&p.id).ok()?;
            Some((dir, files))
        });
        let res = validate(&RunFacts {
            succeeded,
            touched: &touched,
            project: project.as_ref(),
            listing: listing.as_ref().map(|(d, f)| (d.as_path(), f.as_slice())),
        });
        // Local log only: how often a run writes another page than its
        // entry (the Studio then opens that page instead of warning).
        let other_page = wrote_other_page(
            &touched,
            listing.as_ref().map(|(d, _)| d.as_path()),
            res.entry_file.as_deref(),
        );
        tracing::info!(validation = ?res.validation, other_page, "deliverable checked");
        Ok(res)
    })
    .await?;
    Ok(ApiJson(res))
}

/// What the deliverable check reads about a finished run.
struct RunFacts<'a> {
    succeeded: bool,
    /// Absolute (or project-relative) paths the run wrote.
    touched: &'a [String],
    project: Option<&'a Project>,
    /// Project folder and its files; `None` when it cannot be listed.
    listing: Option<(&'a FsPath, &'a [ProjectFile])>,
}

fn verdict(validation: DeliverableValidation, entry: Option<&ProjectFile>) -> DeliverableResponse {
    DeliverableResponse {
        valid: validation == DeliverableValidation::Valid,
        validation,
        entry_file: entry.map(|f| f.path.clone()),
        artifact_kind: entry.map(|f| f.kind),
    }
}

/// The file kinds a project delivers as its entry; `None` accepts any.
fn accepted_kinds(project: &Project) -> Option<&'static [FileKind]> {
    let meta = project.metadata.as_ref()?;
    let hyperframes = meta.intent.as_deref() == Some("hyperframes")
        || meta.extra.get("videoModel").and_then(|v| v.as_str()) == Some("hyperframes");
    Some(match meta.kind? {
        ProjectKind::Prototype | ProjectKind::Template => &[FileKind::Html],
        ProjectKind::Deck => &[FileKind::Html, FileKind::Presentation, FileKind::Pdf],
        ProjectKind::Image => &[FileKind::Image],
        ProjectKind::Video if hyperframes => &[FileKind::Video, FileKind::Html],
        ProjectKind::Video => &[FileKind::Video],
        ProjectKind::Audio => &[FileKind::Audio],
        ProjectKind::Other => return None,
    })
}

/// A relative path with no empty, `.` or `..` segment.
fn safe_relative(path: &str) -> Option<String> {
    let p = path.trim().replace('\\', "/");
    let ok = !p.is_empty()
        && !p.starts_with('/')
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..");
    ok.then_some(p)
}

/// The entry when none is declared: root `index.html`, else the only root
/// HTML file, else the only file of an accepted kind, else the only file.
fn inferred_entry<'f>(
    files: &'f [ProjectFile],
    accepted: Option<&[FileKind]>,
) -> Option<&'f ProjectFile> {
    let only = |mut it: Vec<&'f ProjectFile>| (it.len() == 1).then(|| it.remove(0));
    files
        .iter()
        .find(|f| f.path == "index.html")
        .or_else(|| {
            only(
                files
                    .iter()
                    .filter(|f| !f.path.contains('/') && f.kind == FileKind::Html)
                    .collect(),
            )
        })
        .or_else(|| {
            accepted
                .and_then(|kinds| only(files.iter().filter(|f| kinds.contains(&f.kind)).collect()))
        })
        .or_else(|| only(files.iter().collect()))
}

/// `path` relative to `dir`, both resolved through symlinks (macOS temp
/// folders live under `/private`). `None` outside `dir`.
fn relative_to(dir: &FsPath, path: &str) -> Option<String> {
    let p = PathBuf::from(path);
    let abs = if p.is_absolute() { p } else { dir.join(p) };
    let canon = |x: &FsPath| std::fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
    let rel = canon(&abs).strip_prefix(canon(dir)).ok()?.to_path_buf();
    let rel = rel.to_string_lossy().replace('\\', "/");
    safe_relative(&rel)
}

/// Whether the run wrote an HTML page other than `entry`, comparing paths
/// relative to the project folder `dir` (`old/index.html` is not
/// `index.html`). A page outside the folder is another page.
fn wrote_other_page(touched: &[String], dir: Option<&FsPath>, entry: Option<&str>) -> bool {
    touched
        .iter()
        .filter(|t| t.ends_with(".html") || t.ends_with(".htm"))
        .any(|t| {
            let rel = dir.and_then(|d| relative_to(d, t));
            entry.is_none_or(|e| rel.as_deref() != Some(e))
        })
}

/// Whether the entry page links to a page the run wrote (a multi-page
/// prototype where this turn changed another page).
fn links_to_touched(dir: &FsPath, entry: &str, touched: &HashSet<String>) -> bool {
    let Ok(html) = std::fs::read_to_string(dir.join(entry)) else {
        return false;
    };
    let base = entry.rsplit_once('/').map_or("", |(d, _)| d);
    touched
        .iter()
        .filter(|t| t.ends_with(".html") || t.ends_with(".htm"))
        .any(|t| {
            let local = if base.is_empty() {
                t.as_str()
            } else {
                t.strip_prefix(base)
                    .and_then(|r| r.strip_prefix('/'))
                    .unwrap_or(t)
            };
            [
                format!("href=\"{local}"),
                format!("href=\"./{local}"),
                format!("href='{local}"),
            ]
            .iter()
            .any(|needle| html.contains(needle.as_str()))
        })
}

/// The run-scoped deliverable check of the original app
/// (`run-deliverable-validation.ts`): the run succeeded, wrote files, and
/// one canonical entry of the right kind exists, was written by this run,
/// and can be read.
fn validate(facts: &RunFacts<'_>) -> DeliverableResponse {
    use DeliverableValidation as V;
    if !facts.succeeded {
        return verdict(V::NotSucceeded, None);
    }
    if facts.touched.is_empty() {
        return verdict(V::NoArtifact, None);
    }
    let (Some(project), Some((dir, files))) = (facts.project, facts.listing) else {
        return verdict(V::ProjectMissing, None);
    };
    let meta = project.metadata.as_ref();
    let accepted = accepted_kinds(project);
    let declared = meta
        .and_then(|m| m.entry_file.as_deref())
        .and_then(safe_relative);
    let motion: Vec<&ProjectFile> =
        if meta.and_then(|m| m.intent.as_deref()) == Some("motion-design") {
            files.iter().filter(|f| f.kind == FileKind::Html).collect()
        } else {
            Vec::new()
        };
    let selected = match &declared {
        Some(d) => files.iter().find(|f| &f.path == d),
        None if motion.len() == 1 => Some(motion[0]),
        None => inferred_entry(files, accepted),
    };
    let Some(entry) = selected else {
        return verdict(V::EntryMissing, None);
    };
    let touched: HashSet<String> = facts
        .touched
        .iter()
        .filter_map(|t| relative_to(dir, t))
        .collect();
    if !touched.contains(&entry.path) {
        let prototype = meta.and_then(|m| m.kind) == Some(ProjectKind::Prototype);
        let linked = prototype
            && entry.kind == FileKind::Html
            && links_to_touched(dir, &entry.path, &touched);
        if !linked {
            return verdict(V::EntryNotTouched, Some(entry));
        }
    }
    if accepted.is_some_and(|kinds| !kinds.contains(&entry.kind)) {
        return verdict(V::TypeMismatch, Some(entry));
    }
    let readable = relative_to(dir, &entry.path).is_some()
        && std::fs::File::open(dir.join(&entry.path))
            .and_then(|f| f.metadata())
            .is_ok_and(|m| m.is_file());
    if !readable {
        return verdict(V::EntryUnreadable, Some(entry));
    }
    verdict(V::Valid, Some(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_contracts::ProjectMetadata;

    fn file(path: &str, kind: FileKind) -> ProjectFile {
        serde_json::from_value(serde_json::json!({
            "name": path.rsplit('/').next().unwrap(),
            "path": path,
            "type": "file",
            "size": 1,
            "mtime": 0.0,
            "kind": kind,
            "mime": "text/html",
        }))
        .unwrap()
    }

    fn project(kind: Option<ProjectKind>, entry: Option<&str>) -> Project {
        serde_json::from_value(serde_json::json!({
            "id": "p", "name": "p", "skillId": null, "designSystemId": null,
            "createdAt": 0, "updatedAt": 0,
            "metadata": ProjectMetadata {
                kind,
                entry_file: entry.map(str::to_owned),
                ..ProjectMetadata::default()
            },
        }))
        .unwrap()
    }

    struct Fixture {
        dir: tempfile::TempDir,
        files: Vec<ProjectFile>,
    }

    impl Fixture {
        fn new(files: &[(&str, FileKind, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            for (path, _, body) in files {
                let p = dir.path().join(path);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, body).unwrap();
            }
            let files = files.iter().map(|(p, k, _)| file(p, *k)).collect();
            Self { dir, files }
        }

        fn abs(&self, rel: &str) -> String {
            self.dir.path().join(rel).to_string_lossy().into_owned()
        }

        fn check(&self, project: &Project, touched: &[String]) -> DeliverableResponse {
            validate(&RunFacts {
                succeeded: true,
                touched,
                project: Some(project),
                listing: Some((self.dir.path(), &self.files)),
            })
        }
    }

    use DeliverableValidation as V;

    #[test]
    fn run_gates_come_first() {
        let p = project(Some(ProjectKind::Prototype), None);
        let base = RunFacts {
            succeeded: false,
            touched: &[],
            project: Some(&p),
            listing: None,
        };
        assert_eq!(validate(&base).validation, V::NotSucceeded);
        let ok = RunFacts {
            succeeded: true,
            ..base
        };
        assert_eq!(validate(&ok).validation, V::NoArtifact);
        let t = ["/x/index.html".to_owned()];
        let missing = RunFacts { touched: &t, ..ok };
        assert_eq!(validate(&missing).validation, V::ProjectMissing);
    }

    #[test]
    fn prototype_entry_written_by_the_run_is_valid() {
        let fx = Fixture::new(&[
            ("index.html", FileKind::Html, "<h1>x</h1>"),
            ("a.css", FileKind::Code, "p{}"),
        ]);
        let p = project(Some(ProjectKind::Prototype), None);
        let res = fx.check(&p, &[fx.abs("index.html")]);
        assert_eq!(res.validation, V::Valid);
        assert!(res.valid);
        assert_eq!(res.entry_file.as_deref(), Some("index.html"));
        assert_eq!(res.artifact_kind, Some(FileKind::Html));
        // Relative paths and paths outside the folder.
        assert_eq!(fx.check(&p, &["index.html".into()]).validation, V::Valid);
        assert_eq!(
            fx.check(&p, &["/elsewhere/index.html".into()]).validation,
            V::EntryNotTouched
        );
        assert_eq!(
            fx.check(&p, &[fx.abs("a.css")]).validation,
            V::EntryNotTouched
        );
    }

    #[test]
    fn another_page_is_told_apart_by_its_whole_path() {
        let fx = Fixture::new(&[
            ("index.html", FileKind::Html, "<h1>x</h1>"),
            ("old/index.html", FileKind::Html, "<h1>old</h1>"),
        ]);
        let dir = Some(fx.dir.path());
        let other = |t: &[String]| wrote_other_page(t, dir, Some("index.html"));
        assert!(!other(&[fx.abs("index.html")]));
        assert!(!other(&["index.html".into(), fx.abs("a.css")]));
        assert!(other(&[fx.abs("old/index.html")]));
        assert!(other(&["old/index.html".into()]));
        assert!(other(&["/elsewhere/index.html".into()]));
        assert!(wrote_other_page(&[fx.abs("index.html")], dir, None));
    }

    #[test]
    fn linked_page_counts_for_prototypes() {
        let fx = Fixture::new(&[
            (
                "index.html",
                FileKind::Html,
                "<a href=\"about.html\">About</a>",
            ),
            ("about.html", FileKind::Html, "<h1>About</h1>"),
            ("other.html", FileKind::Html, "<h1>Other</h1>"),
        ]);
        let p = project(Some(ProjectKind::Prototype), None);
        assert_eq!(fx.check(&p, &[fx.abs("about.html")]).validation, V::Valid);
        assert_eq!(
            fx.check(&p, &[fx.abs("other.html")]).validation,
            V::EntryNotTouched
        );
        let deck = project(Some(ProjectKind::Deck), None);
        assert_eq!(
            fx.check(&deck, &[fx.abs("about.html")]).validation,
            V::EntryNotTouched
        );
    }

    #[test]
    fn declared_entry_kind_and_inference() {
        let fx = Fixture::new(&[
            (
                "deck.html",
                FileKind::Html,
                "<section class=slide></section>",
            ),
            ("cover.png", FileKind::Image, "png"),
        ]);
        let declared = project(Some(ProjectKind::Deck), Some("deck.html"));
        assert_eq!(
            fx.check(&declared, &[fx.abs("deck.html")]).validation,
            V::Valid
        );
        let gone = project(Some(ProjectKind::Deck), Some("missing.html"));
        assert_eq!(
            fx.check(&gone, &[fx.abs("deck.html")]).validation,
            V::EntryMissing
        );
        let escape = project(Some(ProjectKind::Deck), Some("../deck.html"));
        // An unsafe declared entry is ignored: inference picks deck.html.
        assert_eq!(
            fx.check(&escape, &[fx.abs("deck.html")]).validation,
            V::Valid
        );
        let image = project(Some(ProjectKind::Image), None);
        // Only root HTML file wins inference; an image project wants images.
        assert_eq!(
            fx.check(&image, &[fx.abs("deck.html")]).validation,
            V::TypeMismatch
        );
        let other = project(Some(ProjectKind::Other), None);
        assert_eq!(
            fx.check(&other, &[fx.abs("deck.html")]).validation,
            V::Valid
        );
        let two = Fixture::new(&[
            ("a.html", FileKind::Html, "a"),
            ("b.html", FileKind::Html, "b"),
        ]);
        assert_eq!(
            two.check(&project(None, None), &[two.abs("a.html")])
                .validation,
            V::EntryMissing
        );
    }

    #[test]
    fn unreadable_entry() {
        let fx = Fixture::new(&[("index.html", FileKind::Html, "x")]);
        std::fs::remove_file(fx.dir.path().join("index.html")).unwrap();
        std::fs::create_dir(fx.dir.path().join("index.html")).unwrap();
        let p = project(Some(ProjectKind::Prototype), None);
        assert_eq!(
            fx.check(&p, &[fx.abs("index.html")]).validation,
            V::EntryUnreadable
        );
    }
}
