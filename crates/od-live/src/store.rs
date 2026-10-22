//! Live artifacts on disk (spec 3.2): `<project>/.live-artifacts/<id>/`
//! holds `artifact.json`, `template.html`, `data.json`, `index.html`
//! (rendered preview), `provenance.json` and `refreshes.jsonl`. Writes go
//! through a temp file and rename; a new artifact is built in a `tmp-…`
//! folder and renamed into place.

use crate::model::{
    self, CreateInput, DATA_FILE, Document, LiveArtifact, PREVIEW_FILE, RefreshStatus,
    SCHEMA_VERSION, Summary, TEMPLATE_FILE, UpdateInput, now_iso,
};
use crate::{LiveError, Result, render};
use serde_json::{Map, Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const ROOT_DIR: &str = ".live-artifacts";
pub const ARTIFACT_FILE: &str = "artifact.json";
pub const PROVENANCE_FILE: &str = "provenance.json";
pub const REFRESHES_FILE: &str = "refreshes.jsonl";
pub const LOCK_FILE: &str = "refresh.lock";

/// Live artifacts of one project.
#[derive(Debug, Clone)]
pub struct LiveStore {
    project_dir: PathBuf,
    project_id: String,
}

/// Paths of one artifact.
#[derive(Debug, Clone)]
pub struct ArtifactPaths {
    pub dir: PathBuf,
}

impl ArtifactPaths {
    #[must_use]
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile_in(dir)?;
    tmp.1.write_all(bytes)?;
    tmp.1.sync_all()?;
    drop(tmp.1);
    fs::rename(&tmp.0, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp.0);
    })?;
    Ok(())
}

fn tempfile_in(dir: &Path) -> Result<(PathBuf, fs::File)> {
    let path = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok((path, file))
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| LiveError::Io(e.into()))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => LiveError::NotFound,
        _ => LiveError::Io(e),
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|e| LiveError::Corrupt(format!("{}: {e}", path.display())))
}

/// The starter template when an agent registers no template.
fn default_template(title: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n    <title>{{{{data.title}}}}</title>\n  </head>\n  <body>\n    <main>\n      <h1>{{{{data.title}}}}</h1>\n      <p>{}</p>\n    </main>\n  </body>\n</html>\n",
        render::escape(title)
    )
}

fn default_provenance(now: &str) -> Value {
    json!({
        "generatedAt": now,
        "generatedBy": "agent",
        "notes": "Created through the live artifact registration service.",
        "sources": [{ "label": "Agent-authored live artifact input", "type": "user_input" }],
    })
}

fn validate_provenance(p: &Value) -> Result<()> {
    if !p.is_object() {
        return Err(LiveError::Validation(
            "provenanceJson must be an object".to_owned(),
        ));
    }
    model::validate_bounded(p, "provenanceJson")
}

impl LiveStore {
    #[must_use]
    pub fn new(project_dir: impl Into<PathBuf>, project_id: impl Into<String>) -> Self {
        Self {
            project_dir: project_dir.into(),
            project_id: project_id.into(),
        }
    }

    #[must_use]
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    #[must_use]
    pub fn root(&self) -> PathBuf {
        self.project_dir.join(ROOT_DIR)
    }

    /// # Errors
    /// [`LiveError::NotFound`] for an unsafe id.
    pub fn paths(&self, id: &str) -> Result<ArtifactPaths> {
        if !model::is_safe_id(id) {
            return Err(LiveError::NotFound);
        }
        Ok(ArtifactPaths {
            dir: self.root().join(id),
        })
    }

    /// Writes template, data, rendered preview, provenance and the record.
    fn write_files(
        &self,
        paths: &ArtifactPaths,
        artifact: &LiveArtifact,
        template: &str,
        provenance: &Value,
    ) -> Result<()> {
        let html = render::render(template, &artifact.document.data_json)?;
        write_atomic(&paths.file(TEMPLATE_FILE), template.as_bytes())?;
        write_json(&paths.file(DATA_FILE), &artifact.document.data_json)?;
        write_atomic(&paths.file(PREVIEW_FILE), html.as_bytes())?;
        write_json(&paths.file(PROVENANCE_FILE), provenance)?;
        write_json(&paths.file(ARTIFACT_FILE), artifact)
    }

    /// Registers a new artifact. The template is validated and rendered
    /// before anything is written.
    ///
    /// # Errors
    /// Validation (including templates with scripts) or I/O errors.
    pub fn create(
        &self,
        input: CreateInput,
        template: Option<&str>,
        provenance: Option<Value>,
        created_by_run_id: Option<String>,
    ) -> Result<LiveArtifact> {
        let title = model::validate_title(&input.title)?;
        let document = input.document.unwrap_or_default();
        model::validate_document(&document)?;
        let now = now_iso();
        let template = template.map_or_else(|| default_template(&title), str::to_owned);
        let provenance = provenance.unwrap_or_else(|| default_provenance(&now));
        validate_provenance(&provenance)?;
        // Render once up front: a bad template never reaches the disk.
        render::render(&template, &document.data_json)?;
        let artifact = LiveArtifact {
            schema_version: SCHEMA_VERSION,
            id: model::new_artifact_id(&title, input.slug.as_deref()),
            project_id: self.project_id.clone(),
            session_id: input.session_id,
            created_by_run_id,
            slug: model::slugify(input.slug.as_deref().unwrap_or(&title)),
            title,
            status: input.status.unwrap_or_default(),
            pinned: input.pinned.unwrap_or(false),
            preview: input.preview.unwrap_or_default(),
            refresh_status: RefreshStatus::Idle,
            created_at: now.clone(),
            updated_at: now,
            last_refreshed_at: None,
            document,
        };
        fs::create_dir_all(self.root())?;
        let tmp = ArtifactPaths {
            dir: self
                .root()
                .join(format!("tmp-{}", uuid::Uuid::new_v4().simple())),
        };
        fs::create_dir(&tmp.dir)?;
        let written = self
            .write_files(&tmp, &artifact, &template, &provenance)
            .and_then(|()| Ok(fs::rename(&tmp.dir, self.paths(&artifact.id)?.dir)?));
        if written.is_err() {
            let _ = fs::remove_dir_all(&tmp.dir);
        }
        written.map(|()| artifact)
    }

    /// Every artifact, most recently updated first.
    ///
    /// # Errors
    /// I/O errors other than a missing folder; corrupt records.
    pub fn list(&self) -> Result<Vec<Summary>> {
        let entries = match fs::read_dir(self.root()) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type()?.is_dir() || name.starts_with("tmp-") || !model::is_safe_id(&name)
            {
                continue;
            }
            match self.get(&name) {
                Ok(a) => out.push(Summary::from(&a)),
                Err(LiveError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// The record, with `document.dataJson` read from `data.json`.
    ///
    /// # Errors
    /// [`LiveError::NotFound`], corrupt files.
    pub fn get(&self, id: &str) -> Result<LiveArtifact> {
        let paths = self.paths(id)?;
        let mut artifact: LiveArtifact = read_json(&paths.file(ARTIFACT_FILE))?;
        if artifact.id != id || artifact.project_id != self.project_id {
            return Err(LiveError::Corrupt(format!(
                "{} does not belong to this project",
                paths.file(ARTIFACT_FILE).display()
            )));
        }
        if let Ok(data) = read_json::<Map<String, Value>>(&paths.file(DATA_FILE)) {
            artifact.document.data_json = data;
        }
        Ok(artifact)
    }

    /// # Errors
    /// [`LiveError::NotFound`], I/O.
    pub fn template(&self, id: &str) -> Result<String> {
        let paths = self.paths(id)?;
        self.get(id)?;
        Ok(fs::read_to_string(paths.file(TEMPLATE_FILE))?)
    }

    /// The rendered preview, rebuilt when missing.
    ///
    /// # Errors
    /// [`LiveError::NotFound`], render or I/O errors.
    pub fn preview_html(&self, id: &str) -> Result<String> {
        let artifact = self.get(id)?;
        let paths = self.paths(id)?;
        match fs::read_to_string(paths.file(PREVIEW_FILE)) {
            Ok(html) => Ok(html),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let template = fs::read_to_string(paths.file(TEMPLATE_FILE))?;
                let html = render::render(&template, &artifact.document.data_json)?;
                write_atomic(&paths.file(PREVIEW_FILE), html.as_bytes())?;
                Ok(html)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Applies an update; a new template or data re-renders the preview.
    ///
    /// # Errors
    /// [`LiveError::NotFound`], validation or I/O errors.
    pub fn update(
        &self,
        id: &str,
        input: UpdateInput,
        template: Option<&str>,
        provenance: Option<Value>,
    ) -> Result<LiveArtifact> {
        let mut artifact = self.get(id)?;
        let paths = self.paths(id)?;
        if let Some(title) = input.title {
            artifact.title = model::validate_title(&title)?;
        }
        if let Some(slug) = input.slug {
            artifact.slug = model::slugify(&slug);
        }
        if let Some(pinned) = input.pinned {
            artifact.pinned = pinned;
        }
        if let Some(status) = input.status {
            artifact.status = status;
        }
        if let Some(preview) = input.preview {
            artifact.preview = preview;
        }
        if let Some(document) = input.document {
            model::validate_document(&document)?;
            artifact.document = document;
        }
        let template = match template {
            Some(t) => t.to_owned(),
            None => fs::read_to_string(paths.file(TEMPLATE_FILE))?,
        };
        let provenance = match provenance {
            Some(p) => {
                validate_provenance(&p)?;
                p
            }
            None => read_json(&paths.file(PROVENANCE_FILE)).unwrap_or_else(|_| json!({})),
        };
        artifact.updated_at = now_iso();
        self.write_files(&paths, &artifact, &template, &provenance)?;
        Ok(artifact)
    }

    /// Removes the artifact folder.
    ///
    /// # Errors
    /// [`LiveError::NotFound`], I/O.
    pub fn delete(&self, id: &str) -> Result<()> {
        self.get(id)?;
        fs::remove_dir_all(self.paths(id)?.dir)?;
        Ok(())
    }

    /// `refreshes.jsonl`, oldest first.
    ///
    /// # Errors
    /// [`LiveError::NotFound`], I/O, corrupt lines.
    pub fn refreshes(&self, id: &str) -> Result<Vec<Value>> {
        self.get(id)?;
        let path = self.paths(id)?.file(REFRESHES_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, l)| {
                serde_json::from_str(l).map_err(|e| {
                    LiveError::Corrupt(format!("{REFRESHES_FILE} line {}: {e}", i + 1))
                })
            })
            .collect()
    }

    pub(crate) fn save_record(&self, artifact: &LiveArtifact) -> Result<()> {
        write_json(&self.paths(&artifact.id)?.file(ARTIFACT_FILE), artifact)
    }

    pub(crate) fn save_data(&self, id: &str, document: &Document) -> Result<()> {
        let paths = self.paths(id)?;
        let template = fs::read_to_string(paths.file(TEMPLATE_FILE))?;
        let html = render::render(&template, &document.data_json)?;
        write_json(&paths.file(DATA_FILE), &document.data_json)?;
        write_atomic(&paths.file(PREVIEW_FILE), html.as_bytes())
    }

    pub(crate) fn append_refresh(&self, id: &str, entry: &Value) -> Result<()> {
        let path = self.paths(id)?.file(REFRESHES_FILE);
        let mut line = serde_json::to_vec(entry).map_err(|e| LiveError::Io(e.into()))?;
        line.push(b'\n');
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        f.write_all(&line)?;
        Ok(())
    }

    pub(crate) fn update_provenance(&self, id: &str, now: &str) -> Result<()> {
        let path = self.paths(id)?.file(PROVENANCE_FILE);
        let mut p: Value = read_json(&path).unwrap_or_else(|_| json!({ "sources": [] }));
        if let Some(obj) = p.as_object_mut() {
            obj.insert("generatedAt".to_owned(), json!(now));
            obj.insert("generatedBy".to_owned(), json!("refresh_runner"));
        }
        write_json(&path, &p)
    }
}
